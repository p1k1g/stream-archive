//! Read-only Diagnostics & Runtime Logs view.

use crate::{
    maintenance_adapter::log_matches_filter, state::MaintenanceSection, theme::*, ui::MainView,
};
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

pub fn render_diagnostics_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let is_logs = state.maintenance_section == MaintenanceSection::Logs;

    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    let switcher = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            filter_pill("diag-tab-checks", "진단 점검", !is_logs).on_click(cx.listener(
                |this, _, _, cx| {
                    this.controller.update(cx, |c, _| {
                        c.set_maintenance_section(MaintenanceSection::Diagnostics)
                    });
                    cx.notify();
                },
            )),
        )
        .child(
            filter_pill("diag-tab-logs", "런타임 로그", is_logs).on_click(cx.listener(
                |this, _, _, cx| {
                    this.controller.update(cx, |c, _| {
                        c.set_maintenance_section(MaintenanceSection::Logs)
                    });
                    cx.notify();
                },
            )),
        );

    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                if is_logs {
                    "런타임 로그"
                } else {
                    "시스템 · 외부 도구 진단"
                },
                if is_logs {
                    "공유 메모리 로그 버퍼(최근 200줄)와 일별 로그 파일 폴더를 확인합니다."
                } else {
                    "Streamlink · yt-dlp · FFmpeg 탐색 상태, DB 무결성, 인증 설정 여부를 읽기 전용으로 점검합니다."
                },
            ))
            .child(switcher),
    );

    if !state.maintenance_message.is_empty() {
        page = page.child(notice_banner(&state.maintenance_message));
    }

    if is_logs {
        let visible_rows: Vec<&crate::state::MaintenanceLogRow> = state
            .log_rows
            .iter()
            .filter(|line| {
                log_matches_filter(
                    &line.text,
                    &line.level,
                    &state.log_level_filter,
                    &state.log_search,
                )
            })
            .collect();
        let visible_count = visible_rows.len();
        let total_count = state.log_rows.len();
        let copy_payload = visible_rows
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        let toolbar = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                Switch::new("log-auto-refresh")
                    .checked(state.log_auto_refresh)
                    .label("1.5초 자동 새로고침")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller
                            .update(cx, |c, _| c.toggle_log_auto_refresh());
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        Button::new("logs-copy-btn")
                            .outline()
                            .small()
                            .disabled(visible_count == 0)
                            .label("로그 복사")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if copy_payload.is_empty() {
                                    this.controller.update(cx, |c, _| {
                                        c.set_maintenance_message("복사할 로그가 없습니다.".into())
                                    });
                                } else {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_payload.clone(),
                                    ));
                                    this.controller.update(cx, |c, _| {
                                        c.set_maintenance_message(format!(
                                            "표시 중인 로그 {visible_count}줄을 클립보드에 복사했습니다."
                                        ))
                                    });
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("logs-refresh")
                            .outline()
                            .small()
                            .label("새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.refresh_logs());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("logs-open-dir")
                            .outline()
                            .small()
                            .label("로그 폴더 열기")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.open_logs_folder());
                            })),
                    )
                    .child(
                        Button::new("logs-clear")
                            .danger()
                            .small()
                            .label("버퍼 비우기")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.clear_logs());
                                cx.notify();
                            })),
                    ),
            );

        let mut level_pills = div().flex().items_center().gap(px(4.0));
        for (idx, (key, label)) in [
            ("ALL", "전체"),
            ("WARN_ERR", "경고·오류"),
            ("ERR", "오류만"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = state.log_level_filter == key;
            let k = key.to_string();
            level_pills = level_pills.child(filter_pill(("log-lvl", idx), label, active).on_click(
                cx.listener(move |this, _, _, cx| {
                    let f = k.clone();
                    this.controller
                        .update(cx, |c, _| c.set_log_level_filter(&f));
                    cx.notify();
                }),
            ));
        }

        let filter_bar = surface_card()
            .p(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(level_pills)
            .child(
                div()
                    .flex_1()
                    .max_w(px(360.0))
                    .child(Input::new(&view.log_search_input).small()),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(TEXT_MUTED))
                    .child(format!("{visible_count} / {total_count}줄")),
            );

        let mut console = surface_card()
            .p(px(12.0))
            .bg(rgb(BG_CHROME))
            .flex()
            .flex_col()
            .gap(px(3.0));
        if state.log_rows.is_empty() {
            console = console.child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT_MUTED))
                    .child("표시할 런타임 로그가 없습니다."),
            );
        } else if visible_rows.is_empty() {
            console = console.child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT_MUTED))
                    .child("선택한 레벨 필터 또는 검색어와 일치하는 로그가 없습니다."),
            );
        } else {
            for line in visible_rows {
                let mut row_el = div()
                    .px(px(6.0))
                    .py(px(1.5))
                    .rounded(px(3.0))
                    .font_family("Consolas")
                    .text_size(px(11.5))
                    .child(line.text.clone());
                row_el = match line.level.as_str() {
                    "error" => row_el.text_color(rgb(ERR_FG)).bg(rgba(0x331c2466)),
                    "warn" => row_el.text_color(rgb(WARN_FG)).bg(rgba(0x31281966)),
                    _ => row_el.text_color(rgb(TEXT_SECONDARY)),
                };
                console = console.child(row_el);
            }
        }
        return page.child(toolbar).child(filter_bar).child(console);
    }

    // Diagnostics checks & version update toolbar
    page = page.child(
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
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(format!("현재 버전 {}", state.app_version)),
                    )
                    .when(!state.update_status_message.is_empty(), |el| {
                        el.child(status_pill(
                            &state.update_status_message,
                            if state.update_available {
                                "accent"
                            } else {
                                "muted"
                            },
                            state.update_available,
                        ))
                    })
                    .when(state.update_available, |el| {
                        el.child(
                            Button::new("diag-open-release-btn")
                                .outline()
                                .xsmall()
                                .label("릴리스 열기")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.open_release_url());
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        Button::new("diag-update-check-btn")
                            .outline()
                            .small()
                            .disabled(state.update_checking)
                            .label(if state.update_checking {
                                "확인 중..."
                            } else {
                                "업데이트 확인"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.check_updates_manual());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("diag-refresh-btn")
                            .outline()
                            .small()
                            .disabled(state.maintenance_busy)
                            .label("상태 새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.refresh_maintenance(false));
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("diag-active-probe-btn")
                            .primary()
                            .small()
                            .disabled(state.maintenance_busy)
                            .label("도구 실행 버전 확인")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller
                                    .update(cx, |c, _| c.refresh_maintenance(true));
                                cx.notify();
                            })),
                    ),
            ),
    );

    let mut list = surface_card().overflow_hidden().flex().flex_col();
    if state.maintenance_diagnostic_rows.is_empty() {
        list = list.child(
            div()
                .p(px(20.0))
                .text_center()
                .text_size(px(12.0))
                .text_color(rgb(TEXT_MUTED))
                .child("진단 정보를 불러오는 중..."),
        );
    } else {
        for row in &state.maintenance_diagnostic_rows {
            list = list.child(
                div()
                    .px(px(12.0))
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(rgb(0x1f252e))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(status_pill(&row.status, &row.status_tone, false))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(TEXT_PRIMARY))
                                            .child(row.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(rgb(TEXT_MUTED))
                                            .child(format!(
                                                "{} · {}",
                                                row.category, row.requirement
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(rgb(TEXT_SECONDARY))
                                    .child(row.summary.clone()),
                            ),
                    )
                    .when(!row.detail.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(TEXT_MUTED))
                                .child(row.detail.clone()),
                        )
                    })
                    .when(!row.remediation.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(WARN_FG))
                                .child(format!("권장 조치: {}", row.remediation)),
                        )
                    }),
            );
        }
    }

    page.child(list)
}
