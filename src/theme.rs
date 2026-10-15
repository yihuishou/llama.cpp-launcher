use egui_thematic::ThemeConfig;

/// WinUI 3 浅色主题预设
///
/// 色值取自 WinUI 3 Fluent 设计令牌（与 `fluent-egui` 的
/// `FluentPalette` 浅色分支一致）：
///   TextPrimary #1B1B1B / TextSecondary #5C5C5C / TextDisabled #A0A0A0
///   Accent #005FB8 / ApplicationBackground #F3F3F3 / LayerBackground #F9F9F9
///   ControlFill #FFFFFF / CardBackground #FFFFFF / CardStroke rgba(0,0,0,0.063)
///   StrokeControl rgba(0,0,0,0.149) / Success #0F7B0F / Warning #9D5D00 / Danger #C42B1C
///
/// 圆角沿用本项目既定的 6px（WinUI 3 官方为控件 4px / 浮层 8px）。
pub fn fluent3_light_preset() -> ThemeConfig {
    ThemeConfig {
        name: "WinUI3 Light".to_string(),
        dark_mode: false,

        // ── 文本颜色 ──
        override_text_color: Some([27, 27, 27, 255]), // #1B1B1B TextPrimary
        override_weak_text_color: Some([92, 92, 92, 255]), // #5C5C5C TextSecondary
        override_hyperlink_color: Some([0, 95, 184, 255]), // #005FB8 Accent

        // ── 背景色（层次：card > nav > app） ──
        override_faint_bg_color: Some([243, 243, 243, 255]), // #F3F3F3 ApplicationBackground
        override_extreme_bg_color: Some([255, 255, 255, 255]), // #FFFFFF
        override_code_bg_color: Some([249, 249, 249, 255]),  // #F9F9F9 LayerBackground

        // ── 窗口 ──
        override_window_fill: Some([243, 243, 243, 255]), // #F3F3F3 ApplicationBackground
        override_window_stroke_color: Some([240, 240, 240, 255]), // #F0F0F0 = CardStroke 合成到白
        override_window_stroke_width: Some(1.0),
        override_window_corner_radius: Some(6), // 本项目既定 6px
        override_window_shadow_size: Some(8),

        // ── 面板（侧边栏用，与卡片同色）──
        override_panel_fill: Some([255, 255, 255, 255]), // #FFFFFF — 同 CardBackground

        // ── 弹窗阴影 ──
        override_popup_shadow_size: Some(12),

        // ── 选区 ──
        override_selection_bg: Some([0, 95, 184, 255]),
        override_selection_stroke_color: Some([0, 86, 166, 255]), // #0056A6 AccentPressed
        override_selection_stroke_width: Some(1.0),

        // ── 语义色（警告 / 错误）──
        override_warn_fg_color: Some([157, 93, 0, 255]), // #9D5D00 Warning
        override_error_fg_color: Some([196, 43, 28, 255]), // #C42B1C Danger

        // ── 非交互控件（卡片/标签/分隔线）──
        override_widget_noninteractive_bg_fill: Some([255, 255, 255, 255]), // #FFFFFF CardBackground
        override_widget_noninteractive_weak_bg_fill: Some([249, 249, 249, 255]), // #F9F9F9 Layer
        override_widget_noninteractive_bg_stroke_color: Some([240, 240, 240, 255]), // #F0F0F0 CardStroke
        override_widget_noninteractive_bg_stroke_width: Some(1.0),
        override_widget_noninteractive_corner_radius: Some(6), // 本项目既定 6px
        override_widget_noninteractive_fg_stroke_color: Some([27, 27, 27, 255]),
        override_widget_noninteractive_fg_stroke_width: Some(1.0),
        override_widget_noninteractive_expansion: Some(0.0),

        // ── 非活动控件（默认按钮/输入框）──
        // 滑块轨道(rail)也用此色绘制。WinUI 3 的 ControlFill 纯白会与卡片白底同色导致轨道隐形，
        // 故取 ControlFillDefault 同族的 ControlFillMouseOver(#F9F9F9) 作默认态，保持轨道可辨。
        override_widget_inactive_bg_fill: Some([249, 249, 249, 255]), // #F9F9F9
        override_widget_inactive_weak_bg_fill: Some([243, 243, 243, 255]), // #F3F3F3
        override_widget_inactive_bg_stroke_color: Some([217, 217, 217, 255]), // #D9D9D9 = StrokeControl 合成到白
        override_widget_inactive_bg_stroke_width: Some(1.0),
        override_widget_inactive_corner_radius: Some(6), // 本项目既定 6px
        override_widget_inactive_fg_stroke_color: Some([27, 27, 27, 255]),
        override_widget_inactive_fg_stroke_width: Some(1.0),
        override_widget_inactive_expansion: Some(0.0),

        // ── 悬停控件 ──
        override_widget_hovered_bg_fill: Some([238, 238, 238, 255]), // #EEEEEE = SubtleHover 合成
        override_widget_hovered_weak_bg_fill: Some([243, 243, 243, 255]),
        override_widget_hovered_bg_stroke_color: Some([0, 95, 184, 255]), // #005FB8 Accent
        override_widget_hovered_bg_stroke_width: Some(1.0),
        override_widget_hovered_corner_radius: Some(6),
        override_widget_hovered_fg_stroke_color: Some([27, 27, 27, 255]),
        override_widget_hovered_fg_stroke_width: Some(1.0),
        override_widget_hovered_expansion: Some(0.0),

        // ── 活动/按下控件 ──
        override_widget_active_bg_fill: Some([0, 86, 166, 255]), // #0056A6 AccentPressed
        override_widget_active_weak_bg_fill: Some([0, 120, 212, 255]), // #0078D4 Accent 预设色
        override_widget_active_bg_stroke_color: Some([0, 64, 110, 255]), // #00406E
        override_widget_active_bg_stroke_width: Some(1.0),
        override_widget_active_corner_radius: Some(6),
        override_widget_active_fg_stroke_color: Some([255, 255, 255, 255]), // OnAccent（浅色主题为白）
        override_widget_active_fg_stroke_width: Some(1.0),
        override_widget_active_expansion: Some(0.0),

        // ── 展开/打开控件 ──
        override_widget_open_bg_fill: Some([238, 238, 238, 255]), // #EEEEEE
        override_widget_open_weak_bg_fill: Some([243, 243, 243, 255]),
        override_widget_open_bg_stroke_color: Some([0, 95, 184, 255]), // #005FB8
        override_widget_open_bg_stroke_width: Some(1.0),
        override_widget_open_corner_radius: Some(6),
        override_widget_open_fg_stroke_color: Some([27, 27, 27, 255]),
        override_widget_open_fg_stroke_width: Some(1.0),
        override_widget_open_expansion: Some(0.0),

        // ── 其他 ──
        override_resize_corner_size: Some(6.0),
        override_text_cursor_width: Some(2.0),
        override_clip_rect_margin: Some(3.0),
        override_button_frame: Some(true),
        override_collapsing_header_frame: Some(false),
        override_indent_has_left_vline: Some(false),
        override_striped: Some(false),
        override_slider_trailing_fill: Some(false),
    }
}

