//! 预设分享 / 引入（B+ 方案）
//!
//! 编码链：`ParamsExport`（参数面板白名单，复用文件导出/导入的同一结构）
//! → JSON（剥离与默认导出相同的字段，典型用户只改少量参数 → 大幅缩短）
//! → gzip（flate2 已有依赖）→ Base64URL → `LCP1-` 前缀。
//!
//! 为什么不用 cmd/bat/短码字典：
//! - cmd /c 字符串传参会错误解析路径引号（实测"文件名、目录名或卷标语法不正确"）
//! - bat 文件按系统代码页读取，UTF-8 写入的中文路径乱码
//! - JSON 的引号由格式自身转义，gzip+Base64URL 无代码页问题，中英文路径安全
//!
//! 版本兼容：`LCP1` 为编码格式版本（升级时改 LCP2 并保留旧解析）；
//! 载荷内 `gen` 记录生成版本，跨版本导入给出警告、允许强制添加。
//! 健壮性：解码容错（跳过粘贴引入的非字母表字符）；导入逐项勾选、
//! 重名自动追加序号，不覆盖本地预设。

use crate::config::settings::{AppSettings, GpuLayersMode, Preset};
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─────────────── 参数白名单（与文件导出/导入共用同一结构） ───────────────

fn default_reasoning() -> String {
    "auto".to_string()
}
fn default_reasoning_format() -> String {
    "auto".to_string()
}
fn default_reasoning_effort() -> String {
    "default".to_string()
}
fn default_reasoning_budget() -> i32 {
    -1
}
fn default_jinja_enabled() -> bool {
    true
}

