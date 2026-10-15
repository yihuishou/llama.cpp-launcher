//! 通用 UI 组件 —— 移植自 HTML 原型的设计语义
//!
//! 卡片、开关已改用 fluent-egui 的 WinUI 3 实现，配色与圆角由库接管，
//! 圆角统一在 `crate::theme::apply_theme` 里覆写。图标取自 iconflow 的 Fluent UI
//! 字体字形（见 `nav_icon_paint`），不再手写矢量路径。以下组件库中无等价物，故自绘：
//!   - `nav_row`：选中态 accent-tint 背景 + accent 色文字，
//!     对齐 HTML `.nav-item.active{color:var(--accent)}`
//!   - `color_swatch` / `rounded_button`：`egui::Button` 配自定义 `.fill()`，
//!     库的 FluentButton 只走主题色、AccentButton 亦不支持自定义填充
//!   - `segmented` / `segmented_width`：库把选中态固定为 `card_bg`（浅主题下近白，
//!     与轨道 `control_bg` 几乎同色而不可见）且不暴露任何覆盖入口，故自绘；
//!     浅主题选中态改为 accent 浅色提亮，深色主题沿用库原样。

use egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use fluent_egui::winui::{self, FluentTheme};

/// 库控件收 FluentTheme 枚举而非 egui::Theme，故从 dark_mode 推导。
pub(crate) fn current_theme(ui: &Ui) -> FluentTheme {
    if ui.visuals().dark_mode {
        FluentTheme::Dark
    } else {
        FluentTheme::Light
    }
}

// ──── 自绘组件的尺寸常量 ────

/// rounded_button 的圆角 = 6px（比 HTML --radius-ctl:8px 更方正，用户明确要求）。
/// 其余控件的圆角由 fluent-egui 提供，并在 `crate::theme::apply_theme` 里覆写。
pub const R_CTL: f32 = 6.0;
/// nav_row 的圆角 = 6px
pub const R_NAV: f32 = 6.0;

// ──── 侧边栏图标 ────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavIcon {
    Server,
    Rpc,
    Model,
    Params,
    Mcp,
    Log,
    RpcLog,
    Commands,
    Presets,
    Settings,
}

// ──── 卡片容器 ────

/// 卡片：标题行（标题居左、可选附加组件右对齐）+ 分隔线 + 内容。
/// 边框、圆角与内边距由 `winui::card_frame` 提供（WinUI 3 标准的 4px 圆角）。
pub fn card<R>(
    ui: &mut Ui,
    title: &str,
    accent: Color32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    card_with_header(ui, title, accent, |_| {}, add_contents)
}

/// 卡片（标题行右侧可附加组件，如操作按钮）。
/// 布局与 `card` 完全一致：标题居左、附加组件右对齐到标题行最右。
pub fn card_with_header<R>(
    ui: &mut Ui,
    title: &str,
    _accent: Color32,
    header: impl FnOnce(&mut Ui),
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    card_with_header_notice(ui, title, _accent, None, header, add_contents)
}

/// 卡片变体：标题右边紧跟一条状态通知（如预设启动成功/失败）。
/// `notice` = Some((是否成功, 消息))；成功绿色 ✓、失败用主题错误色 ✕。
pub fn card_with_header_notice<R>(
    ui: &mut Ui,
    title: &str,
    _accent: Color32,
    notice: Option<(bool, &str)>,
    header: impl FnOnce(&mut Ui),
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    winui::card_frame(current_theme(ui))
        .outer_margin(egui::Margin::symmetric(0, 0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.horizontal(|ui| {
                ui.label(title);
                // 标题右侧紧跟的状态通知（显式颜色，随成败切换）
                if let Some((ok, msg)) = notice {
                    let p = winui::palette(current_theme(ui));
                    let color = if ok { p.success } else { p.danger };
                    ui.label(
                        egui::RichText::new(format!("{} {}", if ok { "✓" } else { "✕" }, msg))
                            .color(color),
                    );
                }
                // 附加组件右对齐到标题行最右
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), header);
            });
            ui.separator();
            ui.add_space(4.0);
            let result = add_contents(ui);
            ui.add_space(4.0);
            result
        })
        .inner
}

// ──── WinUI 3 模态对话框 ────

