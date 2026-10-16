use crate::config::settings::AppSettings;
use crate::engine::ErrorInfo;
use crate::i18n;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) const MAX_LOG_LINES: usize = 10_000; // 日志环形缓冲区最大行数

/// 使用正则表达式匹配分片文件（.partNofM 或 .partNofM.gguf 模式）
fn is_shard_file(filename: &str) -> bool {
    // 匹配 "model.gguf.part1of3" 或 "model.gguf.part1of3.gguf"
    // 匹配 .partNofM 后缀（前面可以是任意字符）
    regex::Regex::new(r"\.part\d+of\d+(?:\.gguf)?$").is_ok_and(|re| re.is_match(filename))
}

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, PartialEq)]
pub enum ServerState {
    Idle,
    Starting,
    Running,
    Stopping,
    Error(ErrorInfo),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub text: String,
    pub level: LogLevel,
}

struct InnerState {
    child: Option<Child>,
    logs: VecDeque<LogEntry>,
    progress: f32, // 预填充进度 0.0~1.0
}

pub struct ServerManager {
    state: ServerState,
    inner: Arc<Mutex<InnerState>>,
    launch_command: Option<String>,
    _threads: Vec<thread::JoinHandle<()>>,
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerManager {
    pub fn new() -> Self {
        Self {
            state: ServerState::Idle,
            inner: Arc::new(Mutex::new(InnerState {
                child: None,
                logs: VecDeque::new(),
                progress: 0.0,
            })),
            launch_command: None,
            _threads: Vec::new(),
        }
    }

    pub fn state(&self) -> ServerState {
        self.state.clone()
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, ServerState::Running)
    }

    pub fn status_text(&self, lang: &i18n::Language) -> String {
        match &self.state {
            ServerState::Idle => i18n::t(i18n::Key::StatusIdle, lang).to_string(),
            ServerState::Starting => i18n::t(i18n::Key::StatusStarting, lang).to_string(),
            ServerState::Running => i18n::t(i18n::Key::StatusRunning, lang).to_string(),
            ServerState::Stopping => i18n::t(i18n::Key::StatusStopping, lang).to_string(),
            ServerState::Error(err) => {
                format!(
                    "{}: {}",
                    i18n::t(i18n::Key::StatusError, lang),
                    err.text(lang)
                )
            }
        }
    }

    // 对外仍返回 Vec，内部使用 VecDeque 作环形缓冲
    pub fn logs(&self) -> Vec<LogEntry> {
        let inner = self.inner.lock().unwrap();
        inner.logs.iter().cloned().collect()
    }

    // 判断 Server 是否已输出 "llama_server: listening on"（表示真正就绪）
    pub fn is_listening(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .logs
            .iter()
            .any(|e| e.text.contains("llama_server: listening on"))
    }

    pub fn clear_logs(&mut self) {
        self.inner.lock().unwrap().logs.clear();
        self.inner.lock().unwrap().progress = 0.0;
    }

    pub fn progress(&self) -> f32 {
        self.inner.lock().unwrap().progress
    }

    /// 基于时间戳+位置的单字母标识符检测日志等级
    pub(crate) fn detect_log_level(line: &str) -> Option<LogLevel> {
        let line = line.trim_start();

        // 必须以数字开头（类似时间戳）
        if !line
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            return None;
        }

        // 找到第一个空格，前面视为时间戳段
        let Some(first_space) = line.find(' ') else {
            return None;
        };

        let ts_part = &line[..first_space];

        // 时间戳段只允许数字和点
        if ts_part.chars().any(|c| !(c.is_ascii_digit() || c == '.')) {
            return None;
        }

        // 至少两个点，看起来更像时间戳而不是普通数字
        let dot_count = ts_part.chars().filter(|&c| c == '.').count();
        if dot_count < 2 {
            return None;
        }

        // 时间戳后是单字母等级标识符 I / W / E，后面接空格或结尾
        let rest = &line[first_space + 1..];
        if rest.is_empty() {
            return None;
        }

        match rest.as_bytes()[0] {
            b'I' if (rest.len() == 1 || rest.as_bytes().get(1).is_some_and(|&b| b == b' ')) => {
                return Some(LogLevel::Info);
            }
            b'W' if (rest.len() == 1 || rest.as_bytes().get(1).is_some_and(|&b| b == b' ')) => {
                return Some(LogLevel::Warn);
            }
            b'E' if (rest.len() == 1 || rest.as_bytes().get(1).is_some_and(|&b| b == b' ')) => {
                return Some(LogLevel::Error);
            }
            _ => {}
        };

