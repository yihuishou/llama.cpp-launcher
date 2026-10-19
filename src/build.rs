//! llama.cpp 源码获取 —— 后台执行 `git clone`（首次拉取）与 `git fetch/checkout`（更新），
//! 落到程序同级目录 `llama-source-code/`。
//!
//! 模式对齐 `downloader::DownloadHandle` / `env_check::EnvCheckHandle`：`Arc<AtomicBool>`
//! 取消标志 + `Arc<Mutex<Status>>` 共享状态 + worker 线程；UI 每帧 `snapshot()` 轮询，
//! 关闭时 `request_cancel()`。git 是长驻子进程，故另持 `Arc<Mutex<Option<Child>>>`，
//! 取消时直接 kill。
//!
//! 进度：给 git 传 `--progress`，独立线程抽干 stderr 并按 `\r` / `\n` 切段解析百分比
//! （git 用回车原地刷新进度），取已见最大值写入 [`CloneStatus::percent`]，UI 据此画进度条；
//! 抽干同时避免管道写满导致 git 阻塞，并保留失败信息。
//!
//! 克隆命令：`git clone --progress [--branch <ref>] <repo> <dest>`，`<ref>` 取 Tag（非空时
//! 优先）否则分支。目标目录固定为「程序所在目录 / `llama-source-code`」，已存在且非空则拒绝。
//! 更新在既有源码目录内执行 [`update_steps`]：有 Tag → `fetch --tags --force` + `checkout <tag>`；
//! 无 Tag → `checkout <branch>` + `pull --ff-only`。
//!
//! 日志：worker 把每条 git 命令与 stderr 输出追加进 [`BuildLog`]（VecDeque 环形缓冲，
//! 容量 [`MAX_LOG_LINES`]，与 server/rpc 日志同规格），供「llama.cpp 编译日志」面板展示；
//! 后续 CMake 配置/构建阶段复用同一缓冲。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::settings::{default_build_generator, AppSettings};
use crate::engine::server::{LogEntry, LogLevel, MAX_LOG_LINES};
use crate::env_check::{EnvCheckEntry, ID_CUDA, ID_NCCL, ID_RCCL, ID_ROCM};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::i18n::Key;

/// 源码目录名（位于程序同级目录下）
pub const SOURCE_DIR_NAME: &str = "llama-source-code";

/// 操作类型：决定结果文案（克隆 / 更新）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOp {
    /// 首次克隆
    Clone,
    /// 更新既有源码目录
    Update,
}

/// 克隆 / 更新状态
#[derive(Clone, Debug, PartialEq, Default)]
pub enum CloneState {
    /// 空闲（初始 / 已取消）
    #[default]
    Idle,
    /// 进行中
    Running,
    /// 成功，携带源码目录
    Done(PathBuf),
    /// 失败：英文错误串（UI 层拼 i18n 前缀）
    Failed(String),
}

/// 克隆 / 更新状态快照（UI 每帧轮询）
#[derive(Clone, Debug, Default)]
pub struct CloneStatus {
    pub state: CloneState,
    /// 进度百分比（0..=100，来自 git `--progress` 输出；尚未解析出时为 None）
    pub percent: Option<u8>,
    /// 当前 / 最近一次操作（决定进度与结果的文案）
    pub op: Option<SourceOp>,
}

/// 构建日志缓冲（克隆/更新与 CMake 配置/构建共用一份）
///
/// 内部为 VecDeque 环形缓冲，容量 [`MAX_LOG_LINES`]，超限丢弃最旧行（同 server/rpc）；
/// 克隆句柄与编译句柄各持一个克隆，指向同一缓冲，因此「编译日志」面板能看到两条链路的输出。
#[derive(Clone, Default)]
pub struct BuildLog {
    lines: Arc<Mutex<VecDeque<LogEntry>>>,
}

impl BuildLog {
    /// 追加一行日志；超出容量时丢弃最旧行
    pub fn push(&self, level: LogLevel, text: impl Into<String>) {
        let text = text.into();
        let mut lines = self.lines.lock().unwrap();
        if lines.len() >= MAX_LOG_LINES {
            lines.pop_front();
        }
        lines.push_back(LogEntry { text, level });
    }

    /// 拍摄当前日志快照（UI 每帧轮询）
    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }

    /// 清空日志
    pub fn clear(&self) {
        self.lines.lock().unwrap().clear();
    }
}

/// 后台克隆 / 更新控制句柄（模式对齐 downloader::DownloadHandle）
pub struct CloneHandle {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<CloneStatus>>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
    log: BuildLog,
}

impl Default for CloneHandle {
    fn default() -> Self {
        Self::new(BuildLog::default())
    }
}

