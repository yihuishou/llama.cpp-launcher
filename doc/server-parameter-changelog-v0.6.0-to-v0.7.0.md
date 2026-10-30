# llama-server 参数变更：v0.6.0 → v0.7.0

**对比文件**：`doc/server-parameter-v0.6.0.txt` vs `doc/server-parameter-v0.7.0.txt`
**生成日期**：2026-10-25

## ❌ 移除的参数（0.6 已标 DEPRECATED，0.7 彻底删除）

均由 `--load-mode` 取代：

| 参数 | 0.6 说明 |
|---|---|
| `--mlock` | 强制模型常驻 RAM，不交换/压缩（env: `LLAMA_ARG_MLOCK`） |
| `--mmap` / `--no-mmap` | 是否 memory-map 模型（env: `LLAMA_ARG_MMAP`） |
| `-dio` / `--direct-io` / `-ndio` / `--no-direct-io` | 是否使用 DirectIO（env: `LLAMA_ARG_DIO`） |

> 注：本项目 `engine/server.rs:658` 已不再拼接 `--mlock`/`--no-mmap`，与新行为一致。0.7 传入旧参数会报未知参数错误。

## ➕ 新增的参数

| 参数 | 说明 |
|---|---|
| `--moe-cache-mib N` | CPU 驻留 MoE 专家的 GPU 缓存大小（MiB），多 GPU 时按 `--tensor-split` 分配（默认 0 禁用，env: `LLAMA_ARG_MOE_CACHE_MIB`） |
| `--log-jsonl` / `--no-log-jsonl` | 以 JSONL 格式（每行一个 JSON 对象）输出日志到 stdout，同时关闭彩色日志（默认禁用，env: `LLAMA_ARG_LOG_JSONL`） |
| `--spec-draft-n-ubatch N` | draft 上下文物理最大 batch size，调小可省 VRAM（默认 0 = 同 `--n-ubatch`，env: `LLAMA_ARG_SPEC_DRAFT_N_UBATCH`） |
| `--spec-draft-sampling {greedy,probabilistic}` | draft 采样方式：greedy 取 argmax；probabilistic 对 draft 采样并由 target 用拒绝采样验证（默认 greedy，env: `LLAMA_ARG_SPEC_DRAFT_SAMPLING`） |

## 🔄 默认值 / 语义变化

| 参数 | v0.6.0 | v0.7.0 |
|---|---|---|
| `--port` | 默认 **8080** | 默认 **9931** ⚠️ |
| `--host` | 单 IP，或以 `.sock` 结尾绑定 UNIX socket（默认 127.0.0.1） | 支持**逗号分隔多地址**监听；多个 TCP 地址时 `::` 仅绑 IPv6；重叠地址行为未定义（默认 127.0.0.1） |
| `--spec-draft-device` / `-devd` | none = 不 offload（默认不 offload） | 默认**跟随 `--device`** |
| `-mmdev` / `--mmproj-device` | 默认 auto | 默认**跟随 `--device`** |
| `-cram` / `--cache-ram` | 默认 8192，-1 无限制，0 禁用 | 新增：decision 模型默认 0（其余不变） |
| `--json-schema` | 示例 `{}` 表示任意 JSON 对象；外部 `$ref` 的 schema 需用 `--grammar` + `json_schema_to_grammar.py` 转换 | 示例改为 `{"type": "object"}`；删除外部 `$ref` 限制说明 |

## 📝 新增环境变量（参数本身不变）

采样类参数新增环境变量支持：

| 参数 | 新增环境变量 |
|---|---|
| `--temp` / `--temperature` | `LLAMA_ARG_TEMPERATURE` |
| `--top-p` | `LLAMA_ARG_TOP_P` |
| `--min-p` | `LLAMA_ARG_MIN_P` |
| `--repeat-penalty` | `LLAMA_ARG_REPEAT_PENALTY` |
| `--presence-penalty` | `LLAMA_ARG_PRESENCE_PENALTY` |
| `--frequency-penalty` | `LLAMA_ARG_FREQUENCY_PENALTY` |
| `--slot-save-path` | `LLAMA_ARG_SLOT_SAVE_PATH` |

## 对启动器的影响

1. **`--port` 默认值 8080 → 9931**：若 `llama_cpp_launcher_settings.json` 中有沿用旧默认值的配置需注意。
2. **`--mlock` / `--no-mmap` 彻底失效**：0.7 传入会报未知参数（本项目已不拼接，无需改动）。
3. rpc-server 与 kvmem 文档无 0.7 版本，本次仅对比 server。
