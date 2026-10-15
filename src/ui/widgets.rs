//! 通用 UI 组件 —— 移植自 HTML 原型的设计语义
//!
//! 设计参数对齐 HTML mockup：
//!   - radius-card: 12px  /  radius-ctl: 6px（更方正，用户要求）
//!   - accent: #60CDFF (dark) / #005FB8 (light)
//!   - 选中导航项：accent-tint 背景 + accent 色文字（对齐 HTML .nav-item.active{color:var(--accent)}）
//!   - Toggle：开关在左、标签在右（对齐 HTML <div class="toggle"><.sw/><span>label</span></div>）
//!   - 分段控件/按钮/输入框统一 6px 圆角矩形（非 pill 椭圆）

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

// ──── 设计常量（对齐 HTML :root 变量，按用户反馈调方） ────

/// 控件圆角 = 6px（比 HTML --radius-ctl:8px 更方正，用户明确要求）
pub const R_CTL: f32 = 6.0;
/// 卡片圆角 = 12px（HTML --radius-card）
pub const R_CARD: f32 = 12.0;
/// 导航项圆角 = 6px
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

/// 卡片：标题（带 accent 竖条） + 分隔线 + 内容，12px 圆角
///
/// 对齐 HTML `.card h3`：
/// ```html
/// <h3><span class="bar"></span>Server 配置</h3>
/// /* .bar{width:3px;height:14px;border-radius:2px;background:var(--accent);} */
/// ```
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
    let fill = ui.visuals().widgets.noninteractive.bg_fill;
    // ★ 关键修复：深色模式下显式调亮 stroke，让卡片边界在 #2B2B2B 上可见
    let stroke_color = if ui.visuals().dark_mode {
        Color32::from_rgb(90, 90, 95) // #5A5A5F — 在深色卡片上清晰可见
    } else {
        Color32::from_rgb(190, 190, 200) // 在浅色卡片上可见
    };
    let frame = egui::Frame::default()
        .fill(fill)
        .stroke(egui::Stroke {
            width: 1.0_f32,
            color: stroke_color,
        })
        .corner_radius(R_CARD)
        .inner_margin(egui::Margin::same(24_i8)) // HTML .card{padding:24px}
        .outer_margin(egui::Margin::symmetric(0, 12)); // 子框之间间距
    frame
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.horizontal(|ui| {
                ui.label(title);
                // 标题右侧紧跟的状态通知（显式颜色，随成败切换）
                if let Some((ok, msg)) = notice {
                    let color = if ok {
                        Color32::from_rgb(52, 168, 83) // #34A853 成功绿
                    } else {
                        ui.visuals().error_fg_color
                    };
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

// ──── iOS Toggle（带标签，开关在左） ────

/// iOS 风格开关 + 标签行。on 状态使用固定的强调色（不随深色模式变暗）。
/// `toggle`：[开关] [标签]；`toggle_trailing`：[标签] …… [开关]（开关在选项末尾）

/// 开关在选项末尾：标签在左，开关靠右（供 KV 缓存配置等整行选项使用）
pub fn toggle_trailing(ui: &mut Ui, on: &mut bool, label: &str, accent: Color32) -> bool {
    draw_toggle(ui, on, label, accent, true)
}

/// `trailing=false`：开关在左、标签在右；`trailing=true`：标签在左、开关靠右（选项末尾）
fn draw_toggle(ui: &mut Ui, on: &mut bool, label: &str, accent: Color32, trailing: bool) -> bool {
    let height = 22.0_f32;
    let toggle_width = 40.0_f32;
    let spacing = 10.0;

    // 用 galley 实际测量标签宽度（替代旧"字符数×13px"估算，避免间距过大/重叠）
    let label_width = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            label.to_string(),
            FontId::default(),
            ui.visuals().text_color(),
        )
        .rect
        .size()
        .x
    });
    let total_width = toggle_width + spacing + label_width;
    let (resp, painter) = ui.allocate_painter(Vec2::new(total_width, height), Sense::click());
    let rect = resp.rect;
    let radius = height / 2.0;

    if trailing {
        // ── 标签文字（左侧）──
        let label_pos = Pos2::new(rect.left(), rect.center().y);
        painter.text(
            label_pos,
            Align2::LEFT_CENTER,
            label,
            FontId::default(),
            ui.visuals().text_color(),
        );
    }

    // ── 开关 track（trailing 时靠右，即选项末尾）──
    let track_rect = if trailing {
        Rect::from_min_max(
            Pos2::new(rect.right() - toggle_width, rect.top()),
            Pos2::new(rect.right(), rect.bottom()),
        )
    } else {
        Rect::from_min_size(rect.left_top(), Vec2::new(toggle_width, height))
    };
    let off_color = if ui.visuals().dark_mode {
        Color32::from_gray(90)
    } else {
        Color32::from_rgb(200, 200, 200)
    };
    let track_fill = if *on { accent } else { off_color };
    painter.rect_filled(track_rect, radius, track_fill);

    // ── knob ──
    let knob_r = radius - 3.0;
    let cx = if *on {
        track_rect.right() - radius
    } else {
        track_rect.left() + radius
    };
    painter.circle_filled(Pos2::new(cx, rect.center().y), knob_r, Color32::WHITE);

    if !trailing {
        // ── 标签文字（右侧）──
        let label_x = rect.left() + toggle_width + spacing;
        let label_pos = Pos2::new(label_x, rect.center().y);
        painter.text(
            label_pos,
            Align2::LEFT_CENTER,
            label,
            FontId::default(),
            ui.visuals().text_color(),
        );
    }

    if resp.clicked() {
        *on = !*on;
        true
    } else {
        false
    }
}

