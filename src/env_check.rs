//! 编译环境检测模块（llama.cpp 手动编译前置条件）。
//!
//! 探测编译 llama.cpp 所需的工具链与可选的 GPU 后端工具包。通用项（git、cmake、C++
//! 编译器、ninja、make、python）沿 PATH 查找；GPU 项仅 CUDA 与 ROCm 做「环境变量 +
//! 已知安装路径」特判：CUDA 取 `CUDA_PATH`、`CUDA_HOME`、NVIDIA 默认目录或 PATH 上的
//! nvcc，ROCm 取 `ROCM_PATH`、`ROCM_HOME`、`/opt/rocm` 或 PATH 上的 hipcc。Windows 下
//! PATH 搜索按 PATHEXT 拼扩展名，并用 `vswhere.exe` 定位 MSVC；Linux 走 PATH 上的
//! `c++`、`clang++`、`g++`。
//!
//! 全部探测走后台线程（Handle 模式，对齐 `downloader::DownloadHandle` /
//! `updater::UpdaterHandle`）：`Arc<AtomicBool>` 取消标志 + `Arc<Mutex<Status>>` 共享
//! 状态 + worker 线程；UI 每帧 `snapshot()` 轮询，关闭时 `request_cancel()`。组件名一律
//! 存 [`i18n::Key`]，渲染时再 `i18n::t()` 解析（语言切换实时生效）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::i18n::Key;

/// 单个组件的探测结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 已找到：`version` 为版本描述（可能为空），`path` 为可执行文件或安装根路径
    Found { version: String, path: PathBuf },
    /// 未找到
    Missing,
}

/// 单个检测项结果（供 UI 逐行渲染）
#[derive(Clone, Debug, PartialEq)]
pub struct CheckResult {
    /// 组件名（i18n 键，渲染时 `t()`）
    pub key: Key,
    /// 是否必需：必需项缺失 → 整体不可编译；可选项缺失仅提示
    pub required: bool,
    /// 探测结果
    pub probe: Probe,
}

impl CheckResult {
    /// 该项是否可用
    pub fn is_found(&self) -> bool {
        matches!(self.probe, Probe::Found { .. })
    }
}

/// 检测整体状态
#[derive(Clone, Debug, PartialEq, Default)]
pub enum EnvCheckState {
    /// 空闲（初始 / 已取消）
    #[default]
    Idle,
    /// 进行中
    Running,
    /// 完成，携带全部检测项结果
    Done(Vec<CheckResult>),
}

/// 检测状态快照（UI 每帧轮询）
#[derive(Clone, Debug, Default)]
pub struct EnvCheckStatus {
    pub state: EnvCheckState,
}