impl CloneHandle {
    /// `log` 与 [`BuildHandle`] 共享同一缓冲时，两个面板看到同一份构建日志
    pub fn new(log: BuildLog) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(CloneStatus::default())),
            child: Arc::new(Mutex::new(None)),
            worker: Arc::new(Mutex::new(None)),
            log,
        }
    }

    /// 构建日志缓冲（与 [`BuildHandle`] 共享同一缓冲，日志面板据此渲染）
    pub fn log(&self) -> &BuildLog {
        &self.log
    }

    /// 启动后台克隆；进行中时忽略（防重复点击）
    pub fn start_clone(&self, repo: String, branch: String, tag: String) {
        self.spawn_worker(SourceOp::Clone, repo, branch, tag);
    }

    /// 启动后台更新（在既有源码目录内切到指定 Tag / 分支）
    pub fn start_update(&self, branch: String, tag: String) {
        self.spawn_worker(SourceOp::Update, String::new(), branch, tag);
    }

    fn spawn_worker(&self, op: SourceOp, repo: String, branch: String, tag: String) {
        if self.is_busy() {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        {
            let mut st = self.status.lock().unwrap();
            st.state = CloneState::Running;
            st.percent = None;
            st.op = Some(op);
        }

        let cancel = Arc::clone(&self.cancel);
        let status = Arc::clone(&self.status);
        let child = Arc::clone(&self.child);
        let log = self.log.clone();
        let handle = thread::spawn(move || {
            let state = match op {
                SourceOp::Clone => run_clone(&cancel, &child, &status, &log, &repo, &branch, &tag),
                SourceOp::Update => run_update(&cancel, &child, &status, &log, &branch, &tag),
            };
            let mut st = status.lock().unwrap();
            // 被取消则回到 Idle，而非展示半份结果
            st.state = if cancel.load(Ordering::SeqCst) {
                CloneState::Idle
            } else {
                state
            };
            st.percent = None;
        });
        *self.worker.lock().unwrap() = Some(handle);
    }

    /// 返回当前状态快照
    pub fn snapshot(&self) -> CloneStatus {
        self.status.lock().unwrap().clone()
    }

    /// 是否正在克隆 / 更新（UI 禁用按钮用）
    pub fn is_busy(&self) -> bool {
        matches!(self.snapshot().state, CloneState::Running)
    }

    /// Drop / 关窗调用：置取消标志并结束子进程树
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                kill_tree(child);
            }
        }
    }
}

impl Drop for CloneHandle {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

/// 程序所在目录下的源码目录（程序同级 `llama-source-code/`）
pub fn source_code_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(SOURCE_DIR_NAME))
}

/// 克隆 / 更新共用的 ref 选择：Tag 非空优先，否则分支
fn resolve_ref<'a>(branch: &'a str, tag: &'a str) -> &'a str {
    if tag.trim().is_empty() {
        branch.trim()
    } else {
        tag.trim()
    }
}

/// 组装 `git clone` 参数：Tag 非空按 Tag 克隆（优先于分支），否则按分支。
/// `--progress` 强制 git 在 stderr 非终端时也输出进度。
fn clone_args(repo: &str, branch: &str, tag: &str, dest: &Path) -> Vec<String> {
    let mut args = vec!["clone".to_string(), "--progress".to_string()];
    let ref_name = resolve_ref(branch, tag);
    if !ref_name.is_empty() {
        args.push("--branch".to_string());
        args.push(ref_name.to_string());
    }
    args.push(repo.trim().to_string());
    args.push(dest.to_string_lossy().into_owned());
    args
}

/// 组装「更新」要依次执行的 git 参数：有 Tag → 取回标签并切到该 tag；无 Tag → 切到分支并快进拉取
fn update_steps(dest: &Path, branch: &str, tag: &str) -> Vec<Vec<String>> {
    let dir = dest.to_string_lossy().into_owned();
    let git_in = |rest: &[&str]| {
        let mut v = vec!["-C".to_string(), dir.clone()];
        v.extend(rest.iter().map(|s| s.to_string()));
        v
    };

    let tag = tag.trim();
    if tag.is_empty() {
        let mut steps = Vec::new();
        if !branch.trim().is_empty() {
            steps.push(git_in(&["checkout", branch.trim()]));
        }
        steps.push(git_in(&["pull", "--ff-only"]));
        steps
    } else {
        vec![
            git_in(&["fetch", "--tags", "--force"]),
            git_in(&["checkout", tag]),
        ]
    }
}

/// 从 git 进度行里取 `%` 前的数字（如 `Receiving objects:  37% (…)` → 37）
fn parse_percent(line: &str) -> Option<u8> {
    let idx = line.find('%')?;
    let digits: String = line[..idx]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse::<u8>().ok().filter(|&v| v <= 100)
}

/// 准备目标目录：不存在则创建；已存在且非空则返回错误串。
fn prepare_dir(dest: &Path) -> Option<String> {
    if dest.exists() {
        let nonempty = std::fs::read_dir(dest)
            .map(|mut it| it.next().is_some())
            .unwrap_or(true);
        if nonempty {
            return Some(format!(
                "destination already exists and is not empty: {}",
                dest.display()
            ));
        }
        None
    } else {
        std::fs::create_dir_all(dest)
            .err()
            .map(|e| format!("cannot create {}: {}", dest.display(), e))
    }
}

/// 首次克隆：准备目录 → `git clone` → 终态
fn run_clone(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    log: &BuildLog,
    repo: &str,
    branch: &str,
    tag: &str,
) -> CloneState {
    let Some(dest) = source_code_dir() else {
        return failed(log, "cannot resolve program directory".to_string());
    };
    if let Some(msg) = prepare_dir(&dest) {
        return failed(log, msg);
    }

    let steps = [clone_args(repo, branch, tag, &dest)];
    run_steps(cancel, child_slot, status, log, steps, &dest)
}

/// 更新：校验既有源码目录 → 依次执行 `update_steps` → 终态
fn run_update(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    log: &BuildLog,
    branch: &str,
    tag: &str,
) -> CloneState {
    let Some(dest) = source_code_dir() else {
        return failed(log, "cannot resolve program directory".to_string());
    };
    if !dest.join(".git").exists() {
        return failed(
            log,
            format!("not a git repository (clone first): {}", dest.display()),
        );
    }

    let steps = update_steps(&dest, branch, tag);
    run_steps(cancel, child_slot, status, log, steps, &dest)
}

/// 记一条错误日志并生成失败终态（避免每个失败点重复两行）
fn failed(log: &BuildLog, msg: String) -> CloneState {
    log.push(LogLevel::Error, msg.clone());
    CloneState::Failed(msg)
}

