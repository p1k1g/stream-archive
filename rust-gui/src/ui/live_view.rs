//! LIVE monitoring workspace view (Zed-inspired crisp density + BGRA RenderImage previews).

use crate::{
    state::LiveChannelRow,
    theme::{self, *},
    ui::MainView,
};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::Input,
    },
    *,
};

pub fn render_live_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let (state, rows) = {
        let ctrl = view.controller.read(cx);
        (ctrl.state.clone(), ctrl.live_model.rows.clone())
    };

    let watcher_btn = if state.live_running {
        Button::new("live-stop-btn")
            .danger()
            .small()
            .disabled(state.live_busy || !state.runtime_ready)
            .label("방송 감시 중지")
            .on_click(cx.listener(|this, _, _, cx| {
                this.controller.update(cx, |c, _| c.live_stop());
                cx.notify();
            }))
    } else {
        Button::new("live-start-btn")
            .primary()
            .small()
            .disabled(state.live_busy || !state.runtime_ready)
            .label("방송 감시 시작")
            .on_click(cx.listener(|this, _, _, cx| {
                this.controller.update(cx, |c, _| c.live_start());
                cx.notify();
            }))
    };

    let refresh_btn = Button::new("live-refresh-btn")
        .outline()
        .small()
        .disabled(state.live_busy)
        .label("새로고침")
        .on_click(cx.listener(|this, _, _, cx| {
            this.controller.update(cx, |c, _| c.live_refresh());
            cx.notify();
        }));

    let filters = [
        ("ALL", format!("전체 ({})", state.live_channel_count)),
        (
            "RECORDING",
            format!("녹화 중 ({})", state.live_recording_count),
        ),
        (
            "OFFLINE",
            format!("오프라인 ({})", state.live_offline_count),
        ),
        ("ERROR", format!("오류 ({})", state.live_error_count)),
    ];

    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    // Header bar
    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                "LIVE 자동 녹화 감시",
                &format!(
                    "엔진: {} · 시작: {} · 감시 상태: {}",
                    state.live_engine, state.live_started_at, state.live_state_label
                ),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(status_pill(
                        &state.live_state_label,
                        if state.live_running { "ok" } else { "muted" },
                        true,
                    ))
                    .child(refresh_btn)
                    .child(watcher_btn),
            ),
    );

    // Storage volumes strip
    if !state.storage_rows.is_empty() {
        let mut strip = div().flex().flex_wrap().gap(px(8.0));
        for (idx, vol) in state.storage_rows.iter().enumerate() {
            let (fg, _, _) = tone_colors(&vol.status_tone);
            strip = strip.child(
                surface_card()
                    .id(("storage-vol", idx))
                    .px(px(10.0))
                    .py(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(vol.volume.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(vol.capacity.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(fg))
                            .child(format!("{} ({})", vol.status, vol.used)),
                    ),
            );
        }
        page = page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div().flex().items_center().justify_between().child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_MUTED))
                            .child(format!(
                                "저장공간 상태 · 최소 여유 기준 {}",
                                state.storage_threshold
                            )),
                    ),
                )
                .child(strip),
        );
    }

    // Filter, sort & search bar
    let mut filter_row = div().flex().items_center().flex_wrap().gap(px(6.0));
    for (idx, (key, label)) in filters.into_iter().enumerate() {
        let active = state.live_filter == key;
        let key_owned = key.to_string();
        filter_row = filter_row.child(filter_pill(("live-filter", idx), &label, active).on_click(
            cx.listener(move |this, _, _, cx| {
                let k = key_owned.clone();
                this.controller.update(cx, |c, _| c.set_live_filter(&k));
                cx.notify();
            }),
        ));
    }

    let mut sort_row = div().flex().items_center().gap(px(4.0));
    for (s_idx, (mode, label)) in [("STATUS", "상태순"), ("ORDER", "이름순")]
        .into_iter()
        .enumerate()
    {
        let active = state.live_sort_mode == mode;
        let mode_owned = mode.to_string();
        sort_row = sort_row.child(filter_pill(("live-sort", s_idx), label, active).on_click(
            cx.listener(move |this, _, _, cx| {
                let m = mode_owned.clone();
                this.controller.update(cx, |c, _| c.set_live_sort_mode(&m));
                cx.notify();
            }),
        ));
    }

    page = page.child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(filter_row)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(220.0))
                                    .child(Input::new(&view.live_search_input).xsmall()),
                            )
                            .child(sort_row),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(rgb(TEXT_MUTED))
                    .child(state.live_message.clone()),
            ),
    );

    // Channel rows container
    if rows.is_empty() {
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
                            .child(if state.live_loaded {
                                "표시할 LIVE 채널이 없습니다."
                            } else {
                                "LIVE 채널 상태를 불러오는 중..."
                            }),
                    )
                    .child(div().text_size(px(11.5)).text_color(rgb(TEXT_MUTED)).child(
                        "채널 탭에서 SOOP / CHZZK / KICK 채널을 등록하거나 필터를 변경하세요.",
                    )),
            );
    } else {
        let mut table = surface_card().overflow_hidden().flex().flex_col();
        table = table.child(
            div()
                .px(px(12.0))
                .py(px(7.0))
                .bg(rgb(BG_HEADER))
                .border_b_1()
                .border_color(rgb(BORDER))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_MUTED))
                .child("채널 · 방송 제목 · 녹화 파일")
                .child("상태 · 작업"),
        );

        for (idx, row) in rows.iter().enumerate() {
            table = table.child(render_live_row(view, idx, row, state.live_busy, cx));
        }
        page = page.child(table);
    }

    page
}

