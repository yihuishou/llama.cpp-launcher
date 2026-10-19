# LLama Launcher — Root

**Generated:** 2026-10-17
**Commit:** e807046
**Branch:** main

## OVERVIEW

llama.cpp 桌面启动器。Rust + eframe/egui 0.35 + fluent-egui（WinUI 3）GUI，管理 llama-server（含 KVMem fork）与 ggml-rpc-server 进程、预设系统、llama.cpp 下载与应用自更新、Windows 开机自启与桌面快捷方式。单 binary，无 Cargo workspace。

## STRUCTURE

```
root/
├── Cargo.toml              # 单 binary v0.2.1 → llama_cpp_launcher(.exe)
├── build.rs                # winres 图标/版本信息（Windows，主动定位 rc.exe，失败仅告警）+ 图标复制（Linux）
├── assets/                 # llama.ico（窗口/exe 图标）、NotoSansSC-Regular.ttf（内嵌 CJK 回退）
├── doc/                    # server / rpc / kvmem 参数参考 txt（v0.6.0）
├── .github/workflows/release.yml  # tag v* → win x64/arm64 + linux x64/aarch64 → GitHub Release
├── src/main.rs             # 入口: logger / --updater-switch / --minimized / native_options / 字体
├── src/app.rs              # LlamaLauncherApp: 路由/菜单/DWM-Mica/开机自启
├── src/build.rs            # 后台 git clone llama.cpp 源码 → 程序同级 llama-source-code/
├── src/env_check.rs        # 编译环境检测（git/cmake/编译器/ninja/python + CUDA/ROCm/RCCL/NCCL）
├── src/config/             # AppSettings/Preset/GpuLayersMode/LlamaCppType + 读写
├── src/engine/             # server/rpc-server 进程管理 + 日志聚合
├── src/ui/                 # 13 个 egui 面板 + widgets/helper
├── src/i18n.rs             # i18n::t(Key, lang)，zh/en key→文案映射
├── src/downloader.rs       # llama.cpp 下载器（官方/镜像/nightly，变体选择）
├── src/updater.rs          # 应用自更新（Windows 热更新 + waiter 进程）
├── src/theme.rs            # Fluent3 主题，accent color
├── src/kv_cache.rs         # GGUF 解析 + KV 缓存/最大上下文估算
├── src/geo.rs              # 地理位置检测（镜像源优先级）
├── src/net_proxy.rs        # ureq Agent（连接超时 + 系统代理）
├── src/spacing_debugger.rs # UI 间距调试工具
└── src/shortcut.rs         # Windows 快捷方式 (.lnk) 创建
```

## WHERE TO LOOK