// 多模态（mmproj / mtmd / video）默认值（与 AppSettings 保持一致，
// 保证旧版导出 JSON 直接反序列化时缺失的新字段有默认值）
fn default_mmproj_auto() -> String {
    "auto".to_string()
}
fn default_mmproj_offload() -> bool {
    true
}
fn default_mmproj_device() -> String {
    String::new()
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
fn default_video_ffmpeg_dir() -> String {
    String::new()
}
fn default_fit() -> String {
    "on".to_string()
}
fn default_fit_target() -> String {
    "1024".to_string()
}
fn default_fit_ctx() -> usize {
    4
}

fn default_kvmem_block_tokens() -> i64 {
    128
}
fn default_kvmem_gen_reserve() -> i64 {
    256
}
fn default_kvmem_gpu_ratio() -> f32 {
    0.50
}
fn default_kvmem_method() -> String {
    "retrieval".to_string()
}
fn default_kvmem_query_last() -> i64 {
    64
}
fn default_kvmem_query_max_tokens() -> i64 {
    512
}
fn default_kvmem_query_replay() -> String {
    "auto".to_string()
}
fn default_kvmem_query_policy() -> String {
    "user".to_string()
}
fn default_kvmem_mtp_state() -> String {
    "replay".to_string()
}
fn default_kvmem_nvme_dir() -> String {
    String::new()
}
fn default_kv_dtype() -> String {
    "q8_0".to_string()
}
fn default_spec_kv_dtype() -> String {
    "f16".to_string()
}

/// 导出/导入/分享的"参数面板"专用结构（不包含 Server/RPC/模型路径/密钥等）
#[derive(Serialize, Deserialize, Clone)]
pub struct ParamsExport {
    pub context: usize,
    pub batch_size: f32,
    pub ubatch_size: f32,
    pub session_timeout: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repeat_penalty: f32,
    pub presence_penalty: f32,
    pub enable_batch_size: bool,
    pub enable_ubatch_size: bool,
    pub enable_session_timeout: bool,
    pub enable_temperature: bool,
    pub enable_top_p: bool,
    pub enable_top_k: bool,
    pub enable_repeat_penalty: bool,
    pub enable_presence_penalty: bool,
    #[serde(default = "default_true")]
    pub enable_flash_attn: bool,
    pub flash_attn: String,
    #[serde(default = "default_true")]
    pub enable_fit: bool,
    #[serde(default = "default_fit")]
    pub fit: String,
    #[serde(default = "default_fit_target")]
    pub fit_target: String,
    #[serde(default = "default_fit_ctx")]
    pub fit_ctx: usize,
    #[serde(default)]
    pub enable_fit_target: bool,
    #[serde(default)]
    pub enable_fit_ctx: bool,

    pub spec_type: String,
    pub spec_draft_n_max: usize,
    pub enable_spec_draft_n_max: bool,
    pub spec_draft_n_min: usize,
    pub spec_draft_p_min: f32,
    pub spec_draft_p_split: f32,
    pub enable_spec_draft_n_min: bool,
    pub enable_spec_draft_p_min: bool,
    pub enable_spec_draft_p_split: bool,
    pub enable_spec_draft_type_k: bool,
    pub enable_spec_draft_type_v: bool,

    #[serde(default = "default_true")]
    pub enable_kv_offload: bool,
    #[serde(default)]
    pub enable_kv_unified: bool,
    pub kv_offload: bool,
    pub cache_type_k: String,
    pub cache_type_v: String,
    #[serde(default)]
    pub enable_cache_type_kv: bool,
    // --mlock / --no-mmap 已被 llama.cpp 弃用（由 --load-mode 取代），对应设置字段
    // 已从 AppSettings/Preset 移除。此处保留是为了让分享码 JSON 结构保持稳定：
    // 旧版本反序列化时这些字段没有 #[serde(default)]，缺失会导致导入失败。
    pub kv_mlock: bool,
    pub kv_mmap: bool,
    pub kv_unified: bool,
    pub swa_full: bool,

    // KVMem（llama-kvmem-server）
    #[serde(default = "default_true")]
    pub kvmem_enabled: bool,
    #[serde(default)]
    pub kvmem_trace: bool,
    #[serde(default)]
    pub kvmem_budget: i64,
    #[serde(default = "default_kvmem_block_tokens")]
    pub kvmem_block_tokens: i64,
    #[serde(default)]
    pub kvmem_sink_tokens: i64,
    #[serde(default = "default_kvmem_gen_reserve")]
    pub kvmem_gen_reserve: i64,
    #[serde(default = "default_kvmem_gpu_ratio")]
    pub kvmem_gpu_ratio: f32,
    #[serde(default)]
    pub kvmem_cpu_gb: f64,
    #[serde(default)]
    pub kvmem_nvme_gb: f64,
    #[serde(default)]
    pub kvmem_recent_tokens: i64,
    #[serde(default = "default_kvmem_method")]
    pub kvmem_method: String,
    #[serde(default = "default_kvmem_query_last")]
    pub kvmem_query_last: i64,
    #[serde(default = "default_kvmem_query_max_tokens")]
    pub kvmem_query_max_tokens: i64,
    #[serde(default = "default_kvmem_query_replay")]
    pub kvmem_query_replay: String,
    #[serde(default = "default_kvmem_query_policy")]
    pub kvmem_query_policy: String,
    #[serde(default = "default_kvmem_mtp_state")]
    pub kvmem_mtp_state: String,
    #[serde(default = "default_kvmem_nvme_dir")]
    pub kvmem_nvme_dir: String,
    #[serde(default)]
    pub kvmem_harvest_v: bool,
    #[serde(default)]
    pub kvmem_raw_k_nvme: bool,
    #[serde(default = "default_kv_dtype")]
    pub kv_dtype: String,
    #[serde(default = "default_spec_kv_dtype")]
    pub spec_kv_dtype: String,

    pub gpu_layers_mode: GpuLayersMode,
    // 独立启用开关（默认开启，关闭时不拼接 --gpu-layers）
    #[serde(default = "default_true")]
    pub enable_gpu_layers: bool,
    #[serde(default)]
    pub enable_split_mode: bool,
    pub split_mode: String,
    pub tensor_split: String,
    pub cpu_moe: bool,
    pub n_cpu_moe: usize,
    #[serde(default)]
    pub n_cpu_ffn: usize,
    #[serde(default)]
    pub static_path: String,

    #[serde(default = "default_true")]
    pub enable_reasoning: bool,
    #[serde(default = "default_true")]
    pub enable_reasoning_format: bool,
    #[serde(default = "default_true")]
    pub enable_reasoning_effort: bool,
    #[serde(default)]
    pub enable_reasoning_budget: bool,
    #[serde(default = "default_true")]
    pub enable_reasoning_preserve: bool,
    #[serde(default = "default_reasoning")]
    pub reasoning: String,
    #[serde(default = "default_reasoning_format")]
    pub reasoning_format: String,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    #[serde(default = "default_reasoning_budget")]
    pub reasoning_budget: i32,
    #[serde(default = "default_true")]
    pub reasoning_preserve: bool,
    // --jinja 外层启用开关（与 AppSettings 保持一致，旧版分享码缺失时默认为 true）
    #[serde(default = "default_true")]
    pub enable_jinja: bool,
    #[serde(default = "default_jinja_enabled")]
    pub jinja_enabled: bool,
    #[serde(default)]
    pub chat_template: String,
    #[serde(default)]
    pub chat_template_file: PathBuf,
    // 多模态（mmproj / mtmd / video）
    #[serde(default = "default_true")]
    pub enable_mmproj_auto: bool,
    #[serde(default = "default_mmproj_auto")]
    pub mmproj_auto: String,
    #[serde(default = "default_true")]
    pub enable_mmproj_offload: bool,
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
    #[serde(default = "default_video_ffmpeg_dir")]
    pub video_ffmpeg_dir: String,
}

impl ParamsExport {
    pub fn from_settings(s: &AppSettings) -> Self {
        Self {
            context: s.context,
            batch_size: s.batch_size,
            ubatch_size: s.ubatch_size,
            session_timeout: s.session_timeout,
            temperature: s.temperature,
            top_p: s.top_p,
            top_k: s.top_k,
            repeat_penalty: s.repeat_penalty,
            presence_penalty: s.presence_penalty,
            enable_batch_size: s.enable_batch_size,
            enable_ubatch_size: s.enable_ubatch_size,
            enable_session_timeout: s.enable_session_timeout,
            enable_temperature: s.enable_temperature,
            enable_top_p: s.enable_top_p,
            enable_top_k: s.enable_top_k,
            enable_repeat_penalty: s.enable_repeat_penalty,
            enable_presence_penalty: s.enable_presence_penalty,
            enable_flash_attn: s.enable_flash_attn,
            flash_attn: s.flash_attn.clone(),
            enable_fit: s.enable_fit,
            fit: s.fit.clone(),
            fit_target: s.fit_target.clone(),
            fit_ctx: s.fit_ctx,
            enable_fit_target: s.enable_fit_target,
            enable_fit_ctx: s.enable_fit_ctx,
            spec_type: s.spec_type.clone(),
            spec_draft_n_max: s.spec_draft_n_max,
            enable_spec_draft_n_max: s.enable_spec_draft_n_max,
            spec_draft_n_min: s.spec_draft_n_min,
            spec_draft_p_min: s.spec_draft_p_min,
            spec_draft_p_split: s.spec_draft_p_split,
            enable_spec_draft_n_min: s.enable_spec_draft_n_min,
            enable_spec_draft_p_min: s.enable_spec_draft_p_min,
            enable_spec_draft_p_split: s.enable_spec_draft_p_split,
            enable_spec_draft_type_k: s.enable_spec_draft_type_k,
            enable_spec_draft_type_v: s.enable_spec_draft_type_v,
            enable_kv_offload: s.enable_kv_offload,
            enable_kv_unified: s.enable_kv_unified,
            kv_offload: s.kv_offload,
            cache_type_k: s.cache_type_k.clone(),
            cache_type_v: s.cache_type_v.clone(),
            enable_cache_type_kv: s.enable_cache_type_kv,
            kv_mlock: false,
            kv_mmap: true,
            kv_unified: s.kv_unified,
            swa_full: s.swa_full,
            kvmem_enabled: s.kvmem_enabled,
            kvmem_trace: s.kvmem_trace,
            kvmem_budget: s.kvmem_budget,
            kvmem_block_tokens: s.kvmem_block_tokens,
            kvmem_sink_tokens: s.kvmem_sink_tokens,
            kvmem_gen_reserve: s.kvmem_gen_reserve,
            kvmem_gpu_ratio: s.kvmem_gpu_ratio,
            kvmem_cpu_gb: s.kvmem_cpu_gb,
            kvmem_nvme_gb: s.kvmem_nvme_gb,
            kvmem_recent_tokens: s.kvmem_recent_tokens,
            kvmem_method: s.kvmem_method.clone(),
            kvmem_query_last: s.kvmem_query_last,
            kvmem_query_max_tokens: s.kvmem_query_max_tokens,
            kvmem_query_replay: s.kvmem_query_replay.clone(),
            kvmem_query_policy: s.kvmem_query_policy.clone(),
            kvmem_mtp_state: s.kvmem_mtp_state.clone(),
            kvmem_nvme_dir: s.kvmem_nvme_dir.clone(),
            kvmem_harvest_v: s.kvmem_harvest_v,
            kvmem_raw_k_nvme: s.kvmem_raw_k_nvme,
            kv_dtype: s.kv_dtype.clone(),
            spec_kv_dtype: s.spec_kv_dtype.clone(),
            gpu_layers_mode: s.gpu_layers_mode,
            enable_gpu_layers: s.enable_gpu_layers,
            enable_split_mode: s.enable_split_mode,
            split_mode: s.split_mode.clone(),
            tensor_split: s.tensor_split.clone(),
            cpu_moe: s.cpu_moe,
            n_cpu_moe: s.n_cpu_moe,
            n_cpu_ffn: s.n_cpu_ffn,
            static_path: s.static_path.clone(),
            enable_reasoning: s.enable_reasoning,
            enable_reasoning_format: s.enable_reasoning_format,
            enable_reasoning_effort: s.enable_reasoning_effort,
            enable_reasoning_budget: s.enable_reasoning_budget,
            enable_reasoning_preserve: s.enable_reasoning_preserve,
            reasoning: s.reasoning.clone(),
            reasoning_format: s.reasoning_format.clone(),
            reasoning_effort: s.reasoning_effort.clone(),
            reasoning_budget: s.reasoning_budget,
            reasoning_preserve: s.reasoning_preserve,
            enable_jinja: s.enable_jinja,
            jinja_enabled: s.jinja_enabled,
            chat_template: s.chat_template.clone(),
            chat_template_file: s.chat_template_file.clone(),
            enable_mmproj_auto: s.enable_mmproj_auto,
            mmproj_auto: s.mmproj_auto.clone(),
            enable_mmproj_offload: s.enable_mmproj_offload,
            mmproj_offload: s.mmproj_offload,
            mmproj_device: s.mmproj_device.clone(),
            image_min_tokens: s.image_min_tokens,
            image_max_tokens: s.image_max_tokens,
            mtmd_batch_max_tokens: s.mtmd_batch_max_tokens,
            video_fps: s.video_fps,
            video_timestamp_interval: s.video_timestamp_interval,
            video_ffmpeg_dir: s.video_ffmpeg_dir.clone(),
        }
    }

    pub fn apply_to(self, s: &mut AppSettings) {
        s.context = self.context;
        s.batch_size = self.batch_size;
        s.ubatch_size = self.ubatch_size;
        s.session_timeout = self.session_timeout;
        s.temperature = self.temperature;
        s.top_p = self.top_p;
        s.top_k = self.top_k;
        s.repeat_penalty = self.repeat_penalty;
        s.presence_penalty = self.presence_penalty;
        s.enable_batch_size = self.enable_batch_size;
        s.enable_ubatch_size = self.enable_ubatch_size;
        s.enable_session_timeout = self.enable_session_timeout;
        s.enable_temperature = self.enable_temperature;
        s.enable_top_p = self.enable_top_p;
        s.enable_top_k = self.enable_top_k;
        s.enable_repeat_penalty = self.enable_repeat_penalty;
        s.enable_presence_penalty = self.enable_presence_penalty;
        s.enable_flash_attn = self.enable_flash_attn;
        s.flash_attn = self.flash_attn;
        s.enable_fit = self.enable_fit;
        s.fit = self.fit;
        s.fit_target = self.fit_target;
        s.fit_ctx = self.fit_ctx;
        s.enable_fit_target = self.enable_fit_target;
        s.enable_fit_ctx = self.enable_fit_ctx;
        s.spec_type = self.spec_type;
        s.spec_draft_n_max = self.spec_draft_n_max;
        s.enable_spec_draft_n_max = self.enable_spec_draft_n_max;
        s.spec_draft_n_min = self.spec_draft_n_min;
        s.spec_draft_p_min = self.spec_draft_p_min;
        s.spec_draft_p_split = self.spec_draft_p_split;
        s.enable_spec_draft_n_min = self.enable_spec_draft_n_min;
        s.enable_spec_draft_p_min = self.enable_spec_draft_p_min;
        s.enable_spec_draft_p_split = self.enable_spec_draft_p_split;
        s.enable_spec_draft_type_k = self.enable_spec_draft_type_k;
        s.enable_spec_draft_type_v = self.enable_spec_draft_type_v;
        s.enable_kv_offload = self.enable_kv_offload;
        s.enable_kv_unified = self.enable_kv_unified;
        s.kv_offload = self.kv_offload;
        s.cache_type_k = self.cache_type_k;
        s.cache_type_v = self.cache_type_v;
        s.enable_cache_type_kv = self.enable_cache_type_kv;
        s.kv_unified = self.kv_unified;
        s.swa_full = self.swa_full;
        s.kvmem_enabled = self.kvmem_enabled;
        s.kvmem_trace = self.kvmem_trace;
        s.kvmem_budget = self.kvmem_budget;
        s.kvmem_block_tokens = self.kvmem_block_tokens;
        s.kvmem_sink_tokens = self.kvmem_sink_tokens;
        s.kvmem_gen_reserve = self.kvmem_gen_reserve;
        s.kvmem_gpu_ratio = self.kvmem_gpu_ratio;
        s.kvmem_cpu_gb = self.kvmem_cpu_gb;
        s.kvmem_nvme_gb = self.kvmem_nvme_gb;
        s.kvmem_recent_tokens = self.kvmem_recent_tokens;
        s.kvmem_method = self.kvmem_method;
        s.kvmem_query_last = self.kvmem_query_last;
        s.kvmem_query_max_tokens = self.kvmem_query_max_tokens;
        s.kvmem_query_replay = self.kvmem_query_replay;
        s.kvmem_query_policy = self.kvmem_query_policy;
        s.kvmem_mtp_state = self.kvmem_mtp_state;
        s.kvmem_nvme_dir = self.kvmem_nvme_dir;
        s.kvmem_harvest_v = self.kvmem_harvest_v;
        s.kvmem_raw_k_nvme = self.kvmem_raw_k_nvme;
        s.kv_dtype = self.kv_dtype;
        s.spec_kv_dtype = self.spec_kv_dtype;
        s.gpu_layers_mode = self.gpu_layers_mode;
        s.enable_gpu_layers = self.enable_gpu_layers;
        s.enable_split_mode = self.enable_split_mode;
        s.split_mode = self.split_mode;
        s.tensor_split = self.tensor_split;
        s.cpu_moe = self.cpu_moe;
        s.n_cpu_moe = self.n_cpu_moe;
        s.n_cpu_ffn = self.n_cpu_ffn;
        s.static_path = self.static_path.clone();
        s.enable_reasoning = self.enable_reasoning;
        s.enable_reasoning_format = self.enable_reasoning_format;
        s.enable_reasoning_effort = self.enable_reasoning_effort;
        s.enable_reasoning_budget = self.enable_reasoning_budget;
        s.enable_reasoning_preserve = self.enable_reasoning_preserve;
        s.reasoning = self.reasoning;
        s.reasoning_format = self.reasoning_format;
        s.reasoning_effort = self.reasoning_effort;
        s.reasoning_budget = self.reasoning_budget;
        s.reasoning_preserve = self.reasoning_preserve;
        s.enable_jinja = self.enable_jinja;
        s.jinja_enabled = self.jinja_enabled;
        s.chat_template = self.chat_template;
        s.chat_template_file = self.chat_template_file;
        s.enable_mmproj_auto = self.enable_mmproj_auto;
        s.mmproj_auto = self.mmproj_auto.clone();
        s.enable_mmproj_offload = self.enable_mmproj_offload;
        s.mmproj_offload = self.mmproj_offload;
        s.mmproj_device = self.mmproj_device;
        s.image_min_tokens = self.image_min_tokens;
        s.image_max_tokens = self.image_max_tokens;
        s.mtmd_batch_max_tokens = self.mtmd_batch_max_tokens;
        s.video_fps = self.video_fps;
        s.video_timestamp_interval = self.video_timestamp_interval;
        s.video_ffmpeg_dir = self.video_ffmpeg_dir;
    }

    /// 从单个预设构造（字段与 AppSettings 同名，直接对拷）；
    /// 分享功能使用——不经过全局 AppSettings
    pub fn from_preset(p: &Preset) -> Self {
        Self {
            context: p.context,
            batch_size: p.batch_size,
            ubatch_size: p.ubatch_size,
            session_timeout: p.session_timeout,
            temperature: p.temperature,
            top_p: p.top_p,
            top_k: p.top_k,
            repeat_penalty: p.repeat_penalty,
            presence_penalty: p.presence_penalty,
            enable_batch_size: p.enable_batch_size,
            enable_ubatch_size: p.enable_ubatch_size,
            enable_session_timeout: p.enable_session_timeout,
            enable_temperature: p.enable_temperature,
            enable_top_p: p.enable_top_p,
            enable_top_k: p.enable_top_k,
            enable_repeat_penalty: p.enable_repeat_penalty,
            enable_presence_penalty: p.enable_presence_penalty,
            enable_flash_attn: p.enable_flash_attn,
            flash_attn: p.flash_attn.clone(),
            enable_fit: p.enable_fit,
            fit: p.fit.clone(),
            fit_target: p.fit_target.clone(),
            fit_ctx: p.fit_ctx,
            enable_fit_target: p.enable_fit_target,
            enable_fit_ctx: p.enable_fit_ctx,
            spec_type: p.spec_type.clone(),
            spec_draft_n_max: p.spec_draft_n_max,
            enable_spec_draft_n_max: p.enable_spec_draft_n_max,
            spec_draft_n_min: p.spec_draft_n_min,
            spec_draft_p_min: p.spec_draft_p_min,
            spec_draft_p_split: p.spec_draft_p_split,
            enable_spec_draft_n_min: p.enable_spec_draft_n_min,
            enable_spec_draft_p_min: p.enable_spec_draft_p_min,
            enable_spec_draft_p_split: p.enable_spec_draft_p_split,
            enable_spec_draft_type_k: p.enable_spec_draft_type_k,
            enable_spec_draft_type_v: p.enable_spec_draft_type_v,
            enable_kv_offload: p.enable_kv_offload,
            enable_kv_unified: p.enable_kv_unified,
            kv_offload: p.kv_offload,
            cache_type_k: p.cache_type_k.clone(),
            cache_type_v: p.cache_type_v.clone(),
            enable_cache_type_kv: p.enable_cache_type_kv,
            kv_mlock: false,
            kv_mmap: true,
            kv_unified: p.kv_unified,
            swa_full: p.swa_full,
            kvmem_enabled: p.kvmem_enabled,
            kvmem_trace: p.kvmem_trace,
            kvmem_budget: p.kvmem_budget,
            kvmem_block_tokens: p.kvmem_block_tokens,
            kvmem_sink_tokens: p.kvmem_sink_tokens,
            kvmem_gen_reserve: p.kvmem_gen_reserve,
            kvmem_gpu_ratio: p.kvmem_gpu_ratio,
            kvmem_cpu_gb: p.kvmem_cpu_gb,
            kvmem_nvme_gb: p.kvmem_nvme_gb,
            kvmem_recent_tokens: p.kvmem_recent_tokens,
            kvmem_method: p.kvmem_method.clone(),
            kvmem_query_last: p.kvmem_query_last,
            kvmem_query_max_tokens: p.kvmem_query_max_tokens,
            kvmem_query_replay: p.kvmem_query_replay.clone(),
            kvmem_query_policy: p.kvmem_query_policy.clone(),
            kvmem_mtp_state: p.kvmem_mtp_state.clone(),
            kvmem_nvme_dir: p.kvmem_nvme_dir.clone(),
            kvmem_harvest_v: p.kvmem_harvest_v,
            kvmem_raw_k_nvme: p.kvmem_raw_k_nvme,
            kv_dtype: p.kv_dtype.clone(),
            spec_kv_dtype: p.spec_kv_dtype.clone(),
            gpu_layers_mode: p.gpu_layers_mode,
            enable_gpu_layers: p.enable_gpu_layers,
            enable_split_mode: p.enable_split_mode,
            split_mode: p.split_mode.clone(),
            tensor_split: p.tensor_split.clone(),
            cpu_moe: p.cpu_moe,
            n_cpu_moe: p.n_cpu_moe,
            n_cpu_ffn: p.n_cpu_ffn,
            static_path: p.static_path.clone(),
            enable_reasoning: p.enable_reasoning,
            enable_reasoning_format: p.enable_reasoning_format,
            enable_reasoning_effort: p.enable_reasoning_effort,
            enable_reasoning_budget: p.enable_reasoning_budget,
            enable_reasoning_preserve: p.enable_reasoning_preserve,
            reasoning: p.reasoning.clone(),
            reasoning_format: p.reasoning_format.clone(),
            reasoning_effort: p.reasoning_effort.clone(),
            reasoning_budget: p.reasoning_budget,
            reasoning_preserve: p.reasoning_preserve,
            enable_jinja: p.enable_jinja,
            jinja_enabled: p.jinja_enabled,
            chat_template: p.chat_template.clone(),
            chat_template_file: p.chat_template_file.clone(),
            enable_mmproj_auto: p.enable_mmproj_auto,
            mmproj_auto: p.mmproj_auto.clone(),
            enable_mmproj_offload: p.enable_mmproj_offload,
            mmproj_offload: p.mmproj_offload,
            mmproj_device: p.mmproj_device.clone(),
            image_min_tokens: p.image_min_tokens,
            image_max_tokens: p.image_max_tokens,
            mtmd_batch_max_tokens: p.mtmd_batch_max_tokens,
            video_fps: p.video_fps,
            video_timestamp_interval: p.video_timestamp_interval,
            video_ffmpeg_dir: p.video_ffmpeg_dir.clone(),
        }
    }

    /// 应用到新预设（name 为最终名称；其余字段用 Preset 默认值兜底）
    pub fn to_preset(self, name: String) -> Preset {
        let mut p = Preset::default();
        p.name = name;
        p.context = self.context;
        p.batch_size = self.batch_size;
        p.ubatch_size = self.ubatch_size;
        p.session_timeout = self.session_timeout;
        p.temperature = self.temperature;
        p.top_p = self.top_p;
        p.top_k = self.top_k;
        p.repeat_penalty = self.repeat_penalty;
        p.presence_penalty = self.presence_penalty;
        p.enable_batch_size = self.enable_batch_size;
        p.enable_ubatch_size = self.enable_ubatch_size;
        p.enable_session_timeout = self.enable_session_timeout;
        p.enable_temperature = self.enable_temperature;
        p.enable_top_p = self.enable_top_p;
        p.enable_top_k = self.enable_top_k;
        p.enable_repeat_penalty = self.enable_repeat_penalty;
        p.enable_presence_penalty = self.enable_presence_penalty;
        p.enable_flash_attn = self.enable_flash_attn;
        p.flash_attn = self.flash_attn;
        p.enable_fit = self.enable_fit;
        p.fit = self.fit;
        p.fit_target = self.fit_target;
        p.fit_ctx = self.fit_ctx;
        p.enable_fit_target = self.enable_fit_target;
        p.enable_fit_ctx = self.enable_fit_ctx;
        p.spec_type = self.spec_type;
        p.spec_draft_n_max = self.spec_draft_n_max;
        p.enable_spec_draft_n_max = self.enable_spec_draft_n_max;
        p.spec_draft_n_min = self.spec_draft_n_min;
        p.spec_draft_p_min = self.spec_draft_p_min;
        p.spec_draft_p_split = self.spec_draft_p_split;
        p.enable_spec_draft_n_min = self.enable_spec_draft_n_min;
        p.enable_spec_draft_p_min = self.enable_spec_draft_p_min;
        p.enable_spec_draft_p_split = self.enable_spec_draft_p_split;
        p.enable_spec_draft_type_k = self.enable_spec_draft_type_k;
        p.enable_spec_draft_type_v = self.enable_spec_draft_type_v;
        p.enable_kv_offload = self.enable_kv_offload;
        p.enable_kv_unified = self.enable_kv_unified;
        p.kv_offload = self.kv_offload;
        p.cache_type_k = self.cache_type_k;
        p.cache_type_v = self.cache_type_v;
        p.enable_cache_type_kv = self.enable_cache_type_kv;
        p.kv_unified = self.kv_unified;
        p.swa_full = self.swa_full;
        p.kvmem_enabled = self.kvmem_enabled;
        p.kvmem_trace = self.kvmem_trace;
        p.kvmem_budget = self.kvmem_budget;
        p.kvmem_block_tokens = self.kvmem_block_tokens;
        p.kvmem_sink_tokens = self.kvmem_sink_tokens;
        p.kvmem_gen_reserve = self.kvmem_gen_reserve;
        p.kvmem_gpu_ratio = self.kvmem_gpu_ratio;
        p.kvmem_cpu_gb = self.kvmem_cpu_gb;
        p.kvmem_nvme_gb = self.kvmem_nvme_gb;
        p.kvmem_recent_tokens = self.kvmem_recent_tokens;
        p.kvmem_method = self.kvmem_method;
        p.kvmem_query_last = self.kvmem_query_last;
        p.kvmem_query_max_tokens = self.kvmem_query_max_tokens;
        p.kvmem_query_replay = self.kvmem_query_replay;
        p.kvmem_query_policy = self.kvmem_query_policy;
        p.kvmem_mtp_state = self.kvmem_mtp_state;
        p.kvmem_nvme_dir = self.kvmem_nvme_dir;
        p.kvmem_harvest_v = self.kvmem_harvest_v;
        p.kvmem_raw_k_nvme = self.kvmem_raw_k_nvme;
        p.kv_dtype = self.kv_dtype;
        p.spec_kv_dtype = self.spec_kv_dtype;
        p.gpu_layers_mode = self.gpu_layers_mode;
        p.enable_gpu_layers = self.enable_gpu_layers;
        p.enable_split_mode = self.enable_split_mode;
        p.split_mode = self.split_mode;
        p.tensor_split = self.tensor_split;
        p.cpu_moe = self.cpu_moe;
        p.n_cpu_moe = self.n_cpu_moe;
        p.n_cpu_ffn = self.n_cpu_ffn;
        p.static_path = self.static_path.clone();
        p.enable_reasoning = self.enable_reasoning;
        p.enable_reasoning_format = self.enable_reasoning_format;
        p.enable_reasoning_effort = self.enable_reasoning_effort;
        p.enable_reasoning_budget = self.enable_reasoning_budget;
        p.enable_reasoning_preserve = self.enable_reasoning_preserve;
        p.reasoning = self.reasoning;
        p.reasoning_format = self.reasoning_format;
        p.reasoning_effort = self.reasoning_effort;
        p.reasoning_budget = self.reasoning_budget;
        p.reasoning_preserve = self.reasoning_preserve;
        p.enable_jinja = self.enable_jinja;
        p.jinja_enabled = self.jinja_enabled;
        p.chat_template = self.chat_template;
        p.chat_template_file = self.chat_template_file;
        p.enable_mmproj_auto = self.enable_mmproj_auto;
        p.mmproj_auto = self.mmproj_auto.clone();
        p.enable_mmproj_offload = self.enable_mmproj_offload;
        p.mmproj_offload = self.mmproj_offload;
        p.mmproj_device = self.mmproj_device;
        p.image_min_tokens = self.image_min_tokens;
        p.image_max_tokens = self.image_max_tokens;
        p.mtmd_batch_max_tokens = self.mtmd_batch_max_tokens;
        p.video_fps = self.video_fps;
        p.video_timestamp_interval = self.video_timestamp_interval;
        p.video_ffmpeg_dir = self.video_ffmpeg_dir;
        p
    }
}

// ─────────────── Base64URL（手写实现，零新依赖；字符集 A-Z a-z 0-9 - _） ───────────────

const B64URL_T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let i = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL_T[(i >> 18) as usize & 63] as char);
        out.push(B64URL_T[(i >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL_T[(i >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL_T[i as usize & 63] as char);
        }
    }
    out
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    // 容错：跳过字母表外字符（粘贴引入的空白/Markdown 残留）
    let filtered: Vec<u8> = s
        .bytes()
        .filter(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
        .collect();
    let mut out = Vec::with_capacity(filtered.len() * 3 / 4);
    for chunk in filtered.chunks(4) {
        let mut i = 0u32;
        for (n, c) in chunk.iter().enumerate() {
            i |= val(*c)? << (18 - 6 * n);
        }
        out.push((i >> 16) as u8);
        if chunk.len() > 2 {
            out.push((i >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(i as u8);
        }
    }
    Some(out)
}

// ─────────────── 编码 / 解码 ───────────────

/// 分享码格式版本前缀（格式变更时升级 LCP2/LCP3…，旧码永久可解析）
const SHARE_PREFIX: &str = "LCP1-";
/// 分享码长度上限（防滥用；远大于正常长度）
const SHARE_CODE_MAX: usize = 64 * 1024;

/// 分享载荷（编码时的顶层结构）
#[derive(Serialize, Deserialize)]
struct SharePayload {
    /// 编码格式版本
    lv: u8,
    /// 生成分享码时的 launcher 版本（跨版本导入警告用）
    gen: String,
    /// 预设列表
    presets: Vec<ShareItem>,
    /// 依赖声明（需要本机配置的内容；旧码无此段时按默认处理）
    #[serde(default)]
    decl: ShareDecl,
}

#[derive(Serialize, Deserialize)]
struct ShareItem {
    name: String,
    /// 参数（已剥离默认值字段的 JSON 对象）
    params: serde_json::Value,
}

/// 依赖声明：分享者声明"要让此预设完整运行，接收方需要准备什么"
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ShareDecl {
    /// 声明的 MCP 工具（名称 + 分享者本机命令，供接收方参考下载）
    #[serde(default)]
    mcp: Vec<ShareMcpDecl>,
    /// 分享参数中含自定义 Jinja 对话模板（内容在 params.chat_template，可阅读/复制）
    #[serde(default)]
    has_template: bool,
    /// 含 GBNF 语法约束（内容在 params.grammar）
    #[serde(default)]
    has_grammar: bool,
    /// 含 JSON Schema 约束（内容在 params.json_schema）
    #[serde(default)]
    has_schema: bool,
    /// 需要配置本机模型文件（恒 true；所有预设运行都需要模型）
    #[serde(default = "default_true")]
    model_required: bool,
    /// 需要配置 llama-server 程序位置（恒 true；AppSettings 级）
    #[serde(default = "default_true")]
    server_required: bool,
}

fn default_true() -> bool {
    true
}

/// 单个 MCP 工具声明
#[derive(Serialize, Deserialize, Clone)]
pub struct ShareMcpDecl {
    name: String,
    /// 分享者本机的启动命令（仅供参考，接收方需改为自己的路径）
    command: String,
    args: Vec<String>,
}

/// 生成依赖声明：扫描启用的 MCP server 与非空的自包含内容字段
fn build_decl(settings: &AppSettings) -> ShareDecl {
    let mcp = if settings.mcp_enabled {
        parse_mcp_servers(&settings.mcp_config_json)
            .into_iter()
            .filter(|(name, _, _)| {
                settings
                    .mcp_server_states
                    .get(name)
                    .copied()
                    .unwrap_or(false)
            })
            .map(|(name, command, args)| ShareMcpDecl {
                name,
                command,
                args,
            })
            .collect()
    } else {
        Vec::new()
    };
    ShareDecl {
        mcp,
        has_template: settings.enable_jinja && !settings.chat_template.trim().is_empty(),
        has_grammar: !settings.grammar.trim().is_empty(),
        has_schema: !settings.json_schema.trim().is_empty(),
        model_required: true,
        server_required: true,
    }
}

/// 生成分享码：剥离与默认导出相同的字段 → gzip → Base64URL → LCP1- 前缀。
/// 同时扫描 AppSettings 生成"依赖声明"（MCP 工具等需本机配置的内容）。
fn encode_share_code(
    items: &[(&str, &Preset)],
    settings: &AppSettings,
) -> Result<(String, ShareDecl), String> {
    let default_params = ParamsExport::from_settings(&AppSettings::default());
    let default_v = serde_json::to_value(&default_params).map_err(|e| e.to_string())?;

    let mut list = Vec::with_capacity(items.len());
    for (name, preset) in items {
        let params = ParamsExport::from_preset(preset);
        let mut v = serde_json::to_value(&params).map_err(|e| e.to_string())?;
        // 剥离与默认值相同的字段（典型用户只改少量参数 → 大幅缩短）
        if let (Some(obj), Some(def)) = (v.as_object_mut(), default_v.as_object()) {
            for (k, dv) in def {
                if obj.get(k) == Some(dv) {
                    obj.remove(k);
                }
            }
        }
        list.push(ShareItem {
            name: (*name).to_string(),
            params: v,
        });
    }

    let payload = SharePayload {
        lv: 1,
        gen: env!("CARGO_PKG_VERSION").to_string(),
        presets: list,
        decl: build_decl(settings),
    };
    let json = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;

    // gzip 压缩（flate2 已有依赖）
    use flate2::write::GzEncoder;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&json).map_err(|e| e.to_string())?;
    let compressed = enc.finish().map_err(|e| e.to_string())?;

    Ok((
        format!("{}{}", SHARE_PREFIX, base64url_encode(&compressed)),
        payload.decl,
    ))
}

/// 从 mcp_config_json 解析 MCP server 定义（容错：格式错误按空处理）
fn parse_mcp_servers(config_json: &str) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    if let Ok(root) = serde_json::from_str::<serde_json::Value>(config_json) {
        if let Some(servers) = root.get("mcpServers").and_then(|v| v.as_object()) {
            for (name, cfg) in servers {
                let command = cfg
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = cfg
                    .get("args")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                out.push((name.clone(), command, args));
            }
        }
    }
    out
}

/// 解析分享码：返回（生成版本，预设列表[名称/参数]）
fn decode_share_code(
    code: &str,
) -> Result<(String, Vec<(String, ParamsExport)>, ShareDecl), String> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    if trimmed.len() > SHARE_CODE_MAX {
        return Err("too-long".to_string());
    }
    let body = trimmed
        .strip_prefix(SHARE_PREFIX)
        .ok_or_else(|| "bad-prefix".to_string())?;
    let compressed = base64url_decode(body).ok_or_else(|| "bad-base64".to_string())?;
    let mut gz = flate2::read::GzDecoder::new(&compressed[..]);
    let mut json = Vec::new();
    std::io::Read::read_to_end(&mut gz, &mut json).map_err(|_| "bad-gzip".to_string())?;
    let payload: SharePayload =
        serde_json::from_slice(&json).map_err(|_| "bad-json".to_string())?;

    // 补回被剥离的默认值字段（缺失键从默认导出补齐）后反序列化
    let default_params = ParamsExport::from_settings(&AppSettings::default());
    let default_v = serde_json::to_value(&default_params).map_err(|e| e.to_string())?;

    let mut items = Vec::with_capacity(payload.presets.len());
    for item in payload.presets {
        let mut pv = item.params;
        if let (Some(obj), Some(def)) = (pv.as_object_mut(), default_v.as_object()) {
            for (k, dv) in def {
                obj.entry(k.clone()).or_insert(dv.clone());
            }
        }
        let params: ParamsExport =
            serde_json::from_value(pv).map_err(|_| "bad-params".to_string())?;
        items.push((item.name, params));
    }
    Ok((payload.gen, items, payload.decl))
}

// ─────────────── 弹窗状态 ───────────────

/// 分享/引入弹窗的运行时状态（不持久化，随主进程生命周期）
#[derive(Default)]
pub struct PresetShareUi {
    pub open: bool,
    /// 0 = 分享预设页，1 = 引入预设页（usize 适配 winui::SegmentedControl）
    pub tab: usize,
    /// 分享页：勾选的预设名
    pub share_selected: Vec<String>,
    /// 分享页：生成的分享码
    pub share_code: String,
    /// 分享页：已复制提示
    pub copied: bool,
    /// 引入页：粘贴的分享码文本
    pub import_text: String,
    /// 引入页：解析出的预设（名称 + 参数）
    pub import_items: Option<Vec<(String, ParamsExport)>>,
    /// 引入页：各预设勾选状态
    pub import_selected: Vec<bool>,
    /// 引入页：分享码生成版本与当前不同（警告）
    pub import_ver_mismatch: bool,
    /// 引入页：结果消息（true = 成功，false = 失败/警告）
    pub import_msg: Option<(bool, String)>,
    /// 引入页：解析出的声明（导入时暂存，添加预设时写入 imported_decl）
    pub import_decl: Option<String>,
    /// 声明阅读窗口：打开状态 + 格式化文本
    pub decl_window_open: bool,
    pub decl_window_text: String,
}

impl PresetShareUi {
    /// 打开弹窗时重置瞬态状态（保留粘贴文本，避免误关丢内容）
    pub fn open(&mut self) {
        self.open = true;
        self.tab = 0;
        self.share_selected.clear();
        self.share_code.clear();
        self.copied = false;
        self.import_items = None;
        self.import_selected.clear();
        self.import_ver_mismatch = false;
        self.import_msg = None;
        self.import_decl = None;
        self.decl_window_open = false;
        self.decl_window_text.clear();
    }
}

// ─────────────── 弹窗 UI ───────────────

/// 分享/引入弹窗（WinUI3 模态对话框：固定 520 宽 / 最大 420 高，组件内自动夹到屏幕内）
pub fn share_window(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    share: &mut PresetShareUi,
    lang: &i18n::Language,
) {
    if !share.open {
        return;
    }
    let should_close = widgets::dialog(
        ctx,
        "preset_share_dialog",
        i18n::t(i18n::Key::ShareWindowTitle, lang),
        520.0,
        420.0,
        |ui, _avail_h| {
            // 顶部标签页：组件库 SegmentedControl（WinUI 分段样式，整宽占满）
            ui.add(
                winui::SegmentedControl::new(&mut share.tab, [
                    i18n::t(i18n::Key::ShareTabShare, lang),
                    i18n::t(i18n::Key::ShareTabImport, lang),
                ])
                .width(ui.available_width()),
            );
            ui.separator();

            if share.tab == 0 {
                render_share_tab(ui, settings, share, lang);
            } else {
                render_import_tab(ui, settings, share, lang);
            }
        },
        |ui| {
            let mut close = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnClose, lang)))
                    .clicked()
                {
                    close = true;
                }
            });
            close
        },
    );
    share.open = !should_close;
}

