# src/ 根散模块

## OVERVIEW

src 根下 12 个顶层单文件模块：app 外壳、i18n、下载/自更新、主题、KV 估算、网络/地理、快捷方式、调试工具、编译环境检测/源码克隆。面板见 ui/，进程见 engine/，配置见 config/。

## STRUCTURE

- main.rs: 入口（`windows_subsystem = "windows"` 无控制台）：init_logger（FileLogger/NoOpLogger 由配置 log_to_file 决定）、`--updater-switch <pid>` waiter 分支、`--minimized` 检测、fluent-egui native_options（app_id、透明窗口）、load_window_icon / load_icon_fonts / load_cjk_font
- app.rs: LlamaLauncherApp（settings/settings_manager/server_manager/rpc_manager/downloader/updater/preset_share/preset_config/nav/lang 等字段）+ eframe::App::ui 路由 + 菜单栏 + DWM/Mica 同步 + 开机自启（HKCU Run）+ 浏览器打开
- build.rs: CloneHandle + SourceOp(Clone/Update) + CloneState(Idle/Running/Done/Failed) + CloneStatus(percent/op)；`start_clone`/`start_update` 后台跑 git（Handle 模式 + kill 子进程取消 + 独立线程按 `\r`/`\n` 切段解析 `--progress` 百分比）；日志：克隆与编译共用 `BuildLog`（Arc<Mutex<VecDeque<LogEntry>>> 环形缓冲，容量 MAX_LOG_LINES=10_000，同 server/rpc；`CloneHandle::new(log)` 与 `BuildHandle::new(log)` 各持一个克隆指向同一缓冲，故日志面板同时看到两条链路），worker 把每条命令（`$ git …` / `$ cmake …`，Info）、stderr/stdout 每行（Info）与失败原因（Error）追加进去；克隆 `git clone --progress [--branch <ref>] <repo>` 到程序同级 `llama-source-code/`，更新在既有目录内 `update_steps`（有 Tag → `fetch --tags --force`+`checkout <tag>`；无 Tag → `checkout <branch>`+`pull --ff-only`）；`resolve_ref` 取 Tag 优先否则分支；另含 CMake 编译开关清单（OptionGroup{General,Cuda,Hip,Ui} + CMAKE_OPTIONS_GENERAL/CUDA/HIP/UI = (`-D` 参数, 说明 i18n 键) 元组 + cmake_options/is_option_selected/set_option_selected/option_label/option_is_disable/selected_cmake_args/sync_options_with_env，布尔项勾选 → -D<名称>=ON，含 `=` 的 spec 原样拼 -D<spec>，如 GGML_RPC 补 =ON、LLAMA_USE_PREBUILT_UI=OFF / LLAMA_BUILD_TESTS=OFF）；默认勾选 GGML_RPC、GGML_CUDA_FA_ALL_QUANTS（通用组）、GGML_CUDA、GGML_HIP 与 LLAMA_BUILD_TESTS/EXAMPLES/APP=OFF（默认值集中在 config/settings.rs 的 default_build_cmake_options）；上游废弃名 `GGML_CUDA_FA_ALL_QUANTS` 界面照旧显示、拼参时经 RENAMED_SPECS 换成 `-DGGML_CUDA_FA_QUANTS=all`；`sync_options_with_env(selected, entries)` 在环境检测落盘时校正勾选——CUDA/ROCm 工具包缺失则清空该组（分组在界面已被隐藏，留着勾选会拼出无效的 `-DGGML_CUDA=ON` 让配置阶段失败），NCCL/RCCL 跟随检测结果，未出现在检测条目里的项（Python 不可用时不会被探测）保持原样；`cmake_commands(settings)` 返回「开始编译」实际执行的 `[配置, 构建]` 两条命令（不加引号，直接交给 Command::args），`cmake_command_preview(settings)` 复用前者渲染预览文本（含空格路径加引号；首行 cmake -S/-B 配置、次行 cmake --build，勾选开关复用 selected_cmake_args，生成器恒拼 `-G Ninja`；`source_dir_ready(settings)` 判定源码目录是否已就绪（存在且非空），与预览的 `-S` 同源解析）；`BuildHandle` + `BuildPhase`(Configure/Compile) + `BuildState`(Idle/Running/Done/Failed(String)) + `BuildStatus{state, phase}` 执行编译：`start(settings)` 此刻快照设置、后台依次跑配置与编译，`run_step` 起线程把 stdout/stderr 逐行写入 BuildLog、非零退出即中止，取消/关窗经 `kill_tree`（Windows 先 `taskkill /PID <pid> /T /F` 整树终止再 kill，否则 ninja/编译器残留占 CPU）；注意与根目录 Cargo `build.rs` 无关
- env_check.rs: Probe/CheckResult/EnvCheckEntry/EnvCheckState + EnvCheckHandle + ID_GIT/ID_CUDA/ID_ROCM/ID_NINJA/ID_RCCL/ID_NCCL（稳定 id，供 UI 判定组件可用性与编译开关同步）；编译环境检测（git/cmake/编译器/ninja/python 均为必需项，Python 可用时追加可选的 RCCL/NCCL），全以版本命令判定；`all_required_found(results)` 与 `all_required_found_entries(entries)` 判定必需项是否齐全（后者按落盘条目判定，空条目 = 未检测过 → false，供「开始编译」门控）
- i18n.rs: Key enum + Language(Zh/En) + t()；所有 UI 文本唯一入口（2603 行，项目第二大文件）
- downloader.rs: DownloadHandle + DownloadVariant + DownloadState + download_in_background()/run_download()；官方/镜像源、nightly channel、流式下载（DlError::Cancelled/Failed）
- updater.rs: UpdaterHandle + UpdateState(Idle/Checking/UpToDate/Available/Downloading/Installing/Error) + UpdateStatus；run_updater_waiter（--updater-switch 等待旧进程退出后换名）+ cleanup_leftovers_on_startup（exe.old）
- theme.rs: parse_hex / accent_color / apply_theme（Fluent3 深色/浅色）
- kv_cache.rs: GgufInfo（gguf-rs 解析 GGUF 头）+ calc_kv_cache_space / calc_max_context / calc_and_format（facade）
- geo.rs: should_use_mirror_first() 地理位置检测 → 镜像源优先级（有内联测试）
- net_proxy.rs: build_agent()（ureq：30s 连接超时、无总超时、系统代理、USER_AGENT）+ CHUNK_SIZE + ERR_NETWORK
- shortcut.rs: create_desktop_shortcut（shortcuts-rs，locale-aware 名称）
- spacing_debugger.rs: SpacingDebugger（debug_mode：记录控件 rect → 悬浮高亮 + 四方向间距标尺）

