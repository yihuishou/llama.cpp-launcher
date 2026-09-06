# src/config/ — 配置与预设目录

## OVERVIEW

AppSettings/Preset/GpuLayersMode 定义、序列化策略、默认值以及配置文件读写入口。不直接参与 UI 渲染或进程管理。

## STRUCTURE

- settings.rs: AppSettings, Preset, GpuLayersMode, SettingsManager + Defaults/序列化辅助。
- mod.rs: pub use 导出配置模块对外接口。

## WHERE TO LOOK

| Task                | Location                                      | Notes                                       |
|---------------------|-----------------------------------------------|---------------------------------------------|
| AppSettings 字段含义    | settings.rs (AppSettings)                     | server/RPC/GPU/预设/UI偏好聚合                    |
| Preset 保存与应用逻辑      | settings.rs (Preset::from_settings, apply_to) | apply_to() 用于把预设写回 AppSettings              |
| GpuLayersMode 序列化细节 | settings.rs (GpuLayersMode + Visitor)         | 支持字符串与数字双格式兼容                               |
| 配置文件路径/读写           | settings.rs (SettingsManager)                 | llama_cpp_launcher_settings.json, load/save |
| 自动检测 server/RPC 路径  | settings.rs (auto_detect_server_path/rpc)     | find_exe_in_dir/find_exe_recursive          |
| 默认值定义              | settings.rs (default_* 函数)                  | 每个 AppSettings 字段对应一个 default_* 函数       |
| MCP 配置解析           | settings.rs (parse_mcp_servers)               | 解析 mcp_config_json 为服务器列表                  |

## CONVENTIONS

- AppSettings: serde Serialize + Deserialize；字段与 UI/启动参数一一对应。
- Preset:
    - apply_to(settings): 将预设值覆盖到指定设置实例，用于"加载预设"按钮。
    - from_settings(settings): 按当前设置生成新预设结构体。
- GpuLayersMode:
    - Auto/All/Manual；to_arg() 负责映射为 llama-server CLI 参数。
    - DeserializeVisitor: 兼容 "auto"/0, "all"/1, "manual"/2 等历史写法。
- default_* 函数: 每个 AppSettings 字段都有对应的 default_* 函数，集中管理默认值。
- 序列化辅助: from_raw_or_k() 处理 k 后缀的数值（如 4k → 4096）。

## ANTI-PATTERNS (THIS PROJECT)

- 绕过 AppSettings/Preset，在 UI 或 engine 中硬编码默认值（必须集中在此目录）。
- 直接修改 settings.rs 中的配置字段类型而不同步更新 UI/序列化逻辑。
- 在 default_* 函数中引入副作用（应仅为纯函数返回默认值）。