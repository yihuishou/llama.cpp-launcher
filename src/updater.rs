//! 软件自更新模块（单文件 exe 绿色版）
//!
//! 流程：
//! 1. check()：GET GitHub latest release API（yihuishou/llama.cpp-launcher），
//!    比对 tag_name 与当前编译版本（CARGO_PKG_VERSION）；
//! 2. 有新版本时 UI 切换为「安装更新」，install() 后台线程流式下载新 exe
//!    （写入 exe 同目录 update/ 子目录，带进度 + 取消；Linux 资产为压缩包，
//!    下载后解压出二进制）；
//! 3. 下载完成 → 自等待者方案（见"自替换"段）：
//!    - 旧版本 spawn 自身（--updater-switch <pid>）作为无窗口等待者；
//!    - 旧 exe 改名为 exe.old 腾出正式名（运行中的 exe 允许改名），
//!      新包改名为正式名；
//!    - 主进程随 Installing 状态关闭窗口退出；
//!    - 等待者等旧进程完全退出后，启动已换入正式名的新版本；
//!    - 新版本启动时清理 exe.old 与 update 目录残留。
//!    全程进程 API + Rust fs（宽字符），中英文路径安全，不依赖 cmd/bat
//!    （cmd /c 引号解析会损坏路径、bat 有代码页乱码问题），且不依赖新版本
//!    exe 的任何配合——官方历史版本同样可直接更新。
//!    Linux/macOS 仍走 sh 后台脚本替换。
//! 4. 主进程随即退出窗口。
//!
//! 网络（超时/UA/代理/分块）统一由 net_proxy 工具模块提供。
//! UI 层：每帧轮询 UpdaterHandle::snapshot() 渲染按钮状态/进度；
//! 应用关闭时无需额外取消（下载线程持有 Arc 克隆，不阻塞退出）。

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde::Deserialize;

use crate::net_proxy as net;

/// 上游仓库 latest release API（直连，官方）
const API_OFFICIAL: &str =
    "https://api.github.com/repos/yihuishou/llama.cpp-launcher/releases/latest";
/// API 镜像基址（gh-proxy 前缀，官方超时/失败时自动回退）
const API_MIRROR: &str =
    "https://gh-proxy.com/https://api.github.com/repos/yihuishou/llama.cpp-launcher/releases/latest";
/// 资产下载镜像前缀（官方 URL 直接使用 asset.browser_download_url）
const DOWNLOAD_MIRROR: &str = "https://gh-proxy.com/https://github.com";
/// 全部源失败时 UI 展示的固定错误消息（网络层；统一入口见 net::ERR_NETWORK）
pub use crate::net_proxy::ERR_NETWORK;

/// 获取当前 CPU 架构标识（用于匹配 Linux 资产）
fn get_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        // 兜底：未知架构返回空字符串，后续匹配会失败
        ""
    }
}

