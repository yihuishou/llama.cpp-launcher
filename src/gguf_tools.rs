//! GGUF 分割 / 合并后台任务 —— 封装 llama.cpp 自带的 `llama-gguf-split`。
//!
//! 与 [`crate::build::CloneHandle`] 同模式：`Arc<AtomicBool>` 取消标志 +
//! `Arc<Mutex<State>>` 共享状态 + `Arc<Mutex<Option<Child>>>` 子进程 + worker 线程。
//! UI 每帧 `snapshot()` 轮询，耗时操作不阻塞渲染线程。

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::config::settings::SettingsManager;

/// 分割 / 合并两种操作共用同一可执行文件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Split,
    Merge,
}

/// 后台任务状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done { output: PathBuf },
    Error(String),
    Cancelled,
}

/// UI 每帧读取的状态快照
#[derive(Debug, Clone)]
pub struct ToolSnapshot {
    pub operation: Operation,
    pub state: ToolState,
    pub elapsed_secs: f64,
    pub last_line: String,
}

/// llama-gguf-split 子进程控制句柄（模式对齐 build::CloneHandle）
pub struct ToolHandle {
    operation: Operation,
    cancel: Arc<AtomicBool>,
    state: Arc<Mutex<ToolState>>,
    child: Arc<Mutex<Option<Child>>>,
    last_line: Arc<Mutex<String>>,
    started: Instant,
    worker: Option<JoinHandle<()>>,
}

impl ToolHandle {
    /// 分割模型：输出前缀 = 输入文件去掉扩展名，工具会追加 -00001-of-000NN.gguf
    pub fn start_split(tool: PathBuf, input: PathBuf, max_size: String) -> Self {
        let output = split_prefix(&input);
        Self::spawn(tool, input, output, Operation::Split, Some(max_size))
    }

    /// 合并分片：输入必须是第一个分片，输出为完整 gguf 路径
    pub fn start_merge(tool: PathBuf, first_shard: PathBuf, output: PathBuf) -> Self {
        Self::spawn(tool, first_shard, output, Operation::Merge, None)
    }

    fn spawn(
        tool: PathBuf,
        input: PathBuf,
        output: PathBuf,
        operation: Operation,
        max_size: Option<String>,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(ToolState::Running));
        let child = Arc::new(Mutex::new(None));
        let last_line = Arc::new(Mutex::new(String::new()));
        let started = Instant::now();

        let mut cmd = Command::new(&tool);
        match operation {
            Operation::Split => {
                cmd.arg("--split");
                if let Some(size) = &max_size {
                    cmd.arg("--split-max-size").arg(size);
                }
                cmd.arg(&input).arg(&output);
            }
            Operation::Merge => {
                cmd.arg("--merge").arg(&input).arg(&output);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let mut worker = None;
        match cmd.spawn() {
            Ok(mut proc) => {
                if let Some(out) = proc.stdout.take() {
                    spawn_line_reader(out, Arc::clone(&last_line));
                }
                if let Some(err) = proc.stderr.take() {
                    spawn_line_reader(err, Arc::clone(&last_line));
                }
                *child.lock().unwrap() = Some(proc);

                let cancel_w = Arc::clone(&cancel);
                let state_w = Arc::clone(&state);
                let child_w = Arc::clone(&child);
                let last_line_w = Arc::clone(&last_line);
                worker = Some(thread::spawn(move || {
                    wait_for_exit(&cancel_w, &state_w, &child_w, &last_line_w, &output);
                }));
            }
            Err(e) => {
                *state.lock().unwrap() =
                    ToolState::Error(format!("failed to start {}: {}", tool.display(), e));
            }
        }

        Self {
            operation,
            cancel,
            state,
            child,
            last_line,
            started,
            worker,
        }
    }

    pub fn snapshot(&self) -> ToolSnapshot {
        ToolSnapshot {
            operation: self.operation,
            state: self.state.lock().unwrap().clone(),
            elapsed_secs: self.started.elapsed().as_secs_f64(),
            last_line: self
                .last_line
                .lock()
                .map(|line| line.clone())
                .unwrap_or_default(),
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(&*self.state.lock().unwrap(), ToolState::Running)
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

impl Drop for ToolHandle {
    fn drop(&mut self) {
        self.request_cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// 定位 llama-gguf-split：先看 llama-server 同目录，再按 launcher 目录递归探测
pub fn find_tool(server_path: &Path) -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "llama-gguf-split.exe"
    } else {
        "llama-gguf-split"
    };
    if let Some(dir) = server_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    SettingsManager::new().auto_detect_gguf_split()
}

/// 分片输出前缀：`model.gguf` → `model`（保留目录）
fn split_prefix(input: &Path) -> PathBuf {
    match input.file_stem() {
        Some(stem) => input.with_file_name(stem),
        None => input.to_path_buf(),
    }
}

/// 合并输出的默认路径：去掉 `-NNNNN-of-NNNNN.gguf` 后缀并补回 `.gguf`
pub fn default_merge_output(first_shard: &Path) -> Option<PathBuf> {
    let name = first_shard.file_name()?.to_str()?;
    let re = regex::Regex::new(r"-\d{5}-of-\d{5}\.gguf$").ok()?;
    let suffix = re.find(name)?;
    let base = name.get(..suffix.start())?;
    if base.is_empty() {
        return None;
    }
    Some(first_shard.with_file_name(format!("{}.gguf", base)))
}

/// `--split-max-size` 只接受 `<数字>M` 或 `<数字>G`
pub fn validate_split_size(raw: &str) -> bool {
    let s = raw.trim();
    let number = s
        .strip_suffix('G')
        .or_else(|| s.strip_suffix('g'))
        .or_else(|| s.strip_suffix('M'))
        .or_else(|| s.strip_suffix('m'));
    let Some(number) = number else {
        return false;
    };
    number
        .parse::<f64>()
        .is_ok_and(|value| value > 0.0 && value.is_finite())
}

/// 合并文件名只允许纯文件名（不含路径分隔符）
pub fn validate_merge_file_name(raw: &str) -> bool {
    let name = raw.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
}

fn spawn_line_reader<R>(stream: R, last_line: Arc<Mutex<String>>)
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if let Ok(mut slot) = last_line.lock() {
                    *slot = trimmed.chars().take(300).collect();
                }
            }
        }
    });
}