/// 分享页：勾选本地预设 → 生成分享码 → 复制
fn render_share_tab(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    share: &mut PresetShareUi,
    lang: &i18n::Language,
) {
    // 全选 / 全不选
    ui.horizontal(|ui| {
        if ui
            .add(winui::FluentButton::new(i18n::t(i18n::Key::ShareSelectAll, lang)))
            .clicked()
        {
            share.share_selected = settings.presets.iter().map(|p| p.name.clone()).collect();
        }
        if ui
            .add(winui::FluentButton::new(i18n::t(i18n::Key::ShareSelectNone, lang)))
            .clicked()
        {
            share.share_selected.clear();
        }
    });
    // 预设选择子框：内容少时收缩到内容高，超出 160px 内部滚动（避免大段留白）
    egui::ScrollArea::vertical()
        .id_salt("share_preset_list")
        .max_height(160.0)
        .show(ui, |ui| {
            for preset in &settings.presets {
                let mut checked = share.share_selected.contains(&preset.name);
                if ui
                    .add(winui::FluentCheckBox::new(&mut checked, &preset.name))
                    .changed()
                {
                    if checked && !share.share_selected.contains(&preset.name) {
                        share.share_selected.push(preset.name.clone());
                    } else if !checked {
                        share.share_selected.retain(|n| n != &preset.name);
                    }
                }
            }
        });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !share.share_selected.is_empty(),
                winui::AccentButton::new(i18n::t(i18n::Key::ShareGenBtn, lang)),
            )
            .clicked()
        {
            let items: Vec<(&str, &Preset)> = settings
                .presets
                .iter()
                .filter(|p| share.share_selected.contains(&p.name))
                .map(|p| (p.name.as_str(), p))
                .collect();
            match encode_share_code(&items, settings) {
                Ok((code, _decl)) => {
                    share.share_code = code;
                    share.copied = false;
                }
                Err(e) => log::warn!("[share] 生成分享码失败: {}", e),
            }
        }
    });

    if !share.share_code.is_empty() {
        ui.add_space(6.0);
        ui.label(i18n::t(i18n::Key::ShareCodeLabel, lang));
        // 只读展示：Label（天然不可编辑）+ 可选中；固定高度子框内滚动，
        // 避免 TextEdit 随内容撑高把弹窗顶出应用界面
        egui::ScrollArea::vertical()
            .id_salt("share_code_view")
            .max_height(110.0)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&share.share_code).monospace())
                        .selectable(true)
                        .wrap(),
                );
            });
        ui.horizontal(|ui| {
            if ui
                .add(winui::FluentButton::new(i18n::t(i18n::Key::ShareCopyBtn, lang)))
                .clicked()
            {
                ui.ctx().copy_text(share.share_code.clone());
                share.copied = true;
            }
            if share.copied {
                ui.small(
                    egui::RichText::new(i18n::t(i18n::Key::ShareCopiedMsg, lang))
                        .color(ui.visuals().text_color()),
                );
            }
        });
    }
}

