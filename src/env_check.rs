//! 编译环境检测模块（llama.cpp 手动编译前置条件）。
//!
//! 探测编译 llama.cpp 所需的工具链与可选的 GPU 后端工具包，统一以「版本命令」判定可用性：
//! 直接调用工具并读取 `--version` 输出，能调起来即视为可用，调不起来（未在全局环境注册）
//! 即不可用。覆盖 git、cmake、C++ 编译器、ninja、python，以及 CUDA（`nvcc --version`）
//! 与 ROCm（`hipcc --version`）。Windows 下另用 `vswhere.exe` 定位 MSVC（编译器通常不在
//! PATH 上）。
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

use serde::{Deserialize, Serialize};

use crate::i18n::Key;

/// 单个组件的探测结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 已找到：`version` 为版本描述（可能为空），`path` 为可执行文件或安装根路径
    Found { version: String, path: PathBuf },
    /// 未找到
    Missing,
}

impl Probe {
    /// 是否已找到
    pub fn is_found(&self) -> bool {
        matches!(self, Probe::Found { .. })
    }
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
        self.probe.is_found()
    }
}

/// 组件稳定 id（配置序列化用，勿轻易改动）
fn key_id(key: Key) -> Option<&'static str> {
    match key {
        Key::EnvCheckGit => Some("git"),
        Key::EnvCheckCmake => Some("cmake"),
        Key::EnvCheckCompiler => Some("compiler"),
        Key::EnvCheckNinja => Some("ninja"),
        Key::EnvCheckPython => Some("python"),
        Key::EnvCheckCuda => Some("cuda"),
        Key::EnvCheckRocm => Some("rocm"),
        _ => None,
    }
}

/// 稳定 id → 组件名 i18n 键（展示用）
pub fn id_to_key(id: &str) -> Option<Key> {
    match id {
        "git" => Some(Key::EnvCheckGit),
        "cmake" => Some(Key::EnvCheckCmake),
        "compiler" => Some(Key::EnvCheckCompiler),
        "ninja" => Some(Key::EnvCheckNinja),
        "python" => Some(Key::EnvCheckPython),
        "cuda" => Some(Key::EnvCheckCuda),
        "rocm" => Some(Key::EnvCheckRocm),
        _ => None,
    }
}

/// 可序列化的检测结果条目（落盘存配置，供下次启动展示）。
///
/// 存稳定 `id` 而非 [`i18n::Key`]：Key 是编译期枚举、不可序列化且可能变动。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvCheckEntry {
    pub id: String,
    pub required: bool,
    pub found: bool,
    pub version: String,
    pub path: String,
}

/// 把一次检测结果转为可落盘的条目列表
pub fn to_entries(results: &[CheckResult]) -> Vec<EnvCheckEntry> {
    results
        .iter()
        .filter_map(|r| {
            key_id(r.key).map(|id| {
                let (version, path) = match &r.probe {
                    Probe::Found { version, path } => {
                        (version.clone(), path.to_string_lossy().into_owned())
                    }
                    Probe::Missing => (String::new(), String::new()),
                };
                EnvCheckEntry {
                    id: id.to_string(),
                    required: r.required,
                    found: r.is_found(),
                    version,
                    path,
                }
            })
        })
        .collect()
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

fn probe_python() -> Probe {
    for name in ["python3", "python"] {
        let probe = probe_tool(name, &["--version"]);
        if probe.is_found() {
            return probe;
        }
    }
    Probe::Missing
}

fn probe_compiler() -> Probe {
    #[cfg(windows)]
    {
        let msvc = probe_msvc();
        if msvc.is_found() {
            return msvc;
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["clang++", "g++"]
    } else {
        &["c++", "clang++", "g++"]
    };
    for name in names {
        let probe = probe_tool(name, &["--version"]);
        if probe.is_found() {
            return probe;
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

fn probe_cuda() -> Probe {
    probe_tool("nvcc", &["--version"])
}

fn probe_rocm() -> Probe {
    probe_tool("hipcc", &["--version"])
}

/// 直接调用 `name version_args`（依赖全局环境/PATH 的注册）：能调起来即视为可用，
/// 调不起来即视为未安装。不再自行扫描 PATH 目录。
fn probe_tool(name: &str, version_args: &[&str]) -> Probe {
    match probe_version(Path::new(name), version_args) {
        Some(version) => Probe::Found {
            version,
            path: PathBuf::from(name),
        },
        None => Probe::Missing,
    }
}

/// 运行 `exe args`：成功调起即 Some（版本为首个非空输出行，可能为空），
/// 调不起来（如 NotFound）返回 None。
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
    Some(
        first_nonempty_line(&stdout)
            .or_else(|| first_nonempty_line(&stderr))
            .unwrap_or_default(),
    )
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
        let git = requirements()
            .into_iter()
            .find(|r| r.key == Key::EnvCheckGit);
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
    fn unregistered_binary_is_missing() {
        assert_eq!(
            probe_tool("__env_check_no_such_binary_zzz__", &["--version"]),
            Probe::Missing
        );
        assert!(!Probe::Missing.is_found());
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