/// `dialog` / `dialog_with_side_panel` 的共享骨架（抽取自原 `dialog`，两个公开函数共用，避免各自漂移）：
/// Modal（暗色遮罩）+ 卡片框（card_bg 填充、1px card_border 描边、6px 圆角、弹窗阴影）
/// + 56px 标题栏（右对齐 32×32 关闭按钮）+ 标题下方 1px 分隔线
/// + 主体区（高度 = 总高 - 标题栏 - 60 底部按钮区预留）
/// + footer 上方 1px 分隔线 + 通栏 footer（Margin::symmetric(24, 14)）。
///
/// - `body` 接收标题分隔线之后的 ui、主体区高度与对话框 Id（供内部 ScrollArea 做 id_salt）；
///   内容布局由 body 自行完成：无侧栏版是 24px 内边距的滚动区，带侧栏版是左侧定宽栏
///   + 1px 竖直分隔线 + 右侧滚动区。
/// - `footer` 是底部按钮区；返回 true 表示该对话框应关闭。
///
/// 返回 true 表示应关闭（点了 × / 按 Esc / 点了遮罩 / footer 返回 true）。
///
/// 基于 `egui::Modal` 构建：居中、不可拖动、不可调整大小（WinUI3 ContentDialog
/// 语义），几何与配色对齐 fluent-egui 的 `ContentDialog`，仅圆角统一为 6px
/// （`crate::theme::apply_theme` 对全局圆角的覆写）。
fn dialog_skeleton<B, F>(
    ctx: &egui::Context,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    width: f32,
    max_height: f32,
    body: B,
    footer: F,
) -> bool
where
    B: FnOnce(&mut egui::Ui, f32, egui::Id),
    F: FnOnce(&mut egui::Ui) -> bool,
{
    let id = egui::Id::new(id_salt);
    // 库控件收 FluentTheme 枚举而非 egui::Theme，故从 dark_mode 推导
    let theme = if ctx.theme() == egui::Theme::Dark {
        FluentTheme::Dark
    } else {
        FluentTheme::Light
    };
    let p = winui::palette(theme);

    // 宽度夹到屏幕内（与库 ContentDialog 相同，下限 160）
    let max_w = (ctx.content_rect().width() - 32.0).max(160.0);
    let width = width.clamp(160.0_f32.min(max_w), max_w);
    // 整体高度同样夹到屏幕内；标题栏为动态高度（最小 18+32=50），按 标题栏 + 60 底部按钮区至少 114 预留
    let max_avail_h = (ctx.content_rect().height() - 32.0).max(114.0);
    let max_h = max_height.clamp(114.0_f32.min(max_avail_h), max_avail_h);

    let frame = egui::Frame::new()
        .fill(p.card_bg)
        .stroke(egui::Stroke::new(1.0, p.card_border))
        .corner_radius(6)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 32,
            spread: 0,
            color: p.shadow,
        });

    let mut close_clicked = false;
    let modal = egui::Modal::new(id)
        .backdrop_color(Color32::from_black_alpha(if theme.is_dark() { 105 } else { 58 }))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            // 标题栏（WinUI3 官方度量）：上边距 18、左右边距 24、下边距 0（紧贴分隔线）
            let title_font = FontId::proportional(20.0);
            let title_row_h = ui.fonts_mut(|f| f.row_height(&title_font));
            // 行高至少容纳 32x32 关闭按钮；上下各 12px 内边距（合计 24 → 标题栏高 56，与官方 MaxHeight 一致），标题与按钮因此垂直居中
            let title_row_h = title_row_h.max(32.0);
            let header_h = title_row_h + 24.0;
            let (header_rect, _) = ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
            let title_center_y = header_rect.top() + header_h / 2.0;
            ui.painter().text(
                Pos2::new(header_rect.left() + 24.0, title_center_y),
                Align2::LEFT_CENTER,
                title,
                title_font,
                p.text_primary,
            );
            let close_rect = Rect::from_center_size(
                Pos2::new(header_rect.right() - 24.0 - 16.0, title_center_y),
                Vec2::splat(32.0),
            );
            let close_resp = ui.interact(close_rect, id.with("close"), Sense::click());
            if close_resp.clicked() {
                close_clicked = true;
            }
            ui.painter().rect_filled(
                close_rect,
                4,
                if close_resp.hovered() {
                    p.subtle_hover
                } else {
                    Color32::TRANSPARENT
                },
            );
            ui.painter().text(
                close_rect.center(),
                Align2::CENTER_CENTER,
                winui::FluentIcon::Close.glyph(),
                winui::icon_font_id(14.0),
                p.text_primary,
            );
            // 标题栏下方 1px 分隔线
            ui.painter().line_segment(
                [
                    Pos2::new(ui.min_rect().left(), ui.cursor().top()),
                    Pos2::new(ui.min_rect().right(), ui.cursor().top()),
                ],
                egui::Stroke::new(1.0, p.card_border),
            );
            // 主体区：高度 = 总高 - 标题栏(动态) - 60(底部按钮区预留)
            let body_h = (max_h - header_h - 60.0).max(60.0);
            body(ui, body_h, id);
            // 底部按钮区：分隔线 + Margin::symmetric(24, 14) 内边距（WinUI3 官方左右内边距 24）
            ui.painter().line_segment(
                [
                    Pos2::new(ui.min_rect().left(), ui.cursor().top()),
                    Pos2::new(ui.min_rect().right(), ui.cursor().top()),
                ],
                egui::Stroke::new(1.0, p.card_border),
            );
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(24, 14))
                .show(ui, |ui| {
                    // 减去左右各 24px 内边距：必须与上方 Frame 的 margin 同步，否则内容区挤不下会溢出撑宽弹窗
                    ui.set_min_width(width - 48.0);
                    footer(ui)
                })
                .inner
        });

    // should_close() 已覆盖遮罩点击与 Esc（仅顶层且无弹层时消费一次 Esc）
    close_clicked || modal.inner || modal.should_close()
}

