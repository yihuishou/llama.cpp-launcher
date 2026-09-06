# LLama Launcher — Root

**Generated:** 2026-09-06
**Commit:** 52d2ab6
**Branch:** main

## OVERVIEW

llama.cpp 桌面启动器。Rust + eframe/egui 0.33 GUI，管理 llama-server/RPC、预设系统、Windows 开机自启和快捷方式。单 binary，无 Cargo workspace。

## STRUCTURE

```
root/
├── Cargo.toml              # 单 binary, llama_cpp_launcher.exe
├── build.rs                # Windows icon / manifest (winres)
├── src/main.rs             # 入口: eframe + CJK字体 + env_logger + 日志配置
├── src/app.rs              # LlamaLauncherApp: UI路由/菜单/状态/开机自启
├── src/config/             # AppSettings/Preset/GpuLayersMode/默认值与读写
├── src/engine/             # llama-server / rpc-server 进程管理与日志聚合
├── src/ui/                 # 11 个 egui 面板 (server/rpc/model/params/mcp/log/rpc_log/commands/presets/settings)
├── src/i18n.rs             # i18n::t(Key, lang)，zh/en key→文案映射
├── src/downloader.rs       # llama.cpp 下载器（官方/镜像源，变体选择）
├── src/updater.rs          # 应用自更新（Windows 热更新）
├── src/theme.rs            # 主题系统（Fluent3 Light/Dark，accent color）
├── src/kv_cache.rs         # KV 缓存空间计算与最大上下文估算
├── src/geo.rs              # 地理位置检测（下载源智能选择）
├── src/net_proxy.rs        # 网络代理支持
├── src/spacing_debugger.rs # UI 间距调试工具
└── src/shortcut.rs         # Windows 快捷方式 (.lnk) 创建
```

## WHERE TO LOOK

| Task                             | Location               | Notes                              |
|----------------------------------|------------------------|------------------------------------|
| UI面板/标签行为                        | src/ui/AGENTS.md       | 路由由 app.rs 控制，面板仅渲染                |
| Server/RPC生命周期、日志                | src/engine/AGENTS.md   | 状态机 + 进程管理 + log聚合                 |
| AppSettings/Preset/GpuLayersMode | src/config/settings.rs | 配置结构体 + Defaults + 读写              |
| i18n键映射 (zh/en)                  | src/i18n.rs            | Key enum + t() 函数，所有 UI 文本入口       |
| Windows快捷方式 (.lnk)               | src/shortcut.rs        | shortcuts-rs 封装                    |
| App结构、菜单与标签路由                    | src/app.rs             | LlamaLauncherApp::ui, tab_selected |
| llama.cpp 下载                      | src/downloader.rs      | 官方/镜像源，变体选择，进度跟踪             |
| 应用自更新                          | src/updater.rs         | Windows 热更新，版本检查                 |
| 主题系统                           | src/theme.rs           | Fluent3 主题，accent color，深色/浅色     |
| KV缓存计算                         | src/kv_cache.rs        | GGUF 元数据解析，空间估算               |

## CODE MAP (核心符号)

| Symbol                                                                | Type        | Location           | Role                                            |
|-----------------------------------------------------------------------|-------------|--------------------|-------------------------------------------------|
| main                                                                  | fn          | main.rs            | eframe 入口，加载字体 & env_logger                     |
| load_cjk_fonts                                                        | fn          | main.rs            | CJK字体加载（fallback）                               |
| LlamaLauncherApp                                                      | struct+impl | app.rs             | App根：settings/server/rpc/tab/lang/auto_start    |
| impl eframe::App for LlamaLauncherApp::ui                             | method      | app.rs             | 每帧 UI 路由 + 菜单栏                                  |
| enable_auto_start / disable_auto_start                                | fn          | app.rs             | Windows 注册表开机自启                                 |
| open_web_client_url, open_repo_url                                    | fn          | app.rs             | ShellExecuteW 打开浏览器                             |
| AppSettings                                                           | struct      | config/settings.rs | 全部配置字段（server/RPC/预设/GPU）                       |
| Preset                                                                | struct      | config/settings.rs | 可保存/应用/删除的预设；含 apply_to()                       |
| GpuLayersMode                                                         | enum        | config/settings.rs | Auto/All/Manual，序列化策略                           |
| SettingsManager                                                       | impl        | config/settings.rs | load/save + auto_detect_server_path/rpc         |
| ServerManager                                                         | struct+impl | engine/server.rs   | llama-server 生命周期、日志、launch_command             |
| RpcManager                                                            | struct+impl | engine/rpc.rs      | rpc-server 生命周期、连接状态                            |
| parse_tags                                                            | fn          | ui/model_panel.rs  | 文件名→9色彩色标签（参数量/量化/版本/训练方法/精度/LoRA/上下文长度/架构/模型名） |
| render_file_list                                                      | fn          | ui/model_panel.rs  | 按 FileMode(Main/Mmproj/Dflash) 过滤并渲染文件列表        |
| log_panel::ui / rpc_ui                                                | fn          | ui/log_panel.rs    | 服务器/RPC 运行日志面板入口                                |
| download_in_background                                                | fn          | downloader.rs      | 后台下载线程入口                                          |
| run_download                                                          | fn          | downloader.rs      | 完整下载流程（获取版本→下载→解压→定位二进制）                  |
| apply_theme                                                           | fn          | theme.rs           | 应用主题到全局视觉样式                                     |
| calc_kv_cache_space                                                   | fn          | kv_cache.rs        | 计算 KV 缓存空间需求                                       |
| create_desktop_shortcut                                               | fn          | shortcut.rs        | 创建桌面快捷方式（Windows .lnk / Linux .desktop）        |