/// 官方 + 镜像依次尝试；返回 Response body 字符串。
/// 网络层（连接/读超时 + 系统代理 + UA）统一走 net::build_agent，
/// 无总超时——大文件下载不会被 `.timeout()` 总超时掐断。
fn fetch_with_fallback(
    official_url: &str,
    mirror_url: &str,
    smart_mirror: bool,
) -> Result<String, String> {
    let mirror_first = smart_mirror && crate::geo::should_use_mirror_first();

    if smart_mirror {
        log::info!(
            "[network] 智能镜像选择: 地理位置检测 = {}",
            if mirror_first {
                "中国大陆"
            } else {
                "其他地区"
            }
        );
        log::info!(
            "[network] 网络请求优先级: {}",
            if mirror_first {
                "镜像源 → 官方源"
            } else {
                "官方源 → 镜像源"
            }
        );
    } else {
        log::info!("[network] API 调用: 始终使用官方源");
    }

    let agent = net::build_agent();
    if mirror_first {
        // 中国大陆：优先使用镜像源
        log::info!("[network] 尝试镜像源: {}", mirror_url);
        if let Ok(body) = fetch_body(&agent, mirror_url) {
            log::info!("[network] ✓ 镜像源请求成功");
            return Ok(body);
        }
        log::warn!("[network] ✗ 镜像源请求失败，回退到官方源");
        // 镜像失败，回退到官方源
        log::info!("[network] 尝试官方源: {}", official_url);
        if let Ok(body) = fetch_body(&agent, official_url) {
            log::info!("[network] ✓ 官方源请求成功");
            return Ok(body);
        }
        log::warn!("[network] ✗ 官方源请求也失败");
    } else {
        // 其他地区或 API 调用：优先使用官方源
        log::info!("[network] 尝试官方源: {}", official_url);
        if let Ok(body) = fetch_body(&agent, official_url) {
            log::info!("[network] ✓ 官方源请求成功");
            return Ok(body);
        }
        log::warn!("[network] ✗ 官方源请求失败，回退到镜像源");
        // 官方失败，回退到镜像源
        log::info!("[network] 尝试镜像源: {}", mirror_url);
        if let Ok(body) = fetch_body(&agent, mirror_url) {
            log::info!("[network] ✓ 镜像源请求成功");
            return Ok(body);
        }
        log::warn!("[network] ✗ 镜像源请求也失败");
    }

    log::error!("[network] ✗ 所有网络源都失败");
    Err(ERR_NETWORK.to_string())
}

/// 单次 GET + 读取 body
fn fetch_body(ag: &ureq::Agent, url: &str) -> Result<String, String> {
    let mut response = ag
        .get(url)
        .header("User-Agent", net::USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    response.body_mut().read_to_string().map_err(|e| e.to_string())
}

// ======================= 公开类型 =======================

/// 更新流程状态（UI 据此渲染按钮与提示）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// 空闲（初始 / 已取消）
    Idle,
    /// 正在查询 GitHub API
    Checking,
    /// 已是最新版本（UI 显示"已是最新"）
    UpToDate,
    /// 发现新版本，携带版本号（如 "0.1.20"）
    Available(String),
    /// 正在下载更新（进度见 UpdateStatus::done / total）
    Downloading,
    /// 下载完成，正在启动自替换（即将重启）
    Installing,
    /// 失败，携带英文错误消息（UI 层拼接 i18n 前缀）
    Error(String),
}

/// 更新状态快照（UI 每帧轮询）
#[derive(Clone, Debug)]
pub struct UpdateStatus {
    pub state: UpdateState,
    /// 已下载字节
    pub done: u64,
    /// 总字节（未知为 0）
    pub total: u64,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            state: UpdateState::Idle,
            done: 0,
            total: 0,
        }
    }
}

/// 后台更新控制句柄（模式对齐 downloader::DownloadHandle）
pub struct UpdaterHandle {
    /// 协作取消标志
    cancel: Arc<AtomicBool>,
    /// 共享状态
    status: Arc<Mutex<UpdateStatus>>,
    /// worker 线程句柄（线程持有 Arc 克隆，不阻塞应用退出）
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for UpdaterHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdaterHandle {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(UpdateStatus::default())),
            worker: Arc::new(Mutex::new(None)),
        }
    }

    /// 检查更新：仅 Idle / UpToDate / Error 时启动检查线程（防重复）
    pub fn check(&self) {
        let busy = matches!(
            self.status.lock().unwrap().state,
            UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing
        );
        if busy {
            return;
        }
        self.status.lock().unwrap().state = UpdateState::Checking;
        self.spawn_worker(WorkerKind::Check);
    }

    /// 开始下载并安装：仅 Available 时启动（Downloading 时忽略）
    pub fn install(&self) {
        let ready = matches!(self.status.lock().unwrap().state, UpdateState::Available(_));
        if !ready {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        self.spawn_worker(WorkerKind::Download);
    }

    /// 返回当前状态快照
    pub fn snapshot(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }

    /// 是否正在下载（UI 禁用按钮用）
    pub fn is_busy(&self) -> bool {
        matches!(
            self.snapshot().state,
            UpdateState::Checking | UpdateState::Downloading | UpdateState::Installing
        )
    }

    /// Drop/关窗调用，协作取消下载
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn spawn_worker(&self, kind: WorkerKind) {
        let cancel = Arc::clone(&self.cancel);
        let status = Arc::clone(&self.status);
        let handle = thread::spawn(move || match kind {
            WorkerKind::Check => worker_check(status),
            WorkerKind::Download => worker_download(cancel, status),
        });
        *self.worker.lock().unwrap() = Some(handle);
    }
}