/// 渲染一个 WinUI3 风格模态对话框（居中 + 遮罩阻断底层输入）。
/// - `title`：已 i18n 化的标题文本（调用方负责 t(Key, lang)）
/// - `width`：固定宽度（会被夹到屏幕内）
/// - `max_height`：整个对话框的最大高度（会被夹到屏幕内）
/// - `content`：中央内容区，超出高度时内部纵向滚动；第二参数为内容区可用高度（已扣除内边距），调用方可据此将控件撑满
/// - `footer`：底部按钮区；返回 true 表示该对话框应关闭
///
/// 返回 true 表示应关闭（点了 × / 按 Esc / 点了遮罩 / footer 返回 true）
///
/// 基于 `egui::Modal` 构建：居中、不可拖动、不可调整大小（WinUI3 ContentDialog
/// 语义），几何与配色对齐 fluent-egui 的 `ContentDialog`，仅圆角统一为 6px
/// （`crate::theme::apply_theme` 对全局圆角的覆写）。
pub fn dialog<C, F>(
    ctx: &egui::Context,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    width: f32,
    max_height: f32,
    content: C,
    footer: F,
) -> bool
where
    C: FnOnce(&mut egui::Ui, f32),
    F: FnOnce(&mut egui::Ui) -> bool,
{
    dialog_skeleton(ctx, id_salt, title, width, max_height, |ui, body_h, id| {
        // 内容区：WinUI3 官方左右内边距 24 + 上下内边距 12；超出 content_view_h 时内部纵向滚动
        // 视口高度 = 总高 - 标题栏(动态) - 60(底部按钮区预留) - 24(内容区垂直内边距)
        let content_view_h = (body_h - 24.0).max(80.0);
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(24, 12))
            .show(ui, |ui| {
                // 浮动滚动条不占布局宽度（默认滚动条会吃掉右侧 10px），保证左右内边距严格对称
                ui.style_mut().spacing.scroll = egui::style::ScrollStyle::floating();
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false])
                    .max_height(content_view_h)
                    .show(ui, |ui| {
                        // 短内容也占满整个视口
                        ui.set_min_height(content_view_h);
                        content(ui, content_view_h);
                    });
            });
    }, footer)
}

