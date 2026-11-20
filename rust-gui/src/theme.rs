//! Zed-inspired crisp dark presentation tokens and reusable GPUI micro-components.

use gpui_kit::{
    component::theme::{Theme, ThemeMode},
    *,
};

pub const BG_CHROME: u32 = 0x101419;
pub const BG_CANVAS: u32 = 0x14181e;
pub const BG_SURFACE: u32 = 0x171c23;
pub const BG_HEADER: u32 = 0x13171d;
pub const BG_RAISED: u32 = 0x1d242e;
pub const BG_SELECTED: u32 = 0x183133;
pub const BORDER: u32 = 0x252c36;
pub const BORDER_STRONG: u32 = 0x323d4c;

pub const TEXT_PRIMARY: u32 = 0xeef3f8;
pub const TEXT_SECONDARY: u32 = 0xb7c4d3;
pub const TEXT_MUTED: u32 = 0x7f8fa3;

pub const ACCENT: u32 = 0x32d5bf;
pub const ACCENT_INK: u32 = 0x091c1c;

pub const OK_FG: u32 = 0x71ddba;
pub const OK_BG: u32 = 0x162e28;
pub const OK_BORDER: u32 = 0x245749;

pub const WARN_FG: u32 = 0xf3c36b;
pub const WARN_BG: u32 = 0x312819;
pub const WARN_BORDER: u32 = 0x5c4928;

pub const ERR_FG: u32 = 0xff8fa3;
pub const ERR_BG: u32 = 0x331c24;
pub const ERR_BORDER: u32 = 0x612c3c;

pub fn configure_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = "Malgun Gothic".into();
        theme.font_size = px(13.0);
        theme.mono_font_family = "Consolas".into();
        theme.mono_font_size = px(12.0);
        theme.radius = px(6.0);
        theme.radius_lg = px(8.0);
        theme.focus_ring = false;

        theme.colors.background = rgb(BG_CANVAS).into();
        theme.colors.foreground = rgb(TEXT_PRIMARY).into();
        theme.colors.title_bar = rgb(BG_CHROME).into();
        theme.colors.title_bar_border = rgb(BORDER).into();
        theme.colors.muted = rgb(BG_SURFACE).into();
        theme.colors.muted_foreground = rgb(TEXT_MUTED).into();
        theme.colors.border = rgb(BORDER).into();
        theme.colors.input = rgb(BORDER_STRONG).into();
        theme.colors.primary = rgb(ACCENT).into();
        theme.colors.primary_hover = rgb(0x4be0cb).into();
        theme.colors.primary_active = rgb(0x24baa5).into();
        theme.colors.primary_foreground = rgb(ACCENT_INK).into();
        theme.colors.secondary = rgb(BG_RAISED).into();
        theme.colors.secondary_hover = rgb(0x252e3a).into();
        theme.colors.secondary_foreground = rgb(TEXT_PRIMARY).into();
        theme.colors.accent = rgb(BG_SELECTED).into();
        theme.colors.accent_foreground = rgb(ACCENT).into();
        theme.colors.switch = rgb(0x283240).into();
        theme.colors.switch_thumb = rgb(0xf1f5f9).into();
        theme.colors.danger = rgb(0xd94b64).into();
        theme.colors.danger_foreground = rgb(0xffffff).into();
        theme.colors.ring = rgb(ACCENT).into();
    });
}

pub fn tone_colors(tone: &str) -> (u32, u32, u32) {
    match tone {
        "ok" => (OK_FG, OK_BG, OK_BORDER),
        "warn" => (WARN_FG, WARN_BG, WARN_BORDER),
        "error" => (ERR_FG, ERR_BG, ERR_BORDER),
        _ => (TEXT_SECONDARY, BG_RAISED, BORDER_STRONG),
    }
}

pub fn platform_badge(platform: &str) -> Div {
    let (fg, bg, border) = match platform {
        "CHZZK" => (0x00ffa3, 0x102d24, 0x1e5c47),
        "KICK" => (0xa3e635, 0x1e2e14, 0x3b5c22),
        _ => (0x6cb2ff, 0x14253b, 0x264873),
    };
    div()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(rgb(bg))
        .border_1()
        .border_color(rgb(border))
        .text_color(rgb(fg))
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .child(platform.to_string())
}

pub fn status_pill(label: &str, tone: &str, with_dot: bool) -> Div {
    let (fg, bg, border) = tone_colors(tone);
    let mut el = div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(12.0))
        .bg(rgb(bg))
        .border_1()
        .border_color(rgb(border))
        .text_color(rgb(fg))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM);
    if with_dot {
        el = el.child(div().size(px(6.0)).rounded_full().bg(rgb(fg)));
    }
    el.child(label.to_string())
}

pub fn metric_chip(label: &str, value: &str, value_color: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgb(BG_SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(TEXT_MUTED))
                .child(label.to_string()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(value_color))
                .child(value.to_string()),
        )
}

pub fn surface_card() -> Div {
    div()
        .rounded(px(8.0))
        .bg(rgb(BG_SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
}

pub fn filter_pill(id: impl Into<ElementId>, label: &str, active: bool) -> Stateful<Div> {
    let (bg, fg, border) = if active {
        (BG_SELECTED, ACCENT, 0x2a6661)
    } else {
        (BG_SURFACE, TEXT_MUTED, BORDER)
    };
    div()
        .id(id)
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgb(bg))
        .border_1()
        .border_color(rgb(border))
        .text_color(rgb(fg))
        .text_size(px(11.5))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .cursor_pointer()
        .hover(|s| s.bg(rgb(BG_RAISED)).text_color(rgb(TEXT_PRIMARY)))
        .child(label.to_string())
}

pub fn progress_bar(percent: f32, tone: &str) -> Div {
    let (fill, _, _) = tone_colors(tone);
    let clamped = percent.clamp(0.0, 100.0);
    let fill_color = if tone == "muted" { ACCENT } else { fill };
    div()
        .w_full()
        .h(px(6.0))
        .rounded(px(3.0))
        .bg(rgb(0x222a34))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .rounded(px(3.0))
                .bg(rgb(fill_color))
                .w(relative(clamped / 100.0)),
        )
}

pub fn section_header(title: &str, subtitle: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(TEXT_MUTED))
                .child(subtitle.to_string()),
        )
}

pub fn notice_banner(message: &str) -> Div {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(6.0))
        .bg(rgb(BG_HEADER))
        .border_1()
        .border_color(rgb(BORDER))
        .text_size(px(12.0))
        .text_color(rgb(TEXT_SECONDARY))
        .child(message.to_string())
}