enum WorkerKind {
    Check,
    Download,
}

// ======================= GitHub API 类型 =======================

/// GitHub latest release API 响应（只取需要的字段）
#[derive(Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

/// 请求 GitHub latest release API：官方直连失败（超时/网络错误）时自动尝试镜像。
/// 全部源失败返回 ERR_NETWORK（固定标记，UI 展示"获取失败：网络错误"）。
fn fetch_release() -> Result<ReleaseInfo, String> {
    let body = fetch_with_fallback(API_OFFICIAL, API_MIRROR, false)?;
    serde_json::from_str::<ReleaseInfo>(&body).map_err(|e| e.to_string())
}

/// 匹配当前平台的 launcher 资产
/// - Windows x64: llama_cpp_launcher_v*.exe
/// - Windows arm64: llama_cpp_launcher_v*_arm64.exe
/// - Linux: llama_cpp_launcher_v*_linux_{arch}.tar.gz
fn pick_launcher_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    let is_linux = cfg!(target_os = "linux");
    let is_arm64 = cfg!(target_arch = "aarch64");

    assets.iter().find(|a| {
        if is_linux {
            // Linux: 匹配 llama_cpp_launcher_v*_linux_{arch}.tar.gz
            let arch = get_arch();
            a.name.starts_with("llama_cpp_launcher_v")
                && a.name.contains("_linux_")
                && a.name.contains(arch)
                && a.name.ends_with(".tar.gz")
        } else if is_arm64 {
            // Windows arm64: 匹配 llama_cpp_launcher_v*_arm64.exe
            a.name.starts_with("llama_cpp_launcher_v")
                && a.name.contains("_arm64")
                && a.name.ends_with(".exe")
        } else {
            // Windows x64: 匹配 llama_cpp_launcher_v*.exe（排除 _arm64）
            a.name.starts_with("llama_cpp_launcher_v")
                && a.name.ends_with(".exe")
                && !a.name.contains("_arm64")
        }
    })
}

/// 版本号比较："v0.1.20" vs "0.1.19" → 新版本存在（按点分数字比较）
fn is_newer(tag: &str, current: &str) -> bool {
    fn nums(s: &str) -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    }
    let t = nums(tag);
    let c = nums(current);
    for (a, b) in t.iter().zip(c.iter()) {
        if a != b {
            return a > b;
        }
    }
    t.len() > c.len()
}

// ======================= 检查线程 =======================

fn worker_check(status: Arc<Mutex<UpdateStatus>>) {
    let current = env!("CARGO_PKG_VERSION");
    match fetch_release() {
        Ok(release) => {
            let mut st = status.lock().unwrap();
            if is_newer(&release.tag_name, current) {
                st.state = UpdateState::Available(trim_v(&release.tag_name));
            } else {
                st.state = UpdateState::UpToDate;
            }
        }
        Err(message) => {
            let mut st = status.lock().unwrap();
            st.state = UpdateState::Error(message);
        }
    }
}

fn trim_v(tag: &str) -> String {
    tag.trim_start_matches('v').to_string()
}

// ======================= 下载线程 =======================

