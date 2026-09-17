//! 设置面板 —— 吸收原菜单栏的全部功能
//!
//! 分区：外观（主题色 8 色 + 深色模式）、语言、启动器（开机自启 / 桌面快捷方式 / 保存加载配置）、
//! 调试（保存日志文件 / 调试模式）、关于（版本 / 项目地址 / 关于弹窗）。

use crate::app::{disable_auto_start, enable_auto_start, open_repo_url};
use crate::config::settings::{AppSettings, SettingsManager};
use crate::i18n;
use crate::ui::widgets;
use egui::{Color32, RichText};

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    settings_manager: &SettingsManager,
    lang: &i18n::Language,
    server_manager: &crate::engine::server::ServerManager,
    show_about: &mut bool,
    debug_mode: &mut bool,
    updater: &crate::updater::UpdaterHandle,
) {
    let accent = crate::theme::accent_color(&settings.accent_color);

    // 注意：不在这里重复渲染标题 "设置"——顶栏已经显示了当前页面标题

    // ── 外观 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::ThemeAppearance, lang),
        accent,
        |ui| {
            ui.label(i18n::t(i18n::Key::ThemeColor, lang));
            ui.horizontal_wrapped(|ui| {
                let colors = [
                    "#0A84FF", "#FF3B30", "#FF9500", "#FFCC00", "#34C759", "#00C7BE", "#AF52DE",
                    "#FF2D55",
                ];
                let cur = crate::theme::parse_hex(&settings.accent_color);
                for c in &colors {
                    let rgb = crate::theme::parse_hex(c);
                    let col = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let selected = rgb == cur;
                    // 判断是否需要深色勾（亮色块用深勾）
                    let needs_dark_check = rgb[0] > 200 && rgb[1] > 200 && rgb[2] > 200;
                    if widgets::color_swatch(ui, col, selected, !needs_dark_check).clicked() {
                        settings.accent_color = c.to_string();
                    }
                }
            });
            ui.add_space(8.0);
            // ★ Toggle 新签名：开关在左，标签在右
            let theme_opts = [
                i18n::t(i18n::Key::ThemeLight, lang),
                i18n::t(i18n::Key::ThemeDark, lang),
                i18n::t(i18n::Key::ThemeSystem, lang),
            ];
            let mut theme_idx = match settings.theme_mode.as_str() {
                "light" => 0,
                "dark" => 1,
                _ => 2,
            };
            widgets::segmented(ui, &theme_opts, &mut theme_idx, accent);
            settings.theme_mode = match theme_idx {
                0 => "light".to_string(),
                1 => "dark".to_string(),
                _ => "auto".to_string(),
            };
        },
    );

    // ── 语言 ──
    widgets::card(ui, i18n::t(i18n::Key::LabelLanguage, lang), accent, |ui| {
        let zh = i18n::t(i18n::Key::LangZh, lang);
        let en = i18n::t(i18n::Key::LangEn, lang);
        let opts = [zh, en];
        let mut sel = match settings.language.as_str() {
            "en" => 1,
            _ => 0,
        };
        widgets::segmented(ui, &opts, &mut sel, accent);
        settings.language = if sel == 1 {
            "en".to_string()
        } else {
            "zh".to_string()
        };
    });

    // ── 启动器 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SettingsLauncher, lang),
        accent,
        |ui| {
            let mut auto = settings.auto_start;
            // ★ Toggle 新签名：返回值直接用于判断是否变更
            if widgets::toggle(
                ui,
                &mut auto,
                i18n::t(i18n::Key::MenuItemAutoStart, lang),
                accent,
            ) {
                settings.auto_start = auto;
                if auto {
                    enable_auto_start(settings.silent_start);
                } else {
                    disable_auto_start();
                }
                let _ = settings_manager.save(settings);
            }

            // 静默启动开关（仅在开机自启开启时显示）
            if settings.auto_start {
                ui.add_space(4.0);
                let mut silent = settings.silent_start;
                if widgets::toggle(
                    ui,
                    &mut silent,
                    i18n::t(i18n::Key::MenuItemSilentStart, lang),
                    accent,
                ) {
                    settings.silent_start = silent;
                    // 更新注册表项以反映新的静默启动设置
                    if settings.auto_start {
                        enable_auto_start(silent);
                    }
                    let _ = settings_manager.save(settings);
                }
            }

            // Linux 兼容模式（仅在 Linux 平台显示）
            #[cfg(target_os = "linux")]
            {
                ui.add_space(4.0);
                let mut compat = settings.linux_compat_mode;
                if widgets::toggle(
                    ui,
                    &mut compat,
                    i18n::t(i18n::Key::LinuxCompatMode, lang),
                    accent,
                ) {
                    settings.linux_compat_mode = compat;
                    let _ = settings_manager.save(settings);
                }
                if settings.linux_compat_mode {
                    ui.label(
                        egui::RichText::new(i18n::t(i18n::Key::LinuxCompatModeDesc, lang))
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                }
            }

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(widgets::rounded_button(
                        i18n::t(i18n::Key::MenuItemSaveConfig, lang),
                        None,
                    ))
                    .clicked()
                {
                    let _ = settings_manager.save(settings);
                }
                if ui
                    .add(widgets::rounded_button(
                        i18n::t(i18n::Key::MenuItemLoadConfig, lang),
                        None,
                    ))
                    .clicked()
                {
                    if let Ok(s) = settings_manager.load() {
                        *settings = s;
                    }
                }
                if ui
                    .add(widgets::rounded_button(
                        i18n::t(i18n::Key::MenuItemCreateShortcut, lang),
                        None,
                    ))
                    .clicked()
                {
                    let _ = crate::shortcut::create_desktop_shortcut();
                }
            });
        },
    );

    // ── 系统服务 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SettingsSystemService, lang),
        accent,
        |ui| {
            ui.label(i18n::t(i18n::Key::SystemServiceDescription, lang));

            if true {
                // cfg!(target_os = "linux") - 暂时注释掉用于开发调试
                // Linux 平台：显示生成按钮

                // 服务状态显示
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    // ★ 不用 .strong()（浅色模式下 strong_text_color=白色→隐形），改用显式主文本色
                ui.label(
                    RichText::new(i18n::t(i18n::Key::SystemServiceStatus, lang))
                        .color(ui.visuals().text_color())
                        .strong(),
                );
                    let status = check_service_status();
                    let (status_text, status_color) = match status.as_str() {
                        "running" => (
                            i18n::t(i18n::Key::SystemServiceStatusRunning, lang),
                            Color32::from_rgb(52, 199, 89), // 绿色
                        ),
                        "stopped" => (
                            i18n::t(i18n::Key::SystemServiceStatusStopped, lang),
                            Color32::from_rgb(255, 59, 48), // 红色
                        ),
                        _ => (
                            i18n::t(i18n::Key::SystemServiceStatusUnknown, lang),
                            Color32::GRAY,
                        ),
                    };
                    ui.label(RichText::new(status_text).color(status_color));
                });

                // 服务控制按钮
                ui.add_space(8.0);
                let service_exists = check_service_exists();
                ui.horizontal_wrapped(|ui| {
                    let status = check_service_status();

                    // 启动按钮（仅在停止时可用）
                    if ui
                        .add_enabled(
                            status != "running",
                            widgets::rounded_button(
                                i18n::t(i18n::Key::SystemServiceStart, lang),
                                None,
                            ),
                        )
                        .clicked()
                    {
                        match run_systemctl("start") {
                            Ok(_) => { /* 状态会在下次刷新时更新 */ }
                            Err(e) => log::warn!("[system-service] 启动失败: {}", e),
                        }
                    }

                    // 停止按钮（仅在运行时可用）
                    if ui
                        .add_enabled(
                            status == "running",
                            widgets::rounded_button(
                                i18n::t(i18n::Key::SystemServiceStop, lang),
                                None,
                            ),
                        )
                        .clicked()
                    {
                        match run_systemctl("stop") {
                            Ok(_) => { /* 状态会在下次刷新时更新 */ }
                            Err(e) => log::warn!("[system-service] 停止失败: {}", e),
                        }
                    }

                    // 重启按钮（仅在运行时可用）
                    if ui
                        .add_enabled(
                            status == "running",
                            widgets::rounded_button(
                                i18n::t(i18n::Key::SystemServiceRestart, lang),
                                None,
                            ),
                        )
                        .clicked()
                    {
                        match run_systemctl("restart") {
                            Ok(_) => { /* 状态会在下次刷新时更新 */ }
                            Err(e) => log::warn!("[system-service] 重启失败: {}", e),
                        }
                    }

                    // 查看服务详情按钮（需要选中预设才能查看）
                    if ui
                        .add_enabled(
                            !settings.system_service_selected_preset.is_empty(),
                            widgets::rounded_button(
                                i18n::t(i18n::Key::SystemServiceViewDetails, lang),
                                None,
                            ),
                        )
                        .clicked()
                    {
                        settings.show_system_service_details = true;
                    }

                    // 卸载服务按钮（仅在服务存在时可用）
                    if ui
                        .add_enabled(
                            service_exists,
                            widgets::rounded_button(
                                i18n::t(i18n::Key::SystemServiceUninstall, lang),
                                None,
                            ),
                        )
                        .clicked()
                    {
                        match uninstall_service() {
                            Ok(_) => { /* 状态会在下次刷新时更新 */ }
                            Err(e) => log::warn!("[system-service] 卸载失败: {}", e),
                        }
                    }
                });

                // 服务不存在时的提示
                if !service_exists {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(i18n::t(i18n::Key::SystemServiceNotFound, lang))
                            .color(egui::Color32::GRAY)
                            .italics(),
                    );
                }

                // 预设选择和应用
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                // ★ 不用 .strong()（浅色模式下 strong_text_color=白色→隐形），改用显式主文本色
                ui.label(
                    RichText::new(i18n::t(i18n::Key::SystemServiceSelectPreset, lang))
                        .color(ui.visuals().text_color())
                        .strong(),
                );

                // 预设选择下拉框
                let preset_names: Vec<String> =
                    settings.presets.iter().map(|p| p.name.clone()).collect();
                let has_presets = !preset_names.is_empty();
                let mut selected_preset = settings.system_service_selected_preset.clone();

                ui.add_enabled_ui(has_presets, |ui| {
                    let placeholder = i18n::t(i18n::Key::SystemServiceNoPresets, lang);
                    egui::ComboBox::from_id_salt("system_service_preset")
                        .selected_text(if has_presets {
                            selected_preset.as_str()
                        } else {
                            placeholder.as_ref()
                        })
                        .show_ui(ui, |ui| {
                            for name in &preset_names {
                                if ui
                                    .selectable_value(&mut selected_preset, name.clone(), name)
                                    .changed()
                                {
                                    break;
                                }
                            }
                        });
                });

                settings.system_service_selected_preset = selected_preset.clone();

                ui.add_space(4.0);
                // 应用预设并更新服务配置按钮
                if ui
                    .add_enabled(
                        !selected_preset.is_empty(),
                        widgets::rounded_button(
                            i18n::t(i18n::Key::SystemServiceApplyAndUpdate, lang),
                            None,
                        ),
                    )
                    .clicked()
                {
                    if let Some(preset) =
                        settings.presets.iter().find(|p| p.name == selected_preset)
                    {
                        // 1. 应用预设到当前设置
                        preset.clone().apply_to(settings);

                        // 2. 生成服务文件并写入
                        let template = i18n::t(i18n::Key::LinuxServiceFileContent, lang);
                        let cmd = server_manager.build_launch_command(settings);
                        let content = build_systemd_service_file(&template, &cmd);

                        match create_service_file(&content) {
                            Ok(_) => { /* 成功 */ }
                            Err(e) => log::warn!("[system-service] 更新配置失败: {}", e),
                        }
                    }
                }

                // 生成和安装服务文件
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    // 生成服务文件按钮
                    if ui
                        .add(widgets::rounded_button(
                            i18n::t(i18n::Key::SystemServiceGenerate, lang),
                            None,
                        ))
                        .clicked()
                    {
                        let template = i18n::t(i18n::Key::LinuxServiceFileContent, lang);
                        let cmd = server_manager.build_launch_command(settings);
                        let content = build_systemd_service_file(&template, &cmd);

                        match create_service_file(&content) {
                            Ok(_) => { /* 成功 */ }
                            Err(e) => log::warn!("[system-service] 创建服务失败: {}", e),
                        }
                    }

                    // 右侧显示服务文件路径
                    ui.label(
                        egui::RichText::new("路径: /etc/systemd/system/llama-server.service")
                            .color(egui::Color32::GRAY)
                            .small(),
                    );
                });
            } else {
                // 非 Linux 平台：显示不可用提示
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(i18n::t(i18n::Key::SystemServiceNotAvailable, lang))
                        .color(egui::Color32::GRAY)
                        .italics(),
                );
            }
        },
    );

    // ── 服务详情弹窗 ──
    if settings.show_system_service_details {
        let mut open = settings.show_system_service_details;
        egui::Window::new(i18n::t(i18n::Key::SystemServiceDetailsServiceWindowTitle, lang))
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_height(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                // 根据选中预设生成服务文件
                let template = i18n::t(i18n::Key::LinuxServiceFileContent, lang);
                let cmd = if let Some(preset) = settings
                    .presets
                    .iter()
                    .find(|p| p.name == settings.system_service_selected_preset)
                {
                    let mut temp_settings = settings.clone();
                    preset.clone().apply_to(&mut temp_settings);
                    server_manager.build_launch_command(&temp_settings)
                } else {
                    server_manager.build_launch_command(settings)
                };
                let content = build_systemd_service_file(&template, &cmd);
                let mut content = content;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut content)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
                ui.separator();
                if ui.button(i18n::t(i18n::Key::BtnCopyServiceFile, lang)).clicked() {
                    ui.ctx().copy_text(content.to_string());
                }
            });
        settings.show_system_service_details = open;
    }

    // ── 调试 ──

    // ── 调试 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::MenuItemDebugMode, lang),
        accent,
        |ui| {
            let mut log_to_file = settings.log_to_file;
            // ★ Toggle 新签名
            if widgets::toggle(
                ui,
                &mut log_to_file,
                i18n::t(i18n::Key::MenuItemLogToFile, lang),
                accent,
            ) {
                crate::set_log_to_file(log_to_file);
                let _ = settings_manager.save(settings);
            }
            settings.log_to_file = log_to_file;

            widgets::toggle(
                ui,
                debug_mode,
                i18n::t(i18n::Key::MenuItemDebugMode, lang),
                accent,
            );
        },
    );

    // ── 关于 ──
    widgets::card(ui, i18n::t(i18n::Key::SettingsAbout, lang), accent, |ui| {
        ui.label(
            RichText::new(i18n::t(i18n::Key::AboutVersion, lang)).color(ui.visuals().text_color()),
        );
        ui.label(i18n::t(i18n::Key::AboutDescription, lang));
        ui.label(RichText::new(i18n::t(i18n::Key::AboutCopyright, lang)).small());
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(widgets::rounded_button(
                    i18n::t(i18n::Key::MenuItemRepo, lang),
                    None,
                ))
                .clicked()
            {
                open_repo_url();
            }
            if ui
                .add(widgets::rounded_button(
                    i18n::t(i18n::Key::AboutTitle, lang),
                    None,
                ))
                .clicked()
            {
                *show_about = true;
            }
        });

        // ── 自更新 ──
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
        let status = updater.snapshot();
        let busy = matches!(
            status.state,
            crate::updater::UpdateState::Checking
                | crate::updater::UpdateState::Downloading
                | crate::updater::UpdateState::Installing
        );
        ui.horizontal(|ui| {
            match status.state {
                crate::updater::UpdateState::Idle
                | crate::updater::UpdateState::UpToDate
                | crate::updater::UpdateState::Error(_) => {
                    if ui
                        .add_enabled(
                            !busy,
                            widgets::rounded_button(i18n::t(i18n::Key::BtnCheckUpdate, lang), None),
                        )
                        .clicked()
                    {
                        updater.check();
                    }
                }
                crate::updater::UpdateState::Available(_) => {
                    if ui
                        .add(widgets::rounded_button(
                            i18n::t(i18n::Key::BtnInstallUpdate, lang),
                            None,
                        ))
                        .clicked()
                    {
                        updater.install();
                    }
                }
                _ => {
                    // Checking / Downloading / Installing：按钮禁用，靠状态文案展示
                    ui.add_enabled(
                        false,
                        widgets::rounded_button(i18n::t(i18n::Key::BtnCheckUpdate, lang), None),
                    );
                }
            }
            ui.small(match &status.state {
                crate::updater::UpdateState::Checking => {
                    egui::RichText::new(i18n::t(i18n::Key::UpdChecking, lang))
                }
                crate::updater::UpdateState::UpToDate => {
                    egui::RichText::new(i18n::t(i18n::Key::UpdLatest, lang))
                }
                crate::updater::UpdateState::Available(v) => {
                    let text = format!("{} v{}", i18n::t(i18n::Key::UpdAvailable, lang), v);
                    egui::RichText::new(text).color(ui.visuals().warn_fg_color)
                }
                crate::updater::UpdateState::Downloading => {
                    let pct = if status.total > 0 {
                        format!(
                            "{} ({:.0}%)",
                            i18n::t(i18n::Key::UpdDownloading, lang),
                            status.done as f64 * 100.0 / status.total as f64
                        )
                    } else {
                        i18n::t(i18n::Key::UpdDownloading, lang).to_string()
                    };
                    egui::RichText::new(pct).color(ui.visuals().text_color())
                }
                crate::updater::UpdateState::Installing => {
                    egui::RichText::new(i18n::t(i18n::Key::UpdInstalling, lang))
                        .color(ui.visuals().warn_fg_color)
                }
                crate::updater::UpdateState::Error(msg) => {
                    if msg == crate::updater::ERR_NETWORK {
                        egui::RichText::new(i18n::t(i18n::Key::UpdNetworkError, lang))
                            .color(ui.visuals().error_fg_color)
                    } else {
                        egui::RichText::new(format!(
                            "{}: {}",
                            i18n::t(i18n::Key::UpdError, lang),
                            msg
                        ))
                        .color(ui.visuals().error_fg_color)
                    }
                }
                crate::updater::UpdateState::Idle => {
                    egui::RichText::new("").color(ui.visuals().text_color())
                }
            });
        });
        // 下载进度条
        if let crate::updater::UpdateState::Downloading = status.state {
            let frac = if status.total > 0 {
                (status.done as f64 / status.total as f64).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            ui.add(egui::ProgressBar::new(frac));
        }
    });
}