| Task                             | Location               | Notes                              |
|----------------------------------|------------------------|------------------------------------|
| src 根散模块（app/i18n/downloader/updater/theme 等） | src/AGENTS.md | Handle 模式、字体三阶段、入口流程 |
| UI面板/标签行为                        | src/ui/AGENTS.md       | 路由由 app.rs 控制，面板仅渲染                |
| Server/RPC生命周期、日志                | src/engine/AGENTS.md   | 状态机 + 进程管理 + log聚合                 |
| AppSettings/Preset/GpuLayersMode | src/config/settings.rs | 配置结构体 + Defaults + 读写              |
| i18n键映射 (zh/en)                  | src/i18n.rs            | Key enum + t() 函数，所有 UI 文本入口       |
| Windows快捷方式 (.lnk)               | src/shortcut.rs        | shortcuts-rs 封装                    |
| App结构、菜单与标签路由                    | src/app.rs             | LlamaLauncherApp::ui, tab_selected |
| llama.cpp 下载                      | src/downloader.rs      | 官方/镜像源，变体选择，进度跟踪             |
| 应用自更新                          | src/updater.rs         | Windows 热更新，版本检查，waiter 进程    |
| 主题系统                           | src/theme.rs           | Fluent3 主题，accent color，深色/浅色     |
| KV缓存计算                         | src/kv_cache.rs        | GGUF 元数据解析，空间估算               |
| server 参数参考                    | doc/*.txt              | 三种 server 的参数清单                  |

## CODE MAP (核心符号)

| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| main | fn | main.rs:122 | - | eframe 入口：logger/--updater-switch/--minimized/native_options |
| load_cjk_font / load_icon_fonts | fn | main.rs:220/191 | 1 | Noto Sans SC 内嵌回退（Proportional+Monospace 尾部）+ iconflow 字体 |
| LlamaLauncherApp | struct+impl | app.rs:30 | 1 | App 根：settings/server/rpc/downloader/updater/tab/lang/字体三阶段 |
| enable/disable_auto_start | fn | app.rs:942/983 | 1 | Windows 注册表开机自启（HKCU Run） |
| AppSettings | struct | config/settings.rs:2113 | 全局 | 全部配置字段；*_actual() 处理 k→值 |
| Preset | struct | config/settings.rs | - | from_settings/apply_to；含 kvmem 全字段 |
| GpuLayersMode | enum | config/settings.rs | - | Auto/All/Manual + 双格式 Visitor |
| LlamaCppType | enum | config/settings.rs | - | Standard/Kvmem，from_server_path() 按 exe 名推断 |
| SettingsManager | struct | config/settings.rs:3266 | 7 | load/save + auto_detect_server_path/rpc |
| ServerManager | struct+impl | engine/server.rs:50 | 11 | llama-server 生命周期、日志、is_listening、progress |
| RpcManager | struct+impl | engine/rpc.rs:38 | 5 | rpc-server 生命周期（与 server 同模式） |
| ErrorInfo | enum | engine/mod.rs:12 | - | 存 i18n 键（Key/WithDetail），status_text(lang) 渲染时解析 |
| parse_tags | fn | ui/model_panel.rs | 1 | 文件名→9色彩色标签 |
| DownloadHandle | struct | downloader.rs:393 | 2 | 后台下载（snapshot/request_cancel） |
| UpdaterHandle | struct | updater.rs:183 | 1 | 自更新句柄（check/install/snapshot） |
| run_updater_waiter | fn | updater.rs:644 | 1 | --updater-switch 等待进程：等旧进程退出后换入新版本 |
| apply_theme / accent_color | fn | theme.rs:39/19 | 1 | Fluent3 主题应用 |
| calc_kv_cache_space / calc_max_context / calc_and_format | fn | kv_cache.rs | 1 | KV 估算（Flash Attention 影响 coef 2.5/3.0） |
| build_agent | fn | net_proxy.rs:31 | 2 | ureq Agent：30s 连接超时（无总超时）+ 系统代理 |
| should_use_mirror_first | fn | geo.rs | 2 | 地理位置→镜像源优先级 |
| SpacingDebugger | struct | spacing_debugger.rs:4 | 1 | debug_mode 间距可视化 |

## 模型标签系统（9 色方案）

`parse_tags()` 将文件名按 `-` 分段后匹配规则着色，渲染为圆角按钮：

| 颜色        | RGB           | 分类               | 判定函数/关键词                                                    | 示例                 |
|-----------|---------------|------------------|-------------------------------------------------------------|--------------------|
| 🟣 紫色     | (180,120,255) | 参数量              | `is_param_size` — 含数字+以 b/m 结尾                              | `7b`, `335m`       |
| 🟠 橙色     | (255,165,0)   | 量化类型             | `is_quantization` — q 后紧跟数字 / iq 后紧跟数字（排除 Qwen/QwQ）         | `q4_k_m`, `iq4_nl` |
| ⚫ 灰色      | (160,160,160) | 版本号              | 纯数字/小数                                                      | `3.1`, `2`         |
| 🟢 绿色     | (100,200,100) | 训练方法             | `is_training_method` — instruct/chat/sft/rlhf/dpo/orpo/grpo | `Instruct`         |
| 🟡 黄色     | (255,215,0)   | 精度               | fp16/bf16/f32/fp8                                           | `FP16`             |
| 🩷 粉色     | (255,100,130) | LoRA/Adapter     | lora/adapter/delta                                          | `LoRA`, `delta`    |
| 🟤 棕色     | (205,133,63)  | 上下文长度            | `is_context_length` — 以 k 结尾+含数字 / long / 128/64/32         | `128k`, `c4k`      |
| 🩵 青色     | (0,210,210)   | 架构类型             | mamba/rwkv/hyena/decoder                                    | `RWKV`, `mamba`    |
| 🔵 蓝色     | (100,150,255) | 模型名称             | 兜底（不匹配以上任何规则）                                               | `Qwen`, `Llama`    |

## 启动流程 (高层)

1. main: init_logger（读配置 log_to_file → FileLogger 写 llama_launcher_<ts>.log，否则 NoOpLogger）。
2. main: `--updater-switch <pid>` → run_updater_waiter() 立即退出（自更新等待者，无 UI）；cleanup_leftovers_on_startup() 清 exe.old。
3. main: fluent_egui::winui::native_options（app_id com.yihuishou.llama_cpp_launcher、透明窗口）；Mica backdrop（首帧 Light，App 首帧 sync 实际主题）。
4. LlamaLauncherApp::new: 系统语言→zh/en；读取 llama_cpp_launcher_settings.json；auto_start_preset_name → 应用预设；apply_theme；字体三阶段（winui_installed → icon_fonts_loaded → icon_fonts_ready，set_fonts 下一帧边界才生效）。
5. update(): tab_selected(i18n key) → 路由到对应面板；`--minimized` 启动时最小化到任务栏。
6. Drop → stop server/RPC + request_cancel 下载/更新 + save settings。

## Windows特有功能 (高层)

- 开机自启: HKCU Run，带 --minimized。
- DWM: DwmSetWindowAttribute 标题栏色/沉浸暗色；透明窗口 clear_color(0,0,0,0) 透出 Mica。
- 自更新: 下载到 update/ → 旧进程退出 → waiter 进程（OpenProcess 等待）换名正式版。
- 桌面快捷方式: shortcuts-rs，locale-aware 名称；打开浏览器: ShellExecuteW。

## 全局约束

- 单 binary，Windows 优先（Linux 次之：X11 + aarch64 交叉编译）。
- 进程管理: std::process::Child + Arc<Mutex<>> + Drop 自动停止。
- 后台任务（下载/自更新）统一 Handle 模式: Arc<AtomicBool> cancel + Arc<Mutex<Status>> + worker 线程，UI 每帧 snapshot()。
- 日志: BufReader → VecDeque<LogEntry>（server/rpc 各自独立），MAX_LOG_LINES=10_000。
- i18n: 所有 UI 文本经 i18n::t(Key, lang)；ErrorInfo 存键、渲染时解析（语言切换实时生效）；后台线程错误串为英文，UI 层拼 i18n 前缀。
- egui 0.35 + fluent-egui 0.1.1 + iconflow 2.1；Result<T, String> + map_err。

## ANTI-PATTERNS (THIS PROJECT)

- 在 UI 面板中直接启动/停止进程（必须走 engine）。
- 绕过 i18n::t() 硬编码中文/英文到 UI 代码。
- main.rs / app.rs 中写死启动参数而不经过 AppSettings/Preset。
- 首帧前访问 ctx.fonts()/set_fonts（字体表未初始化 → panic）。
- 自创 ureq client（必须用 net_proxy::build_agent()）。
- 把 ErrorInfo 存成已解析文本（语言写死，破坏切换实时生效）。

## COMMANDS

```
cargo build --release → target/release/llama_cpp_launcher.exe（同级 llama_cpp_launcher_settings.json）
cargo test            # 9 个内联 #[cfg(test)] 模块，主集群 config/settings.rs:3413-
cargo fmt / cargo clippy
Release: git tag v0.x.y → release.yml 四架构（win x64/arm64、linux x64/aarch64）→ GitHub Release
本地运行参数: --minimized（最小化启动）、--updater-switch <pid>（waiter 进程，无 UI）
```

## NOTES

- start-iq3.ps1（根目录未跟踪）是本地调试脚本，非项目一部分。
- llama.cpp 的 --mlock/--no-mmap 已被 --load-mode 取代，build_launch_command 不再拼接（engine/server.rs:658）。
- KVMem fork（llama-kvmem-server）：LlamaCppType::Kvmem 才拼 19 个 --kvmem* 参数（engine/server.rs kvmem_args）。
- CJK 字体仅内嵌 assets/NotoSansSC-Regular.ttf（include_bytes!），不依赖系统字体。
