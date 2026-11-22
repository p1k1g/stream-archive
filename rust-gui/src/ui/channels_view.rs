//! Channels configuration view: compact single-line table for SOOP, CHZZK, and KICK channels.

use crate::{theme::*, ui::MainView};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::Input,
        switch::Switch,
    },
    prelude::FluentBuilder as _,
    *,
};

fn platform_segment_colors(platform: &str, active: bool) -> (u32, u32, u32) {
    if !active {
        return (BG_CHROME, TEXT_MUTED, BORDER);
    }
    match platform {
        "CHZZK" => (0x102d24, 0x00ffa3, 0x1e5c47),
        "KICK" => (0x1e2e14, 0xa3e635, 0x3b5c22),
        _ => (0x14253b, 0x6cb2ff, 0x264873),
    }
}

pub fn render_channels_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();

    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                "채널 관리",
                "SOOP · CHZZK · KICK 방송 감시 대상과 채널별 저장 폴더를 설정합니다. 저장 시 canonical SQLite에 반영됩니다.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(state.channels_dirty, |el| {
                        el.child(status_pill("변경됨", "warn", true))
                    })
                    .child(
                        Button::new("ch-add-btn")
                            .outline()
                            .small()
                            .disabled(state.config_busy)
                            .label("+ 채널 추가")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.channels_search_input
                                    .update(cx, |s, cx| s.set_value("", window, cx));
                                this.controller.update(cx, |c, _| c.channels_add());
                                this.sync_inputs_from_controller(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ch-reload-btn")
                            .ghost()
                            .small()
                            .disabled(state.config_busy)
                            .label("되돌리기")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.reload_config());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("ch-save-btn")
                            .primary()
                            .small()
                            .disabled(state.config_busy || !state.channels_dirty)
                            .label("저장")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.channels_save());
                                cx.notify();
                            })),
                    ),
            ),
    );

    if !state.config_message.is_empty() {
        page = page.child(notice_banner(&state.config_message));
    }

    if state.channels_rows.is_empty() {
        page = page.child(
            surface_card()
                .p(px(28.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_SECONDARY))
                        .child("등록된 채널이 없습니다."),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(TEXT_MUTED))
                        .child("상단의 '+ 채널 추가' 버튼을 눌러 감시할 채널을 추가하세요."),
                ),
        );
        return page;
    }

    // Platform filter pills & search input
    let total_count = state.channels_rows.len();
    let soop_count = state
        .channels_rows
        .iter()
        .filter(|r| r.platform == "SOOP")
        .count();
    let chzzk_count = state
        .channels_rows
        .iter()
        .filter(|r| r.platform == "CHZZK")
        .count();
    let kick_count = state
        .channels_rows
        .iter()
        .filter(|r| r.platform == "KICK")
        .count();

    let needle = state.channels_search.trim().to_ascii_lowercase();
    let matching_indices: Vec<usize> = state
        .channels_rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            let platform_ok = state.channels_platform_filter == "ALL"
                || row.platform == state.channels_platform_filter;
            let search_ok = needle.is_empty()
                || row.account.to_ascii_lowercase().contains(&needle)
                || row.name.to_ascii_lowercase().contains(&needle)
                || row.outdir.to_ascii_lowercase().contains(&needle)
                || row.platform.to_ascii_lowercase().contains(&needle);
            platform_ok && search_ok
        })
        .map(|(idx, _)| idx)
        .collect();

    let mut plat_filters = div().flex().items_center().flex_wrap().gap(px(6.0));
    for (f_idx, (key, label)) in [
        ("ALL", format!("전체 ({total_count})")),
        ("SOOP", format!("SOOP ({soop_count})")),
        ("CHZZK", format!("CHZZK ({chzzk_count})")),
        ("KICK", format!("KICK ({kick_count})")),
    ]
    .into_iter()
    .enumerate()
    {
        let active = state.channels_platform_filter == key;
        let key_owned = key.to_string();
        plat_filters = plat_filters.child(
            filter_pill(("ch-plat-filter", f_idx), &label, active).on_click(cx.listener(
                move |this, _, _, cx| {
                    let k = key_owned.clone();
                    this.controller
                        .update(cx, |c, _| c.set_channels_platform_filter(&k));
                    cx.notify();
                },
            )),
        );
    }

    page =
        page.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(8.0))
                .child(plat_filters)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .when(matching_indices.len() != total_count, |el| {
                            el.child(div().text_size(px(11.0)).text_color(rgb(TEXT_MUTED)).child(
                                format!("표시 {} / {}", matching_indices.len(), total_count),
                            ))
                        })
                        .child(
                            div()
                                .w(px(240.0))
                                .child(Input::new(&view.channels_search_input).xsmall()),
                        ),
                ),
        );

    let mut table = surface_card().overflow_hidden().flex().flex_col();

    // Compact 1-line table header
    table = table.child(
        div()
            .px(px(10.0))
            .py(px(7.0))
            .bg(rgb(BG_HEADER))
            .border_b_1()
            .border_color(rgb(BORDER))
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(TEXT_MUTED))
            .child(
                div()
                    .w(px(34.0))
                    .flex_shrink_0()
                    .text_center()
                    .child("감시"),
            )
            .child(div().w(px(124.0)).flex_shrink_0().child("플랫폼"))
            .child(div().flex_1().min_w(px(140.0)).child("채널 ID / 계정"))
            .child(div().w(px(132.0)).flex_shrink_0().child("표시 이름"))
            .child(
                div()
                    .flex_1()
                    .min_w(px(120.0))
                    .child("저장 폴더 (비워두면 기본)"),
            )
            .child(
                div()
                    .w(px(138.0))
                    .flex_shrink_0()
                    .text_right()
                    .child(format!("작업 ({}개)", matching_indices.len())),
            ),
    );

    if matching_indices.is_empty() {
        table = table.child(
            div()
                .p(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .text_color(rgb(TEXT_MUTED))
                .child("검색어 또는 플랫폼 필터와 일치하는 채널이 없습니다."),
        );
    }

    for idx in matching_indices {
        let Some(row) = state.channels_rows.get(idx) else {
            continue;
        };
        let Some(inputs) = view.channel_inputs.get(idx) else {
            continue;
        };

        let mut plat_bar = div()
            .w(px(124.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(3.0));
        for (p_idx, plat) in ["SOOP", "CHZZK", "KICK"].into_iter().enumerate() {
            let active = row.platform == plat;
            let (bg, fg, border) = platform_segment_colors(plat, active);
            let p_owned = plat.to_string();
            let disabled = state.config_busy;
            plat_bar = plat_bar.child(
                div()
                    .id(("ch-plat", idx * 4 + p_idx))
                    .px(px(5.0))
                    .py(px(2.5))
                    .rounded(px(4.0))
                    .bg(rgb(bg))
                    .border_1()
                    .border_color(rgb(border))
                    .text_color(rgb(fg))
                    .text_size(px(10.0))
                    .font_weight(if active {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .when(!disabled, |el| {
                        el.cursor_pointer()
                            .hover(|s| {
                                if active {
                                    s
                                } else {
                                    s.bg(rgb(BG_RAISED)).text_color(rgb(TEXT_PRIMARY))
                                }
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let p = p_owned.clone();
                                this.controller
                                    .update(cx, |c, _| c.channels_select_platform(idx, &p));
                                cx.notify();
                            }))
                    })
                    .child(plat),
            );
        }

        let line = div()
            .px(px(10.0))
            .py(px(5.5))
            .border_b_1()
            .border_color(rgb(0x1f252e))
            .hover(|s| s.bg(rgb(0x1a2029)))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .w(px(34.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Switch::new(("ch-enabled", idx))
                            .small()
                            .disabled(state.config_busy)
                            .checked(row.enabled)
                            .on_click(cx.listener(move |this, checked, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.channels_set_enabled(idx, *checked));
                                cx.notify();
                            })),
                    ),
            )
            .child(plat_bar)
            .child(
                div().flex_1().min_w(px(140.0)).child(
                    Input::new(&inputs.account)
                        .xsmall()
                        .disabled(state.config_busy),
                ),
            )
            .child(
                div().w(px(132.0)).flex_shrink_0().child(
                    Input::new(&inputs.name)
                        .xsmall()
                        .disabled(state.config_busy),
                ),
            )
            .child(
                div().flex_1().min_w(px(120.0)).child(
                    Input::new(&inputs.outdir)
                        .xsmall()
                        .disabled(state.config_busy),
                ),
            )
            .child(
                div()
                    .w(px(138.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(4.0))
                    .child(
                        Button::new(("ch-pick", idx))
                            .outline()
                            .xsmall()
                            .disabled(state.config_busy)
                            .label("폴더")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.channels_pick_output(idx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("ch-resolve", idx))
                            .ghost()
                            .xsmall()
                            .disabled(state.config_busy)
                            .label("조회")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.channels_resolve(idx));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("ch-remove", idx))
                            .danger()
                            .xsmall()
                            .disabled(state.config_busy)
                            .label("삭제")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.controller.update(cx, |c, _| c.channels_remove(idx));
                                this.sync_inputs_from_controller(window, cx);
                                cx.notify();
                            })),
                    ),
            );

        table = table.child(line);
    }

    page.child(table)
}