        None
    }

    // 从日志文本中解析 progress = 0.xx，并更新进度值
    // 不修改原始日志内容，只提取进度
    fn parse_progress(text: &str) -> (String, Option<f32>) {
        let mut progress = None;

        if let Some(pos) = text.find("progress = ") {
            let rest = &text[pos + "progress = ".len()..];
            // 取所有终止符中的最小位置（避免空格/逗号优先级问题）
            let end = [
                rest.find(' '),
                rest.find('\t'),
                rest.find(','),
                rest.find('\n'),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(rest.len());
            let num_str = rest[..end].trim();

            if let Ok(v) = num_str.parse::<f32>() {
                progress = Some(v.clamp(0.0, 1.0));
            }
        }

        // 保留原始日志不做裁剪
        (text.to_string(), progress)
    }

    pub fn launch_command(&self) -> Option<String> {
        self.launch_command.clone()
    }

    /// KVMem 专有参数的 CLI 片段：19 个 `--kvmem*` 开关 + `--kv-dtype` + `--spec-kv-dtype`。
    ///
    /// 返回空 Vec = 当前非 KVMem 版本（标准版不认这些参数，遇到即 argparse 失败退出）。
    /// 预览命令与 start() 必须共用本函数，两处各抄一份清单必然漂移。
    fn kvmem_args(settings: &AppSettings) -> Vec<String> {
        if !settings.llama_cpp_type_effective().is_kvmem() {
            return Vec::new();
        }
        let mut args: Vec<String> = Vec::new();
        args.push(if settings.kvmem_enabled {
            "--kvmem".to_string()
        } else {
            "--no-kvmem".to_string()
        });
        args.push(if settings.kvmem_trace {
            "--kvmem-trace".to_string()
        } else {
            "--no-kvmem-trace".to_string()
        });
        if settings.kvmem_budget != 0 {
            args.push("--kvmem-budget".to_string());
            args.push(settings.kvmem_budget.to_string());
        }
        if settings.kvmem_block_tokens != 128 {
            args.push("--kvmem-block-tokens".to_string());
            args.push(settings.kvmem_block_tokens.to_string());
        }
        if settings.kvmem_sink_tokens != 0 {
            args.push("--kvmem-sink-tokens".to_string());
            args.push(settings.kvmem_sink_tokens.to_string());
        }
        if settings.kvmem_gen_reserve != 256 {
            args.push("--kvmem-gen-reserve".to_string());
            args.push(settings.kvmem_gen_reserve.to_string());
        }
        if (settings.kvmem_gpu_ratio - 0.50).abs() > f32::EPSILON {
            args.push("--kvmem-gpu-ratio".to_string());
            args.push(settings.kvmem_gpu_ratio.to_string());
        }
        if settings.kvmem_cpu_gb != 0.0 {
            args.push("--kvmem-cpu-gb".to_string());
            args.push(settings.kvmem_cpu_gb.to_string());
        }
        if settings.kvmem_nvme_gb != 0.0 {
            args.push("--kvmem-nvme-gb".to_string());
            args.push(settings.kvmem_nvme_gb.to_string());
        }
        if settings.kvmem_recent_tokens != 0 {
            args.push("--kvmem-recent-tokens".to_string());
            args.push(settings.kvmem_recent_tokens.to_string());
        }
        if settings.kvmem_method != "retrieval" {
            args.push("--kvmem-method".to_string());
            args.push(settings.kvmem_method.clone());
        }
        if settings.kvmem_query_last != 64 {
            args.push("--kvmem-query-last".to_string());
            args.push(settings.kvmem_query_last.to_string());
        }
        if settings.kvmem_query_max_tokens != 512 {
            args.push("--kvmem-query-max-tokens".to_string());
            args.push(settings.kvmem_query_max_tokens.to_string());
        }
        if settings.kvmem_query_replay != "auto" {
            args.push("--kvmem-query-replay".to_string());
            args.push(settings.kvmem_query_replay.clone());
        }
        if settings.kvmem_query_policy != "user" {
            args.push("--kvmem-query-policy".to_string());
            args.push(settings.kvmem_query_policy.clone());
        }
        if settings.kvmem_mtp_state != "replay" {
            args.push("--kvmem-mtp-state".to_string());
            args.push(settings.kvmem_mtp_state.clone());
        }
        // 留空 = 不下发，走服务端默认；beta2 默认是 Unix 路径 /tmp/kvmem_nvme，Windows 上不可用
        if !settings.kvmem_nvme_dir.is_empty() {
            args.push("--kvmem-nvme-dir".to_string());
            args.push(settings.kvmem_nvme_dir.clone());
        }
        if settings.kvmem_harvest_v {
            args.push("--kvmem-harvest-v".to_string());
        }
        // 文档：--kvmem-raw-k-nvme 需要配合 --kvmem-nvme-gb，NVMe 容量为 0 时下发无意义
        if settings.kvmem_raw_k_nvme && settings.kvmem_nvme_gb > 0.0 {
            args.push("--kvmem-raw-k-nvme".to_string());
        }
        // 这两个恒定下发（含默认值）：kvmem 的 --kv-dtype 默认 q8_0，
        // 而标准版 --cache-type-k 默认 f16 —— 不显式下发会静默改变 KV 精度/显存/质量。
        args.push("--kv-dtype".to_string());
        args.push(settings.kv_dtype.clone());
        args.push("--spec-kv-dtype".to_string());
        args.push(settings.spec_kv_dtype.clone());
        args
    }

    /// 根据当前设置构建启动命令字符串（不启动服务器）
    pub fn build_launch_command(&self, settings: &AppSettings) -> String {
        let server_path = &settings.server_path;
        let model_path = &settings.model_path;
        let is_kvmem = settings.llama_cpp_type_effective().is_kvmem();

        let mut args: Vec<String> = Vec::new();

        // RPC 模式
        if settings.rpc_mode {
            let clean_endpoints: String = settings
                .rpc_endpoints
                .split(',')
                .map(|s| s.trim().trim_end_matches('+').to_string())
                .collect::<Vec<_>>()
                .join(",");
            args.push("--rpc".to_string());
            args.push(clean_endpoints);
        }

        args.push("--model".to_string());
        args.push(model_path.display().to_string());
        args.push("--host".to_string());
        args.push(settings.host.clone());
        args.push("--port".to_string());
        args.push(settings.port.to_string());
        args.push("--ctx-size".to_string());
        args.push(settings.context_actual().to_string());
        args.push("--parallel".to_string());
        args.push(settings.parallel_slots.to_string());
        if settings.enable_batch_size {
            args.push("--batch-size".to_string());
            args.push(settings.batch_size_actual().to_string());
        }
        if settings.enable_ubatch_size {
            args.push("--ubatch-size".to_string());
            args.push(settings.ubatch_size_actual().to_string());
        }
        if settings.enable_session_timeout {
            args.push("--timeout".to_string());
            args.push(settings.session_timeout.to_string());
        }
        args.push("--gpu-layers".to_string());
        args.push(settings.gpu_layers_mode.to_arg());

        // 采样参数
        if settings.enable_temperature {
            args.push("--temperature".to_string());
            args.push(settings.temperature.to_string());
        }
        if settings.enable_top_p {
            args.push("--top-p".to_string());
            args.push(settings.top_p.to_string());
        }
        if settings.enable_top_k {
            args.push("--top-k".to_string());
            args.push(settings.top_k.to_string());
        }
        if settings.enable_repeat_penalty {
            args.push("--repeat-penalty".to_string());
            args.push(settings.repeat_penalty.to_string());
        }
        if settings.enable_presence_penalty {
            args.push("--presence-penalty".to_string());
            args.push(settings.presence_penalty.to_string());
        }

        // 思考参数
        if settings.enable_reasoning && !settings.reasoning.is_empty() && settings.reasoning != "auto" {
            args.push("--reasoning".to_string());
            args.push(settings.reasoning.clone());
        }
        if settings.enable_reasoning_format && !settings.reasoning_format.is_empty() && settings.reasoning_format != "auto" {
            args.push("--reasoning-format".to_string());
            args.push(settings.reasoning_format.clone());
        }
        if settings.enable_reasoning_effort && !settings.reasoning_effort.is_empty() && settings.reasoning_effort != "default" {
            args.push("--reasoning-effort".to_string());
            args.push(settings.reasoning_effort.clone());
        }
        if settings.enable_reasoning_budget && settings.reasoning_budget != -1 {
            args.push("--reasoning-budget".to_string());
            args.push(settings.reasoning_budget.to_string());
        }
        if settings.enable_reasoning_preserve {
            if settings.reasoning_preserve {
                args.push("--reasoning-preserve".to_string());
            } else {
                args.push("--no-reasoning-preserve".to_string());
            }
        }

        // 会话模板参数
        if !settings.chat_template.is_empty() {
            args.push("--chat-template".to_string());
            args.push(settings.chat_template.clone());
        }
        if !settings.chat_template_file.as_os_str().is_empty() {
            args.push("--chat-template-file".to_string());
            args.push(settings.chat_template_file.display().to_string());
        }
        if settings.jinja_enabled {
            args.push("--jinja".to_string());
        } else {
            args.push("--no-jinja".to_string());
        }

        // 采样器扩展
        if settings.enable_min_p && settings.min_p > 0.0 {
            args.push("--min-p".to_string());
            args.push(settings.min_p.to_string());
        }
        if settings.enable_top_n_sigma && settings.top_n_sigma > 0.0 {
            args.push("--top-n-sigma".to_string());
            args.push(settings.top_n_sigma.to_string());
        }
        if settings.enable_xtc && settings.xtc_probability > 0.0 {
            args.push("--xtc-probability".to_string());
            args.push(settings.xtc_probability.to_string());
            if settings.xtc_threshold < 1.0 {
                args.push("--xtc-threshold".to_string());
                args.push(settings.xtc_threshold.to_string());
            }
        }
        if settings.enable_typical_p && settings.typical_p < 1.0 {
            args.push("--typical-p".to_string());
            args.push(settings.typical_p.to_string());
        }
        if settings.enable_mirostat && settings.mirostat != 0 {
            args.push("--mirostat".to_string());
            args.push(settings.mirostat.to_string());
            if settings.mirostat_lr != 0.10 {
                args.push("--mirostat-lr".to_string());
                args.push(settings.mirostat_lr.to_string());
            }
            if settings.mirostat_ent != 5.00 {
                args.push("--mirostat-ent".to_string());
                args.push(settings.mirostat_ent.to_string());
            }
        }
        if settings.enable_dynatemp && settings.dynatemp_range > 0.0 {
            args.push("--dynatemp-range".to_string());
            args.push(settings.dynatemp_range.to_string());
            if settings.dynatemp_exp != 1.0 {
                args.push("--dynatemp-exp".to_string());
                args.push(settings.dynatemp_exp.to_string());
            }
        }
        if !settings.sampler_seq.is_empty() {
            args.push("--sampler-seq".to_string());
            args.push(settings.sampler_seq.clone());
        }

        // Flash Attention
        if settings.enable_flash_attn && !settings.flash_attn.is_empty() {
            args.push("--flash-attn".to_string());
            args.push(settings.flash_attn.clone());
        }

        // 内存自动调优
        if settings.enable_fit && !settings.fit.is_empty() {
            args.push("--fit".to_string());
            args.push(settings.fit.clone());
        }
        if settings.enable_fit_target && !settings.fit_target.is_empty() {
            args.push("--fit-target".to_string());
            args.push(settings.fit_target.clone());
        }
        if settings.enable_fit_ctx && settings.fit_ctx > 0 {
            args.push("--fit-ctx".to_string());
            args.push(settings.fit_ctx_actual().to_string());
        }

        // 多模态投影
        if !settings.mmproj_path.as_os_str().is_empty() {
            args.push("--mmproj".to_string());
            args.push(settings.mmproj_path.display().to_string());
        }

        // 多模态参数
        if settings.enable_mmproj_auto {
            match settings.mmproj_auto.as_str() {
                "no-mmproj" => args.push("--no-mmproj".to_string()),
                "no-mmproj-auto" => args.push("--no-mmproj-auto".to_string()),
                _ => args.push("--mmproj-auto".to_string()),
            }
        }
        if settings.enable_mmproj_offload {
            if settings.mmproj_offload {
                args.push("--mmproj-offload".to_string());
            } else {
                args.push("--no-mmproj-offload".to_string());
            }
        }
        if !settings.mmproj_device.is_empty() {
            args.push("--mmproj-device".to_string());
            args.push(settings.mmproj_device.clone());
        }
        if settings.image_min_tokens > 0 {
            args.push("--image-min-tokens".to_string());
            args.push(settings.image_min_tokens.to_string());
        }
        if settings.image_max_tokens > 0 {
            args.push("--image-max-tokens".to_string());
            args.push(settings.image_max_tokens.to_string());
        }
        if settings.mtmd_batch_max_tokens != 1024 {
            args.push("--mtmd-batch-max-tokens".to_string());
            args.push(settings.mtmd_batch_max_tokens.to_string());
        }
        if (settings.video_fps - 4.0).abs() > f32::EPSILON {
            args.push("--video-fps".to_string());
            args.push(format!("{}", settings.video_fps));
        }
        if settings.video_timestamp_interval != 5000 {
            args.push("--video-timestamp-interval".to_string());
            args.push(settings.video_timestamp_interval.to_string());
        }
        if !settings.video_ffmpeg_dir.is_empty() {
            args.push("--video-ffmpeg-dir".to_string());
            args.push(settings.video_ffmpeg_dir.clone());
        }

        // 模型别名
        if !settings.alias.is_empty() {
            args.push("--alias".to_string());
            args.push(settings.alias.clone());
        }

        // DFlash / Speculative Decoding 参数
        if !settings.dflash_path.as_os_str().is_empty() {
            args.push("--model-draft".to_string());
            args.push(settings.dflash_path.display().to_string());
        }
        if settings.spec_type != "none" {
            args.push("--spec-type".to_string());
            args.push(settings.spec_type.clone());
        }
        if settings
            .spec_type
            .split(',')
            .any(|s| s.starts_with("draft-"))
        {
            if settings.enable_spec_draft_n_max {
                args.push("--spec-draft-n-max".to_string());
                args.push(settings.spec_draft_n_max.to_string());
            }
            if settings.enable_spec_draft_n_min {
                args.push("--spec-draft-n-min".to_string());
                args.push(settings.spec_draft_n_min.to_string());
            }
            if settings.enable_spec_draft_p_min {
                args.push("--spec-draft-p-min".to_string());
                args.push(format!("{}", settings.spec_draft_p_min));
            }
            if settings.enable_spec_draft_p_split {
                args.push("--spec-draft-p-split".to_string());
                args.push(format!("{}", settings.spec_draft_p_split));
            }
            // KVMem 版无这两个参数（已合并为 --spec-kv-dtype），下发会导致启动失败
            if settings.enable_spec_draft_type_k
                && !settings.spec_draft_type_k.is_empty()
                && !is_kvmem
            {
                args.push("--spec-draft-type-k".to_string());
                args.push(settings.spec_draft_type_k.clone());
            }
            if settings.enable_spec_draft_type_v
                && !settings.spec_draft_type_v.is_empty()
                && !is_kvmem
            {
                args.push("--spec-draft-type-v".to_string());
                args.push(settings.spec_draft_type_v.clone());
            }
        }

        // ngram 各类型独立参数：每个选中的类型输出自己的 size-n / size-m / min-hits
        for (ngram_ty, size_n, size_m, min_hits) in [
            (
                "ngram-simple",
                settings.spec_ngram_simple_size_n,
                settings.spec_ngram_simple_size_m,
                settings.spec_ngram_simple_min_hits,
            ),
            (
                "ngram-map-k",
                settings.spec_ngram_map_k_size_n,
                settings.spec_ngram_map_k_size_m,
                settings.spec_ngram_map_k_min_hits,
            ),
            (
                "ngram-map-k4v",
                settings.spec_ngram_map_k4v_size_n,
                settings.spec_ngram_map_k4v_size_m,
                settings.spec_ngram_map_k4v_min_hits,
            ),
        ] {
            if settings.spec_type.split(',').any(|s| s.trim() == ngram_ty) {
                let prefix = format!("--spec-{}", ngram_ty);
                args.push(format!("{}-size-n", prefix));
                args.push(size_n.to_string());
                args.push(format!("{}-size-m", prefix));
                args.push(size_m.to_string());
                args.push(format!("{}-min-hits", prefix));
                args.push(min_hits.to_string());
            }
        }
        // ngram-mod 专用参数（与 start() 一致：列表含 ngram-mod 时拼接）
        if settings
            .spec_type
            .split(',')
            .any(|s| s.trim() == "ngram-mod")
        {
            args.push("--spec-ngram-mod-n-min".to_string());
            args.push(settings.spec_ngram_mod_n_min.to_string());
            args.push("--spec-ngram-mod-n-max".to_string());
            args.push(settings.spec_ngram_mod_n_max.to_string());
            args.push("--spec-ngram-mod-n-match".to_string());
            args.push(settings.spec_ngram_mod_n_match.to_string());
        }

        // KV 缓存（与 start() 一致：true 拼 --kv-offload，false 拼 --no-kv-offload）
        if settings.enable_kv_offload && settings.kv_offload {
            args.push("--kv-offload".to_string());
        } else if settings.enable_kv_offload {
            args.push("--no-kv-offload".to_string());
        }
        // KVMem 版无这两个参数（已合并为 --kv-dtype），下发会导致启动失败
        if settings.enable_cache_type_k && !settings.cache_type_k.is_empty() && !is_kvmem {
            args.push("--cache-type-k".to_string());
            args.push(settings.cache_type_k.clone());
        }
        if settings.enable_cache_type_v && !settings.cache_type_v.is_empty() && !is_kvmem {
            args.push("--cache-type-v".to_string());
            args.push(settings.cache_type_v.clone());
        }
        // 注：--mlock / --no-mmap 已被 llama.cpp 标记为 DEPRECATED（由 --load-mode 取代），
        // 实际启动不发送它们；此处同样不拼接，保证「启动命令」预览与真实命令一致。
        if settings.enable_kv_unified && settings.kv_unified {
            args.push("--kv-unified".to_string());
        }
        if settings.enable_swa_full && settings.swa_full {
            args.push("--swa-full".to_string());
        }

        // 上下文检查点
        if settings.enable_ctx_checkpoints && settings.ctx_checkpoints != 32 {
            args.push("--ctx-checkpoints".to_string());
            args.push(settings.ctx_checkpoints.to_string());
        }
        if settings.enable_checkpoint_min_step && settings.checkpoint_min_step != 8192 {
            args.push("--checkpoint-min-step".to_string());
            args.push(settings.checkpoint_min_step.to_string());
        }

        // GPU 与设备分配
        if settings.enable_split_mode && !settings.split_mode.is_empty() && settings.split_mode != "layer" {
            args.push("--split-mode".to_string());
            args.push(settings.split_mode.clone());
        }
        if !settings.tensor_split.is_empty() {
            args.push("--tensor-split".to_string());
            args.push(settings.tensor_split.clone());
        }
        if settings.cpu_moe {
            args.push("--cpu-moe".to_string());
        }
        if !settings.cpu_moe && settings.n_cpu_moe > 0 {
            args.push("--n-cpu-moe".to_string());
            args.push(settings.n_cpu_moe.to_string());
        }
        if settings.enable_override_tensor && !settings.override_tensor.is_empty() {
            args.push("--override-tensor".to_string());
            args.push(settings.override_tensor.clone());
        }

        // CPU 线程数
        if settings.threads >= 0 {
            args.push("--threads".to_string());
            args.push(settings.threads.to_string());
        }
        if settings.threads_batch >= 0 {
            args.push("--threads-batch".to_string());
            args.push(settings.threads_batch.to_string());
        }
        if settings.n_predict >= 0 {
            args.push("--n-predict".to_string());
            args.push(settings.n_predict.to_string());
        }
        if settings.keep > 0 {
            args.push("--keep".to_string());
            args.push(settings.keep.to_string());
        }
        if settings.seed >= 0 {
            args.push("--seed".to_string());
            args.push(settings.seed.to_string());
        }
        if settings.enable_main_gpu && settings.main_gpu != 0 {
            args.push("--main-gpu".to_string());
            args.push(settings.main_gpu.to_string());
        }
        if !settings.device.is_empty() {
            args.push("--device".to_string());
            args.push(settings.device.clone());
        }

        // 长上下文 / 提示缓存
        if settings.enable_cache_prompt && settings.cache_prompt {
            args.push("--cache-prompt".to_string());
        } else if settings.enable_cache_prompt {
            args.push("--no-cache-prompt".to_string());
        }
        if settings.enable_cache_reuse && settings.cache_reuse > 0 {
            args.push("--cache-reuse".to_string());
            args.push(settings.cache_reuse.to_string());
        }
        if settings.enable_context_shift && settings.context_shift {
            args.push("--context-shift".to_string());
        }

        // 结构化输出
        if !settings.json_schema.is_empty() {
            args.push("--json-schema".to_string());
            args.push(settings.json_schema.clone());
        }
        if !settings.grammar.is_empty() {
            args.push("--grammar".to_string());
            args.push(settings.grammar.clone());
        }

        if settings.verbose {
            args.push("--verbose".to_string());
        }

        // 日志时间戳
        if settings.log_timestamps {
            args.push("--log-timestamps".to_string());
        } else {
            args.push("--no-log-timestamps".to_string());
        }

        // 日志级别（仅开启时拼接）
        if settings.enable_log_verbosity {
            args.push("--log-verbosity".to_string());
            args.push(settings.log_verbosity.to_string());
        }

        // 加载模式
        if settings.enable_load_mode && !settings.load_mode.is_empty() && settings.load_mode != "auto" {
            args.push("--load-mode".to_string());
            args.push(settings.load_mode.clone());
        }
        if settings.enable_tensor_read_lazy && !settings.tensor_read_lazy.is_empty() && settings.tensor_read_lazy != "auto" {
            args.push("--lazy-mode".to_string());
            args.push(settings.tensor_read_lazy.clone());
        }

        // API 安全 / 部署
        if !settings.api_key.is_empty() {
            args.push("--api-key".to_string());
            args.push(settings.api_key.clone());
        }
        if !settings.api_prefix.is_empty() {
            args.push("--api-prefix".to_string());
            args.push(settings.api_prefix.clone());
        }
        if !settings.cors_origins.is_empty() {
            args.push("--cors-origins".to_string());
            args.push(settings.cors_origins.clone());
        }
        if !settings.ssl_cert_file.as_os_str().is_empty() {
            args.push("--ssl-cert-file".to_string());
            args.push(settings.ssl_cert_file.display().to_string());
        }
        if !settings.ssl_key_file.as_os_str().is_empty() {
            args.push("--ssl-key-file".to_string());
            args.push(settings.ssl_key_file.display().to_string());
        }
        if settings.reuse_port {
            args.push("--reuse-port".to_string());
        }
        if settings.enable_numa && !settings.numa.is_empty() {
            args.push("--numa".to_string());
            args.push(settings.numa.clone());
        }
        if settings.enable_cpu_mask && !settings.cpu_mask.is_empty() {
            args.push("--cpu-mask".to_string());
            args.push(settings.cpu_mask.clone());
        }
        if settings.enable_cpu_range && !settings.cpu_range.is_empty() {
            args.push("--cpu-range".to_string());
            args.push(settings.cpu_range.clone());
        }
        if settings.enable_cpu_mask_batch && !settings.cpu_mask_batch.is_empty() {
            args.push("--cpu-mask-batch".to_string());
            args.push(settings.cpu_mask_batch.clone());
        }
        if settings.enable_cpu_range_batch && !settings.cpu_range_batch.is_empty() {
            args.push("--cpu-range-batch".to_string());
            args.push(settings.cpu_range_batch.clone());
        }

        // ===== 高级参数（长上下文 / 采样 / 适配器 / 服务 / 加载性能）=====
        if settings.cache_ram_enabled {
            args.push("--cache-ram".to_string());
            args.push(settings.cache_ram.to_string());
        }
        if settings.enable_no_host && settings.no_host {
            args.push("--no-host".to_string());
        }
        if !settings.rope_scaling.is_empty() {
            args.push("--rope-scaling".to_string());
            args.push(settings.rope_scaling.clone());
        }
        if (settings.rope_scale - 1.0).abs() > f32::EPSILON {
            args.push("--rope-scale".to_string());
            args.push(settings.rope_scale.to_string());
        }
        if settings.rope_freq_base > 0.0 {
            args.push("--rope-freq-base".to_string());
            args.push(settings.rope_freq_base.to_string());
        }
        if settings.rope_freq_scale > 0.0 {
            args.push("--rope-freq-scale".to_string());
            args.push(settings.rope_freq_scale.to_string());
        }
        if settings.yarn_orig_ctx > 0 {
            args.push("--yarn-orig-ctx".to_string());
            args.push(settings.yarn_orig_ctx.to_string());
        }
        if settings.yarn_ext_factor >= 0.0 {
            args.push("--yarn-ext-factor".to_string());
            args.push(settings.yarn_ext_factor.to_string());
        }
        if settings.yarn_attn_factor >= 0.0 {
            args.push("--yarn-attn-factor".to_string());
            args.push(settings.yarn_attn_factor.to_string());
        }
        if settings.yarn_beta_slow >= 0.0 {
            args.push("--yarn-beta-slow".to_string());
            args.push(settings.yarn_beta_slow.to_string());
        }
        if settings.yarn_beta_fast >= 0.0 {
            args.push("--yarn-beta-fast".to_string());
            args.push(settings.yarn_beta_fast.to_string());
        }
        if settings.frequency_penalty.abs() > f32::EPSILON {
            args.push("--frequency-penalty".to_string());
            args.push(settings.frequency_penalty.to_string());
        }
        if settings.repeat_last_n >= 0 {
            args.push("--repeat-last-n".to_string());
            args.push(settings.repeat_last_n.to_string());
        }
        if settings.dry_multiplier > 0.0 {
            args.push("--dry-multiplier".to_string());
            args.push(settings.dry_multiplier.to_string());
            args.push("--dry-base".to_string());
            args.push(settings.dry_base.to_string());
            args.push("--dry-allowed-length".to_string());
            args.push(settings.dry_allowed_length.to_string());
            args.push("--dry-penalty-last-n".to_string());
            args.push(settings.dry_penalty_last_n.to_string());
        }
        if !settings.dry_sequence_breaker.is_empty() {
            args.push("--dry-sequence-breaker".to_string());
            args.push(settings.dry_sequence_breaker.clone());
        }
        if !settings.logit_bias.is_empty() {
            args.push("--logit-bias".to_string());
            args.push(settings.logit_bias.clone());
        }
        if !settings.lora_path.is_empty() {
            args.push("--lora".to_string());
            args.push(settings.lora_path.clone());
        }
        if !settings.lora_scaled.is_empty() {
            args.push("--lora-scaled".to_string());
            args.push(settings.lora_scaled.clone());
        }
        if !settings.control_vector.is_empty() {
            args.push("--control-vector".to_string());
            args.push(settings.control_vector.clone());
        }
        if !settings.slot_save_path.is_empty() {
            args.push("--slot-save-path".to_string());
            args.push(settings.slot_save_path.clone());
        }
        if settings.sleep_idle_seconds > 0 {
            args.push("--sleep-idle-seconds".to_string());
            args.push(settings.sleep_idle_seconds.to_string());
        }
        if settings.threads_http > 0 {
            args.push("--threads-http".to_string());
            args.push(settings.threads_http.to_string());
        }
        if settings.metrics_enabled {
            args.push("--metrics".to_string());
        }
        if !settings.api_key_file.is_empty() {
            args.push("--api-key-file".to_string());
            args.push(settings.api_key_file.clone());
        }
        if settings.check_tensors {
            args.push("--check-tensors".to_string());
        }
        if !settings.override_kv.is_empty() {
            args.push("--override-kv".to_string());
            args.push(settings.override_kv.clone());
        }
        if settings.prio != 0 {
            args.push("--prio".to_string());
            args.push(settings.prio.to_string());
        }
        if settings.enable_warmup {
            if settings.warmup_enabled {
                args.push("--warmup".to_string());
            } else {
                args.push("--no-warmup".to_string());
            }
        }
        if settings.enable_cont_batching {
            if settings.cont_batching {
                args.push("--cont-batching".to_string());
            } else {
                args.push("--no-cont-batching".to_string());
            }
        }
        if !settings.grammar_file.is_empty() {
            args.push("--grammar-file".to_string());
            args.push(settings.grammar_file.clone());
        }
        if !settings.json_schema_file.is_empty() {
            args.push("--json-schema-file".to_string());
            args.push(settings.json_schema_file.clone());
        }
        if settings.embeddings_enabled {
            args.push("--embeddings".to_string());
        }
        if settings.enable_pooling && !settings.pooling.is_empty() {
            args.push("--pooling".to_string());
            args.push(settings.pooling.clone());
        }
        if settings.rerank_enabled {
            args.push("--rerank".to_string());
        }

        // MCP 配置（预览：内联 JSON 走 --mcp-servers-json，文档定义的内联形式；
        // 实际启动用 --mcp-servers-config 传生成文件路径，两者语义等价）
        if settings.mcp_enabled {
            if let Some(mcp_json) = settings.build_effective_mcp_json() {
                args.push("--mcp-servers-json".to_string());
                args.push(mcp_json.to_string());
            }
        }

        args.extend(Self::kvmem_args(settings));

        format!("{} {}", server_path.display(), args.join(" "))
    }

    /// 检查 llama-server 文件是否存在
    pub fn check_server(&self, path: &std::path::Path) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }
        std::path::Path::new(path).exists()
    }

    /// 对 llama-server 文件授权读写权限（Linux 专用）
    #[cfg(target_os = "linux")]
    pub fn authorize_server(&self, path: &std::path::Path) -> Result<(), String> {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        if path.as_os_str().is_empty() {
            return Err("llama-server 路径为空".to_string());
        }

        if !path.exists() {
            return Err("llama-server 文件不存在".to_string());
        }

        // 设置读写执行权限 (rwxr-xr-x = 0o755)
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("设置 llama-server 权限失败：{}", e))?;

        Ok(())
    }

    pub fn start(&mut self, settings: &AppSettings) {
        if self.is_running() {
            return;
        }

        let server_path = settings.server_path.clone();
        let model_path = settings.model_path.clone();

        if server_path.as_os_str().is_empty() || model_path.as_os_str().is_empty() {
            self.state = ServerState::Error(ErrorInfo::Key(i18n::Key::ErrServerModelMissing));
            return;
        }

        // 检查模型路径是否为分片文件（如 model.gguf.part2of3）
        let model_filename = model_path.file_name().unwrap_or_default().to_string_lossy();
        let effective_model_path = if is_shard_file(&model_filename) {
            // 直接将 .partNofM 替换为 .part1ofM
            let re = regex::Regex::new(r"\.part(\d+)of(\d+)").unwrap();
            let resolved = re.replace(&model_filename, |caps: &regex::Captures| {
                let total = &caps[2];
                format!(".part1of{}", total)
            });
            // 解析为完整路径（相对于模型目录）
            model_path
                .parent()
                .map(|p| p.join(resolved.to_string()))
                .unwrap_or_else(|| std::path::PathBuf::from(resolved.to_string()))
        } else {
            model_path.clone()
        };

        if settings.port == settings.rpc_port {
            self.state = ServerState::Error(ErrorInfo::Key(i18n::Key::ErrPortConflict));
            return;
        }

        self.state = ServerState::Starting;
        self.clear_logs();
        self.launch_command = None;
        self._threads.clear();

        let mut cmd = Command::new(&server_path);
        let is_kvmem = settings.llama_cpp_type_effective().is_kvmem();

        // RPC 模式（置于启动命令首位）
        if settings.rpc_mode {
            // 剥离每段地址末尾的 + 号（多卡标记不参与启动参数）
            let clean_endpoints: String = settings
                .rpc_endpoints
                .split(',')
                .map(|s| s.trim().trim_end_matches('+').to_string())
                .collect::<Vec<_>>()
                .join(",");
            cmd.arg("--rpc").arg(&clean_endpoints);
        }

        cmd.arg("--model")
            .arg(&effective_model_path)
            .arg("--host")
            .arg(&settings.host)
            .arg("--port")
            .arg(settings.port.to_string())
            .arg("--ctx-size")
            .arg(settings.context_actual().to_string())
            .arg("--parallel")
            .arg(settings.parallel_slots.to_string());

        if settings.enable_batch_size {
            cmd.arg("--batch-size")
                .arg(settings.batch_size_actual().to_string());
        }
        if settings.enable_ubatch_size {
            cmd.arg("--ubatch-size")
                .arg(settings.ubatch_size_actual().to_string());
        }
        if settings.enable_session_timeout {
            cmd.arg("--timeout")
                .arg(settings.session_timeout.to_string());
        }

        cmd.arg("--gpu-layers")
            .arg(settings.gpu_layers_mode.to_arg());

        // 采样参数（根据启用标志决定是否拼接；ignore_* 系列为反向语义）
        if settings.enable_temperature {
            cmd.arg("--temperature")
                .arg(settings.temperature.to_string());
        }
        if settings.enable_top_p {
            cmd.arg("--top-p").arg(settings.top_p.to_string());
        }
        if settings.enable_top_k {
            cmd.arg("--top-k").arg(settings.top_k.to_string());
        }
        if settings.enable_repeat_penalty {
            cmd.arg("--repeat-penalty")
                .arg(settings.repeat_penalty.to_string());
        }
        if settings.enable_presence_penalty {
            cmd.arg("--presence-penalty")
                .arg(settings.presence_penalty.to_string());
        }

        // 思考参数（--reasoning 系列，非默认值才拼接）
        if settings.enable_reasoning && !settings.reasoning.is_empty() && settings.reasoning != "auto" {
            cmd.arg("--reasoning").arg(&settings.reasoning);
        }
        if settings.enable_reasoning_format && !settings.reasoning_format.is_empty() && settings.reasoning_format != "auto" {
            cmd.arg("--reasoning-format")
                .arg(&settings.reasoning_format);
        }
        if settings.enable_reasoning_effort && !settings.reasoning_effort.is_empty() && settings.reasoning_effort != "default" {
            cmd.arg("--reasoning-effort")
                .arg(&settings.reasoning_effort);
        }
        if settings.enable_reasoning_budget && settings.reasoning_budget != -1 {
            cmd.arg("--reasoning-budget")
                .arg(settings.reasoning_budget.to_string());
        }
        // 思考保留开关：开启 --reasoning-preserve，关闭 --no-reasoning-preserve
        if settings.enable_reasoning_preserve {
            if settings.reasoning_preserve {
                cmd.arg("--reasoning-preserve");
            } else {
                cmd.arg("--no-reasoning-preserve");
            }
        }

        // 会话模板参数
        if !settings.chat_template.is_empty() {
            cmd.arg("--chat-template").arg(&settings.chat_template);
        }
        if !settings.chat_template_file.as_os_str().is_empty() {
            cmd.arg("--chat-template-file")
                .arg(&settings.chat_template_file);
        }
        // Jinja 对话模板引擎开关：启用用 --jinja，禁用用 --no-jinja
        if settings.jinja_enabled {
            cmd.arg("--jinja");
        } else {
            cmd.arg("--no-jinja");
        }

        // 采样器扩展（llama.cpp b10488+ 新采样器）
        if settings.enable_min_p && settings.min_p > 0.0 {
            cmd.arg("--min-p").arg(settings.min_p.to_string());
        }
        if settings.enable_top_n_sigma && settings.top_n_sigma > 0.0 {
            cmd.arg("--top-n-sigma")
                .arg(settings.top_n_sigma.to_string());
        }
        if settings.enable_xtc && settings.xtc_probability > 0.0 {
            cmd.arg("--xtc-probability")
                .arg(settings.xtc_probability.to_string());
            if settings.xtc_threshold < 1.0 {
                cmd.arg("--xtc-threshold")
                    .arg(settings.xtc_threshold.to_string());
            }
        }
        if settings.enable_typical_p && settings.typical_p < 1.0 {
            cmd.arg("--typical-p").arg(settings.typical_p.to_string());
        }
        if settings.enable_mirostat && settings.mirostat != 0 {
            cmd.arg("--mirostat").arg(settings.mirostat.to_string());
            if settings.mirostat_lr != 0.10 {
                cmd.arg("--mirostat-lr")
                    .arg(settings.mirostat_lr.to_string());
            }
            if settings.mirostat_ent != 5.00 {
                cmd.arg("--mirostat-ent")
                    .arg(settings.mirostat_ent.to_string());
            }
        }
        if settings.enable_dynatemp && settings.dynatemp_range > 0.0 {
            cmd.arg("--dynatemp-range")
                .arg(settings.dynatemp_range.to_string());
            if settings.dynatemp_exp != 1.0 {
                cmd.arg("--dynatemp-exp")
                    .arg(settings.dynatemp_exp.to_string());
            }
        }
        if !settings.sampler_seq.is_empty() {
            cmd.arg("--sampler-seq").arg(&settings.sampler_seq);
        }

        // Flash Attention
        if settings.enable_flash_attn && !settings.flash_attn.is_empty() {
            cmd.arg("--flash-attn").arg(&settings.flash_attn);
        }

        // 内存自动调优
        if settings.enable_fit && !settings.fit.is_empty() {
            cmd.arg("--fit").arg(&settings.fit);
        }
        if settings.enable_fit_target && !settings.fit_target.is_empty() {
            cmd.arg("--fit-target").arg(&settings.fit_target);
        }
        if settings.enable_fit_ctx && settings.fit_ctx > 0 {
            cmd.arg("--fit-ctx")
                .arg(settings.fit_ctx_actual().to_string());
        }

        // 多模态投影
        if !settings.mmproj_path.as_os_str().is_empty() {
            cmd.arg("--mmproj").arg(&settings.mmproj_path);
        }

        // 多模态参数
        if settings.enable_mmproj_auto {
            match settings.mmproj_auto.as_str() {
                "no-mmproj" => {
                    cmd.arg("--no-mmproj");
                }
                "no-mmproj-auto" => {
                    cmd.arg("--no-mmproj-auto");
                }
                _ => {
                    cmd.arg("--mmproj-auto");
                }
            }
        }
        if settings.enable_mmproj_offload {
            if settings.mmproj_offload {
                cmd.arg("--mmproj-offload");
            } else {
                cmd.arg("--no-mmproj-offload");
            }
        }
        if !settings.mmproj_device.is_empty() {
            cmd.arg("--mmproj-device").arg(&settings.mmproj_device);
        }
        if settings.image_min_tokens > 0 {
            cmd.arg("--image-min-tokens")
                .arg(settings.image_min_tokens.to_string());
        }
        if settings.image_max_tokens > 0 {
            cmd.arg("--image-max-tokens")
                .arg(settings.image_max_tokens.to_string());
        }
        if settings.mtmd_batch_max_tokens != 1024 {
            cmd.arg("--mtmd-batch-max-tokens")
                .arg(settings.mtmd_batch_max_tokens.to_string());
        }
        if (settings.video_fps - 4.0).abs() > f32::EPSILON {
            cmd.arg("--video-fps")
                .arg(format!("{}", settings.video_fps));
        }
        if settings.video_timestamp_interval != 5000 {
            cmd.arg("--video-timestamp-interval")
                .arg(settings.video_timestamp_interval.to_string());
        }
        if !settings.video_ffmpeg_dir.is_empty() {
            cmd.arg("--video-ffmpeg-dir")
                .arg(&settings.video_ffmpeg_dir);
        }

        // 模型别名
        if !settings.alias.is_empty() {
            cmd.arg("--alias").arg(&settings.alias);
        }

        // DFlash / Speculative Decoding 参数整合
        let dflash_configured = !settings.dflash_path.as_os_str().is_empty();

        // 1) --model-draft: 如果配置了 DFlash，始终写入
        if dflash_configured {
            cmd.arg("--model-draft").arg(&settings.dflash_path);
        }

        // 2) --spec-type: 仅当用户明确选择非 none 时写入，不再自动 fallback dflash
        if settings.spec_type != "none" {
            cmd.arg("--spec-type").arg(&settings.spec_type);
        }

        // 3) --spec-draft-*: 仅在 spec_type 为 draft-* 时写入
        if settings
            .spec_type
            .split(',')
            .any(|s| s.starts_with("draft-"))
        {
            if settings.enable_spec_draft_n_max {
                cmd.arg("--spec-draft-n-max")
                    .arg(settings.spec_draft_n_max.to_string());
            }
            if settings.enable_spec_draft_n_min {
                cmd.arg("--spec-draft-n-min")
                    .arg(settings.spec_draft_n_min.to_string());
            }
            if settings.enable_spec_draft_p_min {
                cmd.arg("--spec-draft-p-min")
                    .arg(format!("{}", settings.spec_draft_p_min));
            }
            if settings.enable_spec_draft_p_split {
                cmd.arg("--spec-draft-p-split")
                    .arg(format!("{}", settings.spec_draft_p_split));
            }
            // KVMem 版无这两个参数（已合并为 --spec-kv-dtype），下发会导致启动失败
            if settings.enable_spec_draft_type_k
                && !settings.spec_draft_type_k.is_empty()
                && !is_kvmem
            {
                cmd.arg("--spec-draft-type-k")
                    .arg(&settings.spec_draft_type_k);
            }
            if settings.enable_spec_draft_type_v
                && !settings.spec_draft_type_v.is_empty()
                && !is_kvmem
            {
                cmd.arg("--spec-draft-type-v")
                    .arg(&settings.spec_draft_type_v);
            }
        }

        // 4) --spec-ngram-{type}-*: 各类型独立参数，每个选中的类型输出自己的 size-n / size-m / min-hits
        for (ngram_ty, size_n, size_m, min_hits) in [
            (
                "ngram-simple",
                settings.spec_ngram_simple_size_n,
                settings.spec_ngram_simple_size_m,
                settings.spec_ngram_simple_min_hits,
            ),
            (
                "ngram-map-k",
                settings.spec_ngram_map_k_size_n,
                settings.spec_ngram_map_k_size_m,
                settings.spec_ngram_map_k_min_hits,
            ),
            (
                "ngram-map-k4v",
                settings.spec_ngram_map_k4v_size_n,
                settings.spec_ngram_map_k4v_size_m,
                settings.spec_ngram_map_k4v_min_hits,
            ),
        ] {
            if settings.spec_type.split(',').any(|s| s.trim() == ngram_ty) {
                let prefix = format!("--spec-{}", ngram_ty);
                cmd.arg(format!("{}-size-n", prefix))
                    .arg(size_n.to_string());
                cmd.arg(format!("{}-size-m", prefix))
                    .arg(size_m.to_string());
                cmd.arg(format!("{}-min-hits", prefix))
                    .arg(min_hits.to_string());
            }
        }

        // 5) --spec-ngram-mod-*: ngram-mod 专用参数
        if settings.spec_type.split(',').any(|s| s == "ngram-mod") {
            cmd.arg("--spec-ngram-mod-n-min")
                .arg(settings.spec_ngram_mod_n_min.to_string());
            cmd.arg("--spec-ngram-mod-n-max")
                .arg(settings.spec_ngram_mod_n_max.to_string());
            cmd.arg("--spec-ngram-mod-n-match")
                .arg(settings.spec_ngram_mod_n_match.to_string());
        }

        // KV 缓存配置
        if settings.enable_kv_offload && settings.kv_offload {
            cmd.arg("--kv-offload");
        } else if settings.enable_kv_offload {
            cmd.arg("--no-kv-offload");
        }
        // KVMem 版无这两个参数（已合并为 --kv-dtype），下发会导致启动失败
        if settings.enable_cache_type_k && !settings.cache_type_k.is_empty() && !is_kvmem {
            cmd.arg("--cache-type-k").arg(&settings.cache_type_k);
        }
        if settings.enable_cache_type_v && !settings.cache_type_v.is_empty() && !is_kvmem {
            cmd.arg("--cache-type-v").arg(&settings.cache_type_v);
        }
        // 模型加载模式（--load-mode 替代已废弃的 --mmap/--mlock）
        // 仅非 auto 时拼接；auto 不拼接任何参数，由 llama-server 使用默认行为
        if settings.enable_load_mode && !settings.load_mode.is_empty() && settings.load_mode != "auto" {
            cmd.arg("--load-mode").arg(&settings.load_mode);
        }
        if settings.enable_tensor_read_lazy && !settings.tensor_read_lazy.is_empty() && settings.tensor_read_lazy != "auto" {
            cmd.arg("--lazy-mode").arg(&settings.tensor_read_lazy);
        }
        if settings.enable_kv_unified && settings.kv_unified {
            cmd.arg("--kv-unified");
        }
        if settings.enable_swa_full && settings.swa_full {
            cmd.arg("--swa-full");
        }

        // 上下文检查点
        if settings.enable_ctx_checkpoints && settings.ctx_checkpoints != 32 {
            cmd.arg("--ctx-checkpoints")
                .arg(settings.ctx_checkpoints.to_string());
        }

        // 最小检查点步长
        if settings.enable_checkpoint_min_step && settings.checkpoint_min_step != 8192 {
            cmd.arg("--checkpoint-min-step")
                .arg(settings.checkpoint_min_step.to_string());
        }

        // GPU 与设备分配
        if settings.enable_split_mode && !settings.split_mode.is_empty() && settings.split_mode != "layer" {
            cmd.arg("--split-mode").arg(&settings.split_mode);
        }
        if !settings.tensor_split.is_empty() {
            cmd.arg("--tensor-split").arg(&settings.tensor_split);
        }
        if settings.cpu_moe {
            cmd.arg("--cpu-moe");
        }
        // 关闭 cpu_moe 时才拼接 --n-cpu-moe
        if !settings.cpu_moe && settings.n_cpu_moe > 0 {
            cmd.arg("--n-cpu-moe").arg(settings.n_cpu_moe.to_string());
        }
        // 指定特定张量到缓冲区（非空才拼接）
        if settings.enable_override_tensor && !settings.override_tensor.is_empty() {
            cmd.arg("--override-tensor").arg(&settings.override_tensor);
        }

        // CPU 线程数（llama.cpp b10488+；-1 = 不拼接沿用默认）
        if settings.threads >= 0 {
            cmd.arg("--threads").arg(settings.threads.to_string());
        }
        if settings.threads_batch >= 0 {
            cmd.arg("--threads-batch")
                .arg(settings.threads_batch.to_string());
        }
        // 生成长度上限（-1 = 不拼接 = 无限生成）
        if settings.n_predict >= 0 {
            cmd.arg("--n-predict").arg(settings.n_predict.to_string());
        }
        // 保留前缀 token 数
        if settings.keep > 0 {
            cmd.arg("--keep").arg(settings.keep.to_string());
        }
        // 随机种子（-1 = 不拼接 = 随机）
        if settings.seed >= 0 {
            cmd.arg("--seed").arg(settings.seed.to_string());
        }
        // 主 GPU（多卡时指定）
        if settings.enable_main_gpu && settings.main_gpu != 0 {
            cmd.arg("--main-gpu").arg(settings.main_gpu.to_string());
        }
        // 设备（多卡时指定）
        if !settings.device.is_empty() {
            cmd.arg("--device").arg(&settings.device);
        }
        // 长上下文 / 提示缓存
        if settings.enable_cache_prompt && settings.cache_prompt {
            cmd.arg("--cache-prompt");
        } else if settings.enable_cache_prompt {
            cmd.arg("--no-cache-prompt");
        }
        if settings.enable_cache_reuse && settings.cache_reuse > 0 {
            cmd.arg("--cache-reuse")
                .arg(settings.cache_reuse.to_string());
        }
        if settings.enable_context_shift && settings.context_shift {
            cmd.arg("--context-shift");
        }
        // 结构化输出（JSON Schema / Grammar）
        if !settings.json_schema.is_empty() {
            cmd.arg("--json-schema").arg(&settings.json_schema);
        }
        if !settings.grammar.is_empty() {
            cmd.arg("--grammar").arg(&settings.grammar);
        }

        if settings.verbose {
            cmd.arg("--verbose");
        }

        // 日志时间戳
        if settings.log_timestamps {
            cmd.arg("--log-timestamps");
        } else {
            cmd.arg("--no-log-timestamps");
        }

        // 日志级别（仅开启时拼接；0=generic 1=error 2=warning 3=info 4=trace 5=debug）
        if settings.enable_log_verbosity {
            cmd.arg("--log-verbosity")
                .arg(settings.log_verbosity.to_string());
        }

        // 离线模式：拼接 --offline（如 llama.cpp 支持）
        if settings.offline_mode {
            cmd.arg("--offline");
        }

        // 网页客户端开关：启用用 --webui，禁用用 --no-webui
        if settings.web_ui_enabled {
            cmd.arg("--webui");
        } else {
            cmd.arg("--no-webui");
        }

        // ===== 高级参数（长上下文 / 采样 / 适配器 / 服务 / 加载性能）=====
        if settings.cache_ram_enabled {
            cmd.arg("--cache-ram").arg(settings.cache_ram.to_string());
        }
        if settings.enable_no_host && settings.no_host {
            cmd.arg("--no-host");
        }
        if !settings.rope_scaling.is_empty() {
            cmd.arg("--rope-scaling").arg(&settings.rope_scaling);
        }
        if (settings.rope_scale - 1.0).abs() > f32::EPSILON {
            cmd.arg("--rope-scale").arg(settings.rope_scale.to_string());
        }
        if settings.rope_freq_base > 0.0 {
            cmd.arg("--rope-freq-base")
                .arg(settings.rope_freq_base.to_string());
        }
        if settings.rope_freq_scale > 0.0 {
            cmd.arg("--rope-freq-scale")
                .arg(settings.rope_freq_scale.to_string());
        }
        if settings.yarn_orig_ctx > 0 {
            cmd.arg("--yarn-orig-ctx")
                .arg(settings.yarn_orig_ctx.to_string());
        }
        if settings.yarn_ext_factor >= 0.0 {
            cmd.arg("--yarn-ext-factor")
                .arg(settings.yarn_ext_factor.to_string());
        }
        if settings.yarn_attn_factor >= 0.0 {
            cmd.arg("--yarn-attn-factor")
                .arg(settings.yarn_attn_factor.to_string());
        }
        if settings.yarn_beta_slow >= 0.0 {
            cmd.arg("--yarn-beta-slow")
                .arg(settings.yarn_beta_slow.to_string());
        }
        if settings.yarn_beta_fast >= 0.0 {
            cmd.arg("--yarn-beta-fast")
                .arg(settings.yarn_beta_fast.to_string());
        }
        if settings.frequency_penalty.abs() > f32::EPSILON {
            cmd.arg("--frequency-penalty")
                .arg(settings.frequency_penalty.to_string());
        }
        if settings.repeat_last_n >= 0 {
            cmd.arg("--repeat-last-n")
                .arg(settings.repeat_last_n.to_string());
        }
        if settings.dry_multiplier > 0.0 {
            cmd.arg("--dry-multiplier")
                .arg(settings.dry_multiplier.to_string());
            cmd.arg("--dry-base").arg(settings.dry_base.to_string());
            cmd.arg("--dry-allowed-length")
                .arg(settings.dry_allowed_length.to_string());
            cmd.arg("--dry-penalty-last-n")
                .arg(settings.dry_penalty_last_n.to_string());
        }
        if !settings.dry_sequence_breaker.is_empty() {
            cmd.arg("--dry-sequence-breaker")
                .arg(&settings.dry_sequence_breaker);
        }
        if !settings.logit_bias.is_empty() {
            cmd.arg("--logit-bias").arg(&settings.logit_bias);
        }
        if !settings.lora_path.is_empty() {
            cmd.arg("--lora").arg(&settings.lora_path);
        }
        if !settings.lora_scaled.is_empty() {
            cmd.arg("--lora-scaled").arg(&settings.lora_scaled);
        }
        if !settings.control_vector.is_empty() {
            cmd.arg("--control-vector").arg(&settings.control_vector);
        }
        if !settings.slot_save_path.is_empty() {
            cmd.arg("--slot-save-path").arg(&settings.slot_save_path);
        }
        if settings.sleep_idle_seconds > 0 {
            cmd.arg("--sleep-idle-seconds")
                .arg(settings.sleep_idle_seconds.to_string());
        }
        if settings.threads_http > 0 {
            cmd.arg("--threads-http")
                .arg(settings.threads_http.to_string());
        }
        if settings.metrics_enabled {
            cmd.arg("--metrics");
        }
        if !settings.api_key_file.is_empty() {
            cmd.arg("--api-key-file").arg(&settings.api_key_file);
        }
        if settings.check_tensors {
            cmd.arg("--check-tensors");
        }
        if !settings.override_kv.is_empty() {
            cmd.arg("--override-kv").arg(&settings.override_kv);
        }
        if settings.prio != 0 {
            cmd.arg("--prio").arg(settings.prio.to_string());
        }
        if settings.enable_warmup {
            if settings.warmup_enabled {
                cmd.arg("--warmup");
            } else {
                cmd.arg("--no-warmup");
            }
        }
        if settings.enable_cont_batching {
            if settings.cont_batching {
                cmd.arg("--cont-batching");
            } else {
                cmd.arg("--no-cont-batching");
            }
        }
        if !settings.grammar_file.is_empty() {
            cmd.arg("--grammar-file").arg(&settings.grammar_file);
        }
        if !settings.json_schema_file.is_empty() {
            cmd.arg("--json-schema-file")
                .arg(&settings.json_schema_file);
        }
        if settings.embeddings_enabled {
            cmd.arg("--embeddings");
        }
        if settings.enable_pooling && !settings.pooling.is_empty() {
            cmd.arg("--pooling").arg(&settings.pooling);
        }
        if settings.rerank_enabled {
            cmd.arg("--rerank");
        }

        // MCP 工具：生成"当前启用"的 MCP 配置文件并传给 llama-server
        // （无启用 server 或生成失败时函数返回 None，不拼接参数）
        if let Some(mcp_config_path) = settings.write_effective_mcp_config() {
            cmd.arg("--mcp-servers-config").arg(&mcp_config_path);
        }

        // API 安全 / 部署（空值不拼接）
        if !settings.api_key.is_empty() {
            cmd.arg("--api-key").arg(&settings.api_key);
        }
        if !settings.api_prefix.is_empty() {
            cmd.arg("--api-prefix").arg(&settings.api_prefix);
        }
        if !settings.cors_origins.is_empty() {
            cmd.arg("--cors-origins").arg(&settings.cors_origins);
        }
        if !settings.ssl_cert_file.as_os_str().is_empty() {
            cmd.arg("--ssl-cert-file").arg(&settings.ssl_cert_file);
        }
        if !settings.ssl_key_file.as_os_str().is_empty() {
            cmd.arg("--ssl-key-file").arg(&settings.ssl_key_file);
        }
        if settings.reuse_port {
            cmd.arg("--reuse-port");
        }
        if settings.enable_numa && !settings.numa.is_empty() {
            cmd.arg("--numa").arg(&settings.numa);
        }
        if settings.enable_cpu_mask && !settings.cpu_mask.is_empty() {
            cmd.arg("--cpu-mask").arg(&settings.cpu_mask);
        }
        if settings.enable_cpu_range && !settings.cpu_range.is_empty() {
            cmd.arg("--cpu-range").arg(&settings.cpu_range);
        }
        if settings.enable_cpu_mask_batch && !settings.cpu_mask_batch.is_empty() {
            cmd.arg("--cpu-mask-batch").arg(&settings.cpu_mask_batch);
        }
        if settings.enable_cpu_range_batch && !settings.cpu_range_batch.is_empty() {
            cmd.arg("--cpu-range-batch").arg(&settings.cpu_range_batch);
        }

        cmd.args(Self::kvmem_args(settings));

        // 记录启动命令
        let cmd_str = format!(
            "{} {}",
            server_path.display(),
            cmd.get_args()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        );

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Windows: 隐藏子进程的命令行窗口
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        match cmd.spawn() {
            Ok(child) => {
                {
                    let mut inner = self.inner.lock().unwrap();
                    inner.child = Some(child);
                }
                self.launch_command = Some(cmd_str);

                let inner_clone = Arc::clone(&self.inner);
                let stdout_thread = thread::spawn(move || {
                    let stdout = {
                        let mut inner = inner_clone.lock().unwrap();
                        if let Some(ref mut child) = inner.child {
                            child.stdout.take()
                        } else {
                            None
                        }
                    };
                    if let Some(stdout) = stdout {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            match line {
                                Ok(l) => {
                                    // 优先使用基于时间戳+位置的单字母等级检测
                                    let level = match Self::detect_log_level(&l) {
                                        Some(level) => level,
                                        None => {
                                            if l.contains("WARN") || l.contains("warn") {
                                                LogLevel::Warn
                                            } else if l.contains("ERROR") || l.contains("error") {
                                                LogLevel::Error
                                            } else {
                                                LogLevel::Info
                                            }
                                        }
                                    };

                                    let (text, p) = Self::parse_progress(&l);
                                    let mut inner = inner_clone.lock().unwrap();
                                    if let Some(v) = p {
                                        inner.progress = v;
                                    }
                                    // 超过上限时丢弃最旧的一行
                                    if inner.logs.len() >= MAX_LOG_LINES {
                                        inner.logs.pop_front();
                                    }
                                    inner.logs.push_back(LogEntry { text, level });
                                }
                                Err(_) => break,
                            }
                        }
                    }
                });

                let inner_clone2 = Arc::clone(&self.inner);
                let stderr_thread = thread::spawn(move || {
                    let stderr = {
                        let mut inner = inner_clone2.lock().unwrap();
                        if let Some(ref mut child) = inner.child {
                            child.stderr.take()
                        } else {
                            None
                        }
                    };
                    if let Some(stderr) = stderr {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            match line {
                                Ok(l) => {
                                    // 优先使用基于时间戳+位置的单字母等级检测
                                    let level = match Self::detect_log_level(&l) {
                                        Some(level) => level,
                                        None => {
                                            if l.contains("WARN") || l.contains("warn") {
                                                LogLevel::Warn
                                            } else if l.contains("ERROR") || l.contains("error") {
                                                LogLevel::Error
                                            } else {
                                                LogLevel::Info
                                            }
                                        }
                                    };
                                    let (text, p) = Self::parse_progress(&l);
                                    let mut inner = inner_clone2.lock().unwrap();
                                    if let Some(v) = p {
                                        inner.progress = v;
                                    }
                                    // 超过上限时丢弃最旧的一行
                                    if inner.logs.len() >= MAX_LOG_LINES {
                                        inner.logs.pop_front();
                                    }
                                    inner.logs.push_back(LogEntry { text, level });
                                }
                                Err(_) => break,
                            }
                        }
                    }
                });

                self._threads.push(stdout_thread);
                self._threads.push(stderr_thread);
            }
            Err(e) => {
                self.state = ServerState::Error(ErrorInfo::WithDetail(
                    i18n::Key::ErrStartFailed,
                    e.to_string(),
                ));
                self.launch_command = None;
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.inner.lock().unwrap().child.take() {
            self.state = ServerState::Stopping;
            let _ = child.kill();
            let _ = child.wait();
            self.state = ServerState::Idle;
        }
        self.launch_command = None;
        self._threads.clear();
    }

    pub fn poll_logs(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(ref mut child) = inner.child {
            if let Ok(Some(status)) = child.try_wait() {
                let exit_msg = if status.success() {
                    i18n::t(i18n::Key::StatusServerExited, &i18n::Language::En).to_string()
                } else {
                    format!(
                        "{}: {:?}",
                        i18n::t(i18n::Key::StatusServerCrashed, &i18n::Language::En),
                        status.code()
                    )
                };
                // 超过上限时丢弃最旧的一行
                if inner.logs.len() >= MAX_LOG_LINES {
                    inner.logs.pop_front();
                }
                inner.logs.push_back(LogEntry {
                    text: exit_msg,
                    level: LogLevel::Warn,
                });
                self.state = ServerState::Idle;
                // 清除 child，防止每帧重复添加崩溃日志
                inner.child = None;
            }
        }
        drop(inner);

        if matches!(self.state, ServerState::Starting) {
            self.state = ServerState::Running;
        }
    }
}