/// WinUI 3 深色主题预设
///
/// 色值取自 WinUI 3 Fluent 设计令牌（与 `fluent-egui` 的
/// `FluentPalette` 深色分支一致）：
///   TextPrimary #FFFFFF / TextSecondary #C8C8C8 / TextDisabled #7E7E7E
///   Accent #60CDFF / ApplicationBackground #202020 / LayerBackground #181818
///   CardBackground #2B2B2B / ControlFill #3A3A3A / ControlFillHover #424242
///   ControlFillPressed #323232 / CardStroke rgba(255,255,255,0.07)
///   Success #6CCB5F / Warning #FCE100 / Danger #FF99A4
pub fn fluent3_dark_preset() -> ThemeConfig {
    let mut t = fluent3_light_preset();
    t.dark_mode = true;

    // ── 文本颜色 ──
    t.override_text_color = Some([255, 255, 255, 255]); // #FFFFFF TextPrimary
    t.override_weak_text_color = Some([200, 200, 200, 255]); // #C8C8C8 TextSecondary

    // ── 背景层次（主内容区 > 侧边栏 > 卡片表面）──
    t.override_faint_bg_color = Some([32, 32, 32, 255]); // #202020 ApplicationBackground
    t.override_extreme_bg_color = Some([24, 24, 24, 255]); // #181818 LayerBackground
    t.override_code_bg_color = Some([43, 43, 43, 255]); // #2B2B2B CardBackground

    // ── 窗口填充 = 主内容区背景 ──
    t.override_window_fill = Some([32, 32, 32, 255]); // #202020
    t.override_window_stroke_color = Some([46, 46, 46, 255]); // #2E2E2E = CardStroke 合成到 #202020
    t.override_window_corner_radius = Some(6); // 本项目既定 6px
    t.override_window_shadow_size = Some(16);

    // ── 面板 = 侧边栏背景（与卡片同色）──
    t.override_panel_fill = Some([43, 43, 43, 255]); // #2B2B2B — 同 CardBackground

    // ── 弹窗阴影 ──
    t.override_popup_shadow_size = Some(20);

    // ── 语义色 ──
    t.override_warn_fg_color = Some([252, 225, 0, 255]); // #FCE100 Warning
    t.override_error_fg_color = Some([255, 153, 164, 255]); // #FF99A4 Danger

    // ── 非交互控件（卡片 surface = #2B2B2B）──
    t.override_widget_noninteractive_bg_fill = Some([43, 43, 43, 255]); // #2B2B2B
    t.override_widget_noninteractive_weak_bg_fill = Some([32, 32, 32, 255]); // #202020
    t.override_widget_noninteractive_bg_stroke_color = Some([46, 46, 46, 255]); // #2E2E2E
    t.override_widget_noninteractive_fg_stroke_color = Some([200, 200, 200, 255]);
    t.override_widget_noninteractive_corner_radius = Some(6); // 本项目既定 6px

    // ── 非活动控件（输入框/按钮默认态）──
    // ControlFill(#3A3A3A) 比卡片(#2B2B2B)亮，滑块轨可见。
    t.override_widget_inactive_bg_fill = Some([58, 58, 58, 255]); // #3A3A3A ControlFill
    t.override_widget_inactive_weak_bg_fill = Some([43, 43, 43, 255]); // #2B2B2B
    t.override_widget_inactive_bg_stroke_color = Some([77, 77, 77, 255]); // #4D4D4D = StrokeControl 合成到 #3A3A3A
    t.override_widget_inactive_corner_radius = Some(6); // 本项目既定 6px
    t.override_widget_inactive_fg_stroke_color = Some([200, 200, 200, 255]);

    // ── 悬停控件 ──
    t.override_widget_hovered_bg_fill = Some([66, 66, 66, 255]); // #424242 ControlFillHover
    t.override_widget_hovered_weak_bg_fill = Some([43, 43, 43, 255]);
    t.override_widget_hovered_bg_stroke_color = Some([77, 77, 77, 255]);
    t.override_widget_hovered_fg_stroke_color = Some([255, 255, 255, 255]);
    t.override_widget_hovered_corner_radius = Some(6);

    // ── 活动/按下控件 ──
    // 深色主题的 WinUI 3 OnAccent 为黑色，但按下底色是中性灰而非强调色，
    // 黑字会不可读，故沿用白字。
    t.override_widget_active_bg_fill = Some([50, 50, 50, 255]); // #323232 ControlFillPressed
    t.override_widget_active_weak_bg_fill = Some([50, 50, 50, 255]);
    t.override_widget_active_bg_stroke_color = Some([63, 63, 63, 255]); // #3F3F3F
    t.override_widget_active_fg_stroke_color = Some([255, 255, 255, 255]);
    t.override_widget_active_corner_radius = Some(6);

    // ── 展开/打开控件 ──
    t.override_widget_open_bg_fill = Some([66, 66, 66, 255]); // #424242
    t.override_widget_open_weak_bg_fill = Some([43, 43, 43, 255]);
    t.override_widget_open_bg_stroke_color = Some([77, 77, 77, 255]);
    t.override_widget_open_fg_stroke_color = Some([255, 255, 255, 255]);
    t.override_widget_open_corner_radius = Some(6);

    t
}

