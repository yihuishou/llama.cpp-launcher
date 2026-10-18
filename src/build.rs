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

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

/// 后台克隆 / 更新控制句柄（模式对齐 downloader::DownloadHandle）
pub struct CloneHandle {
    cancel: Arc<AtomicBool>,
    status: Arc<Mutex<CloneStatus>>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for CloneHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CloneHandle {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(CloneStatus::default())),
            child: Arc::new(Mutex::new(None)),
            worker: Arc::new(Mutex::new(None)),
        }
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
        let handle = thread::spawn(move || {
            let state = match op {
                SourceOp::Clone => run_clone(&cancel, &child, &status, &repo, &branch, &tag),
                SourceOp::Update => run_update(&cancel, &child, &status, &branch, &tag),
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

    /// Drop / 关窗调用：置取消标志并 kill 子进程
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
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
    repo: &str,
    branch: &str,
    tag: &str,
) -> CloneState {
    let Some(dest) = source_code_dir() else {
        return CloneState::Failed("cannot resolve program directory".to_string());
    };
    if let Some(msg) = prepare_dir(&dest) {
        return CloneState::Failed(msg);
    }

    let steps = [clone_args(repo, branch, tag, &dest)];
    run_steps(cancel, child_slot, status, steps, &dest)
}

/// 更新：校验既有源码目录 → 依次执行 `update_steps` → 终态
fn run_update(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    branch: &str,
    tag: &str,
) -> CloneState {
    let Some(dest) = source_code_dir() else {
        return CloneState::Failed("cannot resolve program directory".to_string());
    };
    if !dest.join(".git").exists() {
        return CloneState::Failed(format!(
            "not a git repository (clone first): {}",
            dest.display()
        ));
    }

    let steps = update_steps(&dest, branch, tag);
    run_steps(cancel, child_slot, status, steps, &dest)
}

/// 依次执行若干 git 命令，任一失败即中止；全部成功则返回 `Done(dest)`
fn run_steps(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
    steps: impl IntoIterator<Item = Vec<String>>,
    dest: &Path,
) -> CloneState {
    for args in steps {
        match run_git(cancel, child_slot, status, &args) {
            Ok((exit, _)) if exit.success() => {}
            Ok((exit, text)) => return CloneState::Failed(describe_failure(&text, &exit)),
            Err(e) => return CloneState::Failed(e),
        }
    }
    CloneState::Done(dest.to_path_buf())
}

/// 跑一条 git 命令并等待结束，返回 (退出状态, stderr 文本)
fn run_git(
    cancel: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
    status: &Arc<Mutex<CloneStatus>>,
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

    // 独立线程抽干 stderr：逐段解析进度（顺带防止管道写满阻塞 git）并留存失败信息
    let stderr_reader = child.stderr.take().map(|pipe| {
        let status = Arc::clone(status);
        thread::spawn(move || read_progress(pipe, &status))
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
fn read_progress(pipe: impl Read, status: &Arc<Mutex<CloneStatus>>) -> String {
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
            record_segment(&segment, &mut all, &mut max_percent, status);
        }
    }
    record_segment(&pending, &mut all, &mut max_percent, status);

    String::from_utf8_lossy(&all).into_owned()
}

/// 记录一段 stderr：累积文本 + 取更大百分比写入共享状态
fn record_segment(
    segment: &[u8],
    all: &mut Vec<u8>,
    max_percent: &mut Option<u8>,
    status: &Arc<Mutex<CloneStatus>>,
) {
    if segment.is_empty() {
        return;
    }
    all.extend_from_slice(segment);
    all.push(b'\n');

    let text = String::from_utf8_lossy(segment);
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
                    let _ = child.kill();
                }
            }
        }
        let waited = {
            let mut slot = child_slot
                .lock()
                .map_err(|_| "clone state poisoned".to_string())?;
            slot.as_mut()
                .ok_or_else(|| "clone process lost".to_string())?
                .try_wait()
        };
        match waited {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(format!("git wait failed: {}", e)),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let handle = CloneHandle::new();
        assert!(!handle.is_busy());
        assert_eq!(handle.snapshot().state, CloneState::Idle);
        assert_eq!(handle.snapshot().percent, None);
        assert_eq!(handle.snapshot().op, None);
    }
}