/// 构建 systemd 服务文件内容
/// 将模板中的 ExecStart 行替换为实际的启动命令，并自动填充当前用户信息
fn build_systemd_service_file(template: &str, cmd: &str) -> String {
    // 获取当前用户名
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "your-username".to_string());

    // 获取用户 home 目录
    let home_dir = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("/home/{}", username));

    // 先替换用户信息占位符
    let template = template
        .replace("your-username", &username)
        .replace("/home/your-username", &home_dir);

    let mut lines: Vec<String> = template.lines().map(String::from).collect();
    let mut in_exec_start = false;

    for line in &mut lines {
        if line.starts_with("ExecStart=") {
            *line = format!("ExecStart={}", cmd);
            in_exec_start = true;
        } else if in_exec_start && line.starts_with("    ") {
            // 跳过原模板中 ExecStart 的续行
            line.clear();
        } else {
            in_exec_start = false;
        }
    }

    // 移除连续的空行
    let mut result = Vec::new();
    let mut prev_empty = false;
    for line in lines {
        let is_empty = line.trim().is_empty();
        if !is_empty || !prev_empty {
            result.push(line);
        }
        prev_empty = is_empty;
    }

    result.join("\n")
}

/// 检查服务状态
fn check_service_status() -> String {
    let output = std::process::Command::new("systemctl")
        .args(["is-active", "llama-server.service"])
        .output();
    match output {
        Ok(o) => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if status == "active" {
                "running".to_string()
            } else if status == "inactive" || status == "failed" {
                "stopped".to_string()
            } else {
                "unknown".to_string()
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

/// 检查服务是否存在
fn check_service_exists() -> bool {
    let output = std::process::Command::new("systemctl")
        .args(["list-unit-files", "llama-server.service"])
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains("llama-server.service")
        }
        Err(_) => false,
    }
}

/// 执行 systemctl 命令（需要 pkexec）
fn run_systemctl(action: &str) -> Result<String, String> {
    let output = std::process::Command::new("pkexec")
        .args(["systemctl", action, "llama-server.service"])
        .output()
        .map_err(|e| format!("执行失败: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(if stderr.is_empty() {
            "操作被取消".to_string()
        } else {
            stderr
        })
    }
}

