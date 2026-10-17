use crate::config::settings::AppSettings;
use crate::engine::rpc::RpcManager;
use crate::engine::server::{LogEntry, LogLevel, ServerManager};
use crate::i18n;
use crate::ui::widgets;
use fluent_egui::winui;

/// 日志源抽象：保证「服务器日志」与「远程调用日志」两个面板功能完全一致、永不漂移
trait LogSource {
    fn logs(&self) -> Vec<LogEntry>;
    fn clear_logs(&mut self);
}

impl LogSource for ServerManager {
    fn logs(&self) -> Vec<LogEntry> {
        ServerManager::logs(self)
    }
    fn clear_logs(&mut self) {
        ServerManager::clear_logs(self)
    }
}

impl LogSource for RpcManager {
    fn logs(&self) -> Vec<LogEntry> {
        RpcManager::logs(self)
    }
    fn clear_logs(&mut self) {
        RpcManager::clear_logs(self)
    }
}

fn render(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    title_key: i18n::Key,
    lang: &i18n::Language,
    source: &mut dyn LogSource,
    progress: f32,
    scroll_salt: &'static str,
) {
    let accent = crate::theme::accent_color(&settings.accent_color);
    widgets::card(ui, i18n::t(title_key, lang), accent, |ui| {
        let auto_scroll_before = settings.auto_scroll_logs;

        ui.horizontal(|ui| {
            if ui
                .add(winui::FluentButton::new(i18n::t(
                    i18n::Key::BtnClearLogs,
                    lang,
                )))
                .clicked()
            {
                source.clear_logs();
            }
            ui.label(i18n::t(i18n::Key::LabelMaxLogLines, lang));
            let mut max_lines = settings.max_log_lines as f64;
            ui.add(
                winui::NumberBox::new(&mut max_lines)
                    .range(-1.0..=10000.0)
                    .width(88.0)
                    .spin_button_placement(winui::NumberBoxSpinButtonPlacement::Compact),
            );
            settings.max_log_lines = max_lines as i32;
            ui.small(i18n::t(i18n::Key::HintLogSession, lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::toggle_trailing(
                    ui,
                    &mut settings.auto_scroll_logs,
                    i18n::t(i18n::Key::CheckboxAutoScroll, lang),
                    accent,
                );
            });
        });

        let should_scroll_to_bottom = !auto_scroll_before && settings.auto_scroll_logs;

        ui.add_space(4.0);

        if progress > 0.0 {
            ui.horizontal(|ui| {
                ui.small(i18n::t(i18n::Key::LabelPreFillProgress, lang));
                ui.add(
                    winui::FluentProgressBar::new(progress).show_percentage(true),
                );
            });
            ui.add_space(4.0);
        }

        if settings.max_log_lines != 0 {
            // 日志区撑满剩余高度（auto_shrink(false)），但留 18px 余量：
            // 否则 card 总高精确顶满外层 ScrollArea 的 inner 边界，
            // 任何微差都会触发外层垂直滚动条（同 presets 空态问题），
            // 且滚动条占宽后日志换行加高，锁存持续显示。
            let max_h = (ui.available_height() - 18.0).max(64.0);
            let mut viewport_rect = egui::Rect::NOTHING;
            let scroll_output = egui::ScrollArea::vertical()
                .auto_shrink(false)
                .max_height(max_h)
                .id_salt(scroll_salt)
                .stick_to_bottom(settings.auto_scroll_logs)
                .show(ui, |ui| {
                    if should_scroll_to_bottom {
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    }

                    let mut logs = source.logs();
                    if settings.max_log_lines > 0 && logs.len() > settings.max_log_lines as usize {
                        let start_index = logs.len() - settings.max_log_lines as usize;
                        logs.drain(..start_index);
                    }

                    // ★ 整块日志合并为单一 LayoutJob 文本（Label 选择器）：
                    // egui 的文本选择只在单个 widget 内有效，逐行 label 无法跨行选择；
                    // 合并后按级别着色保持不变，且拖选可跨任意行延伸。
                    let mut job = egui::text::LayoutJob::default();
                    let text_color = ui.visuals().text_color();
                    let palette = winui::palette(widgets::current_theme(ui));
                    for entry in &logs {
                        let (prefix, color) = match entry.level {
                            LogLevel::Info => ("", text_color),
                            LogLevel::Warn => ("⚠ ", palette.warning),
                            LogLevel::Error => ("✖ ", palette.danger),
                        };
                        let line = format!("{}{}\n", prefix, entry.text);
                        job.append(
                            &line,
                            0.0,
                            egui::TextFormat::simple(egui::FontId::default(), color),
                        );
                    }
                    ui.add(egui::Label::new(job).selectable(true));
                    viewport_rect = ui.clip_rect();
                });

            // ★ 拖选越界自动滚动（egui 无内置）：按住左键拖选时指针越过视口
            //   上/下边缘，则持续推进滚动偏移，使选区向该方向扩展。
            //   注：egui 在拖拽中本身仍响应滚轮（ScrollArea 滚轮不依赖拖拽状态），
            //   合并单 widget 后"按住选择 + 滚轮"即可延伸选区，无需额外处理。
            auto_scroll_on_drag(ui.ctx(), scroll_output.id, viewport_rect);
        }
    });
}