fn render_live_row(
    view: &MainView,
    idx: usize,
    row: &LiveChannelRow,
    live_busy: bool,
    cx: &mut Context<MainView>,
) -> Div {
    let target_stop = row.target.clone();
    let target_resume = row.target.clone();
    let target_recheck = row.target.clone();
    let target_pw = row.target.clone();
    let name_pw = row.name.clone();
    let stream_platform = row.platform.clone();
    let stream_account = row.account.clone();
    let play_file = row.file.clone();
    let folder_file = if !row.file.is_empty() {
        row.file.clone()
    } else {
        row.output_dir.clone()
    };

    let thumb = if let Some(image) = row.thumbnail_image.clone() {
        div()
            .w(px(112.0))
            .h(px(63.0))
            .rounded(px(5.0))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .bg(rgb(BG_CHROME))
            .flex_shrink_0()
            .child(img(image).w_full().h_full().object_fit(ObjectFit::Cover))
    } else if let Some(logo) = view.platform_icon(&row.platform) {
        div()
            .w(px(112.0))
            .h(px(63.0))
            .rounded(px(5.0))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BG_CHROME))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(34.0))
                    .rounded(px(6.0))
                    .overflow_hidden()
                    .child(img(logo).w_full().h_full().object_fit(ObjectFit::Contain)),
            )
    } else {
        div()
            .w(px(112.0))
            .h(px(63.0))
            .rounded(px(5.0))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BG_CHROME))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(platform_badge(&row.platform))
    };

    let mut meta_parts = Vec::new();
    if !row.size.is_empty() {
        meta_parts.push(row.size.clone());
    }
    if !row.started_at.is_empty() {
        meta_parts.push(format!("시작 {}", row.started_at));
    }
    if !row.detail.is_empty() {
        meta_parts.push(row.detail.clone());
    }

    let mut actions = div().flex().items_center().gap(px(6.0)).flex_shrink_0();
    if !stream_account.is_empty() {
        actions = actions.child(
            Button::new(("live-watch", idx))
                .ghost()
                .xsmall()
                .label("방송 보기")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let plat = stream_platform.clone();
                    let acc = stream_account.clone();
                    this.controller
                        .update(cx, |c, _| c.live_open_stream(plat, acc));
                })),
        );
    }
    if !play_file.is_empty() {
        actions = actions.child(
            Button::new(("live-play", idx))
                .primary()
                .xsmall()
                .label("영상 재생")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let path = play_file.clone();
                    this.controller.update(cx, |c, _| c.open_media_file(path));
                })),
        );
    }
    if !folder_file.is_empty() {
        actions = actions.child(
            Button::new(("live-folder", idx))
                .outline()
                .xsmall()
                .label("폴더 열기")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let path = folder_file.clone();
                    this.controller.update(cx, |c, _| c.live_open_folder(path));
                })),
        );
    }
    if row.password_required {
        actions = actions.child(
            Button::new(("live-pw", idx))
                .warning()
                .xsmall()
                .disabled(live_busy)
                .label("비밀번호")
                .on_click(cx.listener(move |this, _, window, cx| {
                    let t = target_pw.clone();
                    let n = name_pw.clone();
                    this.open_password_modal(t, n, window, cx);
                })),
        );
    }
    if row.can_stop_once {
        actions = actions.child(
            Button::new(("live-stop-once", idx))
                .danger()
                .xsmall()
                .disabled(live_busy)
                .label("이번 방송 중지")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let t = target_stop.clone();
                    this.controller
                        .update(cx, |c, _| c.live_action(t, "stop".into()));
                    cx.notify();
                })),
        );
    }
    if row.can_resume {
        actions = actions.child(
            Button::new(("live-resume", idx))
                .primary()
                .xsmall()
                .disabled(live_busy)
                .label("감시 재개")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let t = target_resume.clone();
                    this.controller
                        .update(cx, |c, _| c.live_action(t, "resume".into()));
                    cx.notify();
                })),
        );
    }
    if row.can_recheck {
        actions = actions.child(
            Button::new(("live-recheck", idx))
                .ghost()
                .xsmall()
                .disabled(live_busy)
                .label("재확인")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let t = target_recheck.clone();
                    this.controller
                        .update(cx, |c, _| c.live_action(t, "recheck".into()));
                    cx.notify();
                })),
        );
    }

    let mut header_line = div()
        .flex()
        .items_center()
        .flex_wrap()
        .gap(px(6.0))
        .child(platform_badge(&row.platform))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(if row.name.is_empty() {
                    row.account.clone()
                } else {
                    row.name.clone()
                }),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(TEXT_MUTED))
                .child(row.account.clone()),
        )
        .child(theme::status_pill(
            &row.status_label,
            &row.status_tone,
            row.status == "RECORDING",
        ));

    if !row.elapsed.is_empty() {
        header_line = header_line.child(
            div()
                .px(px(6.0))
                .py(px(1.5))
                .rounded(px(4.0))
                .bg(rgb(BG_RAISED))
                .border_1()
                .border_color(rgb(BORDER))
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT_PRIMARY))
                .child(row.elapsed.clone()),
        );
    }
    if !row.speed.is_empty() {
        header_line = header_line.child(
            div()
                .px(px(6.0))
                .py(px(1.5))
                .rounded(px(4.0))
                .bg(rgb(OK_BG))
                .border_1()
                .border_color(rgb(OK_BORDER))
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(OK_FG))
                .child(row.speed.clone()),
        );
    }

    let mut info = div().flex().flex_col().gap(px(3.0)).flex_1().min_w_0();
    info = info.child(header_line);

    if !row.title.is_empty() {
        info = info.child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(TEXT_SECONDARY))
                .truncate()
                .child(row.title.clone()),
        );
    }

    if !meta_parts.is_empty() {
        info = info.child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(TEXT_MUTED))
                .truncate()
                .child(meta_parts.join(" · ")),
        );
    }

    if !row.file.is_empty() {
        info = info.child(
            div()
                .text_size(px(10.5))
                .text_color(rgb(0x6e8094))
                .truncate()
                .child(row.file.clone()),
        );
    }

    div()
        .px(px(12.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(rgb(0x1f252e))
        .hover(|s| s.bg(rgb(0x1b212a)))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .flex_1()
                .min_w_0()
                .child(thumb)
                .child(info),
        )
        .child(actions)
}