fn worker_download(cancel: Arc<AtomicBool>, status: Arc<Mutex<UpdateStatus>>) {
    let result = run_download(&cancel, &status);
    match result {
        Ok(new_exe) => {
            let mut st = status.lock().unwrap();
            if cancel.load(Ordering::SeqCst) {
                st.state = UpdateState::Idle;
            } else {
                // 下载完成：启动自替换（拉起的命令行已捕获新 exe 路径）
                st.state = UpdateState::Installing;
                st.total = st.done;
                drop(st);
                replace_and_restart(&new_exe);
            }
        }
        Err(message) => {
            let mut st = status.lock().unwrap();
            if cancel.load(Ordering::SeqCst) {
                st.state = UpdateState::Idle;
            } else {
                st.state = UpdateState::Error(message);
            }
        }
    }
}

/// 下载新版本到 exe 同目录 update/ 子目录；成功返回新二进制完整路径。
/// Windows 下载 .exe；Linux 下载 .tar.gz 并解压得到二进制。
/// 官方下载源失败（超时/网络错误）时自动尝试镜像；全部失败返回 ERR_NETWORK。
fn run_download(cancel: &AtomicBool, status: &Arc<Mutex<UpdateStatus>>) -> Result<PathBuf, String> {
    // 1) 获取 release 信息与资产
    let release = fetch_release()?;
    let asset = pick_launcher_asset(&release.assets)
        .ok_or_else(|| format!("no launcher asset in release {}", release.tag_name))?;

    // 2) 目标目录：exe 同目录/update
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe_path.parent().ok_or("no exe parent dir")?.to_path_buf();
    let update_dir = exe_dir.join("update");
    fs::create_dir_all(&update_dir).map_err(|e| format!("create update dir failed: {}", e))?;

    let is_linux = cfg!(target_os = "linux");
    let new_binary = if is_linux {
        update_dir.join("llama_cpp_launcher_new")
    } else {
        update_dir.join("llama_cpp_launcher_new.exe")
    };

    // 3) 流式下载（partial + rename 原子落盘）；官方/镜像两源依次尝试
    {
        let mut st = status.lock().unwrap();
        st.state = UpdateState::Downloading;
        st.done = 0;
        st.total = asset.size;
    }
    let partial_ext = if is_linux { ".tar.gz" } else { ".exe" };
    let partial = update_dir.join(format!(".partial.new{}", partial_ext));
    let agent = net::build_agent();

    // asset.browser_download_url 形如 https://github.com/.../releases/download/...
    // 官方源直接使用；镜像源去掉 github.com 前缀后接在 DOWNLOAD_MIRROR 之后
    // （DOWNLOAD_MIRROR 以 gh-proxy.com/https://github.com 结尾，不可重复拼接前缀）
    let official_url = asset.browser_download_url.clone();
    let mirror_url = match official_url.strip_prefix("https://github.com") {
        Some(rest) => format!("{}{}", DOWNLOAD_MIRROR, rest),
        None => official_url.clone(),
    };

    // 智能镜像：根据地理位置决定下载优先级
    let mirror_first = crate::geo::should_use_mirror_first();
    log::info!(
        "[update] 智能镜像选择: 地理位置检测 = {}",
        if mirror_first {
            "中国大陆"
        } else {
            "其他地区"
        }
    );
    log::info!(
        "[update] 下载优先级: {}",
        if mirror_first {
            "镜像源 → 官方源"
        } else {
            "官方源 → 镜像源"
        }
    );

    // 根据地理位置决定下载顺序
    let urls = if mirror_first {
        [mirror_url, official_url]
    } else {
        [official_url, mirror_url]
    };

    for (i, url) in urls.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("download cancelled".to_string());
        }
        let _ = fs::remove_file(&partial); // 上一源失败的残留
        let source = if (i == 0 && mirror_first) || (i == 1 && !mirror_first) {
            "镜像源"
        } else {
            "官方源"
        };
        log::info!("[update] 尝试{}: {}", source, url);
        let mut response = match agent.get(url).header("User-Agent", net::USER_AGENT).call() {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[update] ✗ {} 失败: {}", source, e);
                continue; // 该源失败，尝试下一个
            }
        };
        // 流式写入
        let result = (|| -> Result<u64, String> {
            let file =
                File::create(&partial).map_err(|e| format!("create temp file failed: {}", e))?;
            let mut writer = BufWriter::new(file);
            let mut reader = response.body_mut().as_reader();
            let mut buf = vec![0u8; net::CHUNK_SIZE];
            let mut done: u64 = 0;
            loop {
                let n = reader
                    .read(&mut buf)
                    .map_err(|e| format!("download failed: {}", e))?;
                if n == 0 {
                    break;
                }
                writer
                    .write_all(&buf[..n])
                    .map_err(|e| format!("write failed: {}", e))?;
                done += n as u64;
                status.lock().unwrap().done = done;
                if cancel.load(Ordering::SeqCst) {
                    return Err("download cancelled".to_string());
                }
            }
            writer.flush().map_err(|e| e.to_string())?;
            Ok(done)
        })();
        let done = match result {
            Ok(d) => d,
            Err(msg) => {
                if msg == "download cancelled" {
                    return Err(msg);
                }
                continue; // 该源失败，尝试下一个
            }
        };
        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&partial);
            return Err("download cancelled".to_string());
        }
        // 下载完整性校验：实际字节应与资产声明一致
        if done != asset.size {
            log::warn!(
                "[update] ✗ {} 数据不完整: 期望 {} 字节，实际 {} 字节",
                source,
                asset.size,
                done
            );
            continue; // 该源数据不完整，尝试下一个
        }
        log::info!("[update] ✓ 下载成功: {}", url);
        fs::rename(&partial, &new_binary).map_err(|e| format!("rename temp file failed: {}", e))?;

        // Linux: 需要从 .tar.gz 解压出二进制文件
        if is_linux {
            return extract_linux_binary(&new_binary, &update_dir);
        }
        return Ok(new_binary);
    }
    let _ = fs::remove_file(&partial);
    Err(ERR_NETWORK.to_string()) // 官方 + 镜像均失败
}