/// 依次执行若干 git 命令，任一失败即中止；全部成功则返回 `Done(dest)`
fn run_steps(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    log: &BuildLog,
    steps: impl IntoIterator<Item = Vec<String>>,
    dest: &Path,
) -> CloneState {
    for args in steps {
        match run_git(cancel, child_slot, status, log, &args) {
            Ok((exit, _)) if exit.success() => {}
            Ok((exit, text)) => return failed(log, describe_failure(&text, &exit)),
            Err(e) => return failed(log, e),
        }
    }
    CloneState::Done(dest.to_path_buf())
}

/// 跑一条 git 命令并等待结束，返回 (退出状态, stderr 文本)
fn run_git(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    log: &BuildLog,
    args: &[String],
) -> Result<(ExitStatus, String), String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start git: {}", e))?;

    log.push(LogLevel::Info, format!("$ git {}", args.join(" ")));

    // 独立线程抽干 stderr：逐段解析进度（顺带防止管道写满阻塞 git）并留存失败信息
    let stderr_reader = child.stderr.take().map(|pipe| {
        let status = Arc::clone(status);
        let log = log.clone();
        thread::spawn(move || read_progress(pipe, &status, &log))
    });

    *child_slot.lock().unwrap() = Some(child);

    let waited = wait_for_exit(cancel, child_slot);

    // 收尾：清空子进程槽
    if let Ok(mut slot) = child_slot.lock() {
        *slot = None;
    }
    let stderr_text = stderr_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    waited.map(|exit| (exit, stderr_text))
}

/// 读取 git stderr：按 `\r` / `\n` 切段，逐段解析百分比并刷新进度；返回完整文本供失败诊断
fn read_progress(pipe: impl Read, status: &Arc<Mutex<CloneStatus>>, log: &BuildLog) -> String {
    let mut reader = BufReader::new(pipe);
    let mut buf = [0u8; 4096];
    let mut pending: Vec<u8> = Vec::new();
    let mut all: Vec<u8> = Vec::new();
    let mut max_percent: Option<u8> = None;

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&buf[..n]);
        while let Some(idx) = pending.iter().position(|&b| b == b'\r' || b == b'\n') {
            let mut segment: Vec<u8> = pending.drain(..=idx).collect();
            segment.pop(); // 去掉分隔符
            record_segment(&segment, &mut all, &mut max_percent, status, log);
        }
    }
    record_segment(&pending, &mut all, &mut max_percent, status, log);

    String::from_utf8_lossy(&all).into_owned()
}

/// 记录一段 stderr：追加构建日志 + 累积文本 + 取更大百分比写入共享状态
fn record_segment(
    segment: &[u8],
    all: &mut Vec<u8>,
    max_percent: &mut Option<u8>,
    status: &Arc<Mutex<CloneStatus>>,
    log: &BuildLog,
) {
    if segment.is_empty() {
        return;
    }
    all.extend_from_slice(segment);
    all.push(b'\n');

    let text = String::from_utf8_lossy(segment);
    log.push(LogLevel::Info, text.to_string());
    if let Some(pct) = parse_percent(&text) {
        if max_percent.is_none_or(|m| pct > m) {
            *max_percent = Some(pct);
            if let Ok(mut st) = status.lock() {
                st.percent = Some(pct);
            }
        }
    }
}

/// 轮询等待子进程退出；取消时 kill 之
fn wait_for_exit(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<ExitStatus, String> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            if let Ok(mut slot) = child_slot.lock() {
                if let Some(child) = slot.as_mut() {
                    kill_tree(child);
                }
            }
        }
        let waited = {
            let mut slot = child_slot
                .lock()
                .map_err(|_| "process state poisoned".to_string())?;
            slot.as_mut()
                .ok_or_else(|| "process lost".to_string())?
                .try_wait()
        };
        match waited {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(format!("process wait failed: {}", e)),
        }
    }
}

/// 结束子进程及其子孙进程（CMake → Ninja → 编译器链 / git 的辅助进程）。
/// Windows 上 `Child::kill` 只杀直接子进程，忍者与编译器会残留并继续占 CPU，
/// 故优先用 `taskkill /T /F` 整树终止；其他平台退回 kill。
fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
}

/// 失败时取 stderr 最后一行非空文本，兜底用退出码
fn describe_failure(stderr: &str, status: &ExitStatus) -> String {
    last_nonempty_line(stderr).unwrap_or_else(|| format!("git exited with {}", status))
}

fn last_nonempty_line(s: &str) -> Option<String> {
    s.lines()
        .map(str::trim)
        .rev()
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// CMake 编译开关所属分组（决定面板上的分区与可用性门控）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionGroup {
    /// 与后端无关（通用构建开关），始终可用
    General,
    /// 需要 CUDA 工具包（`nvcc`）
    Cuda,
    /// 需要 ROCm 工具包（`hipcc`）
    Hip,
    /// 与后端无关（网页客户端资源），始终可用
    Ui,
}

/// 与后端无关的通用编译开关：(`-D` 后面的参数, 说明 i18n 键)。
/// 部分选项上游默认为 ON，故此处显式固定为 `=OFF`（勾选即关闭该特性）；
/// `GGML_CUDA_FA_ALL_QUANTS` 是 CUDA / ROCm / MUSA 共用的开关（`ggml_cuda_fattn_vec_instances`
/// 由三个后端共同调用），故放通用组；界面保留上游旧名，拼参时改名为 `GGML_CUDA_FA_QUANTS=all`
pub const CMAKE_OPTIONS_GENERAL: &[(&str, Key)] = &[
    ("GGML_RPC", Key::BuildOptGgmlRpc),
    ("GGML_CUDA_FA_ALL_QUANTS", Key::BuildOptCudaFaAllQuants),
    ("LLAMA_BUILD_TESTS=OFF", Key::BuildOptLlamaBuildTests),
    ("LLAMA_BUILD_EXAMPLES=OFF", Key::BuildOptLlamaBuildExamples),
    ("LLAMA_BUILD_APP=OFF", Key::BuildOptLlamaBuildApp),
    ("LLAMA_OPENSSL=OFF", Key::BuildOptLlamaOpenssl),
];