## 模型标签系统（9 色方案）

`parse_tags()` 将文件名按 `-` 分段后匹配规则着色，渲染为圆角按钮：

| 颜色        | RGB           | 分类               | 判定函数/关键词                                                    | 示例                 |
|-----------|---------------|------------------|-------------------------------------------------------------|--------------------|
| 🟣 紫色     | (180,120,255) | 参数量              | `is_param_size` — 含数字+以 b/m 结尾                              | `7b`, `335m`       |
| 🟠 橙色     | (255,165,0)   | 量化类型             | `is_quantization` — q 后紧跟数字 / iq 后紧跟数字（排除 Qwen/QwQ）         | `q4_k_m`, `iq4_nl` |
| ⚫ 灰色      | (160,160,160) | 版本号              | 纯数字/小数                                                      | `3.1`, `2`         |
| 🟢 绿色     | (100,200,100) | 训练方法             | `is_training_method` — instruct/chat/sft/rlhf/dpo/orpo/grpo | `Instruct`         |
| 🟡 **黄色** | (255,215,0)   | **精度**           | fp16/bf16/f32/fp8                                           | `FP16`             |
| 🩷 **粉色** | (255,100,130) | **LoRA/Adapter** | lora/adapter/delta                                          | `LoRA`, `delta`    |
| 🟤 **棕色** | (205,133,63)  | **上下文长度**        | `is_context_length` — 以 k 结尾+含数字 / long / 128/64/32         | `128k`, `c4k`      |
| 🩵 **青色** | (0,210,210)   | **架构类型**         | mamba/rwkv/hyena/decoder                                    | `RWKV`, `mamba`    |
| 🔵 蓝色     | (100,150,255) | 模型名称             | 兜底（不匹配以上任何规则）                                               | `Qwen`, `Llama`    |

## 启动流程 (高层)

1. main: 加载CJK字体, set_zoom_factor(1.5), 创建 eframe。
2. LlamaLauncherApp::new: 系统语言→zh/en; 读取 llama_cpp_launcher_settings.json。
3. auto_start_preset_name存在 → 应用预设 → auto_start_server_on_first_frame=true。
4. update(): tab_selected(i18n key) → 路由到对应面板。
5. Drop trait → stop server/RPC + save settings。

## Windows特有功能 (高层)

- 开机自启: HKEY_CURRENT_USER\...\Run (enable/disable_auto_start)。
- 桌面快捷方式: shortcuts-rs, locale-aware名称。

## 全局约束

- 单binary, Windows优先。
- 进程管理: std::process::Child + Arc<Mutex<>> + Drop自动停止。
- 日志: BufReader → VecDeque<LogEntry>(server/rpc 各自独立), 10_000行上限。
- i18n: 所有UI文本通过 i18n::t(Key, lang), 禁止硬编码中文/英文到 UI 代码中。
- egui 0.33 API; Result<T, String> + map_err。

## ANTI-PATTERNS (THIS PROJECT)

- 在 UI 面板中直接启动/停止进程（必须走 engine）。
- 绕过 i18n::t() 硬编码中文/英文到 UI 代码。
- main.rs / app.rs 中写死启动参数而不经过 AppSettings/Preset。

## COMMANDS

- cargo build --release → target/release/llama_cpp_launcher.exe（同级 llama_cpp_launcher_settings.json）。