/// 从 .tar.gz 解压出 launcher 二进制文件
fn extract_linux_binary(archive: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let file = File::open(archive).map_err(|e| format!("open archive failed: {}", e))?;
    let decoder = flate2::bufread::GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);

    // 解压到目标目录
    tar.unpack(dest_dir)
        .map_err(|e| format!("extract tar.gz failed: {}", e))?;

    // 查找解压出的二进制文件（llama_cpp_launcher 或 llama_cpp_launcher.exe）
    let entries = fs::read_dir(dest_dir).map_err(|e| format!("read dir failed: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // 匹配 launcher 二进制（排除压缩包本身）
            if (name.starts_with("llama_cpp_launcher") || name == "llama-cpp-launcher")
                && !name.ends_with(".tar.gz")
                && !name.ends_with(".partial")
            {
                // 确保文件可执行
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
                }
                return Ok(path);
            }
        }
    }

    Err("launcher binary not found in archive".to_string())
}

// ======================= 自替换（自等待者方案） =======================

/// 旧版本：下载完成后交接替换。
/// 1) spawn 自身（`--updater-switch <pid>`）作为无窗口等待者；
/// 2) 旧 exe 改名为 exe.old 腾出正式名（运行中的 exe 允许改名，进程继续运行）；
/// 3) 新包改名为正式名（失败则回滚旧包）；主进程随 Installing 状态自行退出；
/// 4) 等待者等旧进程完全退出后，启动已换入正式名的新版本，自身退出。
///
/// 全程进程 API + Rust fs（宽字符），中英文路径安全；无 cmd/bat
/// （cmd /c 引号解析损坏路径、bat 代码页乱码）。不依赖新版本 exe 的任何配合，
/// 官方历史版本同样可直接更新。
fn replace_and_restart(new_binary: &Path) {
    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };

    #[cfg(target_os = "windows")]
    {
        let old_path = exe_path.with_extension("exe.old");
        // 1) 等待者 = 自身二进制（先于改名 spawn；无 UI，仅等待与启动新版本）
        let mut cmd = std::process::Command::new(&exe_path);
        cmd.arg("--updater-switch")
            .arg(std::process::id().to_string());
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        if let Err(e) = cmd.spawn() {
            log::warn!("[updater] 启动等待者失败，放弃自动替换: {}", e);
            return;
        }
        // 2) 旧 exe 改名腾出正式名（自身映像允许改名，进程不受影响）
        if let Err(e) = std::fs::rename(&exe_path, &old_path) {
            log::warn!("[updater] 旧 exe 改名失败，放弃自动替换: {}", e);
            return;
        }
        // 3) 新包顶替正式名；失败则回滚旧包，保持可用状态
        if let Err(e) = std::fs::rename(new_binary, &exe_path) {
            log::warn!("[updater] 新包改名失败，回滚: {}", e);
            let _ = std::fs::rename(&old_path, &exe_path);
            return;
        }
        // 4) update/ 目录此时已空，best-effort 删除
        if let Some(dir) = new_binary.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        log::info!("[updater] 交接完成，等待者已启动，主进程即将退出");
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Linux/macOS：sh 后台执行，sleep 等主进程退出后替换并重启
        let old_path = exe_path.with_extension("old");
        let script = format!(
            "(sleep 3; mv \"{}\" \"{}\"; mv \"{}\" \"{}\"; rm -f \"{}\"; \"{}\" &) >/dev/null 2>&1 &",
            exe_path.display(),
            old_path.display(),
            new_binary.display(),
            exe_path.display(),
            old_path.display(),
            exe_path.display(),
        );
        let _ = std::process::Command::new("sh")
            .args(["-c", &script])
            .spawn();
    }
}