/// CUDA 工具包相关的编译开关：(`-D` 后面的参数, 说明 i18n 键)
pub const CMAKE_OPTIONS_CUDA: &[(&str, Key)] = &[
    ("GGML_CUDA", Key::BuildOptCuda),
    ("GGML_CUDA_GRAPHS", Key::BuildOptCudaGraphs),
    ("GGML_CUDA_FORCE_MMQ", Key::BuildOptCudaForceMmq),
    ("GGML_CUDA_FORCE_CUBLAS", Key::BuildOptCudaForceCublas),
    ("GGML_CUDA_NO_VMM", Key::BuildOptCudaNoVmm),
    ("GGML_CUDA_NO_PEER_COPY", Key::BuildOptCudaNoPeerCopy),
    ("GGML_CUDA_NCCL", Key::BuildOptCudaNccl),
    ("GGML_CUDA_DEBUG", Key::BuildOptCudaDebug),
];

/// ROCm 工具包相关的编译开关：(`-D` 后面的参数, 说明 i18n 键)
pub const CMAKE_OPTIONS_HIP: &[(&str, Key)] = &[
    ("GGML_HIP", Key::BuildOptHip),
    ("GGML_HIP_RCCL", Key::BuildOptHipRccl),
    ("GGML_HIP_GRAPHS", Key::BuildOptHipGraphs),
    ("GGML_HIP_NO_VMM", Key::BuildOptHipNoVmm),
    ("GGML_HIP_EXPORT_METRICS", Key::BuildOptHipExportMetrics),
];

/// 网页客户端资源相关的编译开关：(`-D` 后面的参数, 说明 i18n 键)。
/// `LLAMA_USE_PREBUILT_UI` 上游默认为 ON，故此处显式固定为 `=OFF`（勾选即关闭 HF 下载）
pub const CMAKE_OPTIONS_UI: &[(&str, Key)] = &[
    ("LLAMA_BUILD_UI", Key::BuildOptLlamaBuildUi),
    ("LLAMA_USE_PREBUILT_UI=OFF", Key::BuildOptLlamaUsePrebuiltUi),
];

/// 某分组下的全部开关；参数不含 `=` 的按布尔项处理（勾选 → 补 `=ON`）
pub fn cmake_options(group: OptionGroup) -> &'static [(&'static str, Key)] {
    match group {
        OptionGroup::General => CMAKE_OPTIONS_GENERAL,
        OptionGroup::Cuda => CMAKE_OPTIONS_CUDA,
        OptionGroup::Hip => CMAKE_OPTIONS_HIP,
        OptionGroup::Ui => CMAKE_OPTIONS_UI,
    }
}

/// 全部开关参数（通用组 → CUDA 组 → ROCm 组 → 网页客户端组）
fn all_cmake_options() -> impl Iterator<Item = &'static str> {
    CMAKE_OPTIONS_GENERAL
        .iter()
        .chain(CMAKE_OPTIONS_CUDA)
        .chain(CMAKE_OPTIONS_HIP)
        .chain(CMAKE_OPTIONS_UI)
        .map(|(option, _)| *option)
}

/// 指定开关是否已勾选
pub fn is_option_selected(selected: &[String], option: &str) -> bool {
    selected.iter().any(|e| e == option)
}

/// 勾选 / 取消勾选一个开关（重复勾选不会重复入列）
pub fn set_option_selected(selected: &mut Vec<String>, option: &str, on: bool) {
    if on {
        if !is_option_selected(selected, option) {
            selected.push(option.to_string());
        }
    } else {
        selected.retain(|e| e != option);
    }
}

/// 开关清单里的 spec 去掉 `=值` 后缀后的选项名，用于界面显示
pub fn option_label(spec: &str) -> &str {
    spec.split_once('=').map_or(spec, |(name, _)| name)
}

/// spec 是否用固定值关掉了某个特性（如 `LLAMA_OPENSSL=OFF`）：勾选即「禁用」
pub fn option_is_disable(spec: &str) -> bool {
    spec.split_once('=')
        .is_some_and(|(_, value)| value.eq_ignore_ascii_case("OFF"))
}

/// 上游已废弃的 spec 名：界面沿用旧名（用户按旧名辨识），拼进命令行时换成新名
const RENAMED_SPECS: &[(&str, &str)] = &[("GGML_CUDA_FA_ALL_QUANTS", "GGML_CUDA_FA_QUANTS=all")];

/// 单个 spec 对应的 CMake 参数：布尔项补 `=ON`，含 `=` 的原样，废弃名换成新名
fn cmake_arg(spec: &str) -> String {
    let spec = RENAMED_SPECS
        .iter()
        .find(|(old, _)| *old == spec)
        .map_or(spec, |(_, new)| *new);
    if spec.contains('=') {
        format!("-D{}", spec)
    } else {
        format!("-D{}=ON", spec)
    }
}

/// 把勾选的开关按清单顺序拼成 CMake 参数（清单外的名字被忽略）
pub fn selected_cmake_args(selected: &[String]) -> Vec<String> {
    all_cmake_options()
        .filter(|option| is_option_selected(selected, option))
        .map(cmake_arg)
        .collect()
}

/// 后端分组与其依赖的检测项 id：对应工具包缺失时整组取消勾选
const BACKEND_GROUPS: &[(OptionGroup, &str)] =
    &[(OptionGroup::Cuda, ID_CUDA), (OptionGroup::Hip, ID_ROCM)];

