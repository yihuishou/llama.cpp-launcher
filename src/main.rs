#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

pub mod app;
pub mod config;
pub mod downloader;
pub mod engine;
pub mod geo;
pub mod i18n;
pub mod kv_cache;
pub mod net_proxy;
pub mod shortcut;
mod spacing_debugger;
pub mod theme;
pub mod ui;
pub mod updater;

use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record};
use std::env;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// 文件日志写入开关（全局标志，由帮助菜单复选框控制）
static LOG_TO_FILE_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_log_to_file(enabled: bool) {
    LOG_TO_FILE_ENABLED.store(enabled, Ordering::Relaxed);
}

struct FileLogger {
    writer: Mutex<BufWriter<std::fs::File>>,
}

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) && LOG_TO_FILE_ENABLED.load(Ordering::Relaxed) {
            let mut w = self.writer.lock().unwrap();
            writeln!(
                w,
                "[{}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.args()
            )
            .ok();
            w.flush().ok(); // 强制刷新，确保日志立即写入磁盘
        }
    }
    fn flush(&self) {
        self.writer.lock().unwrap().flush().ok();
    }
}

fn init_logger() {
    // 获取 exe 同级目录
    let exe_path = env::current_exe().unwrap_or_default();
    let exe_dir = exe_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    // 读取配置，获取 log_to_file 和 linux_compat_mode 设置
    let config_path = exe_dir.join("llama_cpp_launcher_settings.json");
    let (log_enabled, linux_compat_mode) = if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                match serde_json::from_str::<crate::config::settings::AppSettings>(&content) {
                    Ok(s) => (s.log_to_file, s.linux_compat_mode),
                    Err(_) => (false, true),
                }
            }
            Err(_) => (false, true),
        }
    } else {
        (false, true)
    };

    // Linux 兼容模式：检测远程桌面环境并强制软件渲染
    #[cfg(target_os = "linux")]
    if linux_compat_mode {
        let is_remote = env::var("XDG_SESSION_TYPE")
            .unwrap_or_default()
            .contains("remote")
            || env::var("SESSIONNAME")
                .unwrap_or_default()
                .to_uppercase()
                .contains("RDP")
            || env::var("XRDP_SESSION").is_ok()
            || env::var("VNCDESKTOP").is_ok()
            || env::var("X2GO_SESSION").is_ok()
            || env::var("REMOTEDESKTOP").is_ok()
            || (env::var("XDG_SESSION_TYPE").unwrap_or_default() == "x11"
                && env::var("DISPLAY").unwrap_or_default().is_empty());

        if is_remote {
            env::set_var("GALLIUM_DRIVER", "llvmpipe");
            env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
            log::info!("检测到远程桌面环境，已启用软件渲染（llvmpipe）");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = linux_compat_mode;

    // 根据配置决定是否初始化文件日志器
    if log_enabled {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let log_path = exe_dir.join(format!("llama_launcher_{}.log", timestamp));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("Failed to create log file");

        let logger = FileLogger {
            writer: Mutex::new(BufWriter::new(file)),
        };

        if log::set_boxed_logger(Box::new(logger)).is_ok() {
            log::set_max_level(LevelFilter::Info);
        }

        // 同步全局开关状态
        LOG_TO_FILE_ENABLED.store(true, Ordering::Relaxed);
    } else {
        // 未启用文件日志，使用空 logger（仅记录到内存）
        struct NoOpLogger;
        impl Log for NoOpLogger {
            fn enabled(&self, _metadata: &Metadata) -> bool {
                false
            }
            fn log(&self, _record: &Record) {}
            fn flush(&self) {}
        }
        let _ = log::set_boxed_logger(Box::new(NoOpLogger));
        log::set_max_level(LevelFilter::Info);

        LOG_TO_FILE_ENABLED.store(false, Ordering::Relaxed);
    }
}

use app::LlamaLauncherApp;
use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

fn main() -> eframe::Result {
    init_logger();

    // 自更新等待者进程：旧版本 spawn 自身（--updater-switch <pid>），
    // 等旧进程完全退出后启动已换入正式名的新版本。无 UI，完毕即退出。
    #[cfg(target_os = "windows")]
    if std::env::args().nth(1).as_deref() == Some("--updater-switch") {
        if let Ok(pid) = std::env::args().nth(2).unwrap_or_default().parse::<u32>() {
            crate::updater::run_updater_waiter(pid);
            std::process::exit(0);
        }
    }

    // 启动清理：上次更新遗留的旧版备份（exe.old）与空 update 目录
    #[cfg(target_os = "windows")]
    crate::updater::cleanup_leftovers_on_startup();

    // 检测命令行参数是否包含 --minimized（开机自启时使用）
    let start_minimized = env::args().any(|arg| arg == "--minimized");

    // 使用统一的主窗口尺寸
    let default_size = egui::vec2(1300.0, 800.0);

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size(default_size)
        .with_title("llama.cpp launcher");
    let viewport = if let Some(icon) = load_window_icon() {
        viewport.with_icon(icon)
    } else {
        viewport
    };

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "llama.cpp launcher",
        options,
        Box::new(move |cc| {
            // 配置 CJK 中文字体，解决中文乱码问题
            let mut fonts = FontDefinitions::default();
            load_cjk_fonts(&mut fonts);
            load_icon_fonts(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(LlamaLauncherApp::new(cc, start_minimized)))
        }),
    )
}

fn load_window_icon() -> Option<egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../assets/llama.ico"))
        .ok()?
        .to_rgba8();
    Some(egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

/// 加载内置字体（编译时嵌入，适配 egui 0.34）
fn load_cjk_fonts(fonts: &mut FontDefinitions) {
    // 只嵌入 CJK 中文字体（egui 内置 NotoEmoji-Regular 已支持 emoji）
    let cjk_bytes = include_bytes!("../assets/NotoSansSC-Regular.ttf");

    // 注册 CJK 字体数据
    fonts.font_data.insert(
        "Noto Sans SC".to_owned(),
        Arc::new(FontData::from_owned(cjk_bytes.to_vec())),
    );

    // 在现有字体家族的 **最前面** 插入 CJK 字体，保留 egui 内置 emoji 字体
    // egui 默认: Proportional = [Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]
    // egui 默认: Monospace    = [Hack, Ubuntu-Light, NotoEmoji-Regular, emoji-icon-font]

    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        proportional.insert(0, "Noto Sans SC".to_owned());
    }

    if let Some(monospace) = fonts.families.get_mut(&FontFamily::Monospace) {
        monospace.insert(0, "Noto Sans SC".to_owned());
    }

    log::info!("CJK 字体加载完成 (Noto Sans SC)");
}

/// 加载 iconflow Fluent UI 图标字体
fn load_icon_fonts(fonts: &mut FontDefinitions) {
    let fallback_fonts: Vec<String> = fonts.font_data.keys().cloned().collect();

    for font in iconflow::fonts() {
        fonts.font_data.insert(
            font.family.to_string(),
            Arc::new(FontData::from_static(font.bytes)),
        );
        let family = fonts
            .families
            .entry(FontFamily::Name(font.family.into()))
            .or_default();
        family.insert(0, font.family.to_string());
        for fallback in &fallback_fonts {
            if fallback != font.family {
                family.push(fallback.clone());
            }
        }
    }

    log::info!("iconflow 字体加载完成");
}