## CONVENTIONS

- 后台任务 Handle 模式（downloader/updater 共用）：Arc<AtomicBool> cancel + Arc<Mutex<Status>> + Arc<Mutex<Option<JoinHandle>>>；UI 每帧 snapshot() 轮询，Drop 时 request_cancel()
- 字体三阶段推迟（防首帧 panic）：winui_installed（调 set_fonts）→ icon_fonts_loaded → icon_fonts_ready（set_fonts 改动下一帧边界才生效，排版再晚一帧）
- UI 面向文案走 i18n::t；后台线程错误串为英文（如 UpdateState::Error(String)），UI 层拼 i18n 前缀
- 网络一律 net::build_agent()；下载/更新共享该 Agent 构造
- CJK 字体仅 assets/NotoSansSC-Regular.ttf 经 include_bytes! 内嵌，放 Proportional 与 Monospace 族尾部（拉丁字形仍由默认字体渲染）
- 内联测试分布: i18n.rs 无、geo.rs:94、kv_cache.rs:438、downloader.rs:1194、env_check.rs:536、build.rs:1009（app/updater/theme/shortcut/net_proxy 零测试）

## ANTI-PATTERNS (THIS PROJECT)

- app.rs 中绕过 engine 直接 spawn 进程（进程生命周期归 engine；下载/更新归各自 Handle）
- 首帧前访问 ctx.fonts()/set_fonts（epaint 0.35 字体表未初始化 → panic）
- 绕过 net_proxy::build_agent() 自建 ureq client
- 在 src 根新增模块后忘记在 main.rs 注册 pub mod
- iconflow 图标用 `icon()` 而非 `try_icon()`（前者对不支持的 Style/Size 组合会 panic，每帧调用的组件必须用 try_icon 兜底）
