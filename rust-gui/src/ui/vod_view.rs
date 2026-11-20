//! VOD analysis and download view: URL analyze, thumbnail preview, quality/PART selection, queue enqueue.

use crate::{theme::*, ui::MainView};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        input::Input,
    },
    prelude::FluentBuilder as _,
    *,
};

pub fn render_vod_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    page = page.child(section_header(
        "VOD 분석 · 다운로드",
        "SOOP · CHZZK · KICK VOD URL을 분석해 화질과 PART를 선택한 뒤 바로 다운로드하거나 대기열에 추가합니다.",
    ));

    // URL input bar
    page = page.child(
        surface_card()
            .p(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&view.vod_url_input).small()),
            )
            .child(
                Button::new("vod-analyze-btn")
                    .primary()
                    .small()
                    .disabled(state.vod_busy || !state.vod_can_analyze)
                    .label("분석")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_analyze());
                        cx.notify();
                    })),
            ),
    );

    if !state.vod_message.is_empty() {
        page = page.child(notice_banner(&state.vod_message));
    }

    // Active job / analysis summary card
    let has_info = !state.vod_title.is_empty() || state.vod_running || state.vod_state != "IDLE";
    if has_info {
        let thumb = if let Some(image) = state.vod_thumbnail_image.clone() {
            div()
                .w(px(192.0))
                .h(px(108.0))
                .rounded(px(6.0))
                .overflow_hidden()
                .border_1()
                .border_color(rgb(BORDER_STRONG))
                .bg(rgb(BG_CHROME))
                .flex_shrink_0()
                .child(img(image).w_full().h_full().object_fit(ObjectFit::Cover))
        } else {
            div()
                .w(px(192.0))
                .h(px(108.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(rgb(BORDER))
                .bg(rgb(BG_CHROME))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(if state.vod_platform.is_empty() {
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(TEXT_MUTED))
                        .child("썸네일 없음")
                } else {
                    platform_badge(&state.vod_platform)
                })
        };

        let mut summary = surface_card().p(px(14.0)).flex().flex_col().gap(px(12.0));
        summary = summary.child(
            div().flex().items_start().gap(px(14.0)).child(thumb).child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .when(!state.vod_platform.is_empty(), |el| {
                                el.child(platform_badge(&state.vod_platform))
                            })
                            .child(status_pill(
                                &state.vod_state_label,
                                &state.vod_state_tone,
                                state.vod_running,
                            ))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(state.vod_part_progress.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(if state.vod_title.is_empty() {
                                "VOD 작업".to_string()
                            } else {
                                state.vod_title.clone()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(format!(
                                "{}{} · 길이 {} · 시작 {} · 완료 {}",
                                if state.vod_streamer.is_empty() {
                                    "-"
                                } else {
                                    &state.vod_streamer
                                },
                                if state.vod_streamer_id.is_empty() {
                                    String::new()
                                } else {
                                    format!(" ({})", state.vod_streamer_id)
                                },
                                state.vod_duration,
                                state.vod_started_at,
                                state.vod_finished_at,
                            )),
                    )
                    .when(!state.vod_status_message.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(rgb(TEXT_MUTED))
                                .child(state.vod_status_message.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .pt(px(4.0))
                            .child(
                                div()
                                    .flex_1()
                                    .child(progress_bar(state.vod_percent, &state.vod_state_tone)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(ACCENT))
                                    .child(state.vod_percent_label.clone()),
                            ),
                    ),
            ),
        );

        // Output folder + action buttons
        let mut action_bar = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .pt(px(8.0))
            .border_t_1()
            .border_color(rgb(BORDER));

        action_bar = action_bar.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .flex_1()
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&view.vod_output_input).small()),
                )
                .child(
                    Button::new("vod-pick-dir")
                        .outline()
                        .small()
                        .label("폴더 선택")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.vod_pick_output());
                        })),
                )
                .child(
                    Checkbox::new("vod-merge-cb")
                        .checked(state.vod_merge)
                        .label("PART 병합")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.vod_toggle_merge());
                            cx.notify();
                        })),
                ),
        );

        let mut btns = div().flex().items_center().gap(px(6.0));
        if !state.vod_output_file.is_empty() {
            let path = state.vod_output_file.clone();
            btns = btns.child(
                Button::new("vod-open-folder")
                    .ghost()
                    .small()
                    .label("폴더 열기")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let p = path.clone();
                        this.controller.update(cx, |c, _| c.live_open_folder(p));
                    })),
            );
        }
        if state.vod_can_cancel {
            btns = btns.child(
                Button::new("vod-cancel-btn")
                    .danger()
                    .small()
                    .disabled(state.vod_busy)
                    .label("취소")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_cancel());
                        cx.notify();
                    })),
            );
        }
        btns = btns
            .child(
                Button::new("vod-enqueue-btn")
                    .outline()
                    .small()
                    .disabled(!state.vod_can_enqueue || state.queue_busy)
                    .label("대기열 추가")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_enqueue());
                        cx.notify();
                    })),
            )
            .child(
                Button::new("vod-download-btn")
                    .primary()
                    .small()
                    .disabled(!state.vod_can_download || state.vod_busy)
                    .label("바로 다운로드")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_download());
                        cx.notify();
                    })),
            );

        summary = summary.child(action_bar.child(btns));
        page = page.child(summary);
    }

    // Quality selection
    if !state.vod_qualities.is_empty() {
        let mut q_row = div().flex().flex_wrap().gap(px(6.0));
        for (idx, q) in state.vod_qualities.iter().enumerate() {
            q_row = q_row.child(
                filter_pill(("vod-quality", idx), &q.label, q.selected).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_select_quality(idx));
                        cx.notify();
                    },
                )),
            );
        }
        page = page.child(
            surface_card()
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("화질 선택"),
                )
                .child(q_row),
        );
    }

    // PART selection
    if !state.vod_parts.is_empty() {
        let mut parts_box = surface_card().overflow_hidden().flex().flex_col();
        parts_box = parts_box.child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .bg(rgb(BG_HEADER))
                .border_b_1()
                .border_color(rgb(BORDER))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(format!("PART 선택 ({}개)", state.vod_parts.len())),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("vod-parts-all")
                                .ghost()
                                .xsmall()
                                .label("전체 선택")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.vod_select_all_parts());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("vod-parts-clear")
                                .ghost()
                                .xsmall()
                                .label("전체 해제")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.vod_clear_parts());
                                    cx.notify();
                                })),
                        ),
                ),
        );

        for (idx, part) in state.vod_parts.iter().enumerate() {
            parts_box = parts_box.child(
                div()
                    .id(("vod-part-row", idx))
                    .px(px(12.0))
                    .py(px(7.0))
                    .border_b_1()
                    .border_color(rgb(0x1f252e))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x1b212a)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.controller.update(cx, |c, _| c.vod_toggle_part(idx));
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(if part.selected {
                                        ACCENT
                                    } else {
                                        TEXT_MUTED
                                    }))
                                    .child(if part.selected { "[x]" } else { "[ ]" }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(if part.selected {
                                        TEXT_PRIMARY
                                    } else {
                                        TEXT_MUTED
                                    }))
                                    .child(format!("PART {}", part.part)),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_MUTED))
                            .child(part.duration.clone()),
                    ),
            );
        }
        page = page.child(parts_box);
    }

    page
}