/// `dialog` 的带可选左侧面板变体。
///
/// 布局：56px 标题栏 + 分隔线，然后左侧定宽（`side_width`）侧栏（占满主体区高度，
/// 需要时内部纵向滚动）+ 1px 竖直分隔线（card_border）+ 右侧内容区（24px 水平内边距，
/// 内部纵向滚动），最底部是通栏按钮区（同时位于侧栏与内容区之下）。
/// - `side_width`：侧栏固定宽度
/// - `side`：侧栏内容；接收侧栏 ui（已含内边距，占满主体区高度，超出时内部纵向滚动）
/// - 其余参数与行为同 `dialog`。
#[allow(clippy::too_many_arguments)]
pub fn dialog_with_side_panel<C, S, F>(
    ctx: &egui::Context,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    width: f32,
    side_width: f32,
    max_height: f32,
    content: C,
    side: S,
    footer: F,
) -> bool
where
    C: FnOnce(&mut egui::Ui, f32),
    S: FnOnce(&mut egui::Ui),
    F: FnOnce(&mut egui::Ui) -> bool,
{
    dialog_skeleton(ctx, id_salt, title, width, max_height, |ui, body_h, id| {
        // 内容区视口高度（与无侧栏版同式：主体高 - 上下各 12 内边距）
        let content_view_h = (body_h - 24.0).max(80.0);
        // 侧栏内部高度（主体高 - 上下各 8 内边距）
        let side_inner_h = (body_h - 16.0).max(40.0);
        // 库控件收 FluentTheme 枚举而非 egui::Theme，故从 dark_mode 推导（取分隔线颜色）
        let theme = if ctx.theme() == egui::Theme::Dark {
            FluentTheme::Dark
        } else {
            FluentTheme::Light
        };
        let p = winui::palette(theme);
        let body_origin = ui.min_rect().min;
        ui.horizontal(|ui| {
            // 侧栏与内容区之间不留间距：分隔线恰好落在两者边界上
            ui.spacing_mut().item_spacing.x = 0.0;
            // 左侧栏：固定宽 side_width、占满主体区高度，内部纵向滚动（与内容区同等处理）
            ui.allocate_ui_with_layout(
                Vec2::new(side_width, body_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |side_ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(side_ui, |side_ui| {
                            side_ui
                                .style_mut()
                                .spacing
                                .scroll = egui::style::ScrollStyle::floating();
                            egui::ScrollArea::vertical()
                                .id_salt(id.with("side"))
                                .auto_shrink([false, false])
                                .max_height(side_inner_h)
                                .show(side_ui, |side_ui| {
                                    side_ui.set_min_height(side_inner_h);
                                    side(side_ui);
                                });
                        });
                },
            );
            // 侧栏与内容区之间的 1px 竖直分隔线（贯穿主体区全高）
            let divider_x = body_origin.x + side_width;
            ui.painter().line_segment(
                [
                    Pos2::new(divider_x, body_origin.y),
                    Pos2::new(divider_x, body_origin.y + body_h),
                ],
                egui::Stroke::new(1.0, p.card_border),
            );
            // 右侧内容区：24px 水平内边距 + 内部纵向滚动（与无侧栏版一致）
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::floating();
                    egui::ScrollArea::vertical()
                        .id_salt(id)
                        .auto_shrink([false, false])
                        .max_height(content_view_h)
                        .show(ui, |ui| {
                            ui.set_min_height(content_view_h);
                            content(ui, content_view_h);
                        });
                });
        });
    }, footer)
}

// ──── WinUI 3 开关 ────

/// 标签在左、开关靠右（供 KV 缓存配置等整行选项使用）。
pub fn toggle_trailing(ui: &mut Ui, on: &mut bool, label: &str, _accent: Color32) -> bool {
    ui.add(winui::ToggleSwitch::new(on).label(label)).changed()
}

/// 开关在左、标签在右。
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str, _accent: Color32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = ui.add(winui::ToggleSwitch::new(on)).changed();
        ui.label(label);
    });
    changed
}

// ──── 分段控件 ────

/// 分段控件，宽度按选项文本自适应（等价于 `segmented_width(.., 0.0)`）。
/// 修复库内默认 280px 固定宽度裁切长文案的问题。
pub fn segmented(ui: &mut Ui, options: &[&str], selected: &mut usize, _accent: Color32) -> bool {
    segmented_width(ui, options, selected, _accent, 0.0)
}

