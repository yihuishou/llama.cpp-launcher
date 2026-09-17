use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "llama_cpp_launcher_settings.json";

/// GPU 层数卸载模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuLayersMode {
    Auto,          // 自动
    All,           // 全部卸载到 GPU
    Manual(usize), // 手动指定层数
}

impl serde::Serialize for GpuLayersMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            GpuLayersMode::Auto => serializer.serialize_str("auto"),
            GpuLayersMode::All => serializer.serialize_str("all"),
            GpuLayersMode::Manual(n) => n.serialize(serializer),
        }
    }
}

impl<'de> serde::Deserialize<'de> for GpuLayersMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct GpuLayersModeVisitor;

        impl<'de> de::Visitor<'de> for GpuLayersModeVisitor {
            type Value = GpuLayersMode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("auto, all, or a number")
            }

            fn visit_str<E>(self, value: &str) -> Result<GpuLayersMode, E>
            where
                E: de::Error,
            {
                let v = value.trim();
                if v == "auto" || v == "-1" {
                    Ok(GpuLayersMode::Auto)
                } else if v == "all" || v == "999" {
                    Ok(GpuLayersMode::All)
                } else if let Ok(n) = v.parse::<usize>() {
                    Ok(GpuLayersMode::Manual(n))
                } else {
                    Err(de::Error::invalid_value(de::Unexpected::Str(value), &self))
                }
            }

            fn visit_u64<E>(self, v: u64) -> Result<GpuLayersMode, E>
            where
                E: de::Error,
            {
                Ok(GpuLayersMode::Manual(v as usize))
            }

            fn visit_i64<E>(self, v: i64) -> Result<GpuLayersMode, E>
            where
                E: de::Error,
            {
                if v == -1 {
                    Ok(GpuLayersMode::Auto)
                } else if v == 999 {
                    Ok(GpuLayersMode::All)
                } else {
                    Ok(GpuLayersMode::Manual(v as usize))
                }
            }
        }

        deserializer.deserialize_any(GpuLayersModeVisitor)
    }
}

impl GpuLayersMode {
    /// 生成 --gpu-layers 参数值
    pub fn to_arg(&self) -> String {
        match self {
            GpuLayersMode::Auto => "-1".to_string(),
            GpuLayersMode::All => "256".to_string(),
            GpuLayersMode::Manual(n) => n.to_string(),
        }
    }
}

fn default_flash_attn() -> String {
    "auto".to_string()
}

fn default_fit() -> String {
    "on".to_string() // --fit，"on" = 开启内存自动调优
}

fn default_fit_target() -> String {
    "1024".to_string() // --fit-target，默认 1024 MiB
}

fn default_fit_ctx() -> usize {
    4 // 4k = 4096
}

fn default_load_mode() -> String {
    "auto".to_string() // --load-mode，"auto" = 不拼接并沿用旧版 --mmap/--mlock
}

fn default_tensor_read_lazy() -> String {
    "auto".to_string()
}

// ── 新增参数（llama.cpp b10488+）默认值 ──

// 线程与生成长度
fn default_threads() -> i64 {
    -1 // --threads，-1 = 不拼接（沿用 llama 默认）
}

fn default_threads_batch() -> i64 {
    -1 // --threads-batch，-1 = 不拼接（沿用 llama 默认）
}

fn default_n_predict() -> i64 {
    -1 // --n-predict，-1 = 不拼接（无限生成）
}

fn default_keep_tokens() -> i64 {
    0 // --keep，0 = 不拼接
}

// 随机种子
fn default_seed() -> i64 {
    -1 // --seed，-1 = 不拼接（随机种子）
}

// 主 GPU（多卡时指定）
fn default_main_gpu() -> i64 {
    0 // --main-gpu，默认第一张卡；>=0 时拼接
}

// 采样器扩展
fn default_min_p() -> f32 {
    0.05 // --min-p，默认与 llama 一致
}

fn default_top_n_sigma() -> f32 {
    -1.0 // --top-n-sigma，-1 = 禁用（不拼接）
}

fn default_xtc_probability() -> f32 {
    0.0 // --xtc-probability，0 = 禁用（不拼接）
}

fn default_xtc_threshold() -> f32 {
    0.10
}

fn default_typical_p() -> f32 {
    1.0 // --typical-p，1.0 = 禁用（不拼接）
}

fn default_mirostat() -> i32 {
    0 // --mirostat，0 = 禁用
}

fn default_mirostat_lr() -> f32 {
    0.10
}

fn default_mirostat_ent() -> f32 {
    5.00
}

fn default_dynatemp_range() -> f32 {
    0.0 // --dynatemp-range，0 = 禁用（不拼接）
}

fn default_dynatemp_exp() -> f32 {
    1.0
}

fn default_sampler_seq() -> String {
    "".to_string() // --sampler-seq，空 = 不拼接（沿用 llama 默认 edskypmxt）
}

// 长上下文
fn default_cache_prompt() -> bool {
    true // --cache-prompt / --no-cache-prompt（llama 默认启用）
}

fn default_cache_reuse() -> i64 {
    0 // --cache-reuse，0 = 不拼接
}

fn default_context_shift() -> bool {
    false // --context-shift（llama 默认禁用）
}

// JSON/语法约束
fn default_json_schema() -> String {
    "".to_string() // --json-schema / --json-schema-file
}

fn default_grammar() -> String {
    "".to_string() // --grammar（空 = 不拼接）
}

fn default_api_key() -> String {
    "".to_string() // --api-key（空 = 不拼接）
}

fn default_api_prefix() -> String {
    "".to_string() // --api-prefix（空 = 不拼接）
}

fn default_cors_origins() -> String {
    "".to_string() // --cors-origins（空 = 不拼接）
}

fn default_numa() -> String {
    "".to_string() // --numa（空 = 不拼接）
}

fn default_cache_ram_enabled() -> bool {
    false // --cache-ram 启用开关 (prompt cache 内存上限)
}

fn default_cache_ram() -> usize {
    32768 // --cache-ram N (MiB, -1 不限, 0 禁用)
}

fn default_no_host() -> bool {
    false // --no-host (绕过 host buffer 以使用额外缓冲区)
}

fn default_rope_scaling() -> String {
    String::new() // --rope-scaling {none,linear,yarn} (空不拼接)
}

fn default_rope_scale() -> f32 {
    1.0 // --rope-scale N (上下文扩展倍数)
}

fn default_rope_freq_base() -> f32 {
    0.0 // --rope-freq-base N (0 不拼接)
}

fn default_rope_freq_scale() -> f32 {
    0.0 // --rope-freq-scale N (0 不拼接)
}

fn default_yarn_orig_ctx() -> usize {
    0 // --yarn-orig-ctx N (0 不拼接)
}

fn default_yarn_ext_factor() -> f32 {
    -1.0 // --yarn-ext-factor N (-1 不拼接)
}

fn default_yarn_attn_factor() -> f32 {
    -1.0 // --yarn-attn-factor N (-1 不拼接)
}

fn default_yarn_beta_slow() -> f32 {
    -1.0 // --yarn-beta-slow N (-1 不拼接)
}

fn default_yarn_beta_fast() -> f32 {
    -1.0 // --yarn-beta-fast N (-1 不拼接)
}

fn default_frequency_penalty() -> f32 {
    0.0 // --frequency-penalty N (0 不拼接)
}

fn default_repeat_last_n() -> i64 {
    -1 // --repeat-last-n N (-1 不拼接)
}

fn default_dry_multiplier() -> f32 {
    0.0 // --dry-multiplier N (0 不拼接 = 禁用)
}

fn default_dry_base() -> f32 {
    1.75 // --dry-base N
}

fn default_dry_allowed_length() -> i64 {
    2 // --dry-allowed-length N
}

fn default_dry_penalty_last_n() -> i64 {
    64 // --dry-penalty-last-n N
}

fn default_dry_sequence_breaker() -> String {
    String::new() // --dry-sequence-breaker STRING (空不拼接)
}

fn default_logit_bias() -> String {
    String::new() // --logit-bias (如 EOS-inf, 空不拼接)
}

fn default_lora_path() -> String {
    String::new() // --lora FNAME (逗号分隔多个, 空不拼接)
}

fn default_lora_scaled() -> String {
    String::new() // --lora-scaled FNAME:SCALE,... (空不拼接)
}

fn default_control_vector() -> String {
    String::new() // --control-vector FNAME (空不拼接)
}

fn default_slot_save_path() -> String {
    String::new() // --slot-save-path PATH (空不拼接)
}

fn default_sleep_idle_seconds() -> i64 {
    -1 // --sleep-idle-seconds N (-1 不拼接)
}

fn default_threads_http() -> i64 {
    -1 // --threads-http N (-1 不拼接)
}

fn default_metrics_enabled() -> bool {
    false // --metrics (Prometheus 指标端点)
}

fn default_api_key_file() -> String {
    String::new() // --api-key-file FNAME (空不拼接)
}

fn default_direct_io() -> bool {
    false // --direct-io / --no-direct-io
}

fn default_check_tensors() -> bool {
    false // --check-tensors (加载时校验张量)
}

fn default_override_kv() -> String {
    String::new() // --override-kv KEY=TYPE:VALUE,... (空不拼接)
}

fn default_prio() -> i64 {
    0 // --prio N (0 不拼接)
}

fn default_warmup_enabled() -> bool {
    true // --warmup / --no-warmup (关闭时发 --no-warmup)
}

fn default_cont_batching() -> bool {
    true // --cont-batching / --no-cont-batching (关闭时发 --no-)
}

fn default_grammar_file() -> String {
    String::new() // --grammar-file FNAME (空不拼接)
}

fn default_json_schema_file() -> String {
    String::new() // --json-schema-file FILE (空不拼接)
}

fn default_embeddings_enabled() -> bool {
    false // --embeddings (嵌入模式)
}

fn default_pooling() -> String {
    String::new() // --pooling {none,mean,cls,last,rank} (空不拼接)
}

fn default_rerank_enabled() -> bool {
    false // --rerank / --reranking (重排模式)
}

fn default_web_ui_enabled() -> bool {
    true
}

fn default_log_timestamps() -> bool {
    true
}

fn default_log_verbosity() -> usize {
    3 // --log-verbosity 默认 info
}

fn default_session_timeout() -> usize {
    600 // 会话超时（秒）默认值
}

fn default_auto_scroll_logs() -> bool {
    true
}

fn default_max_log_lines() -> i32 {
    100
}

fn default_log_to_file() -> bool {
    false
}

fn default_linux_compat_mode() -> bool {
    true
}

fn default_dark_mode() -> bool {
    true
}

fn default_theme_mode() -> String {
    "auto".to_string()
}

fn default_accent_color() -> String {
    "#FF2D55".to_string()
}

// llama.cpp 下载变体偏好默认值
fn default_download_variant() -> String {
    "cpu".to_string()
}

// llama.cpp 版本分支默认值
fn default_llama_branch() -> String {
    "main".to_string()
}

// ROCm GPU 目标型号默认值
fn default_rocm_gpu_target() -> String {
    "gfx103X".to_string()
}

// context / batch_size / ubatch_size 以 k 为单位存储 (1k = 1024)
// 反序列化时兼容旧版原始值（如 4096 → 自动转为 4）

fn default_context() -> usize {
    4 // 4k = 4096
}

fn default_batch_size() -> f32 {
    2.0 // 2k = 2048
}

fn default_ubatch_size() -> f32 {
    0.5 // 0.5k = 512
}

/// 值直接以 k 为单位存储，最小值为 1
fn from_raw_or_k(v: usize) -> usize {
    v.max(1)
}

mod deserialize_context {
    use super::from_raw_or_k;
    use serde::{self, Deserialize};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<usize, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = usize::deserialize(deserializer)?;
        Ok(from_raw_or_k(v))
    }
}

mod deserialize_batch_size {
    use serde::{self, Deserialize};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 兼容旧版 usize 值（如 2 → 2.0）
        let v = f32::deserialize(deserializer)?;
        Ok(v.max(0.0001))
    }
}

mod deserialize_ubatch_size {
    use serde::{self, Deserialize};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<f32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 兼容旧版整数格式（如 1 → 1.0）和浮点格式（如 0.5）
        let v = serde_json::Value::deserialize(deserializer)?;
        match v.as_f64() {
            Some(n) => {
                let val = n as f32;
                // 若为较大原始值（如 1024），转换为 k 单位
                if val >= 128.0 {
                    Ok((val / 1024.0).max(0.5))
                } else {
                    Ok(val.max(0.5))
                }
            }
            None => Ok(0.5),
        }
    }
}

// 推测解码（Speculative Decoding）默认值
fn default_spec_type() -> String {
    "none".to_string()
}

fn default_spec_draft_n_max() -> usize {
    2
}

fn default_spec_draft_p_min() -> f32 {
    0.75
}

fn default_spec_draft_p_split() -> f32 {
    1.0
}

fn default_spec_draft_type_k() -> String {
    "f16".to_string()
}

fn default_spec_draft_type_v() -> String {
    "f16".to_string()
}

// ngram 参数默认值（ngram-simple / ngram-map-k / ngram-map-k4v 共用）
fn default_spec_ngram_size_n() -> usize {
    12
}

fn default_spec_ngram_size_m() -> usize {
    48
}

fn default_spec_ngram_min_hits() -> usize {
    1
}

// ngram-mod 参数默认值
fn default_spec_ngram_mod_n_min() -> usize {
    48
}

fn default_spec_ngram_mod_n_max() -> usize {
    64
}

fn default_spec_ngram_mod_n_match() -> usize {
    24
}

// KV 缓存比例默认值
fn default_kv_cache_ratio() -> f32 {
    0.95
}