/// 引入页：粘贴分享码 → 提取 → 勾选 → 添加到本地
fn render_import_tab(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    share: &mut PresetShareUi,
    lang: &i18n::Language,
) {
    ui.label(i18n::t(i18n::Key::SharePasteLabel, lang));
    // 粘贴框：固定高度子框内滚动，长分享码不撑爆弹窗
    egui::ScrollArea::vertical()
        .id_salt("import_text_box")
        .max_height(100.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut share.import_text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4)
                    .font(egui::TextStyle::Monospace),
            );
        });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !share.import_text.trim().is_empty(),
                winui::AccentButton::new(i18n::t(i18n::Key::ShareExtractBtn, lang)),
            )
            .clicked()
        {
            match decode_share_code(&share.import_text) {
                Ok((gen, items, decl)) => {
                    if items.is_empty() {
                        share.import_msg = Some((
                            false,
                            i18n::t(i18n::Key::ShareNoPresetsInCode, lang).to_string(),
                        ));
                        share.import_items = None;
                    } else {
                        share.import_ver_mismatch = gen != env!("CARGO_PKG_VERSION");
                        share.import_selected = vec![true; items.len()];
                        share.import_items = Some(items);
                        // 声明持久化：serde 序列化后存到每个引入的预设
                        // （imported_decl 字段），配置窗口感叹号读取展示
                        share.import_decl = serde_json::to_string(&decl).ok();
                        share.import_msg = None;
                    }
                }
                Err(e) => {
                    let reason = match e.as_str() {
                        "bad-prefix" => i18n::t(i18n::Key::ShareErrBadPrefix, lang).to_string(),
                        "too-long" => i18n::t(i18n::Key::ShareErrTooLong, lang).to_string(),
                        _ => i18n::t(i18n::Key::ShareParseError, lang).to_string(),
                    };
                    share.import_msg = Some((false, reason));
                }
            }
        }
        if ui
            .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnPasteShareCode, lang)))
            .clicked()
        {
            if let Ok(text) =
                arboard::Clipboard::new().and_then(|mut cb| cb.get_text().map(|s| s.to_string()))
            {
                share.import_text = text;
            }
        }
    });

    // 版本不兼容警告（允许强制添加）
    if share.import_ver_mismatch {
        ui.add_space(4.0);
        ui.small(
            egui::RichText::new(i18n::t(i18n::Key::ShareVerWarn, lang))
                .color(ui.visuals().warn_fg_color),
        );
    }

    // 提取结果列表（勾选后添加）：内容少时收缩，超出 160px 内部滚动
    if let Some(items) = &share.import_items {
        egui::ScrollArea::vertical()
            .id_salt("import_preset_list")
            .max_height(160.0)
            .show(ui, |ui| {
                for (i, (name, _)) in items.iter().enumerate() {
                    let mut checked = share.import_selected.get(i).copied().unwrap_or(true);
                    if ui.checkbox(&mut checked, name).changed() {
                        if let Some(s) = share.import_selected.get_mut(i) {
                            *s = checked;
                        }
                    }
                }
            });
    }

    // 结果 / 错误消息
    if let Some((ok, msg)) = &share.import_msg {
        ui.add_space(4.0);
        ui.small(egui::RichText::new(msg).color(if *ok {
            ui.visuals().text_color()
        } else {
            ui.visuals().error_fg_color
        }));
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if let Some(items) = share.import_items.clone() {
            let any_selected = share.import_selected.iter().any(|s| *s);
            if ui
                .add_enabled(any_selected, winui::AccentButton::new(i18n::t(i18n::Key::ShareAddBtn, lang)))
                .clicked()
            {
                let mut added = 0usize;
                for (i, (name, params)) in items.iter().enumerate() {
                    if share.import_selected.get(i).copied().unwrap_or(false) {
                        // 重名处理：追加序号避免覆盖本地预设
                        let mut final_name = name.clone();
                        let mut n = 2;
                        while settings.presets.iter().any(|p| p.name == final_name) {
                            final_name = format!("{} ({})", name, n);
                            n += 1;
                        }
                        let mut preset = params.clone().to_preset(final_name);
                        preset.imported = true;
                        preset.imported_decl = share.import_decl.clone();
                        settings.presets.push(preset);
                        added += 1;
                    }
                }
                share.import_msg = Some((
                    true,
                    format!("{} {}", i18n::t(i18n::Key::ShareImportOk, lang), added),
                ));
                share.import_items = None;
                share.import_text.clear();
            }
            if ui
                .add(winui::FluentButton::new(i18n::t(i18n::Key::ShareCancelBtn, lang)))
                .clicked()
            {
                share.import_items = None;
                share.import_msg = None;
            }
        }
    });
}

