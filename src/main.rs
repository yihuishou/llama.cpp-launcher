#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

pub mod app;
pub mod build;
pub mod config;
pub mod downloader;
pub mod engine;
pub mod env_check;
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

    // 读取配置，获取 log_to_file 设置
    let config_path = exe_dir.join("llama_cpp_launcher_settings.json");
    let log_enabled = if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => serde_json::from_str::<crate::config::settings::AppSettings>(&content)
                .map(|s| s.log_to_file)
                .unwrap_or(false),
            Err(_) => false,
        }
    } else {
        false
    };

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

    // 自更新等待者进程：旧版本 spawn 自身（--updater-switch <pid> <path>），
    // 等旧进程完全退出后启动已换入正式名的新版本。无 UI，完毕即退出。
    if std::env::args().nth(1).as_deref() == Some("--updater-switch") {
        let args: Vec<_> = std::env::args_os().collect();
        let pid = args
            .get(2)
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok());
        // 不用 current_exe() 兜底：Linux 上正式名被覆盖后它会返回 "(deleted)" 路径
        let launch_path = args.get(3).map(std::path::PathBuf::from);
        match (pid, launch_path) {
            (Some(pid), Some(path)) => {
                crate::updater::run_updater_waiter(pid, path);
                std::process::exit(0);
            }
            _ => {
                log::warn!("[updater] 等待者参数无效，退出");
                std::process::exit(1);
            }
        }
    }

    // 启动清理：上次更新遗留的旧版备份（exe.old，仅 Windows）与空 update 目录
    crate::updater::cleanup_leftovers_on_startup();

    // 检测命令行参数是否包含 --minimized（开机自启时使用）
    let start_minimized = env::args().any(|arg| arg == "--minimized");

    // 使用统一的主窗口尺寸
    let default_size = egui::vec2(1300.0, 800.0);

    let mut options = fluent_egui::winui::native_options("llama.cpp launcher");
    // 库默认 app_id 为 "fluent-egui"，沿用会让 Windows 以错误标识登记本应用
    options.viewport = options
        .viewport
        .clone()
        .with_app_id("com.yihuishou.llama_cpp_launcher")
        .with_inner_size(default_size)
        .with_transparent(true);
    if let Some(icon) = load_window_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }

    eframe::run_native(
        "llama.cpp launcher",
        options,
        Box::new(move |cc| {
            match cc.winit_window() {
                Some(window) => {
                    // 主题写死 Light：这里只求尽早把材质应用上，材质本身与主题无关；
                    // 原生边框的深浅由 App 首帧的 sync_native_window_theme 按实际主题改写。
                    let status = fluent_egui::winui::sync_window_backdrop(
                        window,
                        fluent_egui::winui::FluentTheme::Light,
                        fluent_egui::winui::BackdropKind::Auto,
                    );
                    log::info!("[backdrop] status={:?}", status);
                }
                None => log::warn!("[backdrop] 未取到 winit 窗口句柄，材质未应用"),
            }
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

/// 加载 iconflow Fluent UI 图标字体
pub fn load_icon_fonts(fonts: &mut FontDefinitions) {
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

/// 内嵌简体中文字体，作为 Proportional / Monospace 的末位回退
///
/// fluent-egui 只在 Windows 注册 Microsoft YaHei（其 install_system_fonts 里
/// Segoe UI / YaHei 只 push 进 Proportional），egui 自带的 Ubuntu-Light /
/// Hack / NotoEmoji 无汉字字形；而 epaint 0.35 整条字体链都缺字形时会渲染成
/// '◻'（PRIMARY_REPLACEMENT_CHAR），故非 Windows 平台满屏方块。
pub fn load_cjk_font(fonts: &mut FontDefinitions) {
    const CJK: &str = "Noto Sans SC";

    fonts.font_data.insert(
        CJK.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/NotoSansSC-Regular.ttf"
        ))),
    );

    // 放族尾部而非头部：拉丁字形仍由 Ubuntu-Light / Segoe UI 渲染，Windows 上
    // YaHei 排在其前则中文观感不变。Monospace 必须一并补——日志面板用
    // TextStyle::Monospace，该族在任何平台都没有中文字体
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        if !list.iter().any(|name| name == CJK) {
            list.push(CJK.to_owned());
        }
    }

    log::info!("中文字体回退已加载: Noto Sans SC (assets/NotoSansSC-Regular.ttf)");
}
