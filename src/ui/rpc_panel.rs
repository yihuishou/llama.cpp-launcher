use crate::config::settings::AppSettings;
use crate::engine::rpc::RpcManager;
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;

/// 返回平台特定的 RPC 本地文件缓存目录
fn rpc_cache_dir() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            std::path::PathBuf::from(local_app_data)
                .join("llama.cpp")
                .join("rpc")
        } else {
            std::path::PathBuf::from(r"C:\Users\Default\AppData\Local\llama.cpp\rpc")
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            std::path::PathBuf::from(home)
                .join(".cache")
                .join("llama.cpp")
                .join("rpc")
        } else {
            std::path::PathBuf::from("/tmp/llama.cpp/rpc")
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        std::path::PathBuf::from("/tmp/llama.cpp/rpc")
    }
}

/// 递归计算目录大小（字节）
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_file() {
                total += meta.len();
            } else if meta.is_dir() {
                total += dir_size(&entry.path());
            }
        }
    }
    total
}

/// 格式化字节数为人类可读字符串 (MB / GB)
fn format_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        "0 B".to_string()
    }
}

/// 格式化设备信息为简化显示格式
/// 输入: "Device 0: AMD Radeon RX 7900 XTX, gfx1100 (0x1100), VMM: no, Wave Size: 32, VRAM: 24560 MiB"
/// 输出: "ROCm0: AMD Radeon RX 7900 XTX (24560 MiB)"
fn format_device_info(line: &str) -> String {
    // 先去除行首尾空格，处理Linux下可能存在的缩进
    let trimmed_line = line.trim();

    // 提取设备编号
    let device_id = trimmed_line
        .strip_prefix("Device ")
        .and_then(|s| s.split(':').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // 根据品牌确定前缀
    let prefix = if trimmed_line.contains("AMD")
        || trimmed_line.contains("Radeon")
        || trimmed_line.contains("ROCm")
    {
        "ROCm"
    } else if trimmed_line.contains("NVIDIA") || trimmed_line.contains("CUDA") {
        "CUDA"
    } else {
        ""
    };

    // 提取设备名称（第一个逗号前的部分）
    let name = trimmed_line
        .split(':')
        .nth(1)
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // 提取 VRAM 大小
    let vram = trimmed_line
        .split("VRAM:")
        .nth(1)
        .and_then(|s| s.trim().split_whitespace().next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if !device_id.is_empty() && !name.is_empty() {
        if !vram.is_empty() {
            format!("{}{}: {} ({} MiB)", prefix, device_id, name, vram)
        } else {
            format!("{}{}: {}", prefix, device_id, name)
        }
    } else {
        line.to_string()
    }
}

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    settings_manager: &crate::config::settings::SettingsManager,
    lang: &i18n::Language,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] rpc_manager: &RpcManager,
) {
    let accent = crate::theme::accent_color(&settings.accent_color);

    // ggml-rpc-server.exe 路径
    widgets::card(ui, i18n::t(i18n::Key::PanelRpcTitle, lang), accent, |ui| {
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcPath, lang));
            let mut rpc_path_str = settings.rpc_server_path.to_string_lossy().to_string();
            let response = ui.add(winui::FluentTextEdit::new(&mut rpc_path_str));
            if response.changed() {
                settings.rpc_server_path = std::path::PathBuf::from(&rpc_path_str);
            }
        });

        ui.horizontal(|ui| {
            if ui
                .add(winui::FluentButton::new(i18n::t(
                    i18n::Key::BtnBrowse,
                    lang,
                )))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(i18n::t(i18n::Key::DialogSelectRpc, lang))
                    .add_filter(i18n::t(i18n::Key::FilterExecutable, lang), &["exe"])
                    .pick_file()
                {
                    settings.rpc_server_path = path;
                }
            }
            if ui
                .add(winui::FluentButton::new(i18n::t(
                    i18n::Key::BtnAutoDetect,
                    lang,
                )))
                .clicked()
            {
                if let Some(path) = settings_manager.auto_detect_rpc_path() {
                    settings.rpc_server_path = path;
                } else {
                    settings.rpc_server_path = std::path::PathBuf::from("");
                }
            }

            #[cfg(target_os = "linux")]
            {
                let rpc_exists = rpc_manager.check_rpc_server(&settings.rpc_server_path);
                let btn = egui::Button::new(i18n::t(i18n::Key::BtnAutoAuthorize, lang))
                    .min_size(egui::vec2(70.0, 20.0));
                let btn = if rpc_exists {
                    btn
                } else {
                    btn.sense(egui::Sense::hover())
                };
                if ui.add(btn).clicked() {
                    if let Err(e) = rpc_manager.authorize_rpc_server(&settings.rpc_server_path) {
                        log::error!("自动授权失败: {}", e);
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelHost, lang));
            ui.add(winui::FluentTextEdit::new(&mut settings.rpc_host));
            ui.label(i18n::t(i18n::Key::LabelPort, lang));
            let mut port = settings.rpc_port as f64;
            if ui
                .add(
                    winui::NumberBox::new(&mut port)
                        .range(1.0..=65535.0)
                        .width(90.0)
                        .spin_button_placement(winui::NumberBoxSpinButtonPlacement::Compact),
                )
                .changed()
            {
                settings.rpc_port = port as _;
            }
        });

        ui.horizontal(|ui| {
            if ui
                .add(
                    winui::FluentButton::new(i18n::t(i18n::Key::BtnHostLocal, lang))
                        .min_width(40.0),
                )
                .clicked()
            {
                settings.rpc_host = "127.0.0.1".to_string();
            }
            if ui
                .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnHostAny, lang)).min_width(50.0))
                .clicked()
            {
                settings.rpc_host = "0.0.0.0".to_string();
            }
        });

        // 线程数
        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcThreads, lang));
            let mut threads = settings.rpc_threads as f64;
            ui.add(
                winui::NumberBox::new(&mut threads)
                    .range(1.0..=1024.0)
                    .width(90.0)
                    .spin_button_placement(winui::NumberBoxSpinButtonPlacement::Compact),
            );
            settings.rpc_threads = threads as _;
            ui.small(i18n::t(i18n::Key::HintRpcThreads, lang));
        });

        ui.horizontal(|ui| {
            ui.label(i18n::t(i18n::Key::LabelRpcDevice, lang));
            ui.add(winui::FluentTextEdit::new(&mut settings.rpc_device));
            ui.small(i18n::t(i18n::Key::HintRpcDevice, lang));
        });

        // 查看设备列表按钮
        let rpc_available = rpc_manager.check_rpc_server(&settings.rpc_server_path);
        let btn = ui.add_enabled(
            rpc_available,
            winui::FluentButton::new(i18n::t(i18n::Key::BtnViewDeviceList, lang)),
        );
        if btn.clicked() {
            if settings.show_device_list {
                settings.show_device_list = false;
            } else {
                // 执行 ggml-rpc-server.exe -d 获取设备列表
                settings.device_list_output.clear();
                let mut cmd = std::process::Command::new(&settings.rpc_server_path);
                cmd.arg("-d")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000u32);
                }
                match cmd.output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        let raw = if !stdout.is_empty() {
                            stdout
                        } else if !stderr.is_empty() {
                            stderr
                        } else {
                            String::new()
                        };
                        // 只保留包含 "Device" 的行
                        let devices: Vec<String> = raw
                            .lines()
                            .filter(|line| line.contains("Device"))
                            .map(|line| line.trim().to_string())
                            .collect();
                        if devices.is_empty() {
                            settings.device_list_output =
                                i18n::t(i18n::Key::HintDeviceListEmpty, lang).to_string();
                        } else {
                            settings.device_list_output = devices.join("\n");
                        }
                    }
                    Err(e) => {
                        settings.device_list_output = format!("执行失败: {}", e);
                    }
                }
                settings.show_device_list = true;
            }
        }

        // 设备列表输出区域（多行列表样式）
        if settings.show_device_list {
            ui.label(i18n::t(i18n::Key::LabelDeviceListTitle, lang));
            if settings.device_list_output.is_empty()
                || settings.device_list_output == i18n::t(i18n::Key::HintDeviceListEmpty, lang)
            {
                ui.label(i18n::t(i18n::Key::HintDeviceListEmpty, lang));
            } else {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for line in settings.device_list_output.lines() {
                            if !line.is_empty() {
                                // 根据设备品牌设置圆点颜色
                                let dot_color = if line.contains("AMD") {
                                    egui::Color32::from_rgb(220, 50, 50) // AMD 红色
                                } else if line.contains("NVIDIA") {
                                    egui::Color32::from_rgb(50, 180, 50) // NVIDIA 绿色
                                } else if line.contains("Intel") || line.contains("INTEL") {
                                    egui::Color32::from_rgb(50, 100, 220) // Intel 蓝色
                                } else {
                                    accent // 其他使用主题色
                                };
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("●").color(dot_color).size(10.0));
                                    ui.label(
                                        egui::RichText::new(format_device_info(line))
                                            .color(ui.visuals().text_color())
                                            .size(13.0),
                                    );
                                    // 添加至设备按钮
                                    let device_entry = line
                                        .strip_prefix("Device ")
                                        .and_then(|s| s.split(':').next())
                                        .map(|s| s.trim().to_string())
                                        .map(|device_id| {
                                            if line.contains("AMD")
                                                || line.contains("Radeon")
                                                || line.contains("ROCm")
                                            {
                                                format!("ROCm{}", device_id)
                                            } else if line.contains("NVIDIA")
                                                || line.contains("CUDA")
                                            {
                                                format!("CUDA{}", device_id)
                                            } else {
                                                device_id
                                            }
                                        });
                                    if let Some(ref entry) = device_entry {
                                        let already_added = settings
                                            .rpc_device
                                            .split(',')
                                            .any(|s| s.trim() == entry);
                                        let btn = ui.add_enabled(
                                            !already_added,
                                            winui::FluentButton::new(i18n::t(
                                                i18n::Key::BtnAddToDevice,
                                                lang,
                                            )),
                                        );
                                        if btn.clicked() {
                                            if settings.rpc_device.is_empty() {
                                                settings.rpc_device = entry.clone();
                                            } else {
                                                settings.rpc_device =
                                                    format!("{},{}", settings.rpc_device, entry);
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    });
            }
        }
    });

    // ── 缓存 ──
    widgets::card(ui, i18n::t(i18n::Key::SectionCache, lang), accent, |ui| {
        // 缓存大小计算
        let cache_dir = rpc_cache_dir();
        let cache_size = if cache_dir.exists() {
            dir_size(&cache_dir)
        } else {
            0
        };
        let size_text = format_size(cache_size);

        // Toggle + 缓存大小在同一行
        ui.horizontal(|ui| {
            widgets::toggle(
                ui,
                &mut settings.rpc_cache,
                i18n::t(i18n::Key::CheckboxRpcCache, lang),
                accent,
            );
            ui.label(
                egui::RichText::new(&size_text)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        });

        ui.add_space(4.0);

        // 清除缓存按钮
        if ui
            .add(winui::FluentButton::new(i18n::t(
                i18n::Key::BtnClearRpcCache,
                lang,
            )))
            .clicked()
        {
            if cache_dir.exists() {
                match std::fs::remove_dir_all(&cache_dir) {
                    Ok(()) => log::info!("已清除 RPC 缓存目录: {}", cache_dir.display()),
                    Err(e) => log::error!("清除 RPC 缓存目录失败: {} - {}", cache_dir.display(), e),
                }
            }
        }
    });
    ui.add_space(2.0);
}