// 上下文检查点默认值
fn default_ctx_checkpoints() -> usize {
    32
}

// 最小检查点步长默认值
fn default_checkpoint_min_step() -> usize {
    512
}

// 思考与会话默认值
fn default_reasoning() -> String {
    "auto".to_string() // --reasoning: auto/on/off
}

fn default_reasoning_format() -> String {
    "auto".to_string() // --reasoning-format: auto/none/deepseek/deepseek-legacy
}

fn default_reasoning_effort() -> String {
    "default".to_string() // --reasoning-effort: 纯字符串值（如 high）；default = 不拼接
}

fn default_reasoning_budget() -> i32 {
    -1 // --reasoning-budget: -1 = 不限制
}

fn default_jinja_enabled() -> bool {
    true // --jinja / --no-jinja（新版 llama-server 默认启用）
}

fn default_release_channel() -> String {
    "stable".to_string()
}

fn default_true() -> bool {
    true
}

// 多模态（mmproj / mtmd / video）默认值
fn default_mmproj_auto() -> bool {
    true
}

fn default_mmproj_offload() -> bool {
    true
}

fn default_mmproj_device() -> String {
    "auto".to_string()
}

fn default_image_min_tokens() -> i64 {
    0
}

fn default_image_max_tokens() -> i64 {
    0
}

fn default_mtmd_batch_max_tokens() -> usize {
    1024
}

fn default_video_fps() -> f32 {
    4.0
}

fn default_video_timestamp_interval() -> usize {
    5000
}

