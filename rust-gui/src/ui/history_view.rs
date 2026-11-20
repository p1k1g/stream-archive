//! History archive view: ALL/LIVE/VOD filters, search, status, 6-week Korean calendar, open folder.

use crate::{theme::*, ui::MainView};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::Input,
    },
    prelude::FluentBuilder as _,
    *,
};

pub fn render_history_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    // Header + view switcher
    let mut view_bar = div().flex().items_center().gap(px(4.0));
    for (idx, (key, label)) in [("ALL", "전체"), ("LIVE", "LIVE"), ("VOD", "VOD")]
        .into_iter()
        .enumerate()
    {
        let active = state.history_view == key;
        let k = key.to_string();
        view_bar = view_bar.child(filter_pill(("hist-view", idx), label, active).on_click(
            cx.listener(move |this, _, _, cx| {
                let v = k.clone();
                this.controller.update(cx, |c, _| c.history_set_view(&v));
                cx.notify();
            }),
        ));
    }

    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                "LIVE · VOD 기록",
                "녹화 및 VOD 다운로드 이력을 기간·상태·키워드로 검색합니다.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(view_bar)
                    .child(
                        Button::new("hist-refresh-btn")
                            .primary()
                            .small()
                            .disabled(state.history_busy)
                            .label("조회")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.refresh_history());
                                cx.notify();
                            })),
                    ),
            ),
    );

    // Search + Status + Date + Limit filter bar
    let mut status_bar = div().flex().items_center().gap(px(4.0));
    for (idx, (key, label)) in [
        ("", "모든 상태"),
        ("COMPLETED", "완료"),
        ("FAILED", "실패"),
        ("CANCELLED", "취소"),
    ]
    .into_iter()
    .enumerate()
    {
        let active = state.history_status == key;
        let k = key.to_string();
        status_bar = status_bar.child(filter_pill(("hist-status", idx), label, active).on_click(
            cx.listener(move |this, _, _, cx| {
                let s = k.clone();
                this.controller.update(cx, |c, _| c.history_set_status(&s));
                cx.notify();
            }),
        ));
    }

    let mut limit_bar = div().flex().items_center().gap(px(4.0));
    for (idx, lim) in ["100", "300", "1000"].into_iter().enumerate() {
        let active = state.history_limit == lim;
        let l = lim.to_string();
        limit_bar = limit_bar.child(
            filter_pill(("hist-limit", idx), &format!("{lim}건"), active).on_click(cx.listener(
                move |this, _, _, cx| {
                    let s = l.clone();
                    this.controller.update(cx, |c, _| c.history_set_limit(&s));
                    cx.notify();
                },
            )),
        );
    }

    let from_label = if state.history_from_date.is_empty() {
        "시작일 선택".to_string()
    } else {
        format!("시작: {}", state.history_from_date)
    };
    let to_label = if state.history_to_date.is_empty() {
        "종료일 선택".to_string()
    } else {
        format!("종료: {}", state.history_to_date)
    };

    page = page.child(
        surface_card()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&view.history_search_input).small()),
                    )
                    .child(
                        Button::new("hist-from-btn")
                            .outline()
                            .small()
                            .label(from_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.history_calendar_open("from"));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("hist-to-btn")
                            .outline()
                            .small()
                            .label(to_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.history_calendar_open("to"));
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(status_bar)
                    .child(limit_bar),
            ),
    );

    // Inline 6-week Korean calendar panel
    if state.history_calendar_open {
        let target_label = if state.history_calendar_target == "to" {
            "종료일 선택"
        } else {
            "시작일 선택"
        };
        let mut cal_card = surface_card()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(ACCENT))
                                    .child(target_label),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child(state.history_calendar_label.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                Button::new("cal-prev")
                                    .outline()
                                    .xsmall()
                                    .label("이전 달")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.controller
                                            .update(cx, |c, _| c.history_calendar_shift(-1));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("cal-next")
                                    .outline()
                                    .xsmall()
                                    .label("다음 달")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.controller
                                            .update(cx, |c, _| c.history_calendar_shift(1));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("cal-clear")
                                    .ghost()
                                    .xsmall()
                                    .label("날짜 해제")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.controller
                                            .update(cx, |c, _| c.history_calendar_clear());
                                        cx.notify();
                                    })),
                            ),
                    ),
            );

        let mut weekday_row = div().grid().grid_cols(7).gap(px(4.0));
        for wd in ["일", "월", "화", "수", "목", "금", "토"] {
            weekday_row = weekday_row.child(
                div()
                    .text_center()
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT_MUTED))
                    .child(wd),
            );
        }
        cal_card = cal_card.child(weekday_row);

        let mut grid = div().grid().grid_cols(7).gap(px(4.0));
        for (idx, day) in state.history_calendar_days.iter().enumerate() {
            let date_str = day.date.clone();
            let (bg, fg, border) = if day.selected {
                (ACCENT, ACCENT_INK, ACCENT)
            } else if day.in_month {
                (BG_RAISED, TEXT_PRIMARY, BORDER)
            } else {
                (BG_CHROME, 0x4b5766, 0x1b2129)
            };
            grid = grid.child(
                div()
                    .id(("cal-day", idx))
                    .py(px(5.0))
                    .rounded(px(4.0))
                    .bg(rgb(bg))
                    .border_1()
                    .border_color(rgb(border))
                    .text_center()
                    .text_size(px(11.5))
                    .font_weight(if day.selected {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(rgb(fg))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let d = date_str.clone();
                        this.controller
                            .update(cx, |c, _| c.history_calendar_select(d));
                        cx.notify();
                    }))
                    .child(day.day.clone()),
            );
        }
        page = page.child(cal_card.child(grid));
    }

    if !state.history_message.is_empty() {
        page = page.child(notice_banner(&state.history_message));
    }

    if state.history_rows.is_empty() {
        page = page.child(
            surface_card()
                .p(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.5))
                .text_color(rgb(TEXT_MUTED))
                .child("조건에 해당하는 기록이 없습니다."),
        );
        return page;
    }

    let mut table = surface_card().overflow_hidden().flex().flex_col();
    for (idx, row) in state.history_rows.iter().enumerate() {
        let file = row.file.clone();
        table = table.child(
            div()
                .px(px(12.0))
                .py(px(9.0))
                .border_b_1()
                .border_color(rgb(0x1f252e))
                .hover(|s| s.bg(rgb(0x1b212a)))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .px(px(6.0))
                                        .py(px(1.5))
                                        .rounded(px(4.0))
                                        .bg(rgb(BG_RAISED))
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(TEXT_SECONDARY))
                                        .child(row.kind.clone()),
                                )
                                .child(platform_badge(&row.platform))
                                .child(status_pill(&row.state, &row.state_tone, false))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT_PRIMARY))
                                        .truncate()
                                        .child(row.title.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(TEXT_MUTED))
                                .truncate()
                                .child(format!("{} · {} · {}", row.subject, row.timing, row.meta)),
                        )
                        .when(!row.detail.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(TEXT_SECONDARY))
                                    .truncate()
                                    .child(row.detail.clone()),
                            )
                        }),
                )
                .when(!file.is_empty(), |el| {
                    el.child(
                        Button::new(("hist-open", idx))
                            .outline()
                            .xsmall()
                            .label("폴더 열기")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let p = file.clone();
                                this.controller.update(cx, |c, _| c.live_open_folder(p));
                            })),
                    )
                }),
        );
    }

    page.child(table)
}
