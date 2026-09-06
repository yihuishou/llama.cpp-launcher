# src/ui/ — UI 面板目录

## OVERVIEW

11 个 egui 面板 + 模型标签解析。纯渲染，业务逻辑委托给 app/config/engine。详见根 AGENTS.md / engine/AGENTS.md / config/AGENTS.md。

## STRUCTURE

- server_panel: llama-server 路径/端口/槽位、启停/重启、状态、RPC 模式开关
- rpc_panel: ggml-rpc-server 路径/端口/threads/device/cache、启停、状态
- model_panel: GGUF 目录浏览、列表、彩色标签解析、mmproj/DFlash 切换
- params_panel: n_ctx/n_predict/temperature/top_p/top_k/repeat_penalty/kv_offload/cache_type/GPU
- mcp_panel: MCP 服务器配置、状态管理
- log_panel: 服务器日志 ui() + 远程调用日志 rpc_ui()，共享 render()/LogSource trait
- rpc_log_panel: RPC 日志专用面板
- launch_commands_panel: server/RPC 最终启动命令只读展示
- presets_panel: 预设保存/应用/删除/重命名/自启动 + 分享/引入入口；返回 PresetPanelRequest（None/StartServer/StartRpc/OpenConfig）
- preset_share: 预设分享/引入模块（分享码 encode/decode、依赖声明 ShareDecl、引入配置窗口草稿隔离、声明阅读窗口）；进程生命周期仍走 engine，由 app.rs 响应 PresetPanelRequest
- settings_panel: 应用设置（主题、语言、日志、自动更新等）

## WHERE TO LOOK

| Task               | Location                     | Notes                  |
|--------------------|------------------------------|------------------------|
| 面板函数签名与路由约定        | ui.rs / app.rs（tab_selected） | 路由由 app.rs 控制；本目录只负责渲染 |
| model_panel 标签解析规则 | model_panel::parse_tags()    | 按文件名分段着色               |
| 预设分享/引入           | preset_share.rs              | ShareDecl/encode/decode/config_window |
| 日志渲染（跨 server/rpc） | log_panel::render()          | LogSource trait 抽象         |
| UI 组件（卡片/按钮/开关）   | widgets.rs                   | 可复用 UI 组件               |

## CONVENTIONS

- 路由由 app.rs 按 tab_selected(i18n key) 控制；本目录不直接管理标签切换。
- 面板函数签名统一：fn ui(&mut Ui, settings: &mut AppSettings, lang: &Language)
- 例外（允许额外参数）:
    - model_panel: +&mut ServerManager, &mut RpcManager；FileMode(Main/Mmproj/DFlash)；auto_detect_model_dir / is_dflash_file()
    - log_panel: ui() +&mut ServerManager（服务器日志）；rpc_ui() +&mut RpcManager（远程调用日志）
    - presets_panel: +PresetShareUi, PresetConfigUi, &RpcManager, 启动结果通知；返回 PresetPanelRequest 交给 app.rs 执行
- 弹窗（Window）统一由 app.rs/presets_panel 每帧渲染入口调用，窗口内文本同样走 i18n；
  深浅色自适应用 ui.visuals()，主题色填充用 accent + alpha 175（低饱和度规范）。

## ANTI-PATTERNS

- UI文本必须通过 i18n::t(Key, lang)，禁止硬编码。
- rfd: server/rpc面板 pick_file()，model面板 pick_folder()。
- 直接调用进程启动/停止；所有生命周期操作走 engine（ServerManager/RpcManager）。
- 面板中直接修改 AppSettings 持久化状态（应通过 app.rs 的 save()）