// Duplicate definition removed - keeping only one instance above
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    // Server 配置
    pub host: String,
    pub port: u16,
    pub parallel_slots: i64,
    // 推理参数（以 k 为单位存储，1k = 1024）
    #[serde(
        default = "default_context",
        deserialize_with = "deserialize_context::deserialize"
    )]
    pub context: usize, // --ctx-size (k)
    #[serde(
        default = "default_batch_size",
        deserialize_with = "deserialize_batch_size::deserialize"
    )]
    pub batch_size: f32, // --batch-size (k, 0.0001 步进)
    #[serde(
        default = "default_ubatch_size",
        deserialize_with = "deserialize_ubatch_size::deserialize"
    )]
    pub ubatch_size: f32, // --ubatch-size (k, 0.0001 步进)
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repeat_penalty: f32,
    pub presence_penalty: f32,
    // 批次/超时参数启用标志（勾选时拼接启动命令）
    #[serde(default)]
    pub enable_batch_size: bool,
    #[serde(default)]
    pub enable_ubatch_size: bool,
    #[serde(default)]
    pub enable_session_timeout: bool,
    // 采样参数启用标志（勾选时拼接启动命令）
    #[serde(default)]
    pub enable_temperature: bool,
    #[serde(default)]
    pub enable_top_p: bool,
    #[serde(default)]
    pub enable_top_k: bool,
    #[serde(default)]
    pub enable_repeat_penalty: bool,
    #[serde(default)]
    pub enable_presence_penalty: bool,
    #[serde(default = "default_flash_attn")]
    pub flash_attn: String,
    // 内存自动调优 (--fit / --fit-target / --fit-ctx)
    #[serde(default = "default_fit")]
    pub fit: String, // --fit on/off
    #[serde(default = "default_fit_target")]
    pub fit_target: String, // --fit-target MiB（逗号分隔多卡）
    #[serde(default = "default_fit_ctx")]
    pub fit_ctx: usize, // --fit-ctx（最小上下文，k 单位）
    #[serde(default)]
    pub enable_fit_target: bool,
    #[serde(default)]
    pub enable_fit_ctx: bool,

    // 加载模式（新版 --load-mode；"auto" 时沿用旧版 --mmap/--mlock 行为）
    #[serde(default = "default_load_mode")]
    pub load_mode: String, // --load-mode
    #[serde(default = "default_tensor_read_lazy")]
    pub tensor_read_lazy: String, // --tensor-read-lazy

    // ── 线程与生成长度（llama.cpp b10488+）──
    #[serde(default = "default_threads")]
    pub threads: i64, // --threads (-1 不拼接)
    #[serde(default = "default_threads_batch")]
    pub threads_batch: i64, // --threads-batch (-1 不拼接)
    #[serde(default = "default_n_predict")]
    pub n_predict: i64, // --n-predict (-1 不拼接)
    #[serde(default = "default_keep_tokens")]
    pub keep: i64, // --keep (0 不拼接)
    #[serde(default = "default_seed")]
    pub seed: i64, // --seed (-1 不拼接)
    #[serde(default = "default_main_gpu")]
    pub main_gpu: i64, // --main-gpu
    #[serde(default)]
    pub device: String, // --device

    // ── 采样器扩展（近新版本加入）──
    #[serde(default)]
    pub enable_min_p: bool,
    #[serde(default = "default_min_p")]
    pub min_p: f32, // --min-p
    #[serde(default)]
    pub enable_top_n_sigma: bool,
    #[serde(default = "default_top_n_sigma")]
    pub top_n_sigma: f32, // --top-n-sigma
    #[serde(default)]
    pub enable_xtc: bool,
    #[serde(default = "default_xtc_probability")]
    pub xtc_probability: f32, // --xtc-probability
    #[serde(default = "default_xtc_threshold")]
    pub xtc_threshold: f32, // --xtc-threshold
    #[serde(default)]
    pub enable_typical_p: bool,
    #[serde(default = "default_typical_p")]
    pub typical_p: f32, // --typical-p
    #[serde(default)]
    pub enable_mirostat: bool,
    #[serde(default = "default_mirostat")]
    pub mirostat: i32, // --mirostat (0/1/2)
    #[serde(default = "default_mirostat_lr")]
    pub mirostat_lr: f32, // --mirostat-lr
    #[serde(default = "default_mirostat_ent")]
    pub mirostat_ent: f32, // --mirostat-ent
    #[serde(default)]
    pub enable_dynatemp: bool,
    #[serde(default = "default_dynatemp_range")]
    pub dynatemp_range: f32, // --dynatemp-range
    #[serde(default = "default_dynatemp_exp")]
    pub dynatemp_exp: f32, // --dynatemp-exp
    #[serde(default = "default_sampler_seq")]
    pub sampler_seq: String, // --sampler-seq (空不拼接)

    // ── 长上下文 / 提示缓存 ──
    #[serde(default = "default_cache_prompt")]
    pub cache_prompt: bool, // --cache-prompt / --no-cache-prompt
    #[serde(default = "default_cache_reuse")]
    pub cache_reuse: i64, // --cache-reuse (0 不拼接)
    #[serde(default = "default_context_shift")]
    pub context_shift: bool, // --context-shift

    // ── 结构化输出 ──
    #[serde(default = "default_json_schema")]
    pub json_schema: String, // --json-schema (空不拼接)
    #[serde(default = "default_grammar")]
    pub grammar: String, // --grammar (空不拼接)

    // ── API 安全 / 部署 ──
    #[serde(default = "default_api_key")]
    pub api_key: String, // --api-key (空不拼接)
    #[serde(default = "default_api_prefix")]
    pub api_prefix: String, // --api-prefix (空不拼接)
    #[serde(default = "default_cors_origins")]
    pub cors_origins: String, // --cors-origins (空不拼接)
    #[serde(default)]
    pub ssl_cert_file: PathBuf, // --ssl-cert-file (空不拼接)
    #[serde(default)]
    pub ssl_key_file: PathBuf, // --ssl-key-file (空不拼接)
    #[serde(default)]
    pub reuse_port: bool, // --reuse-port
    #[serde(default = "default_numa")]
    pub numa: String, // --numa (空不拼接)
    #[serde(default = "default_cache_ram_enabled")]
    pub cache_ram_enabled: bool, // --cache-ram 启用开关 (prompt cache 内存上限)
    #[serde(default = "default_cache_ram")]
    pub cache_ram: usize, // --cache-ram N (MiB, -1 不限, 0 禁用)
    #[serde(default = "default_no_host")]
    pub no_host: bool, // --no-host (绕过 host buffer 以使用额外缓冲区)
    #[serde(default = "default_rope_scaling")]
    pub rope_scaling: String, // --rope-scaling {none,linear,yarn} (空不拼接)
    #[serde(default = "default_rope_scale")]
    pub rope_scale: f32, // --rope-scale N (上下文扩展倍数)
    #[serde(default = "default_rope_freq_base")]
    pub rope_freq_base: f32, // --rope-freq-base N (0 不拼接)
    #[serde(default = "default_rope_freq_scale")]
    pub rope_freq_scale: f32, // --rope-freq-scale N (0 不拼接)
    #[serde(default = "default_yarn_orig_ctx")]
    pub yarn_orig_ctx: usize, // --yarn-orig-ctx N (0 不拼接)
    #[serde(default = "default_yarn_ext_factor")]
    pub yarn_ext_factor: f32, // --yarn-ext-factor N (-1 不拼接)
    #[serde(default = "default_yarn_attn_factor")]
    pub yarn_attn_factor: f32, // --yarn-attn-factor N (-1 不拼接)
    #[serde(default = "default_yarn_beta_slow")]
    pub yarn_beta_slow: f32, // --yarn-beta-slow N (-1 不拼接)
    #[serde(default = "default_yarn_beta_fast")]
    pub yarn_beta_fast: f32, // --yarn-beta-fast N (-1 不拼接)
    #[serde(default = "default_frequency_penalty")]
    pub frequency_penalty: f32, // --frequency-penalty N (0 不拼接)
    #[serde(default = "default_repeat_last_n")]
    pub repeat_last_n: i64, // --repeat-last-n N (-1 不拼接)
    #[serde(default = "default_dry_multiplier")]
    pub dry_multiplier: f32, // --dry-multiplier N (0 不拼接 = 禁用)
    #[serde(default = "default_dry_base")]
    pub dry_base: f32, // --dry-base N
    #[serde(default = "default_dry_allowed_length")]
    pub dry_allowed_length: i64, // --dry-allowed-length N
    #[serde(default = "default_dry_penalty_last_n")]
    pub dry_penalty_last_n: i64, // --dry-penalty-last-n N
    #[serde(default = "default_dry_sequence_breaker")]
    pub dry_sequence_breaker: String, // --dry-sequence-breaker STRING (空不拼接)
    #[serde(default = "default_logit_bias")]
    pub logit_bias: String, // --logit-bias (如 EOS-inf, 空不拼接)
    #[serde(default = "default_lora_path")]
    pub lora_path: String, // --lora FNAME (逗号分隔多个, 空不拼接)
    #[serde(default = "default_lora_scaled")]
    pub lora_scaled: String, // --lora-scaled FNAME:SCALE,... (空不拼接)
    #[serde(default = "default_control_vector")]
    pub control_vector: String, // --control-vector FNAME (空不拼接)
    #[serde(default = "default_slot_save_path")]
    pub slot_save_path: String, // --slot-save-path PATH (空不拼接)
    #[serde(default = "default_sleep_idle_seconds")]
    pub sleep_idle_seconds: i64, // --sleep-idle-seconds N (-1 不拼接)
    #[serde(default = "default_threads_http")]
    pub threads_http: i64, // --threads-http N (-1 不拼接)
    #[serde(default = "default_metrics_enabled")]
    pub metrics_enabled: bool, // --metrics (Prometheus 指标端点)
    #[serde(default = "default_api_key_file")]
    pub api_key_file: String, // --api-key-file FNAME (空不拼接)
    #[serde(default = "default_direct_io")]
    pub direct_io: bool, // --direct-io / --no-direct-io
    #[serde(default = "default_check_tensors")]
    pub check_tensors: bool, // --check-tensors (加载时校验张量)
    #[serde(default = "default_override_kv")]
    pub override_kv: String, // --override-kv KEY=TYPE:VALUE,... (空不拼接)
    #[serde(default = "default_prio")]
    pub prio: i64, // --prio N (0 不拼接)
    #[serde(default = "default_warmup_enabled")]
    pub warmup_enabled: bool, // --warmup / --no-warmup (关闭时发 --no-warmup)
    #[serde(default = "default_cont_batching")]
    pub cont_batching: bool, // --cont-batching / --no-cont-batching (关闭时发 --no-)
    #[serde(default = "default_grammar_file")]
    pub grammar_file: String, // --grammar-file FNAME (空不拼接)
    #[serde(default = "default_json_schema_file")]
    pub json_schema_file: String, // --json-schema-file FILE (空不拼接)
    #[serde(default = "default_embeddings_enabled")]
    pub embeddings_enabled: bool, // --embeddings (嵌入模式)
    #[serde(default = "default_pooling")]
    pub pooling: String, // --pooling {none,mean,cls,last,rank} (空不拼接)
    #[serde(default = "default_rerank_enabled")]
    pub rerank_enabled: bool, // --rerank / --reranking (重排模式)

    // 推测解码（Speculative Decoding）配置
    #[serde(default = "default_spec_type")]
    pub spec_type: String, // --spec-type
    #[serde(default = "default_spec_draft_n_max")]
    pub spec_draft_n_max: usize, // --spec-draft-n-max
    #[serde(default)]
    pub spec_draft_n_min: usize, // --spec-draft-n-min
    #[serde(default = "default_spec_draft_p_min")]
    pub spec_draft_p_min: f32, // --spec-draft-p-min
    #[serde(default = "default_spec_draft_p_split")]
    pub spec_draft_p_split: f32, // --spec-draft-p-split
    #[serde(default = "default_spec_draft_type_k")]
    pub spec_draft_type_k: String, // --spec-draft-type-k
    #[serde(default = "default_spec_draft_type_v")]
    pub spec_draft_type_v: String, // --spec-draft-type-v
    // draft-* 参数启用开关（关闭时忽略对应配置）
    #[serde(default)]
    pub enable_spec_draft_n_max: bool,
    #[serde(default)]
    pub enable_spec_draft_n_min: bool,
    #[serde(default)]
    pub enable_spec_draft_p_min: bool,
    #[serde(default)]
    pub enable_spec_draft_p_split: bool,
    #[serde(default)]
    pub enable_spec_draft_type_k: bool,
    #[serde(default)]
    pub enable_spec_draft_type_v: bool,
    // ngram 参数（ngram-simple / ngram-map-k / ngram-map-k4v 共用）
    #[serde(default = "default_spec_ngram_size_n")]
    pub spec_ngram_size_n: usize, // --spec-ngram-*-size-n
    #[serde(default = "default_spec_ngram_size_m")]
    pub spec_ngram_size_m: usize, // --spec-ngram-*-size-m
    #[serde(default = "default_spec_ngram_min_hits")]
    pub spec_ngram_min_hits: usize, // --spec-ngram-*-min-hits
    // ngram-mod 专用参数
    #[serde(default = "default_spec_ngram_mod_n_min")]
    pub spec_ngram_mod_n_min: usize, // --spec-ngram-mod-n-min
    #[serde(default = "default_spec_ngram_mod_n_max")]
    pub spec_ngram_mod_n_max: usize, // --spec-ngram-mod-n-max
    #[serde(default = "default_spec_ngram_mod_n_match")]
    pub spec_ngram_mod_n_match: usize, // --spec-ngram-mod-n-match

    // KV 缓存配置
    pub kv_offload: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub kv_unified: bool, // --kv-unified
    #[serde(default)]
    pub swa_full: bool, // --swa-full
    #[serde(default = "default_kv_cache_ratio")]
    pub kv_cache_ratio: f32, // KV 缓存比例 (不拼接启动命令)
    #[serde(default = "default_ctx_checkpoints")]
    pub ctx_checkpoints: usize, // --ctx-checkpoints
    #[serde(default = "default_checkpoint_min_step")]
    pub checkpoint_min_step: usize, // --checkpoint-min-step
    // GPU 与设备分配
    pub gpu_layers_mode: GpuLayersMode,
    pub split_mode: String,
    pub tensor_split: String,
    pub cpu_moe: bool,
    pub n_cpu_moe: usize,
    #[serde(default)]
    pub override_tensor: String, // --override-tensor
    // 高级
    pub verbose: bool,
    // 离线模式
    #[serde(default)]
    pub offline_mode: bool,

    // RPC 模式
    pub rpc_mode: bool,
    pub rpc_endpoints: String,

    // 模型与 RPC 配置
    #[serde(default)]
    pub model_path: PathBuf,
    #[serde(default)]
    pub mmproj_path: PathBuf,
    // ── 多模态（mmproj / mtmd / video）──
    #[serde(default = "default_mmproj_auto")]
    pub mmproj_auto: bool,
    #[serde(default = "default_mmproj_offload")]
    pub mmproj_offload: bool,
    #[serde(default = "default_mmproj_device")]
    pub mmproj_device: String,
    #[serde(default = "default_image_min_tokens")]
    pub image_min_tokens: i64,
    #[serde(default = "default_image_max_tokens")]
    pub image_max_tokens: i64,
    #[serde(default = "default_mtmd_batch_max_tokens")]
    pub mtmd_batch_max_tokens: usize,
    #[serde(default = "default_video_fps")]
    pub video_fps: f32,
    #[serde(default = "default_video_timestamp_interval")]
    pub video_timestamp_interval: usize,
    #[serde(default)]
    pub video_ffmpeg_dir: String,
    #[serde(default)]
    pub dflash_path: PathBuf,
    #[serde(default)]
    pub model_dir: PathBuf,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub rpc_threads: usize,
    #[serde(default)]
    pub rpc_device: String,
    #[serde(default)]
    pub rpc_cache: bool,

    // 网页客户端开关
    #[serde(default = "default_web_ui_enabled")]
    pub web_ui_enabled: bool,

    // 原生日志时间戳
    #[serde(default = "default_log_timestamps")]
    pub log_timestamps: bool,

    // 日志级别（0=generic 1=error 2=warning 3=info 4=trace 5=debug）
    #[serde(default = "default_log_verbosity")]
    pub log_verbosity: usize,

    // 会话超时（秒）
    #[serde(default = "default_session_timeout")]
    pub session_timeout: usize,

    // 思考与会话
    #[serde(default = "default_reasoning")]
    pub reasoning: String, // --reasoning: auto/on/off
    #[serde(default = "default_reasoning_format")]
    pub reasoning_format: String, // --reasoning-format
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String, // --reasoning-effort：纯字符串值（如 high）
    #[serde(default = "default_reasoning_budget")]
    pub reasoning_budget: i32, // --reasoning-budget: -1 = 不限制
    #[serde(default)]
    pub reasoning_preserve: Option<bool>, // --reasoning-preserve / --no-reasoning-preserve：None = 模型默认（不拼接）
    #[serde(default = "default_jinja_enabled")]
    pub jinja_enabled: bool, // --jinja / --no-jinja
    #[serde(default)]
    pub chat_template: String, // --chat-template（Jinja 模板文本）
    #[serde(default)]
    pub chat_template_file: PathBuf, // --chat-template-file（Jinja 模板文件）
    #[serde(default)]
    pub rocm_gpu_target: String, // --rocm-gpu-target

    // MCP 管理（llama.cpp --mcp-servers-config）
    #[serde(default)]
    pub mcp_enabled: bool, // MCP 功能总开关（无启用 server 时不拼接参数）
    #[serde(default)]
    pub mcp_server_states: std::collections::BTreeMap<String, bool>, // 每个 MCP server 的启用状态（按名称）

    // 外部引入标记（通过分享码导入的预设 = true；用于列表显示"引入"标签
    // 与"配置"按钮；配置完成后保留标记，配置内容可随时再改。不进分享码）
    #[serde(default)]
    pub imported: bool,

    // 引入时的依赖声明 JSON（ShareDecl 序列化；感叹号阅读窗口用。不进分享码）
    #[serde(default)]
    pub imported_decl: Option<String>,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: "127.0.0.1".to_string(),
            port: 9931,
            parallel_slots: -1,
            context: 4,       // 4k = 4096
            batch_size: 2.0,  // 2k = 2048
            ubatch_size: 0.5, // 0.5k = 512
            temperature: 0.8,
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            presence_penalty: 0.0,
            enable_batch_size: false,
            enable_ubatch_size: false,
            enable_session_timeout: false,
            enable_temperature: false,
            enable_top_p: false,
            enable_top_k: false,
            enable_repeat_penalty: false,
            enable_presence_penalty: false,
            flash_attn: default_flash_attn(),
            fit: default_fit(),
            fit_target: default_fit_target(),
            fit_ctx: default_fit_ctx(),
            enable_fit_target: false,
            enable_fit_ctx: false,
            load_mode: default_load_mode(),
            tensor_read_lazy: default_tensor_read_lazy(),
            threads: default_threads(),
            threads_batch: default_threads_batch(),
            n_predict: default_n_predict(),
            keep: default_keep_tokens(),
            seed: default_seed(),
            main_gpu: default_main_gpu(),
            device: String::new(),
            enable_min_p: false,
            min_p: default_min_p(),
            enable_top_n_sigma: false,
            top_n_sigma: default_top_n_sigma(),
            enable_xtc: false,
            xtc_probability: default_xtc_probability(),
            xtc_threshold: default_xtc_threshold(),
            enable_typical_p: false,
            typical_p: default_typical_p(),
            enable_mirostat: false,
            mirostat: default_mirostat(),
            mirostat_lr: default_mirostat_lr(),
            mirostat_ent: default_mirostat_ent(),
            enable_dynatemp: false,
            dynatemp_range: default_dynatemp_range(),
            dynatemp_exp: default_dynatemp_exp(),
            sampler_seq: default_sampler_seq(),
            cache_prompt: default_cache_prompt(),
            cache_reuse: default_cache_reuse(),
            context_shift: default_context_shift(),
            json_schema: default_json_schema(),
            grammar: default_grammar(),
            api_key: default_api_key(),
            api_prefix: default_api_prefix(),
            cors_origins: default_cors_origins(),
            ssl_cert_file: PathBuf::new(),
            ssl_key_file: PathBuf::new(),
            reuse_port: false,
            numa: default_numa(),
            cache_ram_enabled: default_cache_ram_enabled(),
            cache_ram: default_cache_ram(),
            no_host: default_no_host(),
            rope_scaling: default_rope_scaling(),
            rope_scale: default_rope_scale(),
            rope_freq_base: default_rope_freq_base(),
            rope_freq_scale: default_rope_freq_scale(),
            yarn_orig_ctx: default_yarn_orig_ctx(),
            yarn_ext_factor: default_yarn_ext_factor(),
            yarn_attn_factor: default_yarn_attn_factor(),
            yarn_beta_slow: default_yarn_beta_slow(),
            yarn_beta_fast: default_yarn_beta_fast(),
            frequency_penalty: default_frequency_penalty(),
            repeat_last_n: default_repeat_last_n(),
            dry_multiplier: default_dry_multiplier(),
            dry_base: default_dry_base(),
            dry_allowed_length: default_dry_allowed_length(),
            dry_penalty_last_n: default_dry_penalty_last_n(),
            dry_sequence_breaker: default_dry_sequence_breaker(),
            logit_bias: default_logit_bias(),
            lora_path: default_lora_path(),
            lora_scaled: default_lora_scaled(),
            control_vector: default_control_vector(),
            slot_save_path: default_slot_save_path(),
            sleep_idle_seconds: default_sleep_idle_seconds(),
            threads_http: default_threads_http(),
            metrics_enabled: default_metrics_enabled(),
            api_key_file: default_api_key_file(),
            direct_io: default_direct_io(),
            check_tensors: default_check_tensors(),
            override_kv: default_override_kv(),
            prio: default_prio(),
            warmup_enabled: default_warmup_enabled(),
            cont_batching: default_cont_batching(),
            grammar_file: default_grammar_file(),
            json_schema_file: default_json_schema_file(),
            embeddings_enabled: default_embeddings_enabled(),
            pooling: default_pooling(),
            rerank_enabled: default_rerank_enabled(),
            spec_type: default_spec_type(),
            spec_draft_n_max: default_spec_draft_n_max(),
            enable_spec_draft_n_max: false,
            spec_draft_n_min: 0,
            spec_draft_p_min: default_spec_draft_p_min(),
            spec_draft_p_split: default_spec_draft_p_split(),
            spec_draft_type_k: default_spec_draft_type_k(),
            spec_draft_type_v: default_spec_draft_type_v(),
            enable_spec_draft_n_min: false,
            enable_spec_draft_p_min: false,
            enable_spec_draft_p_split: false,
            enable_spec_draft_type_k: false,
            enable_spec_draft_type_v: false,
            spec_ngram_size_n: default_spec_ngram_size_n(),
            spec_ngram_size_m: default_spec_ngram_size_m(),
            spec_ngram_min_hits: default_spec_ngram_min_hits(),
            spec_ngram_mod_n_min: default_spec_ngram_mod_n_min(),
            spec_ngram_mod_n_max: default_spec_ngram_mod_n_max(),
            spec_ngram_mod_n_match: default_spec_ngram_mod_n_match(),
            kv_offload: true,
            cache_type_k: "q8_0".to_string(),
            cache_type_v: "q8_0".to_string(),
            kv_unified: true,
            swa_full: false,
            kv_cache_ratio: default_kv_cache_ratio(),
            ctx_checkpoints: default_ctx_checkpoints(),
            checkpoint_min_step: default_checkpoint_min_step(),
            gpu_layers_mode: GpuLayersMode::All,
            split_mode: "none".to_string(),
            tensor_split: "".to_string(),
            cpu_moe: false,
            n_cpu_moe: 0,
            override_tensor: String::new(),
            verbose: false,
            offline_mode: false,
            rpc_mode: false,
            rpc_endpoints: "127.0.0.1:50052".to_string(),
            model_path: PathBuf::new(),
            mmproj_path: PathBuf::new(),
            // 多模态
            mmproj_auto: default_mmproj_auto(),
            mmproj_offload: default_mmproj_offload(),
            mmproj_device: default_mmproj_device(),
            image_min_tokens: default_image_min_tokens(),
            image_max_tokens: default_image_max_tokens(),
            mtmd_batch_max_tokens: default_mtmd_batch_max_tokens(),
            video_fps: default_video_fps(),
            video_timestamp_interval: default_video_timestamp_interval(),
            video_ffmpeg_dir: String::new(),
            dflash_path: PathBuf::new(),
            model_dir: PathBuf::new(),
            alias: String::new(),
            rpc_threads: 8,
            rpc_device: String::new(),
            rpc_cache: false,
            web_ui_enabled: default_web_ui_enabled(),
            log_timestamps: default_log_timestamps(),
            log_verbosity: default_log_verbosity(),
            session_timeout: default_session_timeout(),
            reasoning: default_reasoning(),
            reasoning_format: default_reasoning_format(),
            reasoning_effort: default_reasoning_effort(),
            reasoning_budget: default_reasoning_budget(),
            reasoning_preserve: None,
            jinja_enabled: default_jinja_enabled(),
            chat_template: String::new(),
            chat_template_file: PathBuf::new(),
            rocm_gpu_target: default_rocm_gpu_target(),
            mcp_enabled: false,
            mcp_server_states: std::collections::BTreeMap::new(),
            imported: false,
            imported_decl: None,
        }
    }
}