impl Drop for ServerManager {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod adv_params_tests {
    use super::*;
    use crate::config::settings::AppSettings;

    /// 高级参数默认状态不发送，设置后应出现在启动命令中
    #[test]
    fn test_advanced_params_in_launch_command() {
        let mgr = ServerManager::new();
        let mut s = AppSettings::default();

        // 1) 默认状态：这些参数一个都不该出现
        let base = mgr.build_launch_command(&s);
        for p in [
            "--cache-ram",
            "--no-host",
            "--rope-scaling",
            "--rope-scale",
            "--yarn-orig-ctx",
            "--frequency-penalty",
            "--repeat-last-n",
            "--dry-multiplier",
            "--logit-bias",
            "--lora",
            "--slot-save-path",
            "--sleep-idle-seconds",
            "--threads-http",
            "--metrics",
            "--check-tensors",
            "--no-warmup",
            "--no-cont-batching",
            "--grammar-file",
            "--json-schema-file",
            "--embeddings",
            "--rerank",
        ] {
            assert!(
                !base.contains(p),
                "默认设置不应包含参数 {}，实际命令：\n{}",
                p,
                base
            );
        }

        // 默认时这三个开关为「启用 + 正向」，显式发送正向参数。
        // llama.cpp 本身默认即为开启，显式发送与不发送行为等价。
        // --pooling 同理：默认值为 none，即 llama.cpp 的默认池化方式。
        let base_tokens: Vec<&str> = base.split_whitespace().collect();
        for p in ["--warmup", "--cont-batching"] {
            assert!(
                base_tokens.contains(&p),
                "默认设置应包含参数 {}，实际命令：\n{}",
                p,
                base
            );
        }

        // 2) 全部打开后：逐个校验是否拼接正确
        s.cache_ram_enabled = true;
        s.cache_ram = 32768;
        s.no_host = true;
        s.rope_scaling = "yarn".to_string();
        s.rope_scale = 4.0;
        s.rope_freq_base = 1000000.0;
        s.yarn_orig_ctx = 32768;
        s.frequency_penalty = 0.5;
        s.repeat_last_n = 128;
        s.dry_multiplier = 0.8;
        s.dry_sequence_breaker = "\n:1".to_string();
        s.logit_bias = "EOS-inf".to_string();
        s.lora_path = "adapter.gguf".to_string();
        s.control_vector = "cv.gguf".to_string();
        s.slot_save_path = "E:/slot".to_string();
        s.sleep_idle_seconds = 600;
        s.threads_http = 4;
        s.metrics_enabled = true;
        s.api_key_file = "keys.txt".to_string();
        s.check_tensors = true;
        s.override_kv = "general.rope=float:8".to_string();
        s.prio = 2;
        s.warmup_enabled = false;
        s.cont_batching = false;
        s.grammar_file = "g.gbnf".to_string();
        s.json_schema_file = "s.json".to_string();
        s.embeddings_enabled = true;
        s.pooling = "mean".to_string();
        s.rerank_enabled = true;

        let cmd = mgr.build_launch_command(&s);
        for p in [
            "--cache-ram 32768",
            "--no-host",
            "--rope-scaling yarn",
            "--rope-scale 4",
            "--rope-freq-base 1000000",
            "--yarn-orig-ctx 32768",
            "--frequency-penalty 0.5",
            "--repeat-last-n 128",
            "--dry-multiplier 0.8",
            "--dry-base 1.75",
            "--dry-allowed-length 2",
            "--dry-penalty-last-n 64",
            "--dry-sequence-breaker",
            "--logit-bias EOS-inf",
            "--lora adapter.gguf",
            "--control-vector cv.gguf",
            "--slot-save-path E:/slot",
            "--sleep-idle-seconds 600",
            "--threads-http 4",
            "--metrics",
            "--api-key-file keys.txt",
            "--check-tensors",
            "--override-kv general.rope=float:8",
            "--prio 2",
            "--no-warmup",
            "--no-cont-batching",
            "--grammar-file g.gbnf",
            "--json-schema-file s.json",
            "--embeddings",
            "--pooling mean",
            "--rerank",
        ] {
            assert!(cmd.contains(p), "命令应包含参数 {}，实际命令：\n{}", p, cmd);
        }
    }