// ─────────────── 引入预设配置窗口（草稿隔离） ───────────────

/// 引入预设"配置"窗口的运行时状态（随主进程生命周期）
#[derive(Default)]
pub struct PresetConfigUi {
    pub open: bool,
    /// 正在配置的引入预设名
    pub preset_name: String,
    /// 配置草稿：打开窗口时快照，全部控件绑草稿；确定时才写回真实设置
    pub draft: Option<AppSettings>,
    /// 选中的分类：0=模型管理 1=参数面板 2=MCP 3=Server与网络 4=RPC
    pub category: u8,
    /// 未配置完成的确认条（第一次点确定时置位）
    pub confirm_pending: bool,
    /// 确认保存标志（校验通过或用户强制确认）
    pub confirm_saved: bool,
    /// 结果消息
    pub msg: Option<(bool, String)>,
}

impl PresetConfigUi {
    /// 打开配置窗口：快照当前设置并应用引入预设参数到草稿。
    /// 快照按值接收（调用方 clone 后移动），彻底脱离 settings 借用。
    pub fn open_for(&mut self, preset: &Preset, settings_snapshot: AppSettings) {
        log::info!("[preset_share] open_for: {}", preset.name);
        self.open = true;
        self.preset_name = preset.name.clone();
        self.category = 0;
        self.confirm_pending = false;
        self.confirm_saved = false;
        self.msg = None;
        let mut draft = settings_snapshot;
        preset.clone().apply_to(&mut draft);
        self.draft = Some(draft);
    }
}