impl Preset {
    /// 从当前 AppSettings 创建预设快照
    pub fn from_settings(settings: &AppSettings, name: String) -> Self {
        Self {
            name,
            host: settings.host.clone(),
            port: settings.port,
            parallel_slots: settings.parallel_slots,
            context: settings.context,
            batch_size: settings.batch_size,
            ubatch_size: settings.ubatch_size,
            temperature: settings.temperature,
            top_p: settings.top_p,
            top_k: settings.top_k,
            repeat_penalty: settings.repeat_penalty,
            presence_penalty: settings.presence_penalty,
            enable_batch_size: settings.enable_batch_size,
            enable_ubatch_size: settings.enable_ubatch_size,
            enable_session_timeout: settings.enable_session_timeout,
            enable_temperature: settings.enable_temperature,
            enable_top_p: settings.enable_top_p,
            enable_top_k: settings.enable_top_k,
            enable_repeat_penalty: settings.enable_repeat_penalty,
            enable_presence_penalty: settings.enable_presence_penalty,
            flash_attn: settings.flash_attn.clone(),
            fit: settings.fit.clone(),
            fit_target: settings.fit_target.clone(),
            fit_ctx: settings.fit_ctx,
            enable_fit_target: settings.enable_fit_target,
            enable_fit_ctx: settings.enable_fit_ctx,
            load_mode: settings.load_mode.clone(),
            tensor_read_lazy: settings.tensor_read_lazy.clone(),
            threads: settings.threads,
            threads_batch: settings.threads_batch,
            n_predict: settings.n_predict,
            keep: settings.keep,
            seed: settings.seed,
            main_gpu: settings.main_gpu,
            device: settings.device.clone(),
            enable_min_p: settings.enable_min_p,
            min_p: settings.min_p,
            enable_top_n_sigma: settings.enable_top_n_sigma,
            top_n_sigma: settings.top_n_sigma,
            enable_xtc: settings.enable_xtc,
            xtc_probability: settings.xtc_probability,
            xtc_threshold: settings.xtc_threshold,
            enable_typical_p: settings.enable_typical_p,
            typical_p: settings.typical_p,
            enable_mirostat: settings.enable_mirostat,
            mirostat: settings.mirostat,
            mirostat_lr: settings.mirostat_lr,
            mirostat_ent: settings.mirostat_ent,
            enable_dynatemp: settings.enable_dynatemp,
            dynatemp_range: settings.dynatemp_range,
            dynatemp_exp: settings.dynatemp_exp,
            sampler_seq: settings.sampler_seq.clone(),
            cache_prompt: settings.cache_prompt,
            cache_reuse: settings.cache_reuse,
            context_shift: settings.context_shift,
            json_schema: settings.json_schema.clone(),
            grammar: settings.grammar.clone(),
            api_key: settings.api_key.clone(),
            api_prefix: settings.api_prefix.clone(),
            cors_origins: settings.cors_origins.clone(),
            ssl_cert_file: settings.ssl_cert_file.clone(),
            ssl_key_file: settings.ssl_key_file.clone(),
            reuse_port: settings.reuse_port,
            numa: settings.numa.clone(),
            cache_ram_enabled: settings.cache_ram_enabled,
            cache_ram: settings.cache_ram,
            no_host: settings.no_host,
            rope_scaling: settings.rope_scaling.clone(),
            rope_scale: settings.rope_scale,
            rope_freq_base: settings.rope_freq_base,
            rope_freq_scale: settings.rope_freq_scale,
            yarn_orig_ctx: settings.yarn_orig_ctx,
            yarn_ext_factor: settings.yarn_ext_factor,
            yarn_attn_factor: settings.yarn_attn_factor,
            yarn_beta_slow: settings.yarn_beta_slow,
            yarn_beta_fast: settings.yarn_beta_fast,
            frequency_penalty: settings.frequency_penalty,
            repeat_last_n: settings.repeat_last_n,
            dry_multiplier: settings.dry_multiplier,
            dry_base: settings.dry_base,
            dry_allowed_length: settings.dry_allowed_length,
            dry_penalty_last_n: settings.dry_penalty_last_n,
            dry_sequence_breaker: settings.dry_sequence_breaker.clone(),
            logit_bias: settings.logit_bias.clone(),
            lora_path: settings.lora_path.clone(),
            lora_scaled: settings.lora_scaled.clone(),
            control_vector: settings.control_vector.clone(),
            slot_save_path: settings.slot_save_path.clone(),
            sleep_idle_seconds: settings.sleep_idle_seconds,
            threads_http: settings.threads_http,
            metrics_enabled: settings.metrics_enabled,
            api_key_file: settings.api_key_file.clone(),
            direct_io: settings.direct_io,
            check_tensors: settings.check_tensors,
            override_kv: settings.override_kv.clone(),
            prio: settings.prio,
            warmup_enabled: settings.warmup_enabled,
            cont_batching: settings.cont_batching,
            grammar_file: settings.grammar_file.clone(),
            json_schema_file: settings.json_schema_file.clone(),
            embeddings_enabled: settings.embeddings_enabled,
            pooling: settings.pooling.clone(),
            rerank_enabled: settings.rerank_enabled,
            spec_type: settings.spec_type.clone(),
            spec_draft_n_max: settings.spec_draft_n_max,
            enable_spec_draft_n_max: settings.enable_spec_draft_n_max,
            spec_draft_n_min: settings.spec_draft_n_min,
            spec_draft_p_min: settings.spec_draft_p_min,
            spec_draft_p_split: settings.spec_draft_p_split,
            spec_draft_type_k: settings.spec_draft_type_k.clone(),
            spec_draft_type_v: settings.spec_draft_type_v.clone(),
            enable_spec_draft_n_min: settings.enable_spec_draft_n_min,
            enable_spec_draft_p_min: settings.enable_spec_draft_p_min,
            enable_spec_draft_p_split: settings.enable_spec_draft_p_split,
            enable_spec_draft_type_k: settings.enable_spec_draft_type_k,
            enable_spec_draft_type_v: settings.enable_spec_draft_type_v,
            spec_ngram_size_n: settings.spec_ngram_size_n,
            spec_ngram_size_m: settings.spec_ngram_size_m,
            spec_ngram_min_hits: settings.spec_ngram_min_hits,
            spec_ngram_mod_n_min: settings.spec_ngram_mod_n_min,
            spec_ngram_mod_n_max: settings.spec_ngram_mod_n_max,
            spec_ngram_mod_n_match: settings.spec_ngram_mod_n_match,
            kv_offload: settings.kv_offload,
            cache_type_k: settings.cache_type_k.clone(),
            cache_type_v: settings.cache_type_v.clone(),
            kv_unified: settings.kv_unified,
            swa_full: settings.swa_full,
            kv_cache_ratio: settings.kv_cache_ratio,
            ctx_checkpoints: settings.ctx_checkpoints,
            checkpoint_min_step: settings.checkpoint_min_step,
            gpu_layers_mode: settings.gpu_layers_mode,
            split_mode: settings.split_mode.clone(),
            tensor_split: settings.tensor_split.clone(),
            cpu_moe: settings.cpu_moe,
            n_cpu_moe: settings.n_cpu_moe,
            override_tensor: settings.override_tensor.clone(),
            verbose: settings.verbose,
            offline_mode: settings.offline_mode,
            rpc_mode: settings.rpc_mode,
            rpc_endpoints: settings.rpc_endpoints.clone(),
            model_path: settings.model_path.clone(),
            mmproj_path: settings.mmproj_path.clone(),
            mmproj_auto: settings.mmproj_auto,
            mmproj_offload: settings.mmproj_offload,
            mmproj_device: settings.mmproj_device.clone(),
            image_min_tokens: settings.image_min_tokens,
            image_max_tokens: settings.image_max_tokens,
            mtmd_batch_max_tokens: settings.mtmd_batch_max_tokens,
            video_fps: settings.video_fps,
            video_timestamp_interval: settings.video_timestamp_interval,
            video_ffmpeg_dir: settings.video_ffmpeg_dir.clone(),
            dflash_path: settings.dflash_path.clone(),
            model_dir: settings.model_dir.clone(),
            alias: settings.alias.clone(),
            rpc_threads: settings.rpc_threads,
            rpc_device: settings.rpc_device.clone(),
            rpc_cache: settings.rpc_cache,
            web_ui_enabled: settings.web_ui_enabled,
            log_timestamps: settings.log_timestamps,
            log_verbosity: settings.log_verbosity,
            session_timeout: settings.session_timeout,
            reasoning: settings.reasoning.clone(),
            reasoning_format: settings.reasoning_format.clone(),
            reasoning_effort: settings.reasoning_effort.clone(),
            reasoning_budget: settings.reasoning_budget,
            reasoning_preserve: settings.reasoning_preserve,
            jinja_enabled: settings.jinja_enabled,
            chat_template: settings.chat_template.clone(),
            chat_template_file: settings.chat_template_file.clone(),
            rocm_gpu_target: settings.rocm_gpu_target.clone(),
            mcp_enabled: settings.mcp_enabled,
            mcp_server_states: settings.mcp_server_states.clone(),
            imported: false, // 新保存的预设非外部引入
            imported_decl: None,
        }
    }