    /// 启用开关关闭时不拼接任何形式；开启且内层为假时发送负向参数。
    /// 按空白切分后精确比对 token，避免 "--warmup" 命中 "--no-warmup" 子串。
    #[test]
    fn test_warmup_cont_batching_enable_switch() {
        let mgr = ServerManager::new();
        let mut s = AppSettings::default();

        s.enable_warmup = false;
        s.enable_cont_batching = false;
        let cmd = mgr.build_launch_command(&s);
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        for p in ["--warmup", "--no-warmup", "--cont-batching", "--no-cont-batching"] {
            assert!(
                !tokens.contains(&p),
                "启用开关关闭时不应包含参数 {}，实际命令：\n{}",
                p,
                cmd
            );
        }

        s.enable_warmup = true;
        s.warmup_enabled = false;
        s.enable_cont_batching = true;
        s.cont_batching = false;
        let cmd = mgr.build_launch_command(&s);
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        for p in ["--no-warmup", "--no-cont-batching"] {
            assert!(
                tokens.contains(&p),
                "内层关闭时应包含参数 {}，实际命令：\n{}",
                p,
                cmd
            );
        }
        for p in ["--warmup", "--cont-batching"] {
            assert!(
                !tokens.contains(&p),
                "内层关闭时不应出现正向参数 {}，实际命令：\n{}",
                p,
                cmd
            );
        }
    }

