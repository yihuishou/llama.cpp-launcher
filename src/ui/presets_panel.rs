use crate::config::settings::{AppSettings, Preset};
use crate::engine::rpc::RpcManager;
use crate::engine::server::ServerManager;
use crate::i18n;
use crate::ui::preset_share::{ParamsExport, PresetShareUi};
use crate::ui::widgets;
use fluent_egui::winui;

/// 预设面板对 app 层的动作请求（面板只发请求，执行在 app 层完成）
#[derive(Debug, PartialEq)]
pub enum PresetPanelRequest {
    /// 无动作
    None,
    /// 应用预设参数并启动 llama-server
    StartServer,
    /// 应用预设参数并启动本机 ggml-rpc-server
    StartRpc,
    /// 打开"配置"窗口（携带预设索引）
    OpenConfig(usize),
}

pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    lang: &i18n::Language,
    share: &mut PresetShareUi,
    config: &mut crate::ui::preset_share::PresetConfigUi,
    rpc_manager: &RpcManager,
    server_manager: &ServerManager,
    notice: &Option<(bool, String, f64)>,
) -> PresetPanelRequest {
    let accent = crate::theme::accent_color(&settings.accent_color);
    let mut start_server = false;
    let mut start_rpc = false;
    let mut config_request: Option<Preset> = None;
    // header 闭包中的延迟操作标志（避免与 body 闭包同时借用 &mut share）
    let mut header_share_open = false;

    widgets::card_with_header_notice(
        ui,
        i18n::t(i18n::Key::SectionPresets, lang),
        accent,
        notice.as_ref().map(|(ok, msg, _)| (*ok, msg.as_str())),
        |ui| {
            // 标题行最右：分享/引入预设入口
            // 低饱和度主题色按钮（项目规范，同 app.rs web_accent：主题色 + alpha 175）
            let share_fill =
                egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 175);
            if ui
                .add(widgets::rounded_button(
                    i18n::t(i18n::Key::BtnSharePresets, lang),
                    Some(share_fill),
                ))
                .clicked()
            {
                header_share_open = true;
            }
            // 测试功能提示：黄色感叹号（悬停显示说明），位于分享按钮左边
            // （header 为 right_to_left 布局，后绘制者靠左）
            ui.label(
                egui::RichText::new("⚠")
                    .color(egui::Color32::from_rgb(230, 180, 30))
                    .strong(),
            )
            .on_hover_text(i18n::t(i18n::Key::ShareBetaTip, lang));
        },
        |ui| {
            // 保存预设区域
            ui.horizontal(|ui| {
                ui.label(i18n::t(i18n::Key::LabelPresetName, lang));
                ui.add(winui::FluentTextEdit::new(&mut settings.new_preset_name));
                if ui
                    .add(winui::AccentButton::new(i18n::t(
                        i18n::Key::BtnSavePreset,
                        lang,
                    )))
                    .clicked()
                {
                    let trimmed = settings.new_preset_name.trim().to_string();
                    if !trimmed.is_empty() {
                        let exists = settings.presets.iter().any(|p| p.name == trimmed);
                        if !exists {
                            let preset = Preset::from_settings(settings, trimmed);
                            settings.presets.push(preset);
                        } else {
                            if let Some(idx) =
                                settings.presets.iter().position(|p| p.name == trimmed)
                            {
                                let new_preset = Preset::from_settings(settings, trimmed);
                                settings.presets[idx] = new_preset;
                            }
                        }
                        settings.new_preset_name.clear();
                    }
                }

                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnExportParams,
                        lang,
                    )))
                    .clicked()
                {
                    let params = ParamsExport::from_settings(settings);
                    let json = match serde_json::to_string_pretty(&params) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[导出参数预设] 序列化失败: {}", e);
                            return;
                        }
                    };
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("llama_cpp_launcher_parameter_export.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    {
                        if let Err(e) = std::fs::write(&path, &json) {
                            eprintln!("[导出参数预设] 写入失败: {}", e);
                        }
                    }
                }

                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnImportParams,
                        lang,
                    )))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    {
                        let content = match std::fs::read_to_string(&path) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("[导入参数预设] 读取失败: {}", e);
                                return;
                            }
                        };
                        match serde_json::from_str::<ParamsExport>(&content) {
                            Ok(params) => {
                                params.apply_to(settings);
                            }
                            Err(e) => {
                                eprintln!("[导入参数预设] 解析失败: {}", e);
                            }
                        }
                    }
                }
            });

            ui.separator();

            if settings.presets.is_empty() {
                // 固定 200px 高的居中区域：避免 centered_and_justified 撑满整个
                // ScrollArea 内容区（content == inner 临界触发外层滚动条），
                // 同时保留提示文字垂直居中的视觉。
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 545.0),
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        ui.label(i18n::t(i18n::Key::HintNoPresets, lang));
                    },
                );
            } else {
                // 与日志面板相同的余量处理：列表超高时 ScrollArea 高度最多到
                // 剩余空间-18px，避免 card 总高精确顶满外层 ScrollArea 的 inner
                // 边界（临界触发外层滚动条，与内层滚动条叠加）；列表矮时
                // auto_shrink 默认收缩，布局不受影响。
                let max_h = (ui.available_height() - 18.0).max(64.0);
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        let mut load_index: Option<usize> = None;
                        let mut delete_index: Option<usize> = None;
                        let mut auto_start_preset: Option<String> = None;

                        for (i, preset) in settings.presets.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(false, format!("📦 {}", preset.name))
                                    .clicked()
                                {
                                    load_index = Some(i);
                                }

                                if ui
                                    .add(winui::FluentButton::new(i18n::t(
                                        i18n::Key::BtnApplyPreset,
                                        lang,
                                    )))
                                    .clicked()
                                {
                                    load_index = Some(i);
                                }

                                let mut is_auto = settings
                                    .auto_start_preset_name
                                    .as_ref()
                                    .is_some_and(|name| *name == preset.name);
                                if ui
                                    .add(winui::FluentCheckBox::new(
                                        &mut is_auto,
                                        i18n::t(i18n::Key::CheckboxAutoStartPreset, lang),
                                    ))
                                    .changed()
                                {
                                    if is_auto {
                                        auto_start_preset = Some(preset.name.clone());
                                    } else if settings.auto_start_preset_name.as_ref()
                                        == Some(&preset.name)
                                    {
                                        settings.auto_start_preset_name = None;
                                    }
                                }

                                if ui
                                    .add(winui::FluentButton::new(i18n::t(
                                        i18n::Key::BtnRenamePreset,
                                        lang,
                                    )))
                                    .clicked()
                                {
                                    settings.rename_preset_index = Some(i);
                                    settings.rename_preset_new_name = preset.name.clone();
                                }

                                if ui
                                    .add(winui::FluentButton::new(i18n::t(
                                        i18n::Key::BtnDeletePreset,
                                        lang,
                                    )))
                                    .clicked()
                                {
                                    delete_index = Some(i);
                                }

                                // ── 外部引入的预设：分隔线 + 配置按钮 + 引入标记 ──
                                if preset.imported {
                                    ui.separator();
                                    // ⚙ 配置：同帧直接 open_for（快照设置+应用预设参数），
                                    // 窗口由本函数尾部的 config_window 同帧渲染，无跨帧依赖
                                    if ui
                                        .add(winui::FluentButton::new(i18n::t(
                                            i18n::Key::BtnConfigPreset,
                                            lang,
                                        )))
                                        .on_hover_text(i18n::t(i18n::Key::ConfigBtnTip, lang))
                                        .clicked()
                                    {
                                        // 取 owned 克隆（脱离 settings 借用），
                                        // open_for 在 horizontal 闭包外执行
                                        config_request = Some(preset.clone());
                                    }
                                    // 引入标记：主题色描边圆角矩形，悬停提示
                                    let tag = egui::Button::new(
                                        egui::RichText::new(i18n::t(i18n::Key::ImportedTag, lang))
                                            .color(accent)
                                            .small(),
                                    )
                                    .stroke(egui::Stroke::new(1.0_f32, accent))
                                    .corner_radius(4.0)
                                    .fill(egui::Color32::TRANSPARENT);
                                    if ui
                                        .add(tag)
                                        .on_hover_text(i18n::t(i18n::Key::ImportedTip, lang))
                                        .clicked()
                                    {
                                        config_request = Some(preset.clone());
                                    }
                                }

                                // ── 启动类按钮（与前面按钮竖分隔线隔开；统一 small 尺寸） ──
                                ui.separator();
                                if ui
                                    .add(winui::FluentButton::new(i18n::t(
                                        i18n::Key::BtnStartServer,
                                        lang,
                                    )))
                                    .on_hover_text(i18n::t(i18n::Key::StartServerTip, lang))
                                    .clicked()
                                {
                                    load_index = Some(i);
                                    start_server = true;
                                }
                                let rpc_ok =
                                    rpc_manager.check_rpc_server(&settings.rpc_server_path);
                                let rpc_btn = ui.add_enabled(
                                    rpc_ok,
                                    winui::FluentButton::new(i18n::t(i18n::Key::BtnStartRpc, lang)),
                                );
                                if rpc_btn
                                    .on_hover_text(i18n::t(i18n::Key::StartRpcTip, lang))
                                    .clicked()
                                {
                                    load_index = Some(i);
                                    start_rpc = true;
                                }

                                // ── Linux 服务文件按钮 ──
                                ui.separator();
                                if ui
                                    .add(winui::FluentButton::new(i18n::t(
                                        i18n::Key::BtnLinuxServiceFile,
                                        lang,
                                    )))
                                    .on_hover_text(i18n::t(i18n::Key::LinuxServiceFileHint, lang))
                                    .clicked()
                                {
                                    settings.show_linux_service_file = true;
                                }
                            });
                            ui.separator();
                        }

                        if let Some(name) = auto_start_preset {
                            settings.auto_start_preset_name = Some(name);
                        }

                        if let Some(idx) = load_index {
                            if idx < settings.presets.len() {
                                let preset = settings.presets[idx].clone();
                                preset.apply_to(settings);
                            }
                        }

                        // 开机自启场景：应用预设后自动启动 server（保留原行为）
                        if load_index
                            .map(|idx| {
                                idx < settings.presets.len()
                                    && settings.auto_start_preset_name.as_ref()
                                        == Some(&settings.presets[idx].name)
                            })
                            .unwrap_or(false)
                        {
                            start_server = true;
                        }

                        if let Some(idx) = delete_index {
                            if idx < settings.presets.len() {
                                settings.presets.remove(idx);
                                if let Some(rename_idx) = settings.rename_preset_index {
                                    if rename_idx >= settings.presets.len() {
                                        settings.rename_preset_index = None;
                                        settings.rename_preset_new_name.clear();
                                    }
                                }
                            }
                        }

                        if let Some(idx) = settings.rename_preset_index {
                            if idx < settings.presets.len() {
                                egui::Window::new(i18n::t(i18n::Key::BtnRenamePreset, lang))
                                    .collapsible(false)
                                    .resizable(false)
                                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                                    .fixed_size([150.0, 0.0])
                                    .show(ui.ctx(), |ui| {
                                        ui.label(i18n::t(i18n::Key::LabelPresetName, lang));
                                        ui.add(winui::FluentTextEdit::new(
                                            &mut settings.rename_preset_new_name,
                                        ));
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(winui::AccentButton::new(i18n::t(
                                                    i18n::Key::BtnConfirm,
                                                    lang,
                                                )))
                                                .clicked()
                                            {
                                                let trimmed = settings
                                                    .rename_preset_new_name
                                                    .trim()
                                                    .to_string();
                                                if !trimmed.is_empty() {
                                                    settings.presets[idx].name = trimmed;
                                                }
                                                settings.rename_preset_index = None;
                                                settings.rename_preset_new_name.clear();
                                            }
                                            if ui
                                                .add(winui::FluentButton::new(i18n::t(
                                                    i18n::Key::BtnCancel,
                                                    lang,
                                                )))
                                                .clicked()
                                            {
                                                settings.rename_preset_index = None;
                                                settings.rename_preset_new_name.clear();
                                            }
                                        });
                                    });
                            }
                        }
                    });
            }
        },
    );

    // header 闭包中延迟的分享按钮操作
    if header_share_open {
        share.open();
    }

    // ⚙/引入标记请求的 open_for：在 horizontal 闭包外执行（此处无 preset 借用活跃）
    if let Some(p) = config_request.take() {
        config.open_for(&p, settings.clone());
    }

    // 分享/引入弹窗（独立窗口，规范同 MCP 编辑弹窗）
    crate::ui::preset_share::share_window(ui.ctx(), settings, share, lang);
    // 引入预设配置窗口（草稿隔离，规范同上）
    crate::ui::preset_share::config_window(ui.ctx(), settings, config, share, lang);
    // 依赖声明阅读窗口（配置窗口/分享窗口内按钮打开；egui 窗口均为顶层区域，可叠加）
    crate::ui::preset_share::decl_window(ui.ctx(), share, lang);

    // ── Linux 服务文件窗口 ──
    if settings.show_linux_service_file {
        let mut open = settings.show_linux_service_file;
        egui::Window::new(i18n::t(i18n::Key::LinuxServiceFileWindowTitle, lang))
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .default_height(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(i18n::t(i18n::Key::LinuxServiceFileHint, lang));
                ui.separator();

                // 获取服务器启动命令，替换模板中的 ExecStart 行，并自动填充用户信息
                let template = i18n::t(i18n::Key::LinuxServiceFileContent, lang);
                let content = {
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

                    // 根据当前设置构建启动命令
                    let cmd = server_manager.build_launch_command(settings);
                    // 将启动命令按行分割，替换 ExecStart 行
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
                };

                let mut content = content;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut content)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });

                ui.separator();
                ui.horizontal(|ui| {
                    // 复制到剪贴板
                    let copy_label = if settings.linux_service_file_copied {
                        "✓"
                    } else {
                        &i18n::t(i18n::Key::BtnCopyServiceFile, lang)
                    };
                    if ui.add(winui::FluentButton::new(copy_label)).clicked() {
                        ui.ctx().copy_text(content.to_string());
                        settings.linux_service_file_copied = true;
                    }
                    // 重置复制状态（下次点击前）
                    if settings.linux_service_file_copied {
                        ui.ctx().request_repaint();
                    }

                    // 保存为文件
                    if ui
                        .add(winui::FluentButton::new(i18n::t(
                            i18n::Key::BtnSaveServiceFile,
                            lang,
                        )))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title(i18n::t(i18n::Key::LinuxServiceFileWindowTitle, lang))
                            .add_filter("Service File", &["service"])
                            .save_file()
                        {
                            let _ = std::fs::write(&path, &content);
                        }
                    }
                });
            });
        settings.show_linux_service_file = open;
        // 仅在窗口关闭时重置复制状态
        if !open {
            settings.linux_service_file_copied = false;
        }
    }

    if start_server {
        PresetPanelRequest::StartServer
    } else if start_rpc {
        PresetPanelRequest::StartRpc
    } else {
        PresetPanelRequest::None
    }
}