    /// 将预设应用到 AppSettings
    pub fn apply_to(self, settings: &mut AppSettings) {
        settings.host = self.host;
        settings.port = self.port;
        settings.parallel_slots = self.parallel_slots;
        settings.context = self.context;
        settings.batch_size = self.batch_size;
        settings.ubatch_size = self.ubatch_size;
        settings.temperature = self.temperature;
        settings.top_p = self.top_p;
        settings.top_k = self.top_k;
        settings.repeat_penalty = self.repeat_penalty;
        settings.presence_penalty = self.presence_penalty;
        settings.enable_batch_size = self.enable_batch_size;
        settings.enable_ubatch_size = self.enable_ubatch_size;
        settings.enable_session_timeout = self.enable_session_timeout;
        settings.enable_temperature = self.enable_temperature;
        settings.enable_top_p = self.enable_top_p;
        settings.enable_top_k = self.enable_top_k;
        settings.enable_repeat_penalty = self.enable_repeat_penalty;
        settings.enable_presence_penalty = self.enable_presence_penalty;
        settings.flash_attn = self.flash_attn;
        settings.fit = self.fit;
        settings.fit_target = self.fit_target;
        settings.fit_ctx = self.fit_ctx;
        settings.enable_fit_target = self.enable_fit_target;
        settings.enable_fit_ctx = self.enable_fit_ctx;
        // 加载模式
        settings.load_mode = self.load_mode;
        settings.tensor_read_lazy = self.tensor_read_lazy;
        // 线程与生成长度
        settings.threads = self.threads;
        settings.threads_batch = self.threads_batch;
        settings.n_predict = self.n_predict;
        settings.keep = self.keep;
        settings.seed = self.seed;
        settings.main_gpu = self.main_gpu;
        settings.device = self.device.clone();
        // 采样器扩展
        settings.enable_min_p = self.enable_min_p;
        settings.min_p = self.min_p;
        settings.enable_top_n_sigma = self.enable_top_n_sigma;
        settings.top_n_sigma = self.top_n_sigma;
        settings.enable_xtc = self.enable_xtc;
        settings.xtc_probability = self.xtc_probability;
        settings.xtc_threshold = self.xtc_threshold;
        settings.enable_typical_p = self.enable_typical_p;
        settings.typical_p = self.typical_p;
        settings.enable_mirostat = self.enable_mirostat;
        settings.mirostat = self.mirostat;
        settings.mirostat_lr = self.mirostat_lr;
        settings.mirostat_ent = self.mirostat_ent;
        settings.enable_dynatemp = self.enable_dynatemp;
        settings.dynatemp_range = self.dynatemp_range;
        settings.dynatemp_exp = self.dynatemp_exp;
        settings.sampler_seq = self.sampler_seq;
        // 长上下文 / 提示缓存
        settings.cache_prompt = self.cache_prompt;
        settings.cache_reuse = self.cache_reuse;
        settings.context_shift = self.context_shift;
        // 结构化输出
        settings.json_schema = self.json_schema;
        settings.grammar = self.grammar;
        // API 安全 / 部署
        settings.api_key = self.api_key;
        settings.api_prefix = self.api_prefix;
        settings.cors_origins = self.cors_origins;
        settings.ssl_cert_file = self.ssl_cert_file;
        settings.ssl_key_file = self.ssl_key_file;
        settings.reuse_port = self.reuse_port;
        settings.numa = self.numa;
        settings.cache_ram_enabled = self.cache_ram_enabled;
        settings.cache_ram = self.cache_ram;
        settings.no_host = self.no_host;
        settings.rope_scaling = self.rope_scaling;
        settings.rope_scale = self.rope_scale;
        settings.rope_freq_base = self.rope_freq_base;
        settings.rope_freq_scale = self.rope_freq_scale;
        settings.yarn_orig_ctx = self.yarn_orig_ctx;
        settings.yarn_ext_factor = self.yarn_ext_factor;
        settings.yarn_attn_factor = self.yarn_attn_factor;
        settings.yarn_beta_slow = self.yarn_beta_slow;
        settings.yarn_beta_fast = self.yarn_beta_fast;
        settings.frequency_penalty = self.frequency_penalty;
        settings.repeat_last_n = self.repeat_last_n;
        settings.dry_multiplier = self.dry_multiplier;
        settings.dry_base = self.dry_base;
        settings.dry_allowed_length = self.dry_allowed_length;
        settings.dry_penalty_last_n = self.dry_penalty_last_n;
        settings.dry_sequence_breaker = self.dry_sequence_breaker;
        settings.logit_bias = self.logit_bias;
        settings.lora_path = self.lora_path;
        settings.lora_scaled = self.lora_scaled;
        settings.control_vector = self.control_vector;
        settings.slot_save_path = self.slot_save_path;
        settings.sleep_idle_seconds = self.sleep_idle_seconds;
        settings.threads_http = self.threads_http;
        settings.metrics_enabled = self.metrics_enabled;
        settings.api_key_file = self.api_key_file;
        settings.direct_io = self.direct_io;
        settings.check_tensors = self.check_tensors;
        settings.override_kv = self.override_kv;
        settings.prio = self.prio;
        settings.warmup_enabled = self.warmup_enabled;
        settings.cont_batching = self.cont_batching;
        settings.grammar_file = self.grammar_file;
        settings.json_schema_file = self.json_schema_file;
        settings.embeddings_enabled = self.embeddings_enabled;
        settings.pooling = self.pooling;
        settings.rerank_enabled = self.rerank_enabled;
        // 推测解码（Speculative Decoding）配置
        settings.spec_type = self.spec_type;
        settings.spec_draft_n_max = self.spec_draft_n_max;
        settings.enable_spec_draft_n_max = self.enable_spec_draft_n_max;
        settings.spec_draft_n_min = self.spec_draft_n_min;
        settings.spec_draft_p_min = self.spec_draft_p_min;
        settings.spec_draft_p_split = self.spec_draft_p_split;
        settings.spec_draft_type_k = self.spec_draft_type_k;
        settings.spec_draft_type_v = self.spec_draft_type_v;
        settings.enable_spec_draft_n_min = self.enable_spec_draft_n_min;
        settings.enable_spec_draft_p_min = self.enable_spec_draft_p_min;
        settings.enable_spec_draft_p_split = self.enable_spec_draft_p_split;
        settings.enable_spec_draft_type_k = self.enable_spec_draft_type_k;
        settings.enable_spec_draft_type_v = self.enable_spec_draft_type_v;
        settings.spec_ngram_size_n = self.spec_ngram_size_n;
        settings.spec_ngram_size_m = self.spec_ngram_size_m;
        settings.spec_ngram_min_hits = self.spec_ngram_min_hits;
        settings.spec_ngram_mod_n_min = self.spec_ngram_mod_n_min;
        settings.spec_ngram_mod_n_max = self.spec_ngram_mod_n_max;
        settings.spec_ngram_mod_n_match = self.spec_ngram_mod_n_match;
        settings.kv_offload = self.kv_offload;
        settings.cache_type_k = self.cache_type_k;
        settings.cache_type_v = self.cache_type_v;
        settings.kv_unified = self.kv_unified;
        settings.swa_full = self.swa_full;
        settings.kv_cache_ratio = self.kv_cache_ratio;
        settings.ctx_checkpoints = self.ctx_checkpoints;
        settings.checkpoint_min_step = self.checkpoint_min_step;
        settings.gpu_layers_mode = self.gpu_layers_mode;
        settings.split_mode = self.split_mode;
        settings.tensor_split = self.tensor_split;
        settings.cpu_moe = self.cpu_moe;
        settings.n_cpu_moe = self.n_cpu_moe;
        settings.override_tensor = self.override_tensor;
        settings.verbose = self.verbose;
        settings.offline_mode = self.offline_mode;
        settings.rpc_mode = self.rpc_mode;
        settings.rpc_endpoints = self.rpc_endpoints;
        settings.model_path = self.model_path;
        settings.mmproj_path = self.mmproj_path;
        settings.mmproj_auto = self.mmproj_auto;
        settings.mmproj_offload = self.mmproj_offload;
        settings.mmproj_device = self.mmproj_device.clone();
        settings.image_min_tokens = self.image_min_tokens;
        settings.image_max_tokens = self.image_max_tokens;
        settings.mtmd_batch_max_tokens = self.mtmd_batch_max_tokens;
        settings.video_fps = self.video_fps;
        settings.video_timestamp_interval = self.video_timestamp_interval;
        settings.video_ffmpeg_dir = self.video_ffmpeg_dir.clone();
        settings.dflash_path = self.dflash_path;
        settings.model_dir = self.model_dir;
        settings.alias = self.alias;
        settings.rpc_threads = self.rpc_threads;
        settings.rpc_device = self.rpc_device;
        settings.rpc_cache = self.rpc_cache;
        settings.web_ui_enabled = self.web_ui_enabled;
        settings.log_timestamps = self.log_timestamps;
        settings.log_verbosity = self.log_verbosity;
        settings.session_timeout = self.session_timeout;
        // 思考与会话
        settings.reasoning = self.reasoning;
        settings.reasoning_format = self.reasoning_format;
        settings.reasoning_effort = self.reasoning_effort;
        settings.reasoning_budget = self.reasoning_budget;
        settings.reasoning_preserve = self.reasoning_preserve;
        settings.jinja_enabled = self.jinja_enabled;
        settings.chat_template = self.chat_template;
        settings.chat_template_file = self.chat_template_file;
        settings.rocm_gpu_target = self.rocm_gpu_target.clone();
        // MCP 管理（原始 JSON 不属于 Preset，仅同步启用状态）
        settings.mcp_enabled = self.mcp_enabled;
        settings.mcp_server_states = self.mcp_server_states;
    }
}

/// 最大上下文计算 Promise 的包装器（运行时缓存，不序列化）
/// 实现 Debug 和 Clone 以兼容 AppSettings 的 derive 宏
#[derive(Default)]
pub struct MaxContextPromiseWrapper(pub Option<poll_promise::Promise<Result<usize, String>>>);

impl Clone for MaxContextPromiseWrapper {
    fn clone(&self) -> Self {
        // Promise 不能克隆，每次克隆返回 None（新的空状态）
        Self(None)
    }
}

impl std::fmt::Debug for MaxContextPromiseWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(_) => write!(f, "MaxContextPromiseWrapper(Some(...))"),
            None => write!(f, "MaxContextPromiseWrapper(None)"),
        }
    }
}

/// KV 缓存计算 Promise 的包装器（运行时缓存，不序列化）
/// 实现 Debug 和 Clone 以兼容 AppSettings 的 derive 宏
#[derive(Default)]
pub struct KvCachePromiseWrapper(pub Option<poll_promise::Promise<Result<String, String>>>);