/// 后台检测控制句柄（模式对齐 downloader::DownloadHandle）
pub struct EnvCheckHandle {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<EnvCheckStatus>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for EnvCheckHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvCheckHandle {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(EnvCheckStatus::default())),
            worker: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动后台检测；进行中时忽略（防重复点击）
    pub fn run(&self) {
        if self.is_busy() {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        self.status.lock().unwrap().state = EnvCheckState::Running;

        let cancel = Arc::clone(&self.cancel);
        let status = Arc::clone(&self.status);
        let handle = thread::spawn(move || {
            let mut results = Vec::new();
            for req in requirements() {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let probe = (req.probe)();
                results.push(CheckResult {
                    key: req.key,
                    required: req.required,
                    probe,
                });
            }
            let mut st = status.lock().unwrap();
            // 被取消则回到 Idle，而非展示半份结果
            st.state = if cancel.load(Ordering::SeqCst) {
                EnvCheckState::Idle
            } else {
                EnvCheckState::Done(results)
            };
        });
        *self.worker.lock().unwrap() = Some(handle);
    }

    /// 返回当前状态快照
    pub fn snapshot(&self) -> EnvCheckStatus {
        self.status.lock().unwrap().clone()
    }

    /// 是否正在检测（UI 禁用按钮用）
    pub fn is_busy(&self) -> bool {
        matches!(self.snapshot().state, EnvCheckState::Running)
    }

    /// Drop / 关窗调用，协作取消
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// 同步执行一次完整检测（不经过后台线程；供测试/命令行场景）
pub fn check_now() -> Vec<CheckResult> {
    requirements()
        .into_iter()
        .map(|req| CheckResult {
            key: req.key,
            required: req.required,
            probe: (req.probe)(),
        })
        .collect()
}

/// 所有必需项是否都已找到
pub fn all_required_found(results: &[CheckResult]) -> bool {
    results
        .iter()
        .filter(|r| r.required)
        .all(CheckResult::is_found)
}

/// 检测项定义：组件名 + 是否必需 + 探测函数
struct Requirement {
    key: Key,
    required: bool,
    probe: fn() -> Probe,
}

/// 平台检测清单。GPU 后端（CUDA / ROCm）均为可选项，缺失不算失败。
fn requirements() -> Vec<Requirement> {
    let mut reqs = vec![
        Requirement {
            key: Key::EnvCheckGit,
            required: true,
            probe: probe_git,
        },
        Requirement {
            key: Key::EnvCheckCmake,
            required: true,
            probe: probe_cmake,
        },
        Requirement {
            key: Key::EnvCheckCompiler,
            required: true,
            probe: probe_compiler,
        },
        Requirement {
            key: Key::EnvCheckNinja,
            required: false,
            probe: probe_ninja,
        },
        Requirement {
            key: Key::EnvCheckMake,
            required: false,
            probe: probe_make,
        },
        Requirement {
            key: Key::EnvCheckPython,
            required: false,
            probe: probe_python,
        },
    ];
    reqs.push(Requirement {
        key: Key::EnvCheckCuda,
        required: false,
        probe: probe_cuda,
    });
    reqs.push(Requirement {
        key: Key::EnvCheckRocm,
        required: false,
        probe: probe_rocm,
    });
    reqs
}

fn probe_git() -> Probe {
    probe_tool("git", &["--version"])
}

fn probe_cmake() -> Probe {
    probe_tool("cmake", &["--version"])
}

fn probe_ninja() -> Probe {
    probe_tool("ninja", &["--version"])
}

fn probe_make() -> Probe {
    probe_tool("make", &["--version"])
}

fn probe_python() -> Probe {
    for name in ["python3", "python"] {
        if let Some(exe) = find_in_path(name) {
            let version = probe_version(&exe, &["--version"]).unwrap_or_default();
            return Probe::Found { version, path: exe };
        }
    }
    Probe::Missing
}

fn probe_compiler() -> Probe {
    #[cfg(windows)]
    {
        let msvc = probe_msvc();
        if matches!(msvc, Probe::Found { .. }) {
            return msvc;
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["clang++", "g++"]
    } else {
        &["c++", "clang++", "g++"]
    };
    for name in names {
        if let Some(exe) = find_in_path(name) {
            let version = probe_version(&exe, &["--version"]).unwrap_or_default();
            return Probe::Found { version, path: exe };
        }
    }
    Probe::Missing
}

/// Windows: 用官方 vswhere.exe 定位 MSVC（含 VC Tools 工作负载）
#[cfg(windows)]
fn probe_msvc() -> Probe {
    let vswhere = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .map(|base| {
            base.join("Microsoft Visual Studio")
                .join("Installer")
                .join("vswhere.exe")
        })
        .find(|p| p.is_file());
    let Some(vswhere) = vswhere else {
        return Probe::Missing;
    };
    let out = probe_version(
        &vswhere,
        &[
            "-latest",
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ],
    )
    .unwrap_or_default();
    if out.is_empty() {
        Probe::Missing
    } else {
        Probe::Found {
            version: String::new(),
            path: PathBuf::from(out),
        }
    }
}

/// CUDA：环境变量 → 已知安装路径 → PATH 上的 nvcc（仅此三者）
fn probe_cuda() -> Probe {
    for var in ["CUDA_PATH", "CUDA_HOME"] {
        if let Some(root) = std::env::var_os(var).map(PathBuf::from) {
            if let Some(found) = cuda_probe_root(&root) {
                return found;
            }
        }
    }
    for root in cuda_known_roots() {
        if let Some(found) = cuda_probe_root(&root) {
            return found;
        }
    }
    if let Some(exe) = find_in_path("nvcc") {
        let version = probe_version(&exe, &["--version"]).unwrap_or_default();
        return Probe::Found { version, path: exe };
    }
    Probe::Missing
}

fn cuda_probe_root(root: &Path) -> Option<Probe> {
    let nvcc = cuda_nvcc(root);
    if nvcc.is_file() {
        let version = probe_version(&nvcc, &["--version"]).unwrap_or_default();
        Some(Probe::Found {
            version,
            path: root.to_path_buf(),
        })
    } else {
        None
    }
}

fn cuda_nvcc(root: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "nvcc.exe" } else { "nvcc" };
    root.join("bin").join(exe)
}

#[cfg(windows)]
fn cuda_known_roots() -> Vec<PathBuf> {
    let base = PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA");
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // 版本目录倒序，新版本优先
    roots.sort();
    roots.reverse();
    roots.push(base);
    roots
}

#[cfg(not(windows))]
fn cuda_known_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/local/cuda")]
}

/// ROCm：环境变量 → 已知安装路径 → PATH 上的 hipcc（仅此三者）
fn probe_rocm() -> Probe {
    for var in ["ROCM_PATH", "ROCM_HOME"] {
        if let Some(root) = std::env::var_os(var).map(PathBuf::from) {
            if let Some(found) = rocm_probe_root(&root) {
                return found;
            }
        }
    }
    for root in rocm_known_roots() {
        if let Some(found) = rocm_probe_root(&root) {
            return found;
        }
    }
    if let Some(exe) = find_in_path("hipcc") {
        let version = probe_version(&exe, &["--version"]).unwrap_or_default();
        return Probe::Found { version, path: exe };
    }
    Probe::Missing
}

fn rocm_probe_root(root: &Path) -> Option<Probe> {
    let hipcc = rocm_hipcc(root);
    if hipcc.is_file() {
        let version = probe_version(&hipcc, &["--version"]).unwrap_or_default();
        return Some(Probe::Found {
            version,
            path: root.to_path_buf(),
        });
    }
    // 无 hipcc 时退回 ROCm 版本文件
    let version_file = root.join(".info").join("version");
    if version_file.is_file() {
        let version = std::fs::read_to_string(&version_file)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        return Some(Probe::Found {
            version,
            path: root.to_path_buf(),
        });
    }
    None
}

fn rocm_hipcc(root: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "hipcc.exe" } else { "hipcc" };
    root.join("bin").join(exe)
}