fn wait_for_exit(
    cancel: &AtomicBool,
    state: &Mutex<ToolState>,
    child: &Mutex<Option<Child>>,
    last_line: &Mutex<String>,
    output: &Path,
) {
    loop {
        if cancel.load(Ordering::SeqCst) {
            if let Ok(mut slot) = child.lock() {
                if let Some(child) = slot.as_mut() {
                    kill_tree(child);
                }
            }
            *state.lock().unwrap() = ToolState::Cancelled;
            return;
        }

        let status = {
            let mut slot = child.lock().unwrap();
            match slot.as_mut() {
                Some(child) => child.try_wait(),
                None => return,
            }
        };

        match status {
            Ok(Some(exit_status)) => {
                let mut guard = state.lock().unwrap();
                *guard = if exit_status.success() {
                    ToolState::Done {
                        output: output.to_path_buf(),
                    }
                } else {
                    let line = last_line
                        .lock()
                        .map(|line| line.clone())
                        .unwrap_or_default();
                    ToolState::Error(if line.is_empty() {
                        format!("llama-gguf-split exited with {}", exit_status)
                    } else {
                        line
                    })
                };
                return;
            }
            Ok(None) => thread::sleep(Duration::from_millis(150)),
            Err(e) => {
                *state.lock().unwrap() = ToolState::Error(format!("process wait failed: {}", e));
                return;
            }
        }
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(target_os = "windows")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_size_accepts_g_and_m() {
        assert!(validate_split_size("20G"));
        assert!(validate_split_size("1500M"));
        assert!(validate_split_size("1.5g"));
        assert!(validate_split_size(" 512m "));
    }

    #[test]
    fn split_size_rejects_invalid() {
        assert!(!validate_split_size(""));
        assert!(!validate_split_size("20"));
        assert!(!validate_split_size("G"));
        assert!(!validate_split_size("-1G"));
        assert!(!validate_split_size("abcM"));
    }

    #[test]
    fn merge_name_defaults_to_prefix() {
        let path = Path::new("models/model-Q4_K_M-00001-of-00003.gguf");
        assert_eq!(
            default_merge_output(path),
            Some(PathBuf::from("models/model-Q4_K_M.gguf"))
        );
    }

    #[test]
    fn merge_name_requires_shard_suffix() {
        assert_eq!(default_merge_output(Path::new("models/model.gguf")), None);
    }

    #[test]
    fn split_prefix_drops_extension() {
        assert_eq!(
            split_prefix(Path::new("models/model.gguf")),
            PathBuf::from("models/model")
        );
    }

    #[test]
    fn merge_file_name_rejects_paths() {
        assert!(validate_merge_file_name("merged.gguf"));
        assert!(!validate_merge_file_name(""));
        assert!(!validate_merge_file_name("a/b.gguf"));
        assert!(!validate_merge_file_name("a\\b.gguf"));
        assert!(!validate_merge_file_name(".."));
    }
}