/// 加速库开关与其依赖的检测项 id
const DETECTED_OPTIONS: &[(&str, &str)] =
    &[("GGML_CUDA_NCCL", ID_NCCL), ("GGML_HIP_RCCL", ID_RCCL)];

/// 按环境检测结果校正编译开关的勾选。
///
/// CUDA / ROCm 工具包缺失时取消该组勾选：分组在界面上已被隐藏，留着勾选仍会拼出
/// `-DGGML_CUDA=ON` 让 CMake 配置阶段直接失败。NCCL / RCCL 则完全跟随检测结果
/// （装了勾上、没装取消）。未出现在 `entries` 里的检测项不参与同步——Python 不可用时
/// RCCL / NCCL 根本不会被探测，此时无从判断，保留用户当前选择。
pub fn sync_options_with_env(selected: &mut Vec<String>, entries: &[EnvCheckEntry]) {
    let detection = |id: &str| {
        entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.found)
    };

    for &(group, id) in BACKEND_GROUPS {
        if detection(id) == Some(false) {
            for &(option, _) in cmake_options(group) {
                set_option_selected(selected, option, false);
            }
        }
    }

    for &(option, id) in DETECTED_OPTIONS {
        if let Some(found) = detection(id) {
            set_option_selected(selected, option, found);
        }
    }
}

fn resolve_source_dir(settings: &AppSettings) -> String {
    let configured = settings.build_work_dir.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    source_code_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|| SOURCE_DIR_NAME.to_string())
}

/// 源码目录是否已就绪（存在且非空）：与 [`cmake_command_preview`] 的 `-S` 同源解析，
/// 保证按钮可用性判定与真正参与编译的目录一致。
pub fn source_dir_ready(settings: &AppSettings) -> bool {
    std::fs::read_dir(resolve_source_dir(settings))
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

fn quote_if_needed(value: &str) -> String {
    if value.contains(' ') {
        format!("\"{}\"", value)
    } else {
        value.to_string()
    }
}

/// 「开始编译」实际执行的两条命令：`[配置, 构建]`。参数保持未加引号的原始值，直接交给
/// [`Command::args`]；预览文本由 [`cmake_command_preview`] 按需加引号渲染，两者同源，
/// 避免预览与真正执行的参数漂移。
/// 勾选的开关复用 [`selected_cmake_args`]，保证预览与实际构建参数同源。
/// 生成器恒拼 `-G Ninja`（`build_generator` 留空时回退默认值），不交给 CMake 自动选。
pub fn cmake_commands(settings: &AppSettings) -> [Vec<String>; 2] {
    let source = resolve_source_dir(settings);
    let build_dir = {
        let configured = settings.build_output_dir.trim();
        if configured.is_empty() {
            Path::new(&source).join("build").display().to_string()
        } else {
            configured.to_string()
        }
    };

    let mut configure = vec![
        "cmake".to_string(),
        "-S".to_string(),
        source,
        "-B".to_string(),
        build_dir.clone(),
    ];
    let generator = {
        let configured = settings.build_generator.trim();
        if configured.is_empty() {
            default_build_generator()
        } else {
            configured.to_string()
        }
    };
    configure.push("-G".to_string());
    configure.push(generator);
    let build_type = settings.build_type.trim();
    if !build_type.is_empty() {
        configure.push(format!("-DCMAKE_BUILD_TYPE={}", build_type));
    }
    configure.extend(selected_cmake_args(&settings.build_cmake_options));
    configure.extend(
        settings
            .build_cmake_args
            .split_whitespace()
            .map(str::to_string),
    );

    let mut build = vec!["cmake".to_string(), "--build".to_string(), build_dir];
    if !build_type.is_empty() {
        build.push("--config".to_string());
        build.push(build_type.to_string());
    }
    let targets: Vec<&str> = settings.build_targets.split_whitespace().collect();
    if !targets.is_empty() {
        build.push("--target".to_string());
        build.extend(targets.into_iter().map(str::to_string));
    }
    if settings.build_jobs > 0 {
        build.push("-j".to_string());
        build.push(settings.build_jobs.to_string());
    }

    [configure, build]
}

/// 「开始编译」的命令预览：路径含空格时加引号，便于直接粘到终端；
/// 内容与 [`cmake_commands`] 同源，首行为配置步骤，次行为构建步骤。
pub fn cmake_command_preview(settings: &AppSettings) -> String {
    cmake_commands(settings)
        .iter()
        .map(|args| {
            args.iter()
                .map(|arg| quote_if_needed(arg))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 编译阶段：配置（`cmake -S/-B` 生成构建系统）→ 编译（`cmake --build`）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPhase {
    Configure,
    Compile,
}

/// 编译状态
#[derive(Clone, Debug, PartialEq, Default)]
pub enum BuildState {
    /// 空闲（初始 / 被取消）
    #[default]
    Idle,
    /// 进行中
    Running,
    /// 配置与编译均成功
    Done,
    /// 失败：英文错误串（UI 层拼 i18n 前缀）
    Failed(String),
}

/// 编译状态快照（UI 每帧轮询）
#[derive(Clone, Debug, Default)]
pub struct BuildStatus {
    pub state: BuildState,
    /// 进行中的阶段；非进行中为 None
    pub phase: Option<BuildPhase>,
}

/// 后台编译控制句柄（Handle 模式，对齐 [`CloneHandle`]）
pub struct BuildHandle {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<BuildStatus>>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
    log: BuildLog,
}

impl Default for BuildHandle {
    fn default() -> Self {
        Self::new(BuildLog::default())
    }
}

impl BuildHandle {
    pub fn new(log: BuildLog) -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(BuildStatus::default())),
            child: Arc::new(Mutex::new(None)),
            worker: Arc::new(Mutex::new(None)),
            log,
        }
    }

    /// 启动后台编译；进行中时忽略（防重复点击）。
    /// 设置在此刻快照，构建过程中改界面不影响本次命令。
    pub fn start(&self, settings: &AppSettings) {
        if self.is_busy() {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        {
            let mut st = self.status.lock().unwrap();
            st.state = BuildState::Running;
            st.phase = Some(BuildPhase::Configure);
        }

        let cancel = Arc::clone(&self.cancel);
        let status = Arc::clone(&self.status);
        let child = Arc::clone(&self.child);
        let log = self.log.clone();
        let settings = settings.clone();
        let handle = thread::spawn(move || {
            let state = run_build(&cancel, &child, &status, &log, &settings);
            let mut st = status.lock().unwrap();
            // 被取消则回到 Idle，而非展示半份结果
            st.state = if cancel.load(Ordering::SeqCst) {
                BuildState::Idle
            } else {
                state
            };
            st.phase = None;
        });
        *self.worker.lock().unwrap() = Some(handle);
    }

    /// 返回当前状态快照
    pub fn snapshot(&self) -> BuildStatus {
        self.status.lock().unwrap().clone()
    }

    /// 构建日志缓冲（与 [`CloneHandle`] 共享同一缓冲）
    pub fn log(&self) -> &BuildLog {
        &self.log
    }

    /// 是否正在编译（UI 禁用按钮用）
    pub fn is_busy(&self) -> bool {
        matches!(self.snapshot().state, BuildState::Running)
    }

    /// Drop / 关窗调用：置取消标志并结束整棵编译进程树
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                kill_tree(child);
            }
        }
    }
}