#[cfg(not(windows))]
fn rocm_known_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/opt/rocm")]
}

#[cfg(windows)]
fn rocm_known_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .map(|base| base.join("AMD").join("ROCm"))
        .filter(|p| p.exists())
        .collect()
}

/// PATH 搜索可执行文件；Windows 额外按 PATHEXT 逐个拼扩展名
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;

    #[cfg(windows)]
    let exts: Vec<String> = {
        let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut v: Vec<String> = raw
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if v.is_empty() {
            v.push(".EXE".to_string());
        }
        v
    };

    for dir in std::env::split_paths(&path_var) {
        #[cfg(windows)]
        {
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            for ext in &exts {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// PATH 找 name → 运行 `name version_args` → 组装 Probe
fn probe_tool(name: &str, version_args: &[&str]) -> Probe {
    match find_in_path(name) {
        Some(exe) => {
            let version = probe_version(&exe, version_args).unwrap_or_default();
            Probe::Found { version, path: exe }
        }
        None => Probe::Missing,
    }
}

/// 运行 `exe args`，返回首个非空输出行（stdout 优先，为空则回退 stderr）
fn probe_version(exe: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    // Windows：不弹控制台黑框
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    first_nonempty_line(&stdout).or_else(|| first_nonempty_line(&stderr))
}

fn first_nonempty_line(s: &str) -> Option<String> {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_include_required_core_tools() {
        let keys: Vec<Key> = requirements().iter().map(|r| r.key).collect();
        assert!(keys.contains(&Key::EnvCheckGit));
        assert!(keys.contains(&Key::EnvCheckCmake));
        assert!(keys.contains(&Key::EnvCheckCompiler));
        assert!(keys.contains(&Key::EnvCheckCuda));
        assert!(keys.contains(&Key::EnvCheckRocm));
        let git = requirements().into_iter().find(|r| r.key == Key::EnvCheckGit);
        assert!(git.expect("git 项应存在").required);
    }

    #[test]
    fn all_required_found_respects_required_flag() {
        let found = Probe::Found {
            version: "1.0".to_string(),
            path: PathBuf::from("x"),
        };
        let ok = vec![
            CheckResult {
                key: Key::EnvCheckGit,
                required: true,
                probe: found.clone(),
            },
            CheckResult {
                key: Key::EnvCheckCuda,
                required: false,
                probe: Probe::Missing,
            },
        ];
        assert!(all_required_found(&ok));

        let bad = vec![CheckResult {
            key: Key::EnvCheckGit,
            required: true,
            probe: Probe::Missing,
        }];
        assert!(!all_required_found(&bad));
    }

    #[test]
    fn missing_binary_is_not_found() {
        assert!(find_in_path("__env_check_no_such_binary_zzz__").is_none());
        assert_eq!(
            probe_tool("__env_check_no_such_binary_zzz__", &["--version"]),
            Probe::Missing
        );
    }

    #[test]
    fn first_nonempty_line_skips_blank() {
        assert_eq!(
            first_nonempty_line("\n\n  hello  \nworld\n"),
            Some("hello".to_string())
        );
        assert_eq!(first_nonempty_line("   \n\t\n"), None);
    }
}