impl Clone for KvCachePromiseWrapper {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl std::fmt::Debug for KvCachePromiseWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(_) => write!(f, "KvCachePromiseWrapper(Some(...))"),
            None => write!(f, "KvCachePromiseWrapper(None)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // Server 配置
    pub server_path: PathBuf,
    pub host: String,
    pub port: u16,
    pub parallel_slots: i64,

    // 模型
    pub model_path: PathBuf,
    pub mmproj_path: PathBuf,
    // ── 多模态（mmproj / mtmd / video）──
    #[serde(default = "default_mmproj_auto")]
    pub mmproj_auto: bool,
    #[serde(default = "default_mmproj_offload")]
    pub mmproj_offload: bool,
    #[serde(default = "default_mmproj_device")]
    pub mmproj_device: String,
    #[serde(default = "default_image_min_tokens")]
    pub image_min_tokens: i64,
    #[serde(default = "default_image_max_tokens")]
    pub image_max_tokens: i64,
    #[serde(default = "default_mtmd_batch_max_tokens")]
    pub mtmd_batch_max_tokens: usize,
    #[serde(default = "default_video_fps")]
    pub video_fps: f32,
    #[serde(default = "default_video_timestamp_interval")]
    pub video_timestamp_interval: usize,
    #[serde(default)]
    pub video_ffmpeg_dir: String,
    #[serde(default)]
    pub dflash_path: PathBuf,
    #[serde(default)]
    pub model_dir: PathBuf,
    #[serde(default)]
    pub alias: String, // --alias

    // 推理参数（以 k 为单位存储，1k = 1024）
    #[serde(
        default = "default_context",
        deserialize_with = "deserialize_context::deserialize"
    )]
    pub context: usize, // --ctx-size (k)
    #[serde(
        default = "default_batch_size",
        deserialize_with = "deserialize_batch_size::deserialize"
    )]
    pub batch_size: f32, // --batch-size (k, 0.0001 步进)
    #[serde(
        default = "default_ubatch_size",
        deserialize_with = "deserialize_ubatch_size::deserialize"
    )]
    pub ubatch_size: f32, // --ubatch-size (k, 0.0001 步进)
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repeat_penalty: f32,
    pub presence_penalty: f32,
    // 批次/超时参数启用标志（勾选时拼接启动命令）
    #[serde(default)]
    pub enable_batch_size: bool,
    #[serde(default)]
    pub enable_ubatch_size: bool,
    #[serde(default)]
    pub enable_session_timeout: bool,
    // 采样参数启用标志（勾选时拼接启动命令）
    #[serde(default)]
    pub enable_temperature: bool,
    #[serde(default)]
    pub enable_top_p: bool,
    #[serde(default)]
    pub enable_top_k: bool,
    #[serde(default)]
    pub enable_repeat_penalty: bool,
    #[serde(default)]
    pub enable_presence_penalty: bool,
    #[serde(default = "default_flash_attn")]
    pub flash_attn: String,
    // 内存自动调优 (--fit / --fit-target / --fit-ctx)
    #[serde(default = "default_fit")]
    pub fit: String, // --fit on/off
    #[serde(default = "default_fit_target")]
    pub fit_target: String, // --fit-target MiB（逗号分隔多卡）
    #[serde(default = "default_fit_ctx")]
    pub fit_ctx: usize, // --fit-ctx（最小上下文，k 单位）
    #[serde(default)]
    pub enable_fit_target: bool,
    #[serde(default)]
    pub enable_fit_ctx: bool,

    // 加载模式（新版 --load-mode；"auto" 时沿用旧版 --mmap/--mlock 行为）
    #[serde(default = "default_load_mode")]
    pub load_mode: String, // --load-mode
    #[serde(default = "default_tensor_read_lazy")]
    pub tensor_read_lazy: String, // --tensor-read-lazy

    // ── 线程与生成长度（llama.cpp b10488+）──
    #[serde(default = "default_threads")]
    pub threads: i64, // --threads (-1 不拼接)
    #[serde(default = "default_threads_batch")]
    pub threads_batch: i64, // --threads-batch (-1 不拼接)
    #[serde(default = "default_n_predict")]
    pub n_predict: i64, // --n-predict (-1 不拼接)
    #[serde(default = "default_keep_tokens")]
    pub keep: i64, // --keep (0 不拼接)
    #[serde(default = "default_seed")]
    pub seed: i64, // --seed (-1 不拼接)
    #[serde(default = "default_main_gpu")]
    pub main_gpu: i64, // --main-gpu
    #[serde(default)]
    pub device: String, // --device

    // ── 采样器扩展（近新版本加入）──
    #[serde(default)]
    pub enable_min_p: bool,
    #[serde(default = "default_min_p")]
    pub min_p: f32, // --min-p
    #[serde(default)]
    pub enable_top_n_sigma: bool,
    #[serde(default = "default_top_n_sigma")]
    pub top_n_sigma: f32, // --top-n-sigma
    #[serde(default)]
    pub enable_xtc: bool,
    #[serde(default = "default_xtc_probability")]
    pub xtc_probability: f32, // --xtc-probability
    #[serde(default = "default_xtc_threshold")]
    pub xtc_threshold: f32, // --xtc-threshold
    #[serde(default)]
    pub enable_typical_p: bool,
    #[serde(default = "default_typical_p")]
    pub typical_p: f32, // --typical-p
    #[serde(default)]
    pub enable_mirostat: bool,
    #[serde(default = "default_mirostat")]
    pub mirostat: i32, // --mirostat (0/1/2)
    #[serde(default = "default_mirostat_lr")]
    pub mirostat_lr: f32, // --mirostat-lr
    #[serde(default = "default_mirostat_ent")]
    pub mirostat_ent: f32, // --mirostat-ent
    #[serde(default)]
    pub enable_dynatemp: bool,
    #[serde(default = "default_dynatemp_range")]
    pub dynatemp_range: f32, // --dynatemp-range
    #[serde(default = "default_dynatemp_exp")]
    pub dynatemp_exp: f32, // --dynatemp-exp
    #[serde(default = "default_sampler_seq")]
    pub sampler_seq: String, // --sampler-seq (空不拼接)

    // ── 长上下文 / 提示缓存 ──
    #[serde(default = "default_cache_prompt")]
    pub cache_prompt: bool, // --cache-prompt / --no-cache-prompt
    #[serde(default = "default_cache_reuse")]
    pub cache_reuse: i64, // --cache-reuse (0 不拼接)
    #[serde(default = "default_context_shift")]
    pub context_shift: bool, // --context-shift

    // ── 结构化输出 ──
    #[serde(default = "default_json_schema")]
    pub json_schema: String, // --json-schema (空不拼接)
    #[serde(default = "default_grammar")]
    pub grammar: String, // --grammar (空不拼接)

    // ── API 安全 / 部署 ──
    #[serde(default = "default_api_key")]
    pub api_key: String, // --api-key (空不拼接)
    #[serde(default = "default_api_prefix")]
    pub api_prefix: String, // --api-prefix (空不拼接)
    #[serde(default = "default_cors_origins")]
    pub cors_origins: String, // --cors-origins (空不拼接)
    #[serde(default)]
    pub ssl_cert_file: PathBuf, // --ssl-cert-file (空不拼接)
    #[serde(default)]
    pub ssl_key_file: PathBuf, // --ssl-key-file (空不拼接)
    #[serde(default)]
    pub reuse_port: bool, // --reuse-port
    #[serde(default = "default_numa")]
    pub numa: String, // --numa (空不拼接)
    #[serde(default = "default_cache_ram_enabled")]
    pub cache_ram_enabled: bool, // --cache-ram 启用开关 (prompt cache 内存上限)
    #[serde(default = "default_cache_ram")]
    pub cache_ram: usize, // --cache-ram N (MiB, -1 不限, 0 禁用)
    #[serde(default = "default_no_host")]
    pub no_host: bool, // --no-host (绕过 host buffer 以使用额外缓冲区)
    #[serde(default = "default_rope_scaling")]
    pub rope_scaling: String, // --rope-scaling {none,linear,yarn} (空不拼接)
    #[serde(default = "default_rope_scale")]
    pub rope_scale: f32, // --rope-scale N (上下文扩展倍数)
    #[serde(default = "default_rope_freq_base")]
    pub rope_freq_base: f32, // --rope-freq-base N (0 不拼接)
    #[serde(default = "default_rope_freq_scale")]
    pub rope_freq_scale: f32, // --rope-freq-scale N (0 不拼接)
    #[serde(default = "default_yarn_orig_ctx")]
    pub yarn_orig_ctx: usize, // --yarn-orig-ctx N (0 不拼接)
    #[serde(default = "default_yarn_ext_factor")]
    pub yarn_ext_factor: f32, // --yarn-ext-factor N (-1 不拼接)
    #[serde(default = "default_yarn_attn_factor")]
    pub yarn_attn_factor: f32, // --yarn-attn-factor N (-1 不拼接)
    #[serde(default = "default_yarn_beta_slow")]
    pub yarn_beta_slow: f32, // --yarn-beta-slow N (-1 不拼接)
    #[serde(default = "default_yarn_beta_fast")]
    pub yarn_beta_fast: f32, // --yarn-beta-fast N (-1 不拼接)
    #[serde(default = "default_frequency_penalty")]
    pub frequency_penalty: f32, // --frequency-penalty N (0 不拼接)
    #[serde(default = "default_repeat_last_n")]
    pub repeat_last_n: i64, // --repeat-last-n N (-1 不拼接)
    #[serde(default = "default_dry_multiplier")]
    pub dry_multiplier: f32, // --dry-multiplier N (0 不拼接 = 禁用)
    #[serde(default = "default_dry_base")]
    pub dry_base: f32, // --dry-base N
    #[serde(default = "default_dry_allowed_length")]
    pub dry_allowed_length: i64, // --dry-allowed-length N
    #[serde(default = "default_dry_penalty_last_n")]
    pub dry_penalty_last_n: i64, // --dry-penalty-last-n N
    #[serde(default = "default_dry_sequence_breaker")]
    pub dry_sequence_breaker: String, // --dry-sequence-breaker STRING (空不拼接)
    #[serde(default = "default_logit_bias")]
    pub logit_bias: String, // --logit-bias (如 EOS-inf, 空不拼接)
    #[serde(default = "default_lora_path")]
    pub lora_path: String, // --lora FNAME (逗号分隔多个, 空不拼接)
    #[serde(default = "default_lora_scaled")]
    pub lora_scaled: String, // --lora-scaled FNAME:SCALE,... (空不拼接)
    #[serde(default = "default_control_vector")]
    pub control_vector: String, // --control-vector FNAME (空不拼接)
    #[serde(default = "default_slot_save_path")]
    pub slot_save_path: String, // --slot-save-path PATH (空不拼接)
    #[serde(default = "default_sleep_idle_seconds")]
    pub sleep_idle_seconds: i64, // --sleep-idle-seconds N (-1 不拼接)
    #[serde(default = "default_threads_http")]
    pub threads_http: i64, // --threads-http N (-1 不拼接)
    #[serde(default = "default_metrics_enabled")]
    pub metrics_enabled: bool, // --metrics (Prometheus 指标端点)
    #[serde(default = "default_api_key_file")]
    pub api_key_file: String, // --api-key-file FNAME (空不拼接)
    #[serde(default = "default_direct_io")]
    pub direct_io: bool, // --direct-io / --no-direct-io
    #[serde(default = "default_check_tensors")]
    pub check_tensors: bool, // --check-tensors (加载时校验张量)
    #[serde(default = "default_override_kv")]
    pub override_kv: String, // --override-kv KEY=TYPE:VALUE,... (空不拼接)
    #[serde(default = "default_prio")]
    pub prio: i64, // --prio N (0 不拼接)
    #[serde(default = "default_warmup_enabled")]
    pub warmup_enabled: bool, // --warmup / --no-warmup (关闭时发 --no-warmup)
    #[serde(default = "default_cont_batching")]
    pub cont_batching: bool, // --cont-batching / --no-cont-batching (关闭时发 --no-)
    #[serde(default = "default_grammar_file")]
    pub grammar_file: String, // --grammar-file FNAME (空不拼接)
    #[serde(default = "default_json_schema_file")]
    pub json_schema_file: String, // --json-schema-file FILE (空不拼接)
    #[serde(default = "default_embeddings_enabled")]
    pub embeddings_enabled: bool, // --embeddings (嵌入模式)
    #[serde(default = "default_pooling")]
    pub pooling: String, // --pooling {none,mean,cls,last,rank} (空不拼接)
    #[serde(default = "default_rerank_enabled")]
    pub rerank_enabled: bool, // --rerank / --reranking (重排模式)

    // 推测解码（Speculative Decoding）配置
    #[serde(default = "default_spec_type")]
    pub spec_type: String, // --spec-type
    #[serde(default = "default_spec_draft_n_max")]
    pub spec_draft_n_max: usize, // --spec-draft-n-max
    #[serde(default)]
    pub spec_draft_n_min: usize, // --spec-draft-n-min
    #[serde(default = "default_spec_draft_p_min")]
    pub spec_draft_p_min: f32, // --spec-draft-p-min
    #[serde(default = "default_spec_draft_p_split")]
    pub spec_draft_p_split: f32, // --spec-draft-p-split
    #[serde(default = "default_spec_draft_type_k")]
    pub spec_draft_type_k: String, // --spec-draft-type-k
    #[serde(default = "default_spec_draft_type_v")]
    pub spec_draft_type_v: String, // --spec-draft-type-v
    // draft-* 参数启用开关（关闭时忽略对应配置）
    #[serde(default)]
    pub enable_spec_draft_n_max: bool,
    #[serde(default)]
    pub enable_spec_draft_n_min: bool,
    #[serde(default)]
    pub enable_spec_draft_p_min: bool,
    #[serde(default)]
    pub enable_spec_draft_p_split: bool,
    #[serde(default)]
    pub enable_spec_draft_type_k: bool,
    #[serde(default)]
    pub enable_spec_draft_type_v: bool,
    // ngram 参数（ngram-simple / ngram-map-k / ngram-map-k4v 共用）
    #[serde(default = "default_spec_ngram_size_n")]
    pub spec_ngram_size_n: usize,
    #[serde(default = "default_spec_ngram_size_m")]
    pub spec_ngram_size_m: usize,
    #[serde(default = "default_spec_ngram_min_hits")]
    pub spec_ngram_min_hits: usize,
    // ngram-mod 专用参数
    #[serde(default = "default_spec_ngram_mod_n_min")]
    pub spec_ngram_mod_n_min: usize,
    #[serde(default = "default_spec_ngram_mod_n_max")]
    pub spec_ngram_mod_n_max: usize,
    #[serde(default = "default_spec_ngram_mod_n_match")]
    pub spec_ngram_mod_n_match: usize,

    // KV 缓存配置
    pub kv_offload: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
    pub kv_unified: bool, // --kv-unified
    #[serde(default)]
    pub swa_full: bool, // --swa-full
    #[serde(default = "default_kv_cache_ratio")]
    pub kv_cache_ratio: f32, // KV 缓存比例 (不拼接启动命令)
    #[serde(default = "default_ctx_checkpoints")]
    pub ctx_checkpoints: usize, // --ctx-checkpoints
    #[serde(default = "default_checkpoint_min_step")]
    pub checkpoint_min_step: usize, // --checkpoint-min-step

    // GPU 与设备分配
    pub gpu_layers_mode: GpuLayersMode,
    pub split_mode: String,
    pub tensor_split: String,
    pub cpu_moe: bool,
    pub n_cpu_moe: usize,
    #[serde(default)]
    pub override_tensor: String, // --override-tensor

    // RPC 配置
    pub rpc_server_path: PathBuf,
    pub rpc_host: String,
    pub rpc_port: u16,
    pub rpc_threads: usize,
    pub rpc_device: String,
    pub rpc_cache: bool,

    // 高级
    pub verbose: bool,

    // 离线模式
    #[serde(default)]
    pub offline_mode: bool,

    // RPC 模式 (llama-server)
    #[serde(default)]
    pub rpc_mode: bool,
    #[serde(default)]
    pub rpc_endpoints: String,

    // 网页客户端开关（默认启用）
    #[serde(default = "default_web_ui_enabled")]
    pub web_ui_enabled: bool,

    // 原生日志时间戳（默认启用）
    #[serde(default = "default_log_timestamps")]
    pub log_timestamps: bool,

    // 日志级别（--log-verbosity，0=generic 1=error 2=warning 3=info 4=trace 5=debug）
    #[serde(default = "default_log_verbosity")]
    pub log_verbosity: usize,

    // 会话超时（秒，追加 --timeout）
    #[serde(default = "default_session_timeout")]
    pub session_timeout: usize,

    // 思考与会话
    #[serde(default = "default_reasoning")]
    pub reasoning: String, // --reasoning: auto/on/off
    #[serde(default = "default_reasoning_format")]
    pub reasoning_format: String, // --reasoning-format
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String, // --reasoning-effort：纯字符串值（如 high）
    #[serde(default = "default_reasoning_budget")]
    pub reasoning_budget: i32, // --reasoning-budget: -1 = 不限制
    #[serde(default)]
    pub reasoning_preserve: Option<bool>, // --reasoning-preserve / --no-reasoning-preserve：None = 模型默认（不拼接）
    #[serde(default = "default_jinja_enabled")]
    pub jinja_enabled: bool, // --jinja / --no-jinja
    #[serde(default)]
    pub chat_template: String, // --chat-template（Jinja 模板文本）
    #[serde(default)]
    pub chat_template_file: PathBuf, // --chat-template-file（Jinja 模板文件）

    // MCP 管理（llama.cpp --mcp-servers-config）
    #[serde(default)]
    pub mcp_enabled: bool, // MCP 功能总开关（无启用 server 时不拼接参数）
    #[serde(default)]
    pub mcp_server_states: std::collections::BTreeMap<String, bool>, // 每个 MCP server 的启用状态（按名称）

    // 用户原始 MCP 配置（Cursor-compatible mcpServers JSON 文本，仅全局设置，不进 Preset）
    #[serde(default)]
    pub mcp_config_json: String,

    // MCP 配置编辑器 UI 状态（不序列化）
    #[serde(skip, default)]
    pub mcp_editor_open: bool,
    #[serde(skip, default)]
    pub mcp_editor_text: String,
    #[serde(skip, default)]
    pub mcp_editor_error: String,

    // RPC 设备列表 UI 状态（不序列化）
    #[serde(skip, default)]
    pub show_device_list: bool,
    #[serde(skip, default)]
    pub device_list_output: String,

    // Server 设备列表 UI 状态（不序列化）
    #[serde(skip, default)]
    pub show_server_device_list: bool,
    #[serde(skip, default)]
    pub server_device_list_output: String,

    // Linux 服务文件窗口 UI 状态（不序列化）
    #[serde(skip, default)]
    pub show_linux_service_file: bool,
    #[serde(skip, default)]
    pub linux_service_file_copied: bool,

    // RPC 节点地址输入框临时状态（不序列化）
    #[serde(skip, default)]
    pub rpc_endpoint_input: String,
    // RPC 多卡节点勾选框临时状态（不序列化）
    #[serde(skip, default)]
    pub rpc_endpoint_multi_gpu: bool,

    // 预设
    #[serde(default)]
    pub presets: Vec<Preset>,

    // 预设 UI 状态（不序列化）
    #[serde(skip, default)]
    pub new_preset_name: String,
    #[serde(skip, default)]
    pub rename_preset_index: Option<usize>,
    #[serde(skip, default)]
    pub rename_preset_new_name: String,

    // 自启动预设名称
    #[serde(default)]
    pub auto_start_preset_name: Option<String>,

    // 日志面板设置
    #[serde(default = "default_auto_scroll_logs")]
    pub auto_scroll_logs: bool,
    #[serde(default = "default_max_log_lines")]
    pub max_log_lines: i32,

    // 开机自启动
    #[serde(default)]
    pub auto_start: bool,

    // 静默启动（开机自启时最小化到任务栏）
    #[serde(default)]
    pub silent_start: bool,

    // Linux 兼容模式（远程桌面环境强制软件渲染，防止 NVIDIA 驱动崩溃）
    #[serde(default = "default_linux_compat_mode")]
    pub linux_compat_mode: bool,

    // 文件日志开关（默认开启）
    #[serde(default = "default_log_to_file")]
    pub log_to_file: bool,

    // 界面主题：深色模式（默认开启）
    #[serde(default = "default_dark_mode")]
    pub dark_mode: bool,

    // 界面主题模式："light" / "dark" / "auto"
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,

    // 界面主题色（十六进制，如 #0A84FF），全局强调色
    #[serde(default = "default_accent_color")]
    pub accent_color: String,

    // 界面语言（"zh" / "en"），空字符串表示首次运行按系统区域检测
    #[serde(default)]
    pub language: String,

    // llama.cpp 下载变体偏好：
    // "cpu" | "cuda124" | "cuda133" | "rocm_lemonade" | "rocm10" | "vulkan"
    // （兼容旧值 "gpu"：Windows→cuda124, Linux→vulkan）
    #[serde(default = "default_download_variant")]
    pub download_variant: String,

    // llama.cpp 版本分支："main" | "turboquant"
    // main: 官方 ggml-org/llama.cpp 仓库
    // turboquant: TurboQuant fork 仓库
    #[serde(default = "default_llama_branch")]
    pub llama_branch: String,

    // 发布通道："stable" | "preview"
    // stable: 读取 vX.Y.Z 的 nightly-tag.txt 获取对应 nightly 版本的预编译资产
    // preview: 直接获取最新 nightly release 的预编译资产
    #[serde(default = "default_release_channel")]
    pub release_channel: String,

    // ROCm GPU 目标型号（仅当 download_variant 为 rocm_lemonade 时生效）
    // 可选值："gfx103X" | "gfx110X" | "gfx1150" | "gfx1151" | "gfx120X" | "gfx908" | "gfx90a"
    #[serde(default = "default_rocm_gpu_target")]
    pub rocm_gpu_target: String,

    // 下载内嵌 CUDA 库开关（仅 Windows + CUDA 12/13 变体时有效）
    // 开启时额外下载 cudart-llama-bin-win-cuda-{version}-x64.zip 并解压到 llama/ 目录
    #[serde(default = "default_true")]
    pub download_cuda_lib: bool,

    // llama.cpp 版本信息（不序列化，运行时缓存）
    #[serde(skip, default)]
    pub llama_version: String,

    // 检查更新结果（不序列化，运行时缓存）
    // Some(true)=有新版本 Some(false)=已是最新 None=尚未检查
    #[serde(skip, default)]
    pub update_available: Option<bool>,

    // 新版本号（不序列化，运行时缓存）
    #[serde(skip, default)]
    pub new_version_tag: Option<String>,

    // KV 缓存计算结果（运行时缓存，不序列化）
    #[serde(skip, default)]
    pub kv_cache_result: Option<String>,

    // KV 缓存计算 Promise（运行时缓存，不序列化）
    #[serde(skip, default)]
    pub kv_cache_promise: KvCachePromiseWrapper,

    // 最大上下文计算 Promise（运行时缓存，不序列化）
    #[serde(skip, default)]
    pub max_context_promise: MaxContextPromiseWrapper,

    /// 系统服务面板选中的预设名称
    #[serde(default)]
    pub system_service_selected_preset: String,
    /// 是否显示服务详情弹窗
    #[serde(default)]
    pub show_system_service_details: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            server_path: PathBuf::new(),
            host: "127.0.0.1".to_string(),
            port: 9931,
            parallel_slots: -1,
            model_path: PathBuf::new(),
            mmproj_path: PathBuf::new(),
            // 多模态
            mmproj_auto: default_mmproj_auto(),
            mmproj_offload: default_mmproj_offload(),
            mmproj_device: default_mmproj_device(),
            image_min_tokens: default_image_min_tokens(),
            image_max_tokens: default_image_max_tokens(),
            mtmd_batch_max_tokens: default_mtmd_batch_max_tokens(),
            video_fps: default_video_fps(),
            video_timestamp_interval: default_video_timestamp_interval(),
            video_ffmpeg_dir: String::new(),
            dflash_path: PathBuf::new(),
            model_dir: PathBuf::new(),
            alias: String::new(),
            context: 4,       // 4k = 4096
            batch_size: 2.0,  // 2k = 2048
            ubatch_size: 0.5, // 0.5k = 512
            temperature: 0.8,
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            presence_penalty: 0.0,
            enable_batch_size: false,
            enable_ubatch_size: false,
            enable_session_timeout: false,
            enable_temperature: false,
            enable_top_p: false,
            enable_top_k: false,
            enable_repeat_penalty: false,
            enable_presence_penalty: false,
            flash_attn: default_flash_attn(),
            fit: default_fit(),
            fit_target: default_fit_target(),
            fit_ctx: default_fit_ctx(),
            enable_fit_target: false,
            enable_fit_ctx: false,
            load_mode: default_load_mode(),
            tensor_read_lazy: default_tensor_read_lazy(),
            threads: default_threads(),
            threads_batch: default_threads_batch(),
            n_predict: default_n_predict(),
            keep: default_keep_tokens(),
            seed: default_seed(),
            main_gpu: default_main_gpu(),
            device: String::new(),
            enable_min_p: false,
            min_p: default_min_p(),
            enable_top_n_sigma: false,
            top_n_sigma: default_top_n_sigma(),
            enable_xtc: false,
            xtc_probability: default_xtc_probability(),
            xtc_threshold: default_xtc_threshold(),
            enable_typical_p: false,
            typical_p: default_typical_p(),
            enable_mirostat: false,
            mirostat: default_mirostat(),
            mirostat_lr: default_mirostat_lr(),
            mirostat_ent: default_mirostat_ent(),
            enable_dynatemp: false,
            dynatemp_range: default_dynatemp_range(),
            dynatemp_exp: default_dynatemp_exp(),
            sampler_seq: default_sampler_seq(),
            cache_prompt: default_cache_prompt(),
            cache_reuse: default_cache_reuse(),
            context_shift: default_context_shift(),
            json_schema: default_json_schema(),
            grammar: default_grammar(),
            api_key: default_api_key(),
            api_prefix: default_api_prefix(),
            cors_origins: default_cors_origins(),
            ssl_cert_file: PathBuf::new(),
            ssl_key_file: PathBuf::new(),
            reuse_port: false,
            numa: default_numa(),
            cache_ram_enabled: default_cache_ram_enabled(),
            cache_ram: default_cache_ram(),
            no_host: default_no_host(),
            rope_scaling: default_rope_scaling(),
            rope_scale: default_rope_scale(),
            rope_freq_base: default_rope_freq_base(),
            rope_freq_scale: default_rope_freq_scale(),
            yarn_orig_ctx: default_yarn_orig_ctx(),
            yarn_ext_factor: default_yarn_ext_factor(),
            yarn_attn_factor: default_yarn_attn_factor(),
            yarn_beta_slow: default_yarn_beta_slow(),
            yarn_beta_fast: default_yarn_beta_fast(),
            frequency_penalty: default_frequency_penalty(),
            repeat_last_n: default_repeat_last_n(),
            dry_multiplier: default_dry_multiplier(),
            dry_base: default_dry_base(),
            dry_allowed_length: default_dry_allowed_length(),
            dry_penalty_last_n: default_dry_penalty_last_n(),
            dry_sequence_breaker: default_dry_sequence_breaker(),
            logit_bias: default_logit_bias(),
            lora_path: default_lora_path(),
            lora_scaled: default_lora_scaled(),
            control_vector: default_control_vector(),
            slot_save_path: default_slot_save_path(),
            sleep_idle_seconds: default_sleep_idle_seconds(),
            threads_http: default_threads_http(),
            metrics_enabled: default_metrics_enabled(),
            api_key_file: default_api_key_file(),
            direct_io: default_direct_io(),
            check_tensors: default_check_tensors(),
            override_kv: default_override_kv(),
            prio: default_prio(),
            warmup_enabled: default_warmup_enabled(),
            cont_batching: default_cont_batching(),
            grammar_file: default_grammar_file(),
            json_schema_file: default_json_schema_file(),
            embeddings_enabled: default_embeddings_enabled(),
            pooling: default_pooling(),
            rerank_enabled: default_rerank_enabled(),
            spec_type: default_spec_type(),
            spec_draft_n_max: default_spec_draft_n_max(),
            enable_spec_draft_n_max: false,
            spec_draft_n_min: 0,
            spec_draft_p_min: default_spec_draft_p_min(),
            spec_draft_p_split: default_spec_draft_p_split(),
            spec_draft_type_k: default_spec_draft_type_k(),
            spec_draft_type_v: default_spec_draft_type_v(),
            enable_spec_draft_n_min: false,
            enable_spec_draft_p_min: false,
            enable_spec_draft_p_split: false,
            enable_spec_draft_type_k: false,
            enable_spec_draft_type_v: false,
            spec_ngram_size_n: default_spec_ngram_size_n(),
            spec_ngram_size_m: default_spec_ngram_size_m(),
            spec_ngram_min_hits: default_spec_ngram_min_hits(),
            spec_ngram_mod_n_min: default_spec_ngram_mod_n_min(),
            spec_ngram_mod_n_max: default_spec_ngram_mod_n_max(),
            spec_ngram_mod_n_match: default_spec_ngram_mod_n_match(),
            kv_offload: true,
            cache_type_k: "q8_0".to_string(),
            cache_type_v: "q8_0".to_string(),
            kv_unified: true,
            swa_full: false,
            kv_cache_ratio: default_kv_cache_ratio(),
            ctx_checkpoints: default_ctx_checkpoints(),
            checkpoint_min_step: default_checkpoint_min_step(),
            gpu_layers_mode: GpuLayersMode::All,
            split_mode: "none".to_string(),
            tensor_split: "".to_string(),
            cpu_moe: false,
            n_cpu_moe: 0,
            override_tensor: String::new(),
            rpc_server_path: PathBuf::new(),
            rpc_host: "127.0.0.1".to_string(),
            rpc_port: 50052,
            rpc_threads: 8,
            rpc_device: "".to_string(),
            rpc_cache: false,
            verbose: false,
            offline_mode: false,
            rpc_mode: false,
            rpc_endpoints: "127.0.0.1:50052".to_string(),
            web_ui_enabled: default_web_ui_enabled(),
            log_timestamps: default_log_timestamps(),
            log_verbosity: default_log_verbosity(),
            session_timeout: default_session_timeout(),
            reasoning: default_reasoning(),
            reasoning_format: default_reasoning_format(),
            reasoning_effort: default_reasoning_effort(),
            reasoning_budget: default_reasoning_budget(),
            reasoning_preserve: None,
            jinja_enabled: default_jinja_enabled(),
            chat_template: String::new(),
            chat_template_file: PathBuf::new(),
            mcp_enabled: false,
            mcp_server_states: std::collections::BTreeMap::new(),
            mcp_config_json: String::new(),
            mcp_editor_open: false,
            mcp_editor_text: String::new(),
            mcp_editor_error: String::new(),
            show_device_list: false,
            device_list_output: String::new(),
            show_server_device_list: false,
            server_device_list_output: String::new(),
            show_linux_service_file: false,
            linux_service_file_copied: false,
            rpc_endpoint_input: String::new(),
            rpc_endpoint_multi_gpu: false,
            presets: Vec::new(),
            new_preset_name: String::new(),
            rename_preset_index: None,
            rename_preset_new_name: String::new(),
            auto_scroll_logs: default_auto_scroll_logs(),
            max_log_lines: default_max_log_lines(),
            auto_start: false,
            silent_start: false,
            linux_compat_mode: default_linux_compat_mode(),
            log_to_file: default_log_to_file(),
            dark_mode: true,
            theme_mode: "auto".to_string(),
            accent_color: default_accent_color(),
            language: String::new(),
            download_variant: default_download_variant(),
            llama_branch: default_llama_branch(),
            release_channel: default_release_channel(),
            rocm_gpu_target: default_rocm_gpu_target(),
            download_cuda_lib: true,
            auto_start_preset_name: None,
            llama_version: String::new(),
            update_available: None,
            new_version_tag: None,
            kv_cache_result: None,
            kv_cache_promise: KvCachePromiseWrapper::default(),
            max_context_promise: MaxContextPromiseWrapper::default(),
        system_service_selected_preset: String::new(),
        show_system_service_details: false,
    }
    }
}