    /// KVMem 参数只在 llama_cpp_type 判定为 Kvmem 时出现，且等于默认值的字段不拼接
    #[test]
    fn test_kvmem_params_gated_by_llama_cpp_type() {
        let mgr = ServerManager::new();

        let mut s = AppSettings::default();
        s.server_path = std::path::PathBuf::from("llama-kvmem-server.exe");
        let kvmem_default = mgr.build_launch_command(&s);
        assert!(
            kvmem_default.contains("--kvmem "),
            "KVMem 模式应带 --kvmem，实际命令：\n{}",
            kvmem_default
        );
        assert!(
            kvmem_default.contains("--no-kvmem-trace"),
            "trace 默认关闭应输出显式关闭开关，实际命令：\n{}",
            kvmem_default
        );
        for p in [
            "--kvmem-budget",
            "--kvmem-block-tokens",
            "--kvmem-sink-tokens",
            "--kvmem-gen-reserve",
            "--kvmem-gpu-ratio",
            "--kvmem-cpu-gb",
            "--kvmem-nvme-gb",
        ] {
            assert!(
                !kvmem_default.contains(p),
                "保持默认值时不应拼接 {}，实际命令：\n{}",
                p,
                kvmem_default
            );
        }

        let mut std_s = s.clone();
        std_s.llama_cpp_type = Some(crate::config::settings::LlamaCppType::Standard);
        let std_cmd = mgr.build_launch_command(&std_s);
        assert!(
            !std_cmd.contains("--kvmem"),
            "标准模式不得出现任何 --kvmem*，实际命令：\n{}",
            std_cmd
        );

        s.kvmem_trace = true;
        s.kvmem_enabled = false;
        s.kvmem_budget = 4096;
        s.kvmem_block_tokens = 256;
        s.kvmem_sink_tokens = 512;
        s.kvmem_gen_reserve = 1024;
        s.kvmem_gpu_ratio = 0.75;
        s.kvmem_cpu_gb = 8.0;
        s.kvmem_nvme_gb = 64.0;
        let cmd = mgr.build_launch_command(&s);
        for p in [
            "--no-kvmem",
            "--kvmem-trace",
            "--kvmem-budget 4096",
            "--kvmem-block-tokens 256",
            "--kvmem-sink-tokens 512",
            "--kvmem-gen-reserve 1024",
            "--kvmem-gpu-ratio 0.75",
            "--kvmem-cpu-gb 8",
            "--kvmem-nvme-gb 64",
        ] {
            assert!(cmd.contains(p), "命令应包含参数 {}，实际命令：\n{}", p, cmd);
        }
    }