/// 开关在左，标签在右（保持原有调用方式）
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str, accent: Color32) -> bool {
    draw_toggle(ui, on, label, accent, false)
}

// ──── 分段控件 ────

/// 分段控件：6px 圆角矩形，内边距舒适，选中项高亮
pub fn segmented(ui: &mut Ui, options: &[&str], selected: &mut usize, _accent: Color32) -> bool {
    let mut changed = false;
    // 背景槽：用非交互控件填充色
    let slot_fill = if ui.visuals().dark_mode {
        Color32::from_rgb(50, 50, 50) // #323232 WinUI3 ControlFillPressed
    } else {
        Color32::from_rgb(245, 245, 245) // #F5F5F5 WinUI3 ControlFillPressed
    };
    let frame = egui::Frame::default()
        .fill(slot_fill)
        .corner_radius(R_CTL)
        .inner_margin(egui::Margin::same(2));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, opt) in options.iter().enumerate() {
                let is_sel = *selected == i;
                // 选中态填充：WinUI 3 分段控件选中项用强调色（深色），
                // 浅色模式用白色卡片浮起在浅灰轨道上。
                let sel_fill = if ui.visuals().dark_mode {
                    _accent
                } else {
                    Color32::WHITE // #FFFFFF CardBackground
                };
                // 按钮内边距通过 Frame inner_margin 控制（egui 0.33.3 Button 无 .padding()）
                let resp = if is_sel {
                    ui.add(
                        egui::Button::new(*opt)
                            .fill(sel_fill)
                            .corner_radius(R_CTL - 1.0),
                    )
                } else {
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new(*opt).color(ui.visuals().weak_text_color()),
                        )
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(R_CTL - 1.0)
                        .stroke(egui::Stroke::NONE),
                    )
                };
                if resp.clicked() {
                    *selected = i;
                    changed = true;
                }
            }
        });
    });
    changed
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
/// - 选中：外环 + 内部白色勾（对齐 HTML .color-swatch.active 样式）
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
        // 外环（对应 HTML border-color: var(--text-primary) + box-shadow 双层环效果）
        let ring_radius = size / 2.0 - 0.5;
        painter.circle_stroke(
            center,
            ring_radius,
            Stroke::new(1.5_f32, ui.visuals().text_color()),
        );

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

