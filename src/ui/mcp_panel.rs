use crate::config::settings::{parse_mcp_servers, AppSettings};
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;

/// MCP 管理面板：
/// 读取用户已有的 Cursor-compatible mcpServers 配置 → 可视化 → 启用/禁用 →
/// 启动 llama-server 时由 ServerManager 生成"当前启用"配置并拼接 --mcp-servers-config。
/// 本面板不安装/下载/部署 MCP，也不管理 MCP 进程生命周期（由 llama-server 官方机制处理）。
pub fn ui(ui: &mut egui::Ui, settings: &mut AppSettings, lang: &i18n::Language) {
    let accent = crate::theme::accent_color(&settings.accent_color);

    // 解析当前配置（失败时在设置卡片中提示，不阻塞页面）
    let parsed = parse_mcp_servers(&settings.mcp_config_json);

    // ── MCP 设置 ──
    widgets::card(
        ui,
        i18n::t(i18n::Key::SectionMcpSettings, lang),
        accent,
        |ui| {
            ui.horizontal(|ui| {
                widgets::toggle(
                    ui,
                    &mut settings.mcp_enabled,
                    i18n::t(i18n::Key::CheckboxMcpEnabled, lang),
                    accent,
                );
            });
            ui.horizontal(|ui| {
                if ui
                    .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnEditMcpConfig, lang)))
                    .clicked()
                {
                    settings.mcp_editor_text = settings.mcp_config_json.clone();
                    settings.mcp_editor_error.clear();
                    settings.mcp_editor_open = true;
                }
                // 提示小文字移到按钮右侧
                ui.small(i18n::t(i18n::Key::HintMcpTip, lang));
                // 状态摘要：已启用 x/y，或配置错误提示
                match &parsed {
                    Ok(servers) if !servers.is_empty() => {
                        let enabled = servers
                            .iter()
                            .filter(|s| {
                                settings
                                    .mcp_server_states
                                    .get(&s.name)
                                    .copied()
                                    .unwrap_or(false)
                            })
                            .count();
                        ui.small(format!("{}/{}", enabled, servers.len()));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // 配置为空时不显示错误（用户可能还没填写）
                        if !settings.mcp_config_json.trim().is_empty() {
                            ui.small(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    i18n::t(i18n::Key::ErrMcpConfigInvalid, lang),
                                    e
                                ))
                                .color(ui.visuals().error_fg_color),
                            );
                        }
                    }
                }
            });
        },
    );

    // ── MCP Servers 列表 ──
    if let Ok(servers) = &parsed {
        if !servers.is_empty() {
            widgets::card(
                ui,
                i18n::t(i18n::Key::SectionMcpServers, lang),
                accent,
                |ui| {
                    for s in servers {
                        ui.horizontal(|ui| {
                            // ★ 开关在左、名称在右（与项目 toggle 行样式一致）
                            let state = settings
                                .mcp_server_states
                                .entry(s.name.clone())
                                .or_insert(false);
                            widgets::toggle(ui, state, "", accent);
                            // 不用裸 .strong()：浅色模式下 strong_text_color=白色会隐形，显式指定主文本色
                            ui.label(
                                egui::RichText::new(&s.name)
                                    .color(ui.visuals().text_color())
                                    .strong(),
                            );
                        });
                        // 摘要行：command args… / timeout；无 command 的条目给出明确不支持提示
                        ui.indent(format!("mcp_summary_{}", s.name), |ui| {
                            if !s.is_object {
                                ui.small(
                                    egui::RichText::new(i18n::t(
                                        i18n::Key::HintMcpEntryInvalid,
                                        lang,
                                    ))
                                    .color(ui.visuals().error_fg_color),
                                );
                            } else if s.command.is_empty() {
                                ui.small(
                                    egui::RichText::new(i18n::t(
                                        i18n::Key::HintMcpUnsupported,
                                        lang,
                                    ))
                                    .color(ui.visuals().warn_fg_color),
                                );
                            } else {
                                let mut summary = s.command.clone();
                                if !s.args.is_empty() {
                                    summary.push(' ');
                                    summary.push_str(&s.args.join(" "));
                                }
                                ui.horizontal(|ui| {
                                    ui.small(
                                        egui::RichText::new(summary)
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                    ui.small(
                                        egui::RichText::new(format!("({} ms)", s.timeout_ms))
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                });
                            }
                        });
                        ui.add_space(4.0);
                    }
                },
            );
        }
    }

    // ── 配置编辑弹窗 ──
    editor_window(ui.ctx(), settings, lang);
}

/// MCP 配置编辑窗口：多行文本编辑 + JSON 校验 + 保存/取消
/// （open 状态经局部变量中转，避免 Window::open 与闭包同时借用 settings）
///
/// 布局（egui 标准弹窗规范）：
/// - 窗口尺寸约束在屏幕 90% 以内，内容超长时编辑区内部滚动，
///   保存/取消按钮固定在底部永远可达；
/// - 传入显式 Frame 使标题栏颜色与内容区一致（默认标题栏用
///   widgets.open.weak_bg_fill，深色模式下与 window_fill 有色差）。
fn editor_window(ctx: &egui::Context, settings: &mut AppSettings, lang: &i18n::Language) {
    if !settings.mcp_editor_open {
        return;
    }
    let mut open = true;
    let mut close_after_save = false;
    let screen = ctx.content_rect().size();
    let style = ctx.style_of(ctx.theme());
    egui::Window::new(i18n::t(i18n::Key::TitleMcpEditor, lang))
        .open(&mut open)
        .resizable(true)
        .default_width(520.0)
        .default_height(420.0)
        .min_width(360.0)
        .min_height(240.0)
        .max_width(screen.x * 0.9)
        .max_height(screen.y * 0.9)
        .frame(egui::Frame::window(&style))
        .show(ctx, |ui| {
            ui.small(i18n::t(i18n::Key::HintMcpEditorTip, lang));

            if !settings.mcp_editor_error.is_empty() {
                ui.add_space(4.0);
                ui.small(
                    egui::RichText::new(format!(
                        "{} {}",
                        i18n::t(i18n::Key::ErrMcpConfigInvalid, lang),
                        settings.mcp_editor_error
                    ))
                    .color(ui.visuals().error_fg_color),
                );
            }

            // 底部按钮面板：先于中央区渲染，固定占据底部（保存/取消永远可见）
            egui::Panel::bottom("mcp_editor_buttons")
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(winui::AccentButton::new(i18n::t(i18n::Key::BtnMcpSave, lang)))
                            .clicked()
                        {
                            match parse_mcp_servers(&settings.mcp_editor_text) {
                                Ok(servers) => {
                                    // 保存原始配置；为新出现的 server 默认启用，已有状态保持不变
                                    let mut states = settings.mcp_server_states.clone();
                                    for s in &servers {
                                        states
                                            .entry(s.name.clone())
                                            .or_insert(s.is_object && !s.command.is_empty());
                                    }
                                    // 清理已不存在的 server 状态
                                    states
                                        .retain(|name, _| servers.iter().any(|s| &s.name == name));
                                    settings.mcp_server_states = states;
                                    settings.mcp_config_json = settings.mcp_editor_text.clone();
                                    settings.mcp_editor_error.clear();
                                    close_after_save = true;
                                }
                                Err(e) => {
                                    settings.mcp_editor_error = e;
                                }
                            }
                        }
                        if ui
                            .add(winui::FluentButton::new(i18n::t(i18n::Key::BtnMcpCancel, lang)))
                            .clicked()
                        {
                            close_after_save = true;
                            settings.mcp_editor_error.clear();
                        }
                    });
                });

            // 中央编辑区：占满剩余空间，文本超长时在编辑区内滚动
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 4)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("mcp_editor_scroll")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut settings.mcp_editor_text)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(14)
                                    .font(egui::TextStyle::Monospace),
                            );
                        });
                });
        });
    settings.mcp_editor_open = open && !close_after_save;
}