    /// Phase 2 检索策略 / 落盘参数；等于默认值的字段不拼接
    #[test]
    fn test_kvmem_phase2_retrieval_and_nvme_params() {
        let mgr = ServerManager::new();

        let mut s = AppSettings::default();
        s.server_path = std::path::PathBuf::from("llama-kvmem-server.exe");
        let default_cmd = mgr.build_launch_command(&s);
        for p in [
            "--kvmem-recent-tokens",
            "--kvmem-method",
            "--kvmem-query-last",
            "--kvmem-query-max-tokens",
            "--kvmem-query-replay",
            "--kvmem-query-policy",
            "--kvmem-mtp-state",
            "--kvmem-nvme-dir",
            "--kvmem-harvest-v",
            "--kvmem-raw-k-nvme",
        ] {
            assert!(
                !default_cmd.contains(p),
                "Phase 2 参数 {} 保持默认值时不应拼接，实际命令：\n{}",
                p,
                default_cmd
            );
        }

        s.kvmem_recent_tokens = 333;
        s.kvmem_method = "recency".to_string();
        s.kvmem_query_last = 11;
        s.kvmem_query_max_tokens = 222;
        s.kvmem_query_replay = "legacy".to_string();
        s.kvmem_query_policy = "legacy".to_string();
        s.kvmem_mtp_state = "snapshots".to_string();
        s.kvmem_nvme_dir = "D:\\kvmem".to_string();
        s.kvmem_harvest_v = true;
        s.kvmem_raw_k_nvme = true;
        s.kvmem_nvme_gb = 64.0;

        let cmd = mgr.build_launch_command(&s);
        for p in [
            "--kvmem-recent-tokens 333",
            "--kvmem-method recency",
            "--kvmem-query-last 11",
            "--kvmem-query-max-tokens 222",
            "--kvmem-query-replay legacy",
            "--kvmem-query-policy legacy",
            "--kvmem-mtp-state snapshots",
            "--kvmem-harvest-v",
            "--kvmem-raw-k-nvme",
        ] {
            assert!(cmd.contains(p), "命令应包含参数 {}，实际命令：\n{}", p, cmd);
        }
        assert!(
            cmd.contains("--kvmem-nvme-dir D:\\kvmem"),
            "路径含空格时仍应作为单个参数下发，实际命令：\n{}",
            cmd
        );

        s.kvmem_nvme_gb = 0.0;
        let no_nvme = mgr.build_launch_command(&s);
        assert!(
            !no_nvme.contains("--kvmem-raw-k-nvme"),
            "NVMe 容量为 0 时 raw-K 无处落盘，不应下发，实际命令：\n{}",
            no_nvme
        );
    }