/// 分段控件（可指定整体宽度）。自绘实现，几何/交互/动画/焦点行为对齐
/// fluent-egui 的 `SegmentedControl`，仅浅主题选中态配色不同。
///
/// `width > 0.0` 为固定宽度；`width <= 0.0` 时按选项文本自适应，
/// 并以 280px（库内默认宽度）为下限——短文案保持统一 280px，
/// 仅长文案（如 `deepseek-legacy`）才撑宽，避免被裁切。
pub fn segmented_width(
    ui: &mut Ui,
    options: &[&str],
    selected: &mut usize,
    accent: Color32,
    width: f32,
) -> bool {
    let before = *selected;
    let p = winui::palette(current_theme(ui));
    let count = options.len().max(1);
    let normalized = if options.is_empty() {
        0
    } else {
        (*selected).min(count - 1)
    };
    let normalized_changed = !options.is_empty() && *selected != normalized;
    *selected = normalized;

    let w = if width > 0.0 {
        width
    } else {
        fit_segmented_width(ui, options).max(280.0)
    };
    let avail = ui.available_width().max(0.0);
    let w = if avail > 0.0 { w.min(avail) } else { w };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click());
    if normalized_changed {
        response.mark_changed();
    }
    if ui.is_enabled() && response.has_focus() {
        if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) && *selected + 1 < count {
            *selected += 1;
            response.mark_changed();
        } else if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) && *selected > 0 {
            *selected -= 1;
            response.mark_changed();
        }
    }

    ui.painter().rect(
        rect,
        6,
        p.control_bg,
        Stroke::new(1.0, p.control_border),
        StrokeKind::Inside,
    );
    if ui.is_enabled() && response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            egui::CornerRadius::same(6),
            Stroke::new(2.0, accent),
            StrokeKind::Outside,
        );
    }

    let segment_width = rect.width() / count as f32;

    // 浅主题：库的选中态 `card_bg`（近白）压 `control_bg`（近白）不可见，且库不暴露
    // 覆盖入口，故改绘 accent 浅色（t=0.80 填充、t=0.60 描边）；深色主题沿用库原样。
    let (pill_fill, pill_border) = if ui.visuals().dark_mode {
        (p.card_bg, p.card_border)
    } else {
        (
            crate::theme::lighten_accent(accent, 0.80),
            crate::theme::lighten_accent(accent, 0.60),
        )
    };
    if !options.is_empty() {
        let animated_index = ui.ctx().animate_value_with_time(
            response.id.with("selection"),
            *selected as f32,
            ui.style().animation_time,
        );
        let selected_rect = Rect::from_min_size(
            Pos2::new(
                rect.left() + animated_index * segment_width + 3.0,
                rect.top() + 3.0,
            ),
            Vec2::new(
                (segment_width - 6.0).max(0.0),
                (rect.height() - 6.0).max(0.0),
            ),
        );
        ui.painter().rect(
            selected_rect,
            4,
            pill_fill,
            Stroke::new(1.0, pill_border),
            StrokeKind::Inside,
        );
    }

    for (index, label) in options.iter().enumerate() {
        let segment = Rect::from_min_size(
            Pos2::new(rect.left() + index as f32 * segment_width, rect.top()),
            Vec2::new(segment_width, rect.height()),
        );
        let item_response = ui.interact(segment, response.id.with(index), Sense::click());
        if item_response.clicked() && *selected != index {
            *selected = index;
            response.mark_changed();
        }
        response |= item_response;
        let seg_clip = segment.shrink2(Vec2::new(2.0, 1.0));
        if seg_clip.is_positive() {
            ui.painter()
                .with_clip_rect(seg_clip.intersect(ui.clip_rect()))
                .text(
                    segment.center() + Vec2::new(0.0, -0.5),
                    Align2::CENTER_CENTER,
                    label,
                    egui::TextStyle::Button.resolve(ui.style()),
                    p.text_primary,
                );
        }
    }
    *selected != before
}

/// 依据各选项文本宽度估算分段控件整体宽度（每段 = 最长文本 + 左右内边距）。
fn fit_segmented_width(ui: &Ui, options: &[&str]) -> f32 {
    if options.is_empty() {
        return 0.0;
    }
    let font = egui::TextStyle::Button.resolve(ui.style());
    let max_text_w = options
        .iter()
        .map(|label| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);
    // 每段：文本宽 + 左右各 8px 内边距（库内每段再内缩 2px，留足余量）
    let seg_w = max_text_w + 16.0;
    seg_w * options.len() as f32
}