impl AppSettings {
    /// k 值 → 实际参数值 (value * 1024)
    pub fn context_actual(&self) -> usize {
        self.context * 1024
    }
    pub fn batch_size_actual(&self) -> usize {
        (self.batch_size * 1024.0) as usize
    }
    pub fn ubatch_size_actual(&self) -> usize {
        (self.ubatch_size * 1024.0) as usize
    }
    /// k 值 → 实际 --fit-ctx 参数值 (value * 1024)
    pub fn fit_ctx_actual(&self) -> usize {
        self.fit_ctx * 1024
    }

    /// 构造"当前启用"的 MCP 配置 JSON（纯逻辑，便于测试）。
    ///
    /// 规则（对齐 llama.cpp tools/server/server-mcp.cpp 的解析行为）：
    /// - 未开启 MCP / 无原始配置 / JSON 非法 / 无启用的 server → 返回 None（不拼接参数）
    /// - 只保留处于启用状态且带非空 `command`（stdio 型）的 server
    pub fn build_effective_mcp_json(&self) -> Option<serde_json::Value> {
        if !self.mcp_enabled || self.mcp_config_json.trim().is_empty() {
            return None;
        }
        let root: serde_json::Value = match serde_json::from_str(&self.mcp_config_json) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[mcp] 配置 JSON 解析失败，跳过 MCP 参数: {}", e);
                return None;
            }
        };
        let servers = match root.get("mcpServers").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => {
                log::warn!("[mcp] 配置缺少 mcpServers 对象，跳过 MCP 参数");
                return None;
            }
        };
        let mut effective = serde_json::Map::new();
        for (name, cfg) in servers {
            let enabled = self.mcp_server_states.get(name).copied().unwrap_or(false);
            let has_command = cfg
                .get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| !c.is_empty());
            if enabled && has_command {
                effective.insert(name.clone(), cfg.clone());
            }
        }
        if effective.is_empty() {
            return None;
        }
        Some(serde_json::json!({ "mcpServers": effective }))
    }

    /// 生成"当前启用的 MCP 配置文件"并返回其路径
    /// （写入 launcher exe 同目录 `mcp_servers.json`，与 llama_cpp_launcher_settings.json 同级）。
    /// 无启用 server 或生成失败时返回 None（不拼接参数）。
    pub fn write_effective_mcp_config(&self) -> Option<PathBuf> {
        let out = self.build_effective_mcp_json()?;
        let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        let path = dir.join("mcp_servers.json");
        let content = serde_json::to_string_pretty(&out).unwrap_or_default();
        match std::fs::write(&path, content) {
            Ok(_) => Some(path),
            Err(e) => {
                log::warn!("[mcp] 写入 MCP 配置文件失败: {}", e);
                None
            }
        }
    }
}