    /// KVMem 版把 K/V 两类 KV dtype 合并成 --kv-dtype / --spec-kv-dtype，
    /// 标准版那四个参数必须抑制，否则 kvmem 版 argparse 直接失败退出
    #[test]
    fn test_kvmem_suppresses_standard_only_kv_flags() {
        let mgr = ServerManager::new();

        let mut s = AppSettings::default();
        s.server_path = std::path::PathBuf::from("llama-kvmem-server.exe");
        s.enable_cache_type_k = true;
        s.cache_type_k = "q8_0".to_string();
        s.enable_cache_type_v = true;
        s.cache_type_v = "q8_0".to_string();
        s.spec_type = "draft-mtp".to_string();
        s.enable_spec_draft_type_k = true;
        s.spec_draft_type_k = "q8_0".to_string();
        s.enable_spec_draft_type_v = true;
        s.spec_draft_type_v = "q8_0".to_string();

        let cmd = mgr.build_launch_command(&s);
        for p in [
            "--cache-type-k",
            "--cache-type-v",
            "--spec-draft-type-k",
            "--spec-draft-type-v",
        ] {
            assert!(
                !cmd.contains(p),
                "KVMem 模式不得下发标准版专有参数 {}，实际命令：\n{}",
                p,
                cmd
            );
        }

        // 恒定下发（含默认值），否则 KV 精度会静默从 f16 变成 q8_0
        assert!(
            cmd.contains("--kv-dtype q8_0"),
            "KVMem 模式应恒定下发 --kv-dtype，实际命令：\n{}",
            cmd
        );
        assert!(
            cmd.contains("--spec-kv-dtype f16"),
            "KVMem 模式应恒定下发 --spec-kv-dtype，实际命令：\n{}",
            cmd
        );

        let mut std_s = s.clone();
        std_s.llama_cpp_type = Some(crate::config::settings::LlamaCppType::Standard);
        let std_cmd = mgr.build_launch_command(&std_s);
        assert!(
            std_cmd.contains("--cache-type-k q8_0"),
            "标准模式必须保留 --cache-type-k，实际命令：\n{}",
            std_cmd
        );
        assert!(
            !std_cmd.contains("--kv-dtype"),
            "标准模式不得出现 --kv-dtype，实际命令：\n{}",
            std_cmd
        );
    }