/// 生成服务文件并复制到 systemd 目录
fn create_service_file(content: &str) -> Result<String, String> {
    // 写入临时文件
    let temp_path = "/tmp/llama-server.service";
    std::fs::write(temp_path, content).map_err(|e| format!("写入临时文件失败: {}", e))?;

    // 使用 pkexec 复制到 systemd 目录
    let output = std::process::Command::new("pkexec")
        .args(["cp", temp_path, "/etc/systemd/system/llama-server.service"])
        .output()
        .map_err(|e| format!("复制文件失败: {}", e))?;

    if !output.status.success() {
        return Err("复制文件失败".to_string());
    }

    Ok("/etc/systemd/system/llama-server.service".to_string())
}

/// 卸载服务：先停止（如果运行中），再删除服务文件
fn uninstall_service() -> Result<String, String> {
    // 检查服务是否在运行
    let status = check_service_status();
    if status == "running" {
        // 先停止服务
        run_systemctl("stop")?;
    }

    // 删除服务文件
    let output = std::process::Command::new("pkexec")
        .args(["rm", "/etc/systemd/system/llama-server.service"])
        .output()
        .map_err(|e| format!("删除服务文件失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(if stderr.is_empty() {
            "删除服务文件失败".to_string()
        } else {
            stderr
        });
    }

    // 重新加载 systemd 配置
    let _ = std::process::Command::new("pkexec")
        .args(["systemctl", "daemon-reload"])
        .output();

    Ok("服务已卸载".to_string())
}