/// MCP server 概要（供 UI 列表展示，由 parse_mcp_servers 解析）
#[derive(Debug, Clone)]
pub struct McpServerSummary {
    pub name: String,
    /// stdio 启动命令；为空表示该条目 llama.cpp 无法启动（会被其跳过）
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: i64,
    /// 条目本身是否为合法 JSON object
    pub is_object: bool,
}

/// 解析用户提供的 Cursor-compatible mcpServers JSON，返回 server 概要列表（保持 JSON 顺序）。
///
/// 错误情况（返回 Err 描述）：
/// - JSON 非法
/// - 顶层不是 object
/// - 缺少 `mcpServers` 或其不是 object
pub fn parse_mcp_servers(json_text: &str) -> Result<Vec<McpServerSummary>, String> {
    let root: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("JSON: {}", e))?;
    let root_obj = root
        .as_object()
        .ok_or_else(|| "top-level must be an object".to_string())?;
    let servers = root_obj
        .get("mcpServers")
        .ok_or_else(|| "missing \"mcpServers\"".to_string())?;
    let servers = servers
        .as_object()
        .ok_or_else(|| "\"mcpServers\" must be an object".to_string())?;

    let mut result = Vec::new();
    for (name, cfg) in servers {
        let is_object = cfg.is_object();
        let command = cfg
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let args = cfg
            .get("args")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let timeout_ms = cfg
            .get("timeout_ms")
            .and_then(|t| t.as_i64())
            .unwrap_or(30000);
        result.push(McpServerSummary {
            name: name.clone(),
            command,
            args,
            timeout_ms,
            is_object,
        });
    }
    Ok(result)
}

pub struct SettingsManager {
    config_dir: PathBuf,
}

impl Default for SettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsManager {
    pub fn new() -> Self {
        let config_dir = std::env::current_exe()
            .map(|p| p.parent().unwrap_or(Path::new("")).to_path_buf())
            .unwrap_or_else(|_| PathBuf::from("."));

        Self { config_dir }
    }

    /// 返回配置目录路径（下载功能共用）
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        let path = self.config_dir.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("解析配置失败: {}", e))
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        let path = self.config_dir.join(CONFIG_FILE);
        let content =
            serde_json::to_string_pretty(settings).map_err(|e| format!("序列化配置失败: {}", e))?;
        fs::write(&path, content).map_err(|e| format!("写入配置失败: {}", e))?;
        Ok(())
    }

    /// 在指定目录中查找指定名称的可执行文件
    fn find_exe_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
        let filename = if cfg!(target_os = "windows") {
            format!("{}.exe", name)
        } else {
            name.to_string()
        };
        let path = dir.join(&filename);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// 在指定目录及其子目录中 BFS 查找可执行文件（限深 MAX_DEPTH 层）
    /// 优先级：1. 目录本身  2. 浅层优先；同层中名称含 keyword（忽略大小写）的目录优先，
    /// 再按目录名忽略大小写字典序 —— 保证确定性。
    /// 兼容旧行为：keyword 子目录 1 层的匹配仍最先被检查。
    fn find_exe_recursive(&self, dir: &Path, exe_name: &str, keyword: &str) -> Option<PathBuf> {
        const MAX_DEPTH: usize = 4; // 相对给定目录的最大子目录深度

        // 1. 先在目录本身查找
        if let Some(path) = Self::find_exe_in_dir(dir, exe_name) {
            return Some(path);
        }

        // 2. BFS：入队 dir 的子目录（深度 1），逐层弹出检查
        let keyword_lower = keyword.to_lowercase();
        let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
        for d in Self::list_subdirs_sorted(dir, &keyword_lower) {
            queue.push_back((d, 1));
        }

        while let Some((current, depth)) = queue.pop_front() {
            if let Some(path) = Self::find_exe_in_dir(&current, exe_name) {
                return Some(path);
            }
            // 未超过最大深度时入队其子目录
            if depth < MAX_DEPTH {
                for sub in Self::list_subdirs_sorted(&current, &keyword_lower) {
                    queue.push_back((sub, depth + 1));
                }
            }
        }

        None
    }

    /// 列出指定目录下的子目录（read_dir 错误静默跳过）
    /// 排序规则：名称含 keyword（忽略大小写）者优先，再按目录名忽略大小写字典序（确定性）
    fn list_subdirs_sorted(dir: &Path, keyword_lower: &str) -> Vec<PathBuf> {
        let mut subdirs: Vec<PathBuf> = fs::read_dir(dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        subdirs.sort_by(|a, b| {
            let name = |p: &Path| -> String {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            };
            let a_name = name(a);
            let b_name = name(b);
            let a_kw = a_name.contains(keyword_lower) as u8;
            let b_kw = b_name.contains(keyword_lower) as u8;
            b_kw.cmp(&a_kw).then_with(|| a_name.cmp(&b_name))
        });
        subdirs
    }

    /// 自动检测 llama-server 路径
    /// 搜索：exe 同级目录 → 含 "llama" 名称的子目录
    pub fn auto_detect_server_path(&self) -> Option<PathBuf> {
        self.find_exe_recursive(&self.config_dir, "llama-server", "llama")
    }

    /// 自动检测 rpc-server 路径
    /// 搜索：exe 同级目录 → 含 "llama" 名称的子目录（通常与 llama-server 同目录）
    pub fn auto_detect_rpc_path(&self) -> Option<PathBuf> {
        self.find_exe_recursive(&self.config_dir, "ggml-rpc-server", "llama")
    }
}

/// 判断文件名是否为 llama-server 可执行文件（跨平台）
pub fn is_server_binary_name(name: &str) -> bool {
    if cfg!(target_os = "windows") {
        name == "llama-server.exe"
    } else {
        name == "llama-server"
    }
}

/// 判断文件名是否为 rpc-server（ggml-rpc-server）可执行文件（跨平台）
pub fn is_rpc_binary_name(name: &str) -> bool {
    if cfg!(target_os = "windows") {
        name == "ggml-rpc-server.exe" || name == "rpc-server.exe"
    } else {
        name == "ggml-rpc-server" || name == "rpc-server"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼接平台对应的 exe 文件名
    fn exe_filename(name: &str) -> String {
        if cfg!(target_os = "windows") {
            format!("{}.exe", name)
        } else {
            name.to_string()
        }
    }

    /// 创建目录（含父目录）并写入一个 dummy 可执行文件
    fn make_exe(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).expect("create dir");
        let p = dir.join(name);
        fs::write(&p, b"fake exe").expect("write dummy exe");
        p
    }

    fn manager() -> SettingsManager {
        SettingsManager::new()
    }

    /// 回归：目录本身包含 exe → 直接返回
    #[test]
    fn find_exe_recursive_dir_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe = make_exe(tmp.path(), &exe_filename("llama-server"));
        let found = manager().find_exe_recursive(tmp.path(), "llama-server", "llama");
        assert_eq!(found, Some(exe));
    }

    /// 回归：keyword 命名的子目录 1 层深度包含 exe → 返回
    #[test]
    fn find_exe_recursive_keyword_subdir_one_level() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe = make_exe(
            &tmp.path().join("llama-b10549"),
            &exe_filename("llama-server"),
        );
        let found = manager().find_exe_recursive(tmp.path(), "llama-server", "llama");
        assert_eq!(found, Some(exe));
    }

    /// 新能力：深路径 llama/llama-b10549/bin/llama-server（深度 3）→ 命中
    #[test]
    fn find_exe_recursive_deep_path_depth_3() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let exe = make_exe(
            &tmp.path().join("llama").join("llama-b10549").join("bin"),
            &exe_filename("llama-server"),
        );
        let found = manager().find_exe_recursive(tmp.path(), "llama-server", "llama");
        assert_eq!(found, Some(exe));
    }

    /// 限深生效：深度 5 的目录中的 exe → 不命中
    #[test]
    fn find_exe_recursive_depth_5_not_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // 深度：a(1) / b(2) / c(3) / d(4) / e(5)，exe 位于深度 5 的目录
        let exe = make_exe(
            &tmp.path().join("a").join("b").join("c").join("d").join("e"),
            &exe_filename("llama-server"),
        );
        assert!(exe.exists());
        let found = manager().find_exe_recursive(tmp.path(), "llama-server", "llama");
        assert_eq!(found, None);
    }

    /// download_variant serde 默认值：旧配置（缺字段）反序列化后为 "cpu"
    #[test]
    fn app_settings_download_variant_default() {
        assert_eq!(default_download_variant(), "cpu");
        let json = r#"{"server_path":"","host":"127.0.0.1","port":9931,"parallel_slots":1,"model_path":"","mmproj_path":"","temperature":0.8,"top_p":0.95,"top_k":40,"repeat_penalty":1.1,"presence_penalty":0.0,"kv_offload":false,"cache_type_k":"f16","cache_type_v":"f16","kv_mlock":false,"kv_mmap":true,"kv_unified":false,"gpu_layers_mode":"auto","split_mode":"none","tensor_split":"","cpu_moe":false,"n_cpu_moe":0,"rpc_server_path":"","rpc_host":"127.0.0.1","rpc_port":50052,"rpc_threads":8,"rpc_device":"","rpc_cache":false,"verbose":false}"#;
        let settings: AppSettings = serde_json::from_str(json).expect("旧格式配置应可反序列化");
        assert_eq!(settings.download_variant, "cpu");
    }

    // ──── MCP 解析 / 生效配置 ────

    const MCP_TEST_JSON: &str = r#"{
        "mcpServers": {
            "comfyui": {
                "command": "python",
                "args": ["C:\\AI\\MCP\\ComfyUI\\server.py"]
            },
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@example/filesystem-mcp"],
                "timeout_ms": 60000
            },
            "remote-only": {
                "url": "https://example.com/mcp"
            }
        }
    }"#;

    /// 正常解析：提取全部 server 名称与关键字段，无 command 条目标记
    #[test]
    fn mcp_parse_ok() {
        let servers = parse_mcp_servers(MCP_TEST_JSON).expect("合法配置应解析成功");
        assert_eq!(servers.len(), 3);
        assert_eq!(servers[0].name, "comfyui");
        assert_eq!(servers[0].command, "python");
        assert_eq!(servers[0].args, vec!["C:\\AI\\MCP\\ComfyUI\\server.py"]);
        assert_eq!(servers[1].timeout_ms, 60000);
        assert!(
            servers[2].command.is_empty(),
            "无 command 条目应为空 command"
        );
    }

    /// 错误场景：非法 JSON / 顶层非对象 / 缺 mcpServers / mcpServers 非 object
    #[test]
    fn mcp_parse_errors() {
        assert!(parse_mcp_servers("{ not json").is_err());
        assert!(parse_mcp_servers("[1,2]").is_err());
        assert!(parse_mcp_servers(r#"{"foo":{}}"#).is_err());
        assert!(parse_mcp_servers(r#"{"mcpServers":[1]}"#).is_err());
        // 空 mcpServers 合法（返回空列表）
        assert!(parse_mcp_servers(r#"{"mcpServers":{}}"#)
            .expect("空对象应成功")
            .is_empty());
    }

    /// 生效配置：只保留启用且带 command 的 server；无启用时返回 None
    #[test]
    fn mcp_effective_partial_enable() {
        let mut settings = AppSettings::default();
        settings.mcp_enabled = true;
        settings.mcp_config_json = MCP_TEST_JSON.to_string();
        // 无任何启用 → None
        assert!(settings.build_effective_mcp_json().is_none());
        // 只启用 comfyui（stdio）与 remote-only（无 command，应被过滤）
        settings
            .mcp_server_states
            .insert("comfyui".to_string(), true);
        settings
            .mcp_server_states
            .insert("remote-only".to_string(), true);
        let out = settings
            .build_effective_mcp_json()
            .expect("存在启用的 stdio server");
        let servers = out
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .expect("输出应含 mcpServers 对象");
        assert_eq!(servers.len(), 1, "remote-only 应被过滤");
        assert!(servers.contains_key("comfyui"));
        // 总开关关闭 → None
        settings.mcp_enabled = false;
        assert!(settings.build_effective_mcp_json().is_none());
    }

    /// 旧配置兼容：缺少 MCP 字段的配置反序列化后为默认值
    #[test]
    fn mcp_settings_default_compat() {
        let json = r#"{"server_path":"","host":"127.0.0.1","port":9931,"parallel_slots":1,"model_path":"","mmproj_path":"","temperature":0.8,"top_p":0.95,"top_k":40,"repeat_penalty":1.1,"presence_penalty":0.0,"kv_offload":false,"cache_type_k":"f16","cache_type_v":"f16","kv_mlock":false,"kv_mmap":true,"kv_unified":false,"gpu_layers_mode":"auto","split_mode":"none","tensor_split":"","cpu_moe":false,"n_cpu_moe":0,"rpc_server_path":"","rpc_host":"127.0.0.1","rpc_port":50052,"rpc_threads":8,"rpc_device":"","rpc_cache":false,"verbose":false}"#;
        let settings: AppSettings = serde_json::from_str(json).expect("旧格式配置应可反序列化");
        assert!(!settings.mcp_enabled);
        assert!(settings.mcp_config_json.is_empty());
        assert!(settings.mcp_server_states.is_empty());
        assert!(settings.build_effective_mcp_json().is_none());
    }
}