/// 等待者进程入口（main 检测到 `--updater-switch <pid>` 时调用，无 UI）。
/// 等待旧进程完全退出（含 Drop 保存设置，最多 15s），然后启动已换入正式名的
/// 新版本，自身退出。自身映像 = exe.old（被自己映射，不可删除），
/// 由新版本启动时清理。
#[cfg(target_os = "windows")]
pub fn run_updater_waiter(old_pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    const WAIT_TIMEOUT_MS: u32 = 15_000;

    unsafe {
        // OpenProcess 失败（旧进程已退出）视为已退出，直接继续；
        // 超时（旧进程异常卡住）则放弃启动，避免双窗口共存
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, old_pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, WAIT_TIMEOUT_MS);
            CloseHandle(handle);
        }
    }
    // current_exe 返回创建时的路径（正式名）——此刻该路径已是新版本
    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(&exe_path).spawn();
}

/// 启动清理：删除上一次更新遗留的旧版备份（exe.old，等待者退出后即可删）
/// 与已空的 update 目录（best-effort；失败留给下次启动再清）。
#[cfg(target_os = "windows")]
pub fn cleanup_leftovers_on_startup() {
    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };
    let old_path = exe_path.with_extension("exe.old");
    if old_path.exists() {
        // 等待者可能刚退出还没释放映像，短重试几次
        for _ in 0..10 {
            match std::fs::remove_file(&old_path) {
                Ok(_) => {
                    log::info!("[updater] 已清理旧版备份: {}", old_path.display());
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
            }
        }
    }
    if let Some(dir) = exe_path.parent() {
        let update_dir = dir.join("update");
        if update_dir.is_dir() {
            let _ = std::fs::remove_dir(&update_dir); // 仅空目录可删
        }
    }
}