/// 分类定义（id, i18n 键）
const CONFIG_CATS: &[(u8, i18n::Key)] = &[
    (0, i18n::Key::TabModel),
    (1, i18n::Key::TabParams),
    (2, i18n::Key::TabMcp),
    (3, i18n::Key::SectionNetwork),
    (4, i18n::Key::TabRpc),
];

/// 引入预设配置窗口（草稿隔离：全部控件绑草稿，确定时才写回真实设置）
pub fn config_window(
    ctx: &egui::Context,
    settings: &mut AppSettings,
    cfg: &mut PresetConfigUi,
    share: &mut PresetShareUi,
    lang: &i18n::Language,
) {
    if !cfg.open {
        return;
    }
    log::info!(
        "[preset_share] config_window 渲染中（预设: {}）",
        cfg.preset_name
    );
    let Some(draft) = cfg.draft.as_mut() else {
        log::warn!("[preset_share] draft 缺失，窗口无法显示");
        return;
    };
    // 声明数据来源：正在配置的预设所存的 imported_decl（分享码导入时写入）
    let decl_effective: ShareDecl = settings
        .presets
        .iter()
        .find(|p| p.name == cfg.preset_name)
        .and_then(|p| p.imported_decl.as_deref())
        .and_then(|json| serde_json::from_str::<ShareDecl>(json).ok())
        .unwrap_or_default();

    let mut cancel_clicked = false;
    let mut save_clicked = false;
    // 分类快照：内容闭包取拷贝，避免与侧栏闭包的 &mut cfg.category 写冲突
    // （Rust 2021 不相交捕获按字段划分，两闭包同帧内先后调用也无法共享可变借用）
    let category = cfg.category;
    let accent = crate::theme::accent_color(&settings.accent_color);
    let screen = ctx.content_rect().size();
    // 默认尺寸随应用窗口缩放（封顶 1000x640，下限 680x380），保证弹窗完整落在应用界面内；
    // 组件还会再夹到屏幕内（与原窗口 max 92% 屏幕的意图一致）
    let width = (screen.x * 0.85).clamp(680.0, 1000.0);
    let max_height = (screen.y * 0.85).clamp(380.0, 640.0);
    let should_close = widgets::dialog_with_side_panel(
        ctx,
        "preset_config_dialog",
        i18n::t(i18n::Key::ConfigWindowTitle, lang),
        width,
        150.0,
        max_height,
        // 中央内容区：顶行（预设名 + 声明入口）固定，分类内容占满剩余高度并内部滚动
        |ui, avail_h| {
            let start_y = ui.cursor().top();
            // 顶行：正在配置的预设名 + "分享依赖声明"按钮入口
            ui.horizontal(|ui| {
                // 主文本色随深浅色主题切换（不可用 .strong()：其颜色恒为按下态前景色=白色）
                ui.label(format!(
                    "{}: {}",
                    i18n::t(i18n::Key::ConfigPresetLabel, lang),
                    cfg.preset_name
                ));
                // 声明入口：文字按钮，点击打开依赖声明阅读窗口
                if ui
                    .button(i18n::t(i18n::Key::ShareDeclBtnText, lang))
                    .on_hover_text(i18n::t(i18n::Key::ShareDeclBtnTip, lang))
                    .clicked()
                {
                    // 从正在配置的预设读取声明
                    share.decl_window_text = settings
                        .presets
                        .iter()
                        .find(|p| p.name == cfg.preset_name)
                        .and_then(|p| p.imported_decl.clone())
                        .and_then(|json| serde_json::from_str::<ShareDecl>(&json).ok())
                        .map(|d| format_decl_text(&d, lang))
                        .unwrap_or_default();
                    share.decl_window_open = true;
                }
            });
            ui.separator();
            // 扣除顶行已占用的高度后，让分类内容滚动区撑满剩余空间（同 mcp_panel 的推算方式）
            let used_h = ui.cursor().top() - start_y;
            // 中央配置区：按分类渲染（复用现有面板函数，绑定草稿）
            egui::ScrollArea::vertical()
                .id_salt("preset_cfg_content")
                .auto_shrink(false)
                .max_height((avail_h - used_h).max(80.0))
                .show(ui, |ui| match category {
                    0 => crate::ui::model_panel::ui(ui, draft, lang),
                    1 => crate::ui::params_panel::ui(ui, draft, lang),
                    2 => crate::ui::mcp_panel::ui(ui, draft, lang),
                    3 => render_server_network_card(ui, draft, lang),
                    4 => render_rpc_card(ui, draft, lang),
                    _ => {}
                });
        },
        // 左侧分类导航：全宽按钮、选中态主题色填充（现代化样式）
        |ui| {
            for (cat_id, key) in CONFIG_CATS {
                let selected = cfg.category == *cat_id;
                let text = egui::RichText::new(i18n::t(*key, lang));
                let btn = if selected {
                    egui::Button::new(text.color(widgets::contrast_text(accent)))
                        .fill(accent)
                        .corner_radius(4.0)
                } else {
                    egui::Button::new(text)
                        .fill(ui.visuals().widgets.inactive.bg_fill)
                        .corner_radius(4.0)
                };
                if ui.add_sized([ui.available_width(), 30.0], btn).clicked() {
                    cfg.category = *cat_id;
                }
                ui.add_space(3.0);
            }
        },
        // 底部操作条：确定 / 取消 + 未配置确认消息
        |ui| {
            if cfg.confirm_pending {
                ui.small(
                    egui::RichText::new(i18n::t(i18n::Key::ConfigConfirmPending, lang))
                        .color(ui.visuals().warn_fg_color),
                );
            }
            if let Some((ok, msg)) = &cfg.msg {
                ui.small(egui::RichText::new(msg).color(if *ok {
                    ui.visuals().text_color()
                } else {
                    ui.visuals().error_fg_color
                }));
            }
            // 操作按钮右对齐（顺手好点），加大尺寸；确定为主操作（低饱和主题色填充）
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let primary_fill =
                    egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 175);
                if ui
                    .add_sized(
                        [104.0, 32.0],
                        widgets::rounded_button(
                            i18n::t(i18n::Key::ConfigSaveBtn, lang),
                            Some(primary_fill),
                        ),
                    )
                    .clicked()
                {
                    save_clicked = true;
                }
                if ui
                    .add_sized(
                        [104.0, 32.0],
                        widgets::rounded_button(i18n::t(i18n::Key::ShareCancelBtn, lang), None),
                    )
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
            false
        },
    );

    // footer 闭包只记录点击：内容闭包持有 draft 的可变借用，校验与写回在
    // dialog 返回后进行（同 mcp_panel editor_window 的枚举中转思路）
    if save_clicked {
        if cfg.confirm_pending {
            // 第二次点击：用户确认保存未配置完成的内容
            cfg.msg = Some((
                true,
                i18n::t(i18n::Key::ConfigSavedPartial, lang).to_string(),
            ));
            cfg.confirm_saved = true;
        } else {
            // 校验：模型路径 / 声明的 MCP 命令是否已配置
            let missing = collect_missing(&decl_effective, draft);
            if missing {
                cfg.confirm_pending = true;
            } else {
                cfg.confirm_saved = true;
            }
        }
    }
    if cancel_clicked {
        cfg.confirm_pending = false;
    }

    // 写回：用户点确定（confirm_saved）且草稿存在
    if !should_close && !cancel_clicked && cfg.confirm_saved {
        if let Some(draft) = cfg.draft.clone() {
            // 1) 全局写回（MCP 定义/网络/RPC 等全部草稿内容）。
            //    预设列表保留实时状态：若整体覆盖，会把步骤 2 写入的预设更新
            //    以及确认期间外部对预设的增删改一并冲掉
            let mut merged = draft.clone();
            merged.presets = settings.presets.clone();
            *settings = merged;
            // 2) 更新本预设：用 Preset::from_settings 全量快照回写。
            //    （ParamsExport 仅 39 个参数白名单字段，会把 host/port/sampler_seq
            //    等其余字段留在旧值上，导致"从预设启动"覆盖掉用户的修改）
            if let Some(preset) = settings
                .presets
                .iter_mut()
                .find(|p| p.name == cfg.preset_name)
            {
                let imported_decl = preset.imported_decl.clone();
                *preset = Preset::from_settings(&draft, cfg.preset_name.clone());
                preset.imported = true; // 引入标记持久（配置可随时再改）
                preset.imported_decl = imported_decl; // 声明不随快照丢失
            }
            cfg.msg = Some((true, i18n::t(i18n::Key::ConfigSavedOk, lang).to_string()));
        }
        cfg.confirm_saved = false;
        cfg.open = false;
    } else if should_close || cancel_clicked {
        cfg.open = false;
    }
}

