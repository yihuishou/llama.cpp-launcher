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
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnEditMcpConfig,
                        lang,
                    )))
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
///
/// 基于 `widgets::dialog`（WinUI3 模态对话框）实现：
/// - footer 闭包只记录「点了保存还是取消」，真正的 JSON 解析与状态写回在
///   dialog 返回后进行——content 闭包持有 `settings.mcp_editor_text` 的可变借用，
///   footer 若直接解析会与之冲突，故用局部枚举中转；
/// - 保存失败（JSON 非法）时对话框保持打开，错误显示在内容区顶部。
fn editor_window(ctx: &egui::Context, settings: &mut AppSettings, lang: &i18n::Language) {
    if !settings.mcp_editor_open {
        return;
    }

    // footer 与 content 是同一调用里的两个闭包：用局部枚举中转动作，
    // 让两闭包各自捕获不同字段（Rust 2021 不相交捕获），避免同时借用 settings。
    #[derive(PartialEq)]
    enum McpEditorAction {
        None,
        Save,
        Cancel,
    }
    let mut action = McpEditorAction::None;
    let mut save_failed = false;
    let should_close = widgets::dialog(
        ctx,
        "mcp_editor_dialog",
        i18n::t(i18n::Key::TitleMcpEditor, lang),
        560.0,
        ctx.content_rect().height() * 0.8,
        |ui, avail_h| {
            // 记录绘制提示/错误行之前的光标 y，用于推算编辑器行数
            let start_y = ui.cursor().top();
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
            // 可用高度扣除已占用的提示/错误行高度后推算行数，让编辑器撑满内容区
            let used_h = ui.cursor().top() - start_y;
            let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
            let rows = (((avail_h - used_h) / row_h).floor() as usize).max(6);
            ui.add(
                egui::TextEdit::multiline(&mut settings.mcp_editor_text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .font(egui::TextStyle::Monospace),
            );
        },
        |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(winui::AccentButton::new(i18n::t(
                        i18n::Key::BtnMcpSave,
                        lang,
                    )))
                    .clicked()
                {
                    action = McpEditorAction::Save;
                }
                if ui
                    .add(winui::FluentButton::new(i18n::t(
                        i18n::Key::BtnMcpCancel,
                        lang,
                    )))
                    .clicked()
                {
                    action = McpEditorAction::Cancel;
                }
            });
            action != McpEditorAction::None
        },
    );

    // dialog 返回后 content 闭包已结束，此处可安全读写 settings 各字段
    if should_close {
        match action {
            McpEditorAction::Save => match parse_mcp_servers(&settings.mcp_editor_text) {
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
                }
                Err(e) => {
                    settings.mcp_editor_error = e;
                    save_failed = true;
                }
            },
            McpEditorAction::Cancel => {
                settings.mcp_editor_error.clear();
            }
            McpEditorAction::None => {}
        }
    }
    // 保存失败时对话框保持打开（错误留在内容区顶部供用户修正），其余情形一律收起
    settings.mcp_editor_open = !should_close || save_failed;
}
