# src/engine/ — 引擎目录

## OVERVIEW

llama-server 和 ggml-rpc-server 的进程管理、状态机、日志聚合。具体参数和 UI 交互见根 AGENTS.md / ui/AGENTS.md。

## STRUCTURE

- server.rs: ServerManager, ServerState, LogEntry(text+LogLevel)；llama-server 生命周期 + launch_command 捕获 + 日志等级检测 + KVMem 参数拼接
- rpc.rs: RpcManager, RpcState, RpcConnection；ggml-rpc-server 生命周期（亦捕获 stdout/stderr 日志缓冲，与 server 同模式）
- mod.rs: ErrorInfo(Key/WithDetail)——存 i18n 键，status_text(lang) 渲染时实时解析

## WHERE TO LOOK

| Task                               | Location  | Notes                                    |
|------------------------------------|-----------|------------------------------------------|
| ServerManager 状态机                  | server.rs | Idle → Starting → Running/Stopping/Error |
| RpcManager 生命周期                    | rpc.rs    | Idle → Starting → Running/Stopping       |
| LogEntry / VecDeque<LogEntry> | server.rs:39 | 内部 VecDeque 环形缓冲，logs() 对外转 Vec；容量限制 10_000 行 |
| 日志等级检测                          | server.rs | detect_log_level() 基于时间戳+单字母标识符检测    |
| 进度解析                            | server.rs | parse_progress() 从日志提取 prefill 进度      |
| 启动命令构建                          | server.rs | build_launch_command() 生成 CLI 命令字符串    |
| KVMem 参数拼接                        | server.rs | kvmem_args(): 19 个 --kvmem* + --kv-dtype，仅 LlamaCppType::Kvmem 时 |
| ErrorInfo 语言无关错误                 | mod.rs    | 存 Key/WithDetail，渲染时解析，语言切换实时生效       |

## CONVENTIONS

- Arc<Mutex<InnerState>> 包裹 std::process::Child
- stdout/stderr: 各一个 thread::spawn, BufReader→lines→push_back
- Windows: CREATE_NO_WINDOW (0x08000000), cfg(windows) 分支
- Drop trait 自动 stop()
- 日志等级: 时间戳+单字母标识符（I/W/E）检测，回退到关键字匹配
- 日志容量: MAX_LOG_LINES = 10_000，超出丢弃最旧行
- 命令构建区: server.rs:216-1007（kvmem_args + build_launch_command + 变体参数拼接），改参数集中在此区
- 内联测试: server.rs:1923（rpc.rs 零测试）

## ANTI-PATTERNS

- start(): path非空 + is_file()；已运行则直接返回（幂等）
- 错误消息走 i18n，禁止硬编码。
- stop() 时通过 child.take() 使日志线程自然退出。
- UI 直接调用进程启动/停止（必须通过 ServerManager/RpcManager）