// ──── 状态点 ────

pub fn status_dot(ui: &mut Ui, color: Color32) -> egui::Response {
    // 放大：14×14（原 10×10），更醒目
    let (resp, painter) = ui.allocate_painter(Vec2::new(14.0, 14.0), Sense::hover());
    painter.circle_filled(resp.rect.center(), 7.0, color);
    resp
}

// ──── 侧边栏导航行 ────

/// 导航行：图标 + 文字
///
/// 对齐 HTML `.nav-item`：
/// - 选中态：accent-tint 背景 + **accent 色文字**（不是白色！HTML: `.nav-item.active{color:var(--accent)}`）
/// - 左侧竖条指示器（HTML `::before` 伪元素）
/// - 选中态额外圆点标记（对齐用户截图7的 HTML 版本效果）
/// - 未选中：透明背景 + 次要文字色
pub fn nav_row(
    ui: &mut Ui,
    icon: NavIcon,
    label: &str,
    selected: bool,
    accent: Color32,
) -> Response {
    // 高度对齐 HTML .nav-item（padding 9px + 图标 18px ≈ 36）
    let height = 36.0;
    let available_width = ui.available_width();
    let (id, rect) = ui.allocate_space(Vec2::new(available_width, height));
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();

    // ── 水平内边距：对齐 HTML 原型 ──
    // 选中背景与左侧竖条与侧边栏边缘留 6px 呼吸空间，图标/文字在内部排版。
    // 点击区域保持整行宽度（用户体验更好）。
    let h_pad = 6.0;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + h_pad, rect.top()),
        Pos2::new(rect.right() - h_pad, rect.bottom()),
    );

    // 背景（使用内缩区域，边缘不贴侧边栏）
    let bg = if selected {
        // accent-tint: rgba(accent, 0.22) — 对应 HTML --accent-tint
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 56)
    } else if resp.hovered() {
        if ui.visuals().dark_mode {
            // WinUI3 SubtleHover rgba(255,255,255,15) 合成到面板底 #2B2B2B
            Color32::from_rgb(56, 56, 56) // #383838
        } else {
            // WinUI3 SubtleHover rgba(0,0,0,9) 合成到面板底 #FFFFFF
            Color32::from_rgb(246, 246, 246) // #F6F6F6
        }
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(inner, R_NAV, bg);

    // 左侧竖条（仅选中态，位于整行最左边缘，与选中背景框分离）
    // 对齐 HTML `.nav-item.active::before{left:-10px}` —— 竖条在导航项 padding 之外。
    if selected {
        let bar = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + 6.0),
            Vec2::new(3.0, rect.height() - 12.0),
        );
        painter.rect_filled(bar, 1.5, accent);
    }

    // 图标尺寸对齐 HTML .nav-item .ico（18px）
    let icon_size = 18.0;
    // 对齐 HTML .nav-item：未选中=次要灰(text-secondary)，悬停=主文本色，选中=强调色。
    // 图标颜色与文字保持一致（HTML 图标继承 currentColor）。
    let idle = ui.visuals().weak_text_color();
    let hovered = ui.visuals().text_color();
    let icon_color = if selected {
        accent
    } else if resp.hovered() {
        hovered
    } else {
        idle
    };

    // 图标起始 X（inner 左边缘 + 8px 间距，与竖条/侧边栏边缘充分分离）
    let icon_start_x = inner.left() + 8.0;

    let icon_rect = Rect::from_min_size(
        Pos2::new(icon_start_x, rect.center().y - icon_size / 2.0),
        Vec2::new(icon_size, icon_size),
    );
    nav_icon_paint(painter, icon_rect, icon, icon_color);

    // 文字 —— 选中时用 accent 色（对齐 HTML .nav-item.active{color:var(--accent)}）；
    // 与图标共用同一套配色逻辑（idle 次要灰 / hover 主文本 / selected accent）。
    let text_pos = Pos2::new(icon_start_x + icon_size + 8.0, rect.center().y);
    let label_color = icon_color;
    // 稍小的字体让侧边栏更精致
    painter.text(
        text_pos,
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        label_color,
    );

    resp
}