/// 按住左键拖选时，指针接近/越过日志视口上下边缘 → 按比例推进滚动偏移。
/// 直接改写 ScrollArea State 的公开 offset 字段（每帧调用形成连续滚动）。
/// 仅在用户关闭「自动滚动」时影响实际位置；开启 stick_to_bottom 时
/// egui 会在每帧末尾把偏移拉回底部（拖选本就不适合在 stick 模式下进行）。
fn auto_scroll_on_drag(ctx: &egui::Context, scroll_state_id: egui::Id, viewport: egui::Rect) {
    if viewport == egui::Rect::NOTHING {
        return;
    }
    let Some(pointer) = ctx.input(|i| i.pointer.interact_pos()) else {
        return;
    };
    let dragging = ctx.input(|i| i.pointer.primary_down() && i.pointer.is_decidedly_dragging());
    if !dragging {
        return;
    }

    const EDGE: f32 = 32.0; // 视口上下边缘的触发带高度
    const SPEED: f32 = 18.0; // 每帧最大推进像素
                             // 指针允许略微越过视口（最多 96px）仍视为在拖选本区域
    const OVERSHOOT: f32 = 96.0;

    let mut dy = 0.0;
    if pointer.y < viewport.top() + EDGE && pointer.y > viewport.top() - OVERSHOOT {
        // 越接近/越过上边缘，速度越快（0..1 线性）
        let t = ((viewport.top() + EDGE - pointer.y) / EDGE).clamp(0.0, 1.0);
        dy = -SPEED * t;
    } else if pointer.y > viewport.bottom() - EDGE && pointer.y < viewport.bottom() + OVERSHOOT {
        let t = ((pointer.y - (viewport.bottom() - EDGE)) / EDGE).clamp(0.0, 1.0);
        dy = SPEED * t;
    }
    if dy == 0.0 {
        return;
    }

    if let Some(mut state) = egui::scroll_area::State::load(ctx, scroll_state_id) {
        state.offset.y = (state.offset.y + dy).max(0.0);
        state.store(ctx, scroll_state_id);
    }
}

/// 服务器日志面板（原「运行日志」，已改名）
pub fn ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    server: &mut ServerManager,
    lang: &i18n::Language,
) {
    let progress = server.progress();
    render(
        ui,
        settings,
        i18n::Key::PanelLogTitle,
        lang,
        server,
        progress,
        "server_log_scroll_area",
    );
}

/// 远程调用日志面板（ggml-rpc-server 运行日志）；RPC 无 prefill 进度，传 0.0
pub fn rpc_ui(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    rpc: &mut RpcManager,
    lang: &i18n::Language,
) {
    render(
        ui,
        settings,
        i18n::Key::PanelRpcLogTitle,
        lang,
        rpc,
        0.0,
        "rpc_log_scroll_area",
    );
}