/// 检查声明的必配项是否已配置（模型路径 + 声明的 MCP 命令）
fn collect_missing(decl: &ShareDecl, draft: &AppSettings) -> bool {
    if draft.model_path.as_os_str().is_empty() {
        return true;
    }
    if !decl.mcp.is_empty() {
        let existing = parse_mcp_servers(&draft.mcp_config_json);
        for decl_mcp in &decl.mcp {
            let configured = existing
                .iter()
                .any(|(name, cmd, _)| name == &decl_mcp.name && !cmd.is_empty());
            if !configured {
                return true;
            }
        }
    }
    false
}

/// Server 与网络简版配置卡（配置窗口专用；不含进程管理/下载按钮）
fn render_server_network_card(ui: &mut egui::Ui, s: &mut AppSettings, lang: &i18n::Language) {
    let accent = crate::theme::accent_color(&s.accent_color);
    widgets::card(ui, i18n::t(i18n::Key::SectionNetwork, lang), accent, |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelServerPath, lang));
            let mut p = s.server_path.to_string_lossy().to_string();
            if ui.text_edit_singleline(&mut p).changed() {
                s.server_path = std::path::PathBuf::from(p.trim());
            }
            if ui.button(i18n::t(i18n::Key::BtnBrowse, lang)).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("exe", &["exe"])
                    .pick_file()
                {
                    s.server_path = path;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelHost, lang));
            ui.text_edit_singleline(&mut s.host);
            ui.label(i18n::t(i18n::Key::LabelPort, lang));
            ui.add(egui::DragValue::new(&mut s.port).range(1..=65535));
            ui.label(i18n::t(i18n::Key::LabelParallelSlots, lang));
            ui.add(egui::DragValue::new(&mut s.parallel_slots).range(-1..=1024));
            if s.parallel_slots == 0 {
                s.parallel_slots = 1;
            }
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelSessionTimeout, lang));
            ui.add(egui::DragValue::new(&mut s.session_timeout).range(60..=3600));
            ui.label(i18n::t(i18n::Key::LabelAlias, lang));
            ui.text_edit_singleline(&mut s.alias);
        });
        ui.add_space(4.0);
        ui.separator();
        // API 安全性：卡内子分组
        ui.label(i18n::t(i18n::Key::SectionApiSecurity, lang));
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelApiKey, lang));
            ui.text_edit_singleline(&mut s.api_key);
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelApiPrefix, lang));
            ui.text_edit_singleline(&mut s.api_prefix);
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelCorsOrigins, lang));
            ui.text_edit_singleline(&mut s.cors_origins);
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelNuma, lang));
            ui.text_edit_singleline(&mut s.numa);
        });
    });
}