// ──── 主题切换按钮（双态：sun / moon）────

pub fn theme_toggle_button(ui: &mut Ui, dark: bool, _accent: Color32) -> bool {
    let size = 24.0_f32;
    let (resp, painter) = ui.allocate_painter(Vec2::new(size, size), Sense::click());
    let r = resp.rect;
    let cx = r.center().x;
    let cy = r.center().y;
    let icon_color = ui.visuals().text_color();
    if dark {
        let moon_r = size * 0.34;
        let notch_r = moon_r * 0.85;
        let moon_cx = cx - size * 0.03;
        let notch_offset_x = size * 0.17;
        let notch_offset_y = size * 0.08;
        painter.circle_filled(Pos2::new(moon_cx, cy), moon_r, icon_color);
        painter.circle_filled(
            Pos2::new(moon_cx + notch_offset_x, cy - notch_offset_y),
            notch_r,
            ui.visuals().window_fill,
        );
    } else {
        let sun_r = size * 0.30;
        let ray_len = size * 0.13;
        let ray_w = 1.8_f32;
        painter.circle_filled(r.center(), sun_r, icon_color);
        for i in 0..8 {
            let angle = i as f32 * std::f32::consts::TAU / 8.0;
            let inner_dist = sun_r + size * 0.06;
            let outer_dist = inner_dist + ray_len;
            let p1 = Pos2::new(cx + angle.cos() * inner_dist, cy + angle.sin() * inner_dist);
            let p2 = Pos2::new(cx + angle.cos() * outer_dist, cy + angle.sin() * outer_dist);
            painter.line_segment([p1, p2], Stroke::new(ray_w, icon_color));
        }
    }
    resp.clicked()
}

// ──── 主题色选择器圆圈 ────

/// 主题色选择器的单个色块。
/// - 未选中：纯色圆形
/// - 选中：内部白色勾（无外环高亮）
pub fn color_swatch(
    ui: &mut Ui,
    color: Color32,
    selected: bool,
    accent_for_check: bool,
) -> Response {
    let size = 30.0_f32;
    let (resp, painter) = ui.allocate_painter(Vec2::new(size, size), Sense::click());
    let center = resp.rect.center();
    let radius = size / 2.0 - 2.0;

    // 填充圆形
    painter.circle_filled(center, radius, color);

    if selected {
        // 白色勾 ✓（深色色块用白勾；浅色色块用深勾——这里统一白色+阴影模拟）
        let check_color = if accent_for_check {
            Color32::WHITE
        } else {
            Color32::from_black_alpha(180)
        };
        let s = Stroke::new(2.0_f32, check_color);
        // 画 ✓ 形状
        let arm1_start = Pos2::new(center.x - 5.0, center.y - 1.0);
        let arm1_end = Pos2::new(center.x - 1.5, center.y + 4.0);
        let arm2_end = Pos2::new(center.x + 6.0, center.y - 5.0);
        painter.line_segment([arm1_start, arm1_end], s);
        painter.line_segment([arm1_end, arm2_end], s);
    }

    resp
}

// ──── 圆角矩形按钮辅助 ────

/// 依据填充色亮度返回对比文字色。
///
/// egui 的 `Button` 文字色取自 `widgets.inactive.fg_stroke.color`，**不会**根据
/// `.fill()` 自动反色。浅色主题下该 fg 色是深灰（#1B1B1B），于是「深色填充按钮」
/// 会变成深色文字压在深色底上、不可见（深色主题下 fg 是浅灰，反而正常）。
///
/// 这里用感知亮度（Rec. 709）做阈值：暗底 → 白字，亮底 → 深色字(#1B1B1B)。
pub fn contrast_text(fill: Color32) -> Color32 {
    let y = 0.2126 * fill.r() as f32 + 0.7152 * fill.g() as f32 + 0.0722 * fill.b() as f32;
    if y < 128.0 {
        Color32::WHITE
    } else {
        Color32::from_rgb(27, 27, 27) // #1B1B1B WinUI3 TextPrimary，与浅色主题主文本一致
    }
}