    /// Phase 1 回归：start() 曾完全漏掉 kvmem 参数（预览里有、真实进程 argv 里没有）。
    /// 这里 spawn 真实进程来验证 kvmem 参数确实进了 argv。
    #[test]
    #[cfg(target_os = "windows")]
    fn test_start_passes_kvmem_args_to_spawned_process() {
        let probe = std::path::PathBuf::from("C:\\Windows\\System32\\whoami.exe");
        if !probe.is_file() {
            return;
        }

        let mut s = AppSettings::default();
        s.server_path = probe;
        s.model_path = std::path::PathBuf::from("dummy.gguf");
        s.rpc_port = s.port + 1;
        s.llama_cpp_type = Some(crate::config::settings::LlamaCppType::Kvmem);
        s.kvmem_budget = 4096;
        s.kvmem_query_max_tokens = 222;
        s.kv_dtype = "q4_0".to_string();

        let mut mgr = ServerManager::new();
        mgr.start(&s);
        let cmd = mgr.launch_command().unwrap_or_default();
        assert!(
            cmd.contains("--kvmem-budget 4096"),
            "start() 未把 --kvmem-budget 下发给真实进程，实际：\n{}",
            cmd
        );
        assert!(
            cmd.contains("--kvmem-query-max-tokens 222"),
            "start() 未把 --kvmem-query-max-tokens 下发给真实进程，实际：\n{}",
            cmd
        );
        assert!(
            cmd.contains("--kv-dtype q4_0"),
            "start() 未把 --kv-dtype 下发给真实进程，实际：\n{}",
            cmd
        );
    }
}