/// RPC 简版配置卡（配置窗口专用；仅参数，不含进程管理）
fn render_rpc_card(ui: &mut egui::Ui, s: &mut AppSettings, lang: &i18n::Language) {
    let accent = crate::theme::accent_color(&s.accent_color);
    widgets::card(ui, i18n::t(i18n::Key::TabRpc, lang), accent, |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcEndpoints, lang));
            ui.text_edit_singleline(&mut s.rpc_endpoints);
            ui.small(i18n::t(i18n::Key::HintRpcEndpoints, lang));
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcThreads, lang));
            ui.add(egui::DragValue::new(&mut s.rpc_threads).range(1..=1024));
            ui.label(i18n::t(i18n::Key::LabelRpcDevice, lang));
            ui.text_edit_singleline(&mut s.rpc_device);
        });
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcCache, lang));
            ui.checkbox(&mut s.rpc_cache, "");
        });
    });
}

// ─────────────── 依赖声明阅读窗口 ───────────────

/// 依赖声明阅读窗口：只读展示分享者声明的内容，MCP 命令可复制
pub fn decl_window(ctx: &egui::Context, share: &mut PresetShareUi, lang: &i18n::Language) {
    if !share.decl_window_open {
        return;
    }
    let should_close = widgets::dialog(
        ctx,
        "preset_share_decl_dialog",
        i18n::t(i18n::Key::ShareDeclWindowTitle, lang),
        520.0,
        380.0,
        |ui, _avail_h| {
            ui.small(i18n::t(i18n::Key::ShareDeclWindowHint, lang));
            ui.separator();
            ui.add(
                egui::Label::new(egui::RichText::new(&share.decl_window_text).monospace())
                    .selectable(true)
                    .wrap(),
            );
        },
        |ui| {
            let mut copy = false;
            ui.horizontal(|ui| {
                if ui.button(i18n::t(i18n::Key::ShareCopyBtn, lang)).clicked() {
                    copy = true;
                }
                if share.copied {
                    ui.small(
                        egui::RichText::new(i18n::t(i18n::Key::ShareCopiedMsg, lang))
                            .color(ui.visuals().text_color()),
                    );
                }
            });
            if copy {
                ui.ctx().copy_text(share.decl_window_text.clone());
                share.copied = true;
            }
            false
        },
    );
    share.decl_window_open = !should_close;
}

/// 生成声明阅读文本（声明窗口与感叹号窗口共用）
fn format_decl_text(decl: &ShareDecl, lang: &i18n::Language) -> String {
    let mut out = String::new();

    // 模型文件（恒需）
    out.push_str(&format!(
        "◆ {}\n   {}\n\n",
        i18n::t(i18n::Key::ShareDeclModel, lang),
        i18n::t(i18n::Key::ShareDeclModelHint, lang)
    ));
    // llama-server 程序
    out.push_str(&format!(
        "◆ {}\n   {}\n\n",
        i18n::t(i18n::Key::ShareDeclServer, lang),
        i18n::t(i18n::Key::ShareDeclServerHint, lang)
    ));

    // MCP 工具清单
    if !decl.mcp.is_empty() {
        out.push_str(&format!(
            "◆ {}\n",
            i18n::t(i18n::Key::ShareDeclMcpTitle, lang)
        ));
        for m in &decl.mcp {
            let args_str = if m.args.is_empty() {
                String::new()
            } else {
                format!(" {}", m.args.join(" "))
            };
            out.push_str(&format!("   • {} — {}{}\n", m.name, m.command, args_str));
        }
        out.push_str(&format!(
            "   {}\n\n",
            i18n::t(i18n::Key::ShareDeclMcpHint, lang)
        ));
    }

    // 自包含内容提示（内容在参数中可查看）
    if decl.has_template {
        out.push_str(&format!(
            "◆ {}\n   {}\n\n",
            i18n::t(i18n::Key::ShareDeclTemplate, lang),
            i18n::t(i18n::Key::ShareDeclTemplateHint, lang)
        ));
    }
    if decl.has_grammar {
        out.push_str(&format!(
            "◆ {}\n   {}\n\n",
            i18n::t(i18n::Key::ShareDeclGrammar, lang),
            i18n::t(i18n::Key::ShareDeclGrammarHint, lang)
        ));
    }
    if decl.has_schema {
        out.push_str(&format!(
            "◆ {}\n   {}\n\n",
            i18n::t(i18n::Key::ShareDeclSchema, lang),
            i18n::t(i18n::Key::ShareDeclSchemaHint, lang)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kvmem_fields_roundtrip_through_export() {
        let mut src = AppSettings::default();
        src.kvmem_enabled = false;
        src.kvmem_trace = true;
        src.kvmem_budget = 8192;
        src.kvmem_block_tokens = 256;
        src.kvmem_sink_tokens = 1024;
        src.kvmem_gen_reserve = 512;
        src.kvmem_gpu_ratio = 0.8;
        src.kvmem_cpu_gb = 16.0;
        src.kvmem_nvme_gb = 128.0;
        src.kvmem_recent_tokens = 333;
        src.kvmem_method = "recency".to_string();
        src.kvmem_query_last = 11;
        src.kvmem_query_max_tokens = 222;
        src.kvmem_query_replay = "legacy".to_string();
        src.kvmem_query_policy = "legacy".to_string();
        src.kvmem_mtp_state = "snapshots".to_string();
        src.kvmem_nvme_dir = r"D:\kvmem".to_string();
        src.kvmem_harvest_v = true;
        src.kvmem_raw_k_nvme = true;
        src.kv_dtype = "q4_0".to_string();
        src.spec_kv_dtype = "q5_0".to_string();

        let exported = ParamsExport::from_settings(&src);
        let json = serde_json::to_string(&exported).unwrap();
        let roundtripped: ParamsExport = serde_json::from_str(&json).unwrap();
        let mut dst = AppSettings::default();
        roundtripped.apply_to(&mut dst);

        assert_eq!(dst.kvmem_enabled, false);
        assert_eq!(dst.kvmem_trace, true);
        assert_eq!(dst.kvmem_budget, 8192);
        assert_eq!(dst.kvmem_block_tokens, 256);
        assert_eq!(dst.kvmem_sink_tokens, 1024);
        assert_eq!(dst.kvmem_gen_reserve, 512);
        assert_eq!(dst.kvmem_gpu_ratio, 0.8);
        assert_eq!(dst.kvmem_cpu_gb, 16.0);
        assert_eq!(dst.kvmem_nvme_gb, 128.0);
        assert_eq!(dst.kvmem_recent_tokens, 333);
        assert_eq!(dst.kvmem_method, "recency");
        assert_eq!(dst.kvmem_query_last, 11);
        assert_eq!(dst.kvmem_query_max_tokens, 222);
        assert_eq!(dst.kvmem_query_replay, "legacy");
        assert_eq!(dst.kvmem_query_policy, "legacy");
        assert_eq!(dst.kvmem_mtp_state, "snapshots");
        assert_eq!(dst.kvmem_nvme_dir, r"D:\kvmem");
        assert_eq!(dst.kvmem_harvest_v, true);
        assert_eq!(dst.kvmem_raw_k_nvme, true);
        assert_eq!(dst.kv_dtype, "q4_0");
        assert_eq!(dst.spec_kv_dtype, "q5_0");

        let from_preset = ParamsExport::from_preset(&Preset::from_settings(&src, "x".to_string()));
        assert_eq!(from_preset.kvmem_enabled, false);
        assert_eq!(from_preset.kvmem_trace, true);
        assert_eq!(from_preset.kvmem_budget, 8192);
        assert_eq!(from_preset.kvmem_block_tokens, 256);
        assert_eq!(from_preset.kvmem_sink_tokens, 1024);
        assert_eq!(from_preset.kvmem_gen_reserve, 512);
        assert_eq!(from_preset.kvmem_gpu_ratio, 0.8);
        assert_eq!(from_preset.kvmem_cpu_gb, 16.0);
        assert_eq!(from_preset.kvmem_nvme_gb, 128.0);
        assert_eq!(from_preset.kvmem_recent_tokens, 333);
        assert_eq!(from_preset.kvmem_method, "recency");
        assert_eq!(from_preset.kvmem_query_last, 11);
        assert_eq!(from_preset.kvmem_query_max_tokens, 222);
        assert_eq!(from_preset.kvmem_query_replay, "legacy");
        assert_eq!(from_preset.kvmem_query_policy, "legacy");
        assert_eq!(from_preset.kvmem_mtp_state, "snapshots");
        assert_eq!(from_preset.kvmem_nvme_dir, r"D:\kvmem");
        assert_eq!(from_preset.kvmem_harvest_v, true);
        assert_eq!(from_preset.kvmem_raw_k_nvme, true);
        assert_eq!(from_preset.kv_dtype, "q4_0");
        assert_eq!(from_preset.spec_kv_dtype, "q5_0");
    }
}