/// 解析十六进制颜色（支持 #RRGGBB / RRGGBB），失败时回退到 WinUI 3 强调色
pub fn parse_hex(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&s[0..2], 16),
            u8::from_str_radix(&s[2..4], 16),
            u8::from_str_radix(&s[4..6], 16),
        ) {
            return [r, g, b];
        }
    }
    [0, 95, 184] // #005FB8 WinUI 3 Accent
}

/// 解析配置中的主题色为 egui Color32
pub fn accent_color(s: &str) -> egui::Color32 {
    let c = parse_hex(s);
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

/// 将主题色向白色方向提亮：降低饱和度、增加亮度，得到更柔和的浅色调。
/// `t` = 白(1.0) 时的亮度混合比例（0.0 = 原色，1.0 = 纯白）。
pub fn lighten_accent(accent: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (accent.r() as f32 + (255.0 - accent.r() as f32) * t).round() as u8,
        (accent.g() as f32 + (255.0 - accent.g() as f32) * t).round() as u8,
        (accent.b() as f32 + (255.0 - accent.b() as f32) * t).round() as u8,
    )
}

/// 应用主题到全局视觉样式
///
/// - dark: 深色模式开关
/// - accent: 全局强调色 (RGB)，用于超链接 / 选区 / 悬停描边
pub fn apply_theme(ctx: &egui::Context, dark: bool, accent: [u8; 3]) {
    let mut t = if dark {
        fluent3_dark_preset()
    } else {
        fluent3_light_preset()
    };
    let a = [accent[0], accent[1], accent[2], 255u8];
    t.override_hyperlink_color = Some(a);
    t.override_selection_bg = Some([accent[0], accent[1], accent[2], 96u8]);
    t.override_selection_stroke_color = Some([accent[0], accent[1], accent[2], 220]);
    // 注意：不设置 widget_open/hovered stroke 为 accent 色！
    // 那会导致面板边框（顶栏/侧栏/中央区分隔线）染上主题色紫/蓝色。
    // 这些描边保持 preset 中的中性色（light: #D2D2D7, dark: #48484A）即可。

    let mut visuals = t.to_visuals();
    // 提示框使用更克制的阴影，避免问号帮助框出现大面积黑色晕影。
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [2, 3],
        blur: 4,
        spread: 0,
        color: egui::Color32::from_black_alpha(64),
    };

    // 覆盖全局样式：确保所有按钮和输入框使用 6px 圆角（非 egui 默认的全圆角）
    ctx.set_visuals(visuals);
}