// ──── 自绘矢量图标（SF Symbols 风格） ────

/// 在给定矩形内自绘导航图标（扁平矢量风格，stroke-based）
pub fn nav_icon_paint(painter: &Painter, rect: Rect, kind: NavIcon, color: Color32) {
    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;
    let _cy = rect.center().y; // Settings gear 以 cx 为基准对称绘制，cy 保留备用
                               // 对齐 HTML .nav-item .ico svg{stroke-width:1.8}，图标 18×18 在 24×24 viewBox 内
    let s = Stroke::new(1.8_f32, color);

    // 辅助函数：将 SVG 24×24 坐标系映射到实际 rect
    let sx = |x: f32| rect.left() + (x / 24.0) * w;
    let sy = |y: f32| rect.top() + (y / 24.0) * h;

    match kind {
        NavIcon::Server => {
            // ★ HTML: <rect x="2" y="3" width="20" height="14" rx="2"/>
            //         <path d="M8 21h8M12 17v4"/>
            //         <line x1="7" y1="8" x2="7" y2="12"/><line x1="17" y1="8" x2="17" y2="12"/>
            // 显示器屏幕
            painter.rect_stroke(
                Rect::from_min_max(Pos2::new(sx(2.0), sy(3.0)), Pos2::new(sx(22.0), sy(17.0))),
                2.0,
                s,
                egui::StrokeKind::Middle,
            );
            // 支架
            painter.line_segment(
                [Pos2::new(sx(8.0), sy(21.0)), Pos2::new(sx(16.0), sy(21.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(12.0), sy(17.0)), Pos2::new(sx(12.0), sy(21.0))],
                s,
            );
            // 屏幕内容线
            painter.line_segment(
                [Pos2::new(sx(7.0), sy(8.0)), Pos2::new(sx(7.0), sy(12.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(17.0), sy(8.0)), Pos2::new(sx(17.0), sy(12.0))],
                s,
            );
        }
        NavIcon::Rpc => {
            // ★ 保留不变：三点层级连接图（用户明确要求）
            let r = w * 0.17;
            let top = Pos2::new(cx, rect.top() + r + 1.0);
            let bl = Pos2::new(rect.left() + r + 1.0, rect.bottom() - r - 1.0);
            let br = Pos2::new(rect.right() - r - 1.0, rect.bottom() - r - 1.0);
            painter.line_segment([top, bl], s);
            painter.line_segment([top, br], s);
            painter.circle_stroke(top, r, s);
            painter.circle_stroke(bl, r, s);
            painter.circle_stroke(br, r, s);
        }
        NavIcon::Mcp => {
            // ★ MCP 工具集线：左侧小方块（主程序）连接右侧三个工具节点
            let hub = Rect::from_min_max(Pos2::new(sx(3.0), sy(9.0)), Pos2::new(sx(8.0), sy(15.0)));
            painter.rect_stroke(hub, 1.5, s, egui::StrokeKind::Middle);
            let r = w * 0.11;
            let nodes = [
                Pos2::new(sx(19.0), sy(4.5)),
                Pos2::new(sx(21.0), sy(12.0)),
                Pos2::new(sx(19.0), sy(19.5)),
            ];
            for &node in &nodes {
                painter.line_segment([Pos2::new(sx(8.0), sy(12.0)), node], s);
                painter.circle_stroke(node, r, s);
            }
        }
        NavIcon::Model => {
            // ★ HTML: 圆角六边形（3D 盒子 / AI 模型经典图标）
            // 原始尖角六边形顶点，每对相邻点之间插入圆角过渡点（内缩 ~5%）
            let hex_raw = [
                Pos2::new(sx(12.0), sy(2.37)),  // 顶
                Pos2::new(sx(20.0), sy(6.27)),  // 右上
                Pos2::new(sx(20.0), sy(15.73)), // 右下
                Pos2::new(sx(12.0), sy(19.63)), // 底
                Pos2::new(sx(4.0), sy(15.73)),  // 左下
                Pos2::new(sx(4.0), sy(6.27)),   // 左上
            ];
            let n = hex_raw.len();
            let mut rounded_hex = Vec::with_capacity(n * 3); // 每个角拆成 3 点（入+弧出）
            for i in 0..n {
                let curr = hex_raw[i];
                let prev = hex_raw[(i + n - 1) % n];
                let next = hex_raw[(i + 1) % n];
                // 沿两条边各内缩 8% 作为圆角起止点
                let round_t = 0.12;
                let p_in = Pos2::new(
                    curr.x + (prev.x - curr.x) * round_t,
                    curr.y + (prev.y - curr.y) * round_t,
                );
                let p_out = Pos2::new(
                    curr.x + (next.x - curr.x) * round_t,
                    curr.y + (next.y - curr.y) * round_t,
                );
                rounded_hex.push(p_in);
                rounded_hex.push(curr); // 保留原顶点作为弧中点近似
                rounded_hex.push(p_out);
            }
            painter.add(egui::epaint::PathShape::closed_line(rounded_hex, s));
        }
        NavIcon::Params => {
            // ★ HTML: 三条竖直滑杆 + 圆形旋钮
            // <path d="M4 21v-7m0-4V3m8 18v-9m0-4V3m8 18v-5m0-4V3"/>
            // <circle cx="4" cy="10" r="2"/><circle cx="12" cy="8" r="2"/><circle cx="20" cy="14" r="2"/>
            let sliders = [
                (4.0_f32, 10.0, 21.0), // (track_x, knob_y, track_bottom)
                (12.0, 8.0, 19.0),
                (20.0, 14.0, 17.0),
            ];
            for &(kx, ky, bot) in &sliders {
                painter.line_segment([Pos2::new(sx(kx), sy(3.0)), Pos2::new(sx(kx), sy(bot))], s);
                painter.circle_stroke(Pos2::new(sx(kx), sy(ky)), sx(2.0) - sx(0.0), s);
            }
        }
        NavIcon::Log => {
            // ★ HTML: 文档图标
            // <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
            // <polyline points="14,2 14,8 20,8"/>
            // <line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="14" y2="17"/>
            // 文档外框
            painter.add(egui::epaint::PathShape::closed_line(
                vec![
                    Pos2::new(sx(6.0), sy(2.0)),
                    Pos2::new(sx(14.0), sy(2.0)),
                    Pos2::new(sx(14.0), sy(8.0)),
                    Pos2::new(sx(20.0), sy(8.0)),
                    Pos2::new(sx(20.0), sy(20.0)),
                    Pos2::new(sx(6.0), sy(20.0)),
                ],
                s,
            ));
            // 折角线
            painter.line_segment(
                [Pos2::new(sx(14.0), sy(2.0)), Pos2::new(sx(14.0), sy(8.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(14.0), sy(8.0)), Pos2::new(sx(20.0), sy(8.0))],
                s,
            );
            // 文字行
            painter.line_segment(
                [Pos2::new(sx(8.0), sy(13.0)), Pos2::new(sx(16.0), sy(13.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(8.0), sy(17.0)), Pos2::new(sx(14.0), sy(17.0))],
                s,
            );
        }
        NavIcon::RpcLog => {
            // 远程调用日志图标：与服务器日志同款文档样式，靠文字标签区分
            // ★ HTML: 文档图标
            // <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
            // <polyline points="14,2 14,8 20,8"/>
            // <line x1="8" y1="13" x2="16" y2="13"/><line x1="8" y1="17" x2="14" y2="17"/>
            // 文档外框
            painter.add(egui::epaint::PathShape::closed_line(
                vec![
                    Pos2::new(sx(6.0), sy(2.0)),
                    Pos2::new(sx(14.0), sy(2.0)),
                    Pos2::new(sx(14.0), sy(8.0)),
                    Pos2::new(sx(20.0), sy(8.0)),
                    Pos2::new(sx(20.0), sy(20.0)),
                    Pos2::new(sx(6.0), sy(20.0)),
                ],
                s,
            ));
            // 折角线
            painter.line_segment(
                [Pos2::new(sx(14.0), sy(2.0)), Pos2::new(sx(14.0), sy(8.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(14.0), sy(8.0)), Pos2::new(sx(20.0), sy(8.0))],
                s,
            );
            // 文字行
            painter.line_segment(
                [Pos2::new(sx(8.0), sy(13.0)), Pos2::new(sx(16.0), sy(13.0))],
                s,
            );
            painter.line_segment(
                [Pos2::new(sx(8.0), sy(17.0)), Pos2::new(sx(14.0), sy(17.0))],
                s,
            );
        }
        NavIcon::Commands => {
            // ★ HTML: 播放三角形 <polygon points="5,3 19,12 5,21"/>
            painter.add(egui::epaint::PathShape::closed_line(
                vec![
                    Pos2::new(sx(5.0), sy(3.0)),
                    Pos2::new(sx(19.0), sy(12.0)),
                    Pos2::new(sx(5.0), sy(21.0)),
                ],
                s,
            ));
        }
        NavIcon::Presets => {
            // ★ HTML: 星形 <polygon points="12,2 15.09,8.26 22,9.27 ..."/>
            painter.add(egui::epaint::PathShape::closed_line(
                vec![
                    Pos2::new(sx(12.0), sy(2.0)),
                    Pos2::new(sx(15.09), sy(8.26)),
                    Pos2::new(sx(22.0), sy(9.27)),
                    Pos2::new(sx(17.0), sy(14.14)),
                    Pos2::new(sx(18.18), sy(21.02)),
                    Pos2::new(sx(12.0), sy(18.0)),
                    Pos2::new(sx(5.82), sy(21.02)),
                    Pos2::new(sx(7.0), sy(14.14)),
                    Pos2::new(sx(2.0), sy(9.27)),
                    Pos2::new(sx(8.91), sy(8.26)),
                ],
                s,
            ));
        }
        NavIcon::Settings => {
            // ★ HTML: 圆角齿轮（cog）—— 外圈带圆角齿牙 + 内圆孔
            // 对齐 HTML SF Symbols "gear" / Fluent "Settings" 图标风格
            let gear_cx = sx(12.0); // 齿轮中心（viewBox 12,12）
            let gear_cy = sy(12.0);
            let outer_r = w * 0.42; // 外圈半径（含齿）
            let inner_r = w * 0.22; // 内孔半径
            let hub_r = w * 0.30; // 齿根圆半径（齿与齿之间的凹处）
            let teeth = 8;

            // 用 path 绘制齿轮外轮廓（带齿的闭合路径）
            let mut gear_points = Vec::with_capacity(teeth * 4);
            for i in 0..teeth {
                let base_ang = (i as f32) * std::f32::consts::TAU / teeth as f32;
                // 每个齿：从齿根 → 齿顶右 → 齿顶左 → 回到下一个齿根
                // 齿根起点（凹处）
                let a0 = base_ang - 0.20; // 齿宽约 0.4 rad
                let a1 = base_ang - 0.10; // 齿顶开始
                let a2 = base_ang + 0.10; // 齿顶结束
                let a3 = base_ang + 0.20; // 齿根结束

                gear_points.push(Pos2::new(
                    gear_cx + a0.cos() * hub_r,
                    gear_cy + a0.sin() * hub_r,
                ));
                gear_points.push(Pos2::new(
                    gear_cx + a1.cos() * outer_r,
                    gear_cy + a1.sin() * outer_r,
                ));
                gear_points.push(Pos2::new(
                    gear_cx + a2.cos() * outer_r,
                    gear_cy + a2.sin() * outer_r,
                ));
                gear_points.push(Pos2::new(
                    gear_cx + a3.cos() * hub_r,
                    gear_cy + a3.sin() * hub_r,
                ));
            }
            painter.add(egui::epaint::PathShape::closed_line(gear_points, s));
            // 内圆孔
            painter.circle_stroke(Pos2::new(gear_cx, gear_cy), inner_r, s);
        }
    }
}
