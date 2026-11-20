//! Persistent VOD Queue view: summary counters, expandable rows, progress bars, cancel/retry/remove.

use crate::{theme::*, ui::MainView};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
    },
    prelude::FluentBuilder as _,
    *,
};

pub fn render_queue_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                "VOD 대기열 (Queue)",
                "SQLite에 영구 보관되는 VOD 다운로드 대기열입니다. 항목을 클릭하면 상세 경로와 URL을 확인할 수 있습니다.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(metric_chip("진행", &state.queue_active_count, ACCENT))
                    .child(metric_chip("대기", &state.queue_queued_count, TEXT_PRIMARY))
                    .child(metric_chip("완료", &state.queue_completed_count, OK_FG))
                    .child(metric_chip("실패", &state.queue_failed_count, ERR_FG))
                    .child(
                        Button::new("queue-refresh-btn")
                            .outline()
                            .small()
                            .disabled(state.queue_busy)
                            .label("새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.queue_refresh());
                                cx.notify();
                            })),
                    ),
            ),
    );

    if !state.queue_message.is_empty() {
        page = page.child(notice_banner(&state.queue_message));
    }

    if state.queue_rows.is_empty() {
        page =
            page.child(
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
                            .child(if state.queue_loaded {
                                "대기열에 등록된 작업이 없습니다."
                            } else {
                                "대기열을 불러오는 중..."
                            }),
                    )
                    .child(div().text_size(px(11.5)).text_color(rgb(TEXT_MUTED)).child(
                        "VOD 분석 탭에서 영상을 분석한 뒤 '대기열 추가'를 눌러 등록하세요.",
                    )),
            );
        return page;
    }

    let mut list = surface_card().overflow_hidden().flex().flex_col();
    for (idx, row) in state.queue_rows.iter().enumerate() {
        let expanded = state.expanded_queue_id == row.id;
        let toggle_id = row.id.clone();
        let cancel_id = row.id.clone();
        let retry_id = row.id.clone();
        let remove_id = row.id.clone();
        let folder_path = if !row.output_file.is_empty() {
            row.output_file.clone()
        } else {
            row.output_directory.clone()
        };

        let mut actions = div().flex().items_center().gap(px(6.0)).flex_shrink_0();
        if !folder_path.is_empty() {
            actions = actions.child(
                Button::new(("q-folder", idx))
                    .ghost()
                    .xsmall()
                    .label("폴더")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let p = folder_path.clone();
                        this.controller.update(cx, |c, _| c.live_open_folder(p));
                    })),
            );
        }
        if row.can_cancel {
            actions = actions.child(
                Button::new(("q-cancel", idx))
                    .danger()
                    .xsmall()
                    .disabled(state.queue_busy)
                    .label("취소")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = cancel_id.clone();
                        this.controller
                            .update(cx, |c, _| c.queue_action(id, "cancel".into()));
                        cx.notify();
                    })),
            );
        }
        if row.can_retry {
            actions = actions.child(
                Button::new(("q-retry", idx))
                    .primary()
                    .xsmall()
                    .disabled(state.queue_busy)
                    .label("재시도")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = retry_id.clone();
                        this.controller
                            .update(cx, |c, _| c.queue_action(id, "retry".into()));
                        cx.notify();
                    })),
            );
        }
        if row.can_remove {
            actions = actions.child(
                Button::new(("q-remove", idx))
                    .outline()
                    .xsmall()
                    .disabled(state.queue_busy)
                    .label("삭제")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = remove_id.clone();
                        this.controller
                            .update(cx, |c, _| c.queue_action(id, "remove".into()));
                        cx.notify();
                    })),
            );
        }
        actions = actions.child(
            Button::new(("q-expand", idx))
                .ghost()
                .xsmall()
                .label(if expanded { "접기" } else { "상세" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let id = toggle_id.clone();
                    this.controller
                        .update(cx, |c, _| c.queue_toggle_expand(&id));
                    cx.notify();
                })),
        );

        let mut item_el = div()
            .px(px(12.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(rgb(0x1f252e))
            .hover(|s| s.bg(rgb(0x1b212a)))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .flex_1()
                            .min_w_0()
                            .child(platform_badge(&row.platform))
                            .child(status_pill(
                                &row.state_label,
                                &row.state_tone,
                                row.state == "RUNNING",
                            ))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .truncate()
                                    .child(row.title.clone()),
                            ),
                    )
                    .child(actions),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex_1()
                            .child(progress_bar(row.percent, &row.state_tone)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(format!("{} · {}", row.percent_label, row.part_progress)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!("시도 {}회", row.attempts)),
                    ),
            );

        if !row.message.is_empty() {
            item_el = item_el.child(
                div()
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_MUTED))
                    .child(row.message.clone()),
            );
        }

        if expanded {
            item_el = item_el.child(
                div()
                    .mt(px(4.0))
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(rgb(BG_CHROME))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT_MUTED))
                    .child(format!("작업 ID: {} · 스트리머: {}", row.id, row.streamer))
                    .child(format!("URL: {}", row.url))
                    .child(format!("저장 폴더: {}", row.output_directory))
                    .when(!row.output_file.is_empty(), |el| {
                        el.child(format!("출력 파일: {}", row.output_file))
                    })
                    .child(format!(
                        "등록: {} · 시작: {} · 완료: {}",
                        row.created_at, row.started_at, row.finished_at
                    )),
            );
        }

        list = list.child(item_el);
    }

    page.child(list)
}