impl Drop for BuildHandle {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

/// 依次执行「配置」「编译」两条命令；任一失败即中止
fn run_build(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<BuildStatus>>,
    log: &BuildLog,
    settings: &AppSettings,
) -> BuildState {
    let phases = [BuildPhase::Configure, BuildPhase::Compile];
    for (args, phase) in cmake_commands(settings).iter().zip(phases) {
        if let Ok(mut st) = status.lock() {
            st.phase = Some(phase);
        }
        if let Err(msg) = run_step(cancel, child_slot, log, args) {
            if cancel.load(Ordering::SeqCst) {
                return BuildState::Idle;
            }
            log.push(LogLevel::Error, msg.clone());
            return BuildState::Failed(msg);
        }
        if cancel.load(Ordering::SeqCst) {
            return BuildState::Idle;
        }
    }
    BuildState::Done
}

/// 跑一条命令：stdout / stderr 逐行进构建日志，非零退出返回错误串
fn run_step(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    log: &BuildLog,
    args: &[String],
) -> Result<(), String> {
    let Some((program, program_args)) = args.split_first() else {
        return Err("empty build command".to_string());
    };
    log.push(LogLevel::Info, format!("$ {}", args.join(" ")));

    let mut cmd = Command::new(program);
    cmd.args(program_args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {}: {}", program, e))?;

    // 两路输出各起一个线程抽干：ninja 的编译进度在 stdout，CMake 警告/错误在 stderr
    let mut readers = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        let log = log.clone();
        readers.push(thread::spawn(move || stream_lines(pipe, &log)));
    }
    if let Some(pipe) = child.stderr.take() {
        let log = log.clone();
        readers.push(thread::spawn(move || stream_lines(pipe, &log)));
    }

    *child_slot.lock().unwrap() = Some(child);
    let waited = wait_for_exit(cancel, child_slot);
    if let Ok(mut slot) = child_slot.lock() {
        *slot = None;
    }
    for reader in readers {
        let _ = reader.join();
    }

    let exit = waited?;
    if exit.success() {
        Ok(())
    } else {
        Err(format!("{} failed ({})", program, exit))
    }
}

/// 逐行读取子进程输出写入构建日志（读取线程同时负责排空管道，避免写满阻塞）
fn stream_lines(pipe: impl Read, log: &BuildLog) {
    let mut reader = BufReader::new(pipe);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let text = String::from_utf8_lossy(&buf);
                let line = text.trim_end_matches(['\r', '\n']);
                if !line.is_empty() {
                    log.push(LogLevel::Info, line.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_log_keeps_order_and_caps_at_max_lines() {
        let log = BuildLog::default();
        log.push(LogLevel::Info, "$ git clone --progress repo dest");
        log.push(LogLevel::Warn, "warning: something");

        let lines = log.snapshot();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "$ git clone --progress repo dest");
        assert_eq!(lines[1].level, LogLevel::Warn);

        for i in 0..MAX_LOG_LINES {
            log.push(LogLevel::Info, format!("line {i}"));
        }
        let lines = log.snapshot();
        assert_eq!(lines.len(), MAX_LOG_LINES);
        assert_eq!(lines[0].text, "line 0");

        log.clear();
        assert!(log.snapshot().is_empty());
    }

    #[test]
    fn selected_cmake_args_follows_catalog_order_and_skips_unknown() {
        let selected = vec![
            "GGML_HIP".to_string(),
            "LLAMA_USE_PREBUILT_UI=OFF".to_string(),
            "GGML_NOT_A_REAL_OPTION".to_string(),
            "LLAMA_BUILD_TESTS=OFF".to_string(),
            "GGML_CUDA".to_string(),
            "GGML_RPC".to_string(),
        ];
        assert_eq!(
            selected_cmake_args(&selected),
            [
                "-DGGML_RPC=ON",
                "-DLLAMA_BUILD_TESTS=OFF",
                "-DGGML_CUDA=ON",
                "-DGGML_HIP=ON",
                "-DLLAMA_USE_PREBUILT_UI=OFF"
            ]
        );
        assert!(selected_cmake_args(&[]).is_empty());
    }

    #[test]
    fn set_option_selected_avoids_duplicates() {
        let mut selected = Vec::new();
        set_option_selected(&mut selected, "GGML_CUDA", true);
        set_option_selected(&mut selected, "GGML_CUDA", true);
        assert_eq!(selected, ["GGML_CUDA"]);
        assert!(is_option_selected(&selected, "GGML_CUDA"));

        set_option_selected(&mut selected, "GGML_CUDA", false);
        assert!(selected.is_empty());
        assert!(!is_option_selected(&selected, "GGML_CUDA"));
    }

    #[test]
    fn option_is_disable_detects_pinned_off_specs() {
        for spec in [
            "LLAMA_BUILD_TESTS=OFF",
            "LLAMA_BUILD_EXAMPLES=OFF",
            "LLAMA_BUILD_APP=OFF",
            "LLAMA_OPENSSL=OFF",
            "LLAMA_USE_PREBUILT_UI=OFF",
        ] {
            assert!(option_is_disable(spec), "{spec} 应判定为禁用型");
        }

        assert!(!option_is_disable("GGML_RPC"));
        assert!(!option_is_disable("GGML_CUDA"));
    }

    #[test]
    fn cmake_option_names_are_unique_and_every_group_populated() {
        assert!(!CMAKE_OPTIONS_GENERAL.is_empty());
        assert!(!CMAKE_OPTIONS_CUDA.is_empty());
        assert!(!CMAKE_OPTIONS_HIP.is_empty());
        assert!(!CMAKE_OPTIONS_UI.is_empty());

        let mut names: Vec<&str> = all_cmake_options().collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    #[test]
    fn clone_args_uses_branch_when_no_tag() {
        let args = clone_args("https://x/y.git", "master", "", Path::new("C:/d"));
        assert_eq!(
            args,
            [
                "clone",
                "--progress",
                "--branch",
                "master",
                "https://x/y.git",
                "C:/d"
            ]
        );
    }

    #[test]
    fn clone_args_prefers_tag_over_branch() {
        let args = clone_args("https://x/y.git", "master", "b4000", Path::new("C:/d"));
        assert_eq!(
            args,
            [
                "clone",
                "--progress",
                "--branch",
                "b4000",
                "https://x/y.git",
                "C:/d"
            ]
        );
    }

    #[test]
    fn clone_args_trims_and_omits_blank_ref() {
        let args = clone_args("  https://x/y.git  ", "", "   ", Path::new("C:/d"));
        assert_eq!(args, ["clone", "--progress", "https://x/y.git", "C:/d"]);
    }

    #[test]
    fn update_steps_fetch_tags_then_checkout_tag() {
        let steps = update_steps(Path::new("C:/d"), "master", "b4000");
        assert_eq!(
            steps,
            [
                vec!["-C", "C:/d", "fetch", "--tags", "--force"],
                vec!["-C", "C:/d", "checkout", "b4000"],
            ]
        );
    }

    #[test]
    fn update_steps_checkout_branch_then_ff_pull_when_no_tag() {
        let steps = update_steps(Path::new("C:/d"), " dev ", "  ");
        assert_eq!(
            steps,
            [
                vec!["-C", "C:/d", "checkout", "dev"],
                vec!["-C", "C:/d", "pull", "--ff-only"],
            ]
        );
    }

    #[test]
    fn update_steps_only_pull_when_both_blank() {
        let steps = update_steps(Path::new("C:/d"), "", "");
        assert_eq!(steps, [vec!["-C", "C:/d", "pull", "--ff-only"]]);
    }

    #[test]
    fn parse_percent_reads_git_progress_lines() {
        assert_eq!(
            parse_percent("Receiving objects:  37% (370/1000), 1.2 MiB | 1.5 MiB/s"),
            Some(37)
        );
        assert_eq!(
            parse_percent("Resolving deltas: 100% (200/200), done."),
            Some(100)
        );
        assert_eq!(
            parse_percent("remote: Counting objects:   5% (5/100)"),
            Some(5)
        );
        assert_eq!(parse_percent("Cloning into 'llama-source-code'..."), None);
        assert_eq!(parse_percent(""), None);
    }

    #[test]
    fn source_code_dir_is_named_llama_source_code() {
        let dir = source_code_dir().expect("exe 路径应可解析");
        assert_eq!(dir.file_name().unwrap().to_string_lossy(), SOURCE_DIR_NAME);
    }

    #[test]
    fn handle_starts_idle_and_not_busy() {
        let handle = CloneHandle::default();
        assert!(!handle.is_busy());
        assert_eq!(handle.snapshot().state, CloneState::Idle);
        assert_eq!(handle.snapshot().percent, None);
        assert_eq!(handle.snapshot().op, None);
    }

    #[test]
    fn build_handle_starts_idle_and_shares_clone_log() {
        let log = BuildLog::default();
        let clone = CloneHandle::new(log.clone());
        let build = BuildHandle::new(log);

        assert!(!build.is_busy());
        assert_eq!(build.snapshot().state, BuildState::Idle);
        assert_eq!(build.snapshot().phase, None);

        clone.log().push(LogLevel::Info, "shared line");
        let lines = build.log().snapshot();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "shared line");
    }

    #[test]
    fn option_label_strips_value_suffix() {
        assert_eq!(option_label("LLAMA_BUILD_TESTS=OFF"), "LLAMA_BUILD_TESTS");
        assert_eq!(option_label("GGML_CUDA=ON"), "GGML_CUDA");
        assert_eq!(option_label("GGML_RPC"), "GGML_RPC");
    }

    #[test]
    fn selected_cmake_args_renames_deprecated_spec_on_emit_only() {
        let selected = vec!["GGML_CUDA_FA_ALL_QUANTS".to_string()];

        assert_eq!(
            selected_cmake_args(&selected),
            vec!["-DGGML_CUDA_FA_QUANTS=all"]
        );
        assert_eq!(
            option_label("GGML_CUDA_FA_ALL_QUANTS"),
            "GGML_CUDA_FA_ALL_QUANTS"
        );
    }

    #[test]
    fn cmake_preview_shows_configure_then_build_with_selected_options() {
        let preview = cmake_command_preview(&AppSettings::default());
        let mut lines = preview.lines();

        let configure = lines.next().expect("缺少配置行");
        assert!(configure.starts_with("cmake -S "));
        assert!(configure.contains("-G Ninja"));
        assert!(configure.contains("-DCMAKE_BUILD_TYPE=Release"));
        assert!(configure.contains("-DGGML_RPC=ON"));
        assert!(configure.contains("-DLLAMA_BUILD_TESTS=OFF"));

        let build = lines.next().expect("缺少构建行");
        assert!(build.starts_with("cmake --build "));
        assert!(build.contains("--config Release"));
        assert_eq!(lines.count(), 0, "只应有配置与构建两行");
    }

    #[test]
    fn cmake_preview_quotes_spaced_values_and_appends_extra_args() {
        let settings = AppSettings {
            build_work_dir: "C:/my source/llama".to_string(),
            build_generator: "Visual Studio 17 2022".to_string(),
            build_cmake_args: "-DLLAMA_CURL=OFF -DFOO=1".to_string(),
            build_targets: "llama-server ggml-rpc-server".to_string(),
            build_jobs: 8,
            ..AppSettings::default()
        };

        let preview = cmake_command_preview(&settings);
        let build_dir = Path::new("C:/my source/llama").join("build");

        assert!(preview.contains("-S \"C:/my source/llama\""));
        assert!(preview.contains(&format!("-B \"{}\"", build_dir.display())));
        assert!(preview.contains("-G \"Visual Studio 17 2022\""));
        assert!(preview.contains("-DLLAMA_CURL=OFF -DFOO=1"));
        assert!(preview.contains("--target llama-server ggml-rpc-server"));
        assert!(preview.ends_with("-j 8"));
    }

    #[test]
    fn cmake_preview_prefers_configured_output_dir() {
        let settings = AppSettings {
            build_output_dir: "D:/builds/llama".to_string(),
            ..AppSettings::default()
        };
        let preview = cmake_command_preview(&settings);

        assert!(preview.contains("-B D:/builds/llama"));
        assert!(!preview.contains("D:/builds/llama/build"));
    }

    #[test]
    fn cmake_preview_forces_ninja_when_generator_cleared() {
        let settings = AppSettings {
            build_generator: String::new(),
            ..AppSettings::default()
        };

        assert!(cmake_command_preview(&settings).contains("-G Ninja"));
    }

    #[test]
    fn cmake_commands_stay_unquoted_and_match_preview() {
        let settings = AppSettings {
            build_work_dir: "C:/my source/llama".to_string(),
            ..AppSettings::default()
        };
        let [configure, build] = cmake_commands(&settings);

        assert!(configure.contains(&"C:/my source/llama".to_string()));
        assert_eq!(build[0], "cmake");
        assert_eq!(build[1], "--build");

        let preview = cmake_command_preview(&settings);
        assert!(preview.contains("-S \"C:/my source/llama\""), "{preview}");
    }

    #[test]
    fn sync_options_drops_backend_group_when_toolkit_missing() {
        let mut selected = AppSettings::default().build_cmake_options;
        assert!(selected.contains(&"GGML_CUDA".to_string()));
        assert!(selected.contains(&"GGML_HIP".to_string()));

        let entries = vec![
            EnvCheckEntry {
                id: ID_CUDA.to_string(),
                found: false,
                ..Default::default()
            },
            EnvCheckEntry {
                id: ID_ROCM.to_string(),
                found: true,
                ..Default::default()
            },
        ];
        sync_options_with_env(&mut selected, &entries);

        assert!(!selected.contains(&"GGML_CUDA".to_string()));
        assert!(selected.contains(&"GGML_HIP".to_string()));
    }

    #[test]
    fn sync_options_follows_accelerator_detection_and_skips_undetectable() {
        let mut selected = vec!["GGML_HIP_RCCL".to_string()];

        sync_options_with_env(&mut selected, &[]);
        assert!(selected.contains(&"GGML_HIP_RCCL".to_string()));

        let entries = vec![
            EnvCheckEntry {
                id: ID_RCCL.to_string(),
                found: false,
                ..Default::default()
            },
            EnvCheckEntry {
                id: ID_NCCL.to_string(),
                found: true,
                ..Default::default()
            },
        ];
        sync_options_with_env(&mut selected, &entries);

        assert!(!selected.contains(&"GGML_HIP_RCCL".to_string()));
        assert!(selected.contains(&"GGML_CUDA_NCCL".to_string()));
    }

    #[test]
    fn cmake_preview_defaults_enable_both_backends() {
        let preview = cmake_command_preview(&AppSettings::default());

        assert!(preview.contains("-DGGML_CUDA=ON"));
        assert!(preview.contains("-DGGML_CUDA_FA_QUANTS=all"));
        assert!(!preview.contains("-DGGML_CUDA_FA_ALL_QUANTS"));
        assert!(preview.contains("-DGGML_HIP=ON"));
        assert!(!preview.contains("-DGGML_CUDA_NCCL"));
        assert!(!preview.contains("-DGGML_HIP_RCCL"));
    }
}
