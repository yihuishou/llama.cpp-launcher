use fluent_egui::winui::{self, FluentTheme, ThemeConfig};

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
/// - accent: 全局强调色 (RGB)
pub fn apply_theme(ctx: &egui::Context, dark: bool, accent: [u8; 3]) {
    let theme = if dark { FluentTheme::Dark } else { FluentTheme::Light };
    winui::apply_theme(
        ctx,
        ThemeConfig::new(theme).with_accent(egui::Color32::from_rgb(accent[0], accent[1], accent[2])),
    );

    // 提示框使用更克制的阴影，避免问号帮助框出现大面积黑色晕影
    let mut visuals = ctx.style_of(ctx.theme()).visuals.clone();
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [2, 3],
        blur: 4,
        spread: 0,
        color: egui::Color32::from_black_alpha(64),
    };
    ctx.set_visuals(visuals);
}