/// 创建 6px 圆角矩形的按钮构建器（替代 egui 默认的全圆角 pill 按钮）
///
/// 返回 `egui::Button`，调用方需自行 `ui.add()` 或 `ui.add_enabled()`
///
/// 注意：egui 按钮文字不会因 `.fill()` 自动反色，故在此按填充亮度显式指定
/// 对比文字色，修复浅色主题下「深色填充按钮文字不可见」的问题。
pub fn rounded_button(text: &str, fill: Option<Color32>) -> egui::Button<'_> {
    match fill {
        Some(f) => egui::Button::new(egui::RichText::new(text).color(contrast_text(f)))
            .corner_radius(R_CTL)
            .fill(f),
        None => egui::Button::new(text).corner_radius(R_CTL),
    }
}

// ──── 导航栏图标（iconflow 的 Fluent UI 字体） ────

/// 解析导航图标字形，返回 (字符, 字体 family)。
///
/// 用 `try_icon` 而非 `icon()`：后者在 (Style, Size) 不受支持时 panic，而本函数
/// 每帧被调用，不能炸进程。`Size` 指字体变体档位（10/12/16/20/24/28/32/48/
/// Regular），不是渲染字号——实际字号由 `FontId` 决定，两者独立。
///
/// Regular 档在前：它是非实心的可变字体，描边风格与导航项 13px 文字协调，
/// 且在 18px 下比静态字面锐利；Filled 档仅作兜底。实测两者都覆盖全部 10 个图标。
fn nav_icon_glyph(kind: NavIcon) -> Option<(char, &'static str)> {
    use iconflow::{Pack, Size, Style};

    const FALLBACKS: [(Style, Size); 2] = [
        (Style::Regular, Size::Regular),
        (Style::Filled, Size::Custom(20)),
    ];
    FALLBACKS
        .iter()
        .find_map(|&(style, size)| {
            iconflow::try_icon(Pack::Fluentui, nav_icon_name(kind), style, size).ok()
        })
        .and_then(|icon| Some((char::from_u32(icon.codepoint)?, icon.family)))
}

fn nav_icon_name(kind: NavIcon) -> &'static str {
    match kind {
        NavIcon::Server => "server",
        NavIcon::Rpc => "remote",
        NavIcon::Model => "cube",
        NavIcon::Params => "options",
        NavIcon::Mcp => "toolbox",
        NavIcon::Log => "list",
        NavIcon::RpcLog => "document",
        NavIcon::Commands => "play",
        NavIcon::Presets => "star",
        NavIcon::Settings => "settings",
    }
}

/// 在给定矩形内绘制导航图标（iconflow Fluent UI 字形）。
pub fn nav_icon_paint(painter: &Painter, rect: Rect, kind: NavIcon, color: Color32) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    let Some((ch, family)) = nav_icon_glyph(kind) else {
        // 本函数每帧调用，解析失败是确定性的，不加限制会每秒刷 60 行日志
        WARNED.call_once(|| log::warn!("导航图标解析失败，跳过绘制: {kind:?}"));
        return;
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        ch.to_string(),
        FontId::new(
            rect.height().max(1.0),
            egui::FontFamily::Name(family.into()),
        ),
        color,
    );
}

#[cfg(test)]
mod nav_icon_tests {
    use super::{nav_icon_glyph, NavIcon};

    const ALL: [NavIcon; 10] = [
        NavIcon::Server,
        NavIcon::Rpc,
        NavIcon::Model,
        NavIcon::Params,
        NavIcon::Mcp,
        NavIcon::Log,
        NavIcon::RpcLog,
        NavIcon::Commands,
        NavIcon::Presets,
        NavIcon::Settings,
    ];

    /// iconflow 升级若删掉某个图标名或改掉变体，这里会失败，
    /// 而不是运行时静默不绘制或悄悄换一套外观。
    #[test]
    fn all_nav_icons_resolve() {
        for kind in ALL {
            let (ch, family) =
                super::nav_icon_glyph(kind).unwrap_or_else(|| panic!("{kind:?} 解析失败"));
            assert!(
                !ch.is_control(),
                "{kind:?} 拿到控制字符 U+{:04X}",
                ch as u32
            );
            assert_eq!(
                family, "Fluent UI Regular Resizable",
                "{kind:?} 回退到了非预期字体档"
            );
        }
    }
}
