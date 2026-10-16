/// 帮助按钮组件 - 问号图标气泡提示
///
/// WinUI 3 风格：无边框图标按钮（无背景填充、无描边），图标为 iconflow
/// Fluent UI 的 `question-circle` 字形；空闲时次要灰（weak_text），悬停时
/// 主文本色 + 淡色圆角背景，与 `widgets::nav_row` 的悬停配色一致。
///
/// 气泡提示两种触发方式：
/// - 悬停：egui 标准 tooltip（带延迟）；
/// - 点击：点击图标立即显示（`Tooltip::for_widget` 调用期间常驻），
///   再次点击同一图标或点击其他任意位置关闭。
use crate::ui::widgets;
use egui::{Align2, Color32, FontId, PointerButton, Sense, Tooltip, Vec2};

/// 当前"点击打开"的帮助气泡对应的按钮 id。
///
/// 存于 `Context::data` 临时区（不持久化）；用 newtype 保证 TypeId
/// 不与其它组件的数据键冲突。
#[derive(Clone, Copy, PartialEq)]
struct OpenHelpTooltip(egui::Id);

/// 帮助按钮样式配置（WinUI 3 HelpButton：无边框，仅尺寸可调）
pub struct HelpButtonStyle {
    /// 图标占用区域大小
    pub size: Vec2,
    /// 图标字形渲染字号
    pub text_size: f32,
}

impl Default for HelpButtonStyle {
    fn default() -> Self {
        Self {
            size: Vec2::new(42.0, 42.0),
            text_size: 36.0,
        }
    }
}

/// 解析 question-circle 图标字形，返回 (字形字符串, 字体 family)。
///
/// 用 `try_icon` 而非 `icon()`：后者在 (Style, Size) 不受支持时 panic，而本函数
/// 每帧被调用，不能炸进程。Regular 档在前（描边可变字体，小尺寸下比 Filled
/// 锐利，与导航图标约定一致），Filled 档仅作兜底。
/// iconflow 升级若删掉该图标，退回 "❓" emoji 保底，避免按钮凭空消失。
fn help_glyph() -> (String, Option<&'static str>) {
    use iconflow::{Pack, Size, Style};

    const FALLBACKS: [(Style, Size); 2] = [
        (Style::Regular, Size::Regular),
        (Style::Filled, Size::Custom(16)),
    ];
    FALLBACKS
        .iter()
        .find_map(|&(style, size)| {
            iconflow::try_icon(Pack::FluentUi, "question-circle", style, size).ok()
        })
        .and_then(|icon| {
            char::from_u32(icon.codepoint)
                .map(|ch| (ch.to_string(), Some(icon.family)))
        })
        .unwrap_or_else(|| ("❓".to_string(), None))
}

/// 创建帮助按钮
///
/// # 参数
/// * `ui` - egui UI 上下文
/// * `help_text` - 悬停时显示的帮助文本
///
/// # 返回值
/// 返回 `egui::Response`
///
/// # 示例
/// ```rust
/// // 基础用法
/// helper::help_button(ui, "这是帮助信息");
///
/// // 带自定义样式
/// let style = HelpButtonStyle {
///     size: egui::vec2(16.0, 16.0),
///     text_size: 13.0,
/// };
/// helper::help_button_with_style(ui, "自定义样式帮助", &style);
/// ```
pub fn help_button(ui: &mut egui::Ui, help_text: &str) -> egui::Response {
    help_button_with_style(ui, help_text, &HelpButtonStyle::default())
}

/// 创建带自定义样式的帮助按钮
pub fn help_button_with_style(
    ui: &mut egui::Ui,
    help_text: &str,
    style: &HelpButtonStyle,
) -> egui::Response {
    let (id, rect) = ui.allocate_space(style.size);
    let response = ui.interact(rect, id, Sense::click() | Sense::hover());

    // 点击切换气泡：点自己 → 开/关；点其他任意位置 → 关。
    // 槽位存 ctx 临时数据（insert_temp 不持久化），全局只有一份。
    let slot = egui::Id::new("helper::open_help_tooltip");
    let is_open = ui.ctx().data(|d| d.get_temp::<OpenHelpTooltip>(slot))
        == Some(OpenHelpTooltip(id));
    if response.clicked() {
        if is_open {
            ui.ctx().data_mut(|d| d.remove::<OpenHelpTooltip>(slot));
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(slot, OpenHelpTooltip(id)));
        }
    } else if is_open
        && ui.ctx().input(|i| i.pointer.button_pressed(PointerButton::Primary))
    {
        ui.ctx().data_mut(|d| d.remove::<OpenHelpTooltip>(slot));
    }

    // 悬停背景：WinUI 3 SubtleHover（深色 #383838 / 浅色 #F6F6F6），与 nav_row 一致
    if response.hovered() {
        let bg = if ui.visuals().dark_mode {
            Color32::from_rgb(56, 56, 56)
        } else {
            Color32::from_rgb(246, 246, 246)
        };
        ui.painter()
            .rect_filled(rect, widgets::R_CTL, bg);
    }

    // 配色：空闲 = 次要灰，悬停 = 主文本色（对齐 WinUI 3 图标按钮）
    let color = if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };

    // 绘制图标字形（iconflow Fluent UI 字体族）
    let (glyph, family) = help_glyph();
    let font = match family {
        Some(f) => FontId::new(style.text_size, egui::FontFamily::Name(f.into())),
        None => FontId::proportional(style.text_size),
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, glyph, font, color);

    // 气泡：点击打开期间用 for_widget（调用期间常驻显示），否则走标准悬停 tooltip
    // （on_hover_text 按值消费 response，故两个分支各自返回）
    if is_open {
        Tooltip::for_widget(&response).show(|ui| {
            ui.label(help_text);
        });
        response
    } else {
        response.on_hover_text(help_text)
    }
}

/// 创建小型帮助按钮（更紧凑的样式）
pub fn help_button_small(ui: &mut egui::Ui, help_text: &str) -> egui::Response {
    let style = HelpButtonStyle {
        size: Vec2::new(34.0, 34.0),
        text_size: 30.0,
    };
    help_button_with_style(ui, help_text, &style)
}

/// 创建内联帮助按钮（与文本对齐）
pub fn help_button_inline(ui: &mut egui::Ui, help_text: &str) -> egui::Response {
    let style = HelpButtonStyle {
        size: Vec2::new(32.0, 32.0),
        text_size: 28.0,
    };
    help_button_with_style(ui, help_text, &style)
}
