//! Settings view owning the internal General / Manage (Backup & Restore) boundary.
//! Phase 21.9: Settings > Manage owns backup policy and operations.

use crate::{state::SettingsView, theme::*, ui::MainView};
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

pub fn render_settings_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

    // Header + General / Manage sub-view switcher
    let sub_tabs = div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(
            filter_pill(
                "settings-tab-general",
                "일반 · 인증",
                state.settings_view == SettingsView::General,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.controller
                    .update(cx, |c, _| c.set_settings_view(SettingsView::General));
                cx.notify();
            })),
        )
        .child(
            filter_pill(
                "settings-tab-manage",
                "백업 · 복원",
                state.settings_view == SettingsView::Manage,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.controller
                    .update(cx, |c, _| c.set_settings_view(SettingsView::Manage));
                cx.notify();
            })),
        );

    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(section_header(
                if state.settings_view == SettingsView::General {
                    "설정 · 일반 및 서비스 인증"
                } else {
                    "설정 · 백업 정책 및 복원 관리"
                },
                "모든 설정과 비밀정보 참조는 canonical SQLite(data/stream-archive.db)에 저장됩니다.",
            ))
            .child(sub_tabs),
    );

    if state.settings_view == SettingsView::Manage {
        return page.child(render_backup_manage_section(view, cx));
    }

    // ---- General Settings ----
    if !state.settings_message.is_empty() {
        page = page.child(notice_banner(&state.settings_message));
    }

    let mut env_card = surface_card().p(px(14.0)).flex().flex_col().gap(px(10.0));
    env_card = env_card.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .pb(px(8.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(TEXT_PRIMARY))
                    .child("런타임 · 도구 · 데스크톱 동작"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(state.settings_dirty, |el| {
                        el.child(status_pill("변경됨", "warn", true))
                    })
                    .child(
                        Button::new("settings-refresh-btn")
                            .outline()
                            .small()
                            .disabled(state.settings_busy)
                            .label("새로고침")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.refresh_requested());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-reload-btn")
                            .ghost()
                            .small()
                            .disabled(state.settings_busy)
                            .label("되돌리기")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.settings_reload());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-save-btn")
                            .primary()
                            .small()
                            .disabled(state.settings_busy || !state.settings_dirty)
                            .label("설정 저장")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.settings_save());
                                cx.notify();
                            })),
                    ),
            ),
    );

    for (idx, row) in state.settings_rows.iter().enumerate() {
        if row.key == "STREAM_ARCHIVE_CLOSE_ACTION" {
            let mut pills = div().flex().items_center().gap(px(4.0));
            for (p_idx, (val, label)) in [
                ("EXIT", "프로그램 종료"),
                ("TRAY", "시스템 트레이로 이동"),
                ("ASK", "매번 물어보기"),
            ]
            .into_iter()
            .enumerate()
            {
                let active = row.value.eq_ignore_ascii_case(val);
                let v = val.to_string();
                pills = pills.child(filter_pill(("close-act", p_idx), label, active).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let val_copy = v.clone();
                        this.controller
                            .update(cx, |c, _| c.settings_edit(idx, val_copy));
                        cx.notify();
                    }),
                ));
            }
            env_card = env_card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .py(px(4.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child("창 닫기 동작 (STREAM_ARCHIVE_CLOSE_ACTION)"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(row.description.clone()),
                            ),
                    )
                    .child(pills),
            );
            continue;
        }

        if row.key == "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS" {
            let checked = !row.value.eq_ignore_ascii_case("false");
            env_card = env_card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .py(px(4.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child("VOD 다운로드 완료·실패 트레이 알림"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(TEXT_MUTED))
                                    .child(row.description.clone()),
                            ),
                    )
                    .child(
                        Switch::new("notif-switch")
                            .checked(checked)
                            .on_click(cx.listener(move |this, next, _, cx| {
                                let v = if *next { "true" } else { "false" }.to_string();
                                this.controller.update(cx, |c, _| c.settings_edit(idx, v));
                                cx.notify();
                            })),
                    ),
            );
            continue;
        }

        let Some(input_entity) = view.setting_inputs.get(&row.key) else {
            continue;
        };

        env_card = env_card.child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child(row.key.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(TEXT_MUTED))
                                .child(row.description.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().flex_1().child(Input::new(input_entity).small()))
                        .when(row.pickable, |el| {
                            el.child(
                                Button::new(("setting-pick", idx))
                                    .outline()
                                    .small()
                                    .disabled(state.settings_busy)
                                    .label("찾아보기")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.controller.update(cx, |c, _| c.settings_pick(idx));
                                    })),
                            )
                        }),
                ),
        );
    }

    page = page.child(env_card);

    // ---- Provider Credentials Section ----
    if !state.config_message.is_empty() {
        page = page.child(notice_banner(&state.config_message));
    }

    let prov_card = surface_card()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb(px(8.0))
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("서비스 연결 · 암호화 인증정보 (Windows DPAPI)"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("prov-test-soop")
                                .outline()
                                .small()
                                .disabled(state.config_busy)
                                .label("SOOP 연결 테스트")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.provider_test_soop());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("prov-save-btn")
                                .primary()
                                .small()
                                .disabled(state.config_busy)
                                .label("인증정보 저장")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_provider_inputs(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .grid()
                .grid_cols(2)
                .gap(px(10.0))
                .child(labeled_input(
                    "SOOP 사용자명",
                    None,
                    Input::new(&view.prov_soop_user).small(),
                ))
                .child(labeled_input(
                    "SOOP 비밀번호",
                    Some(state.provider_soop_password_set),
                    Input::new(&view.prov_soop_pass).small().mask_toggle(),
                ))
                .child(labeled_input(
                    "Cloudflare Worker URL (/soop/url 포함)",
                    None,
                    Input::new(&view.prov_worker_url).small(),
                ))
                .child(labeled_input(
                    "Cloudflare Worker API Key (API_SECRET)",
                    Some(state.provider_worker_api_key_set),
                    Input::new(&view.prov_worker_key).small().mask_toggle(),
                ))
                .child(labeled_input(
                    "CHZZK NID_AUT",
                    Some(state.provider_chzzk_nid_aut_set),
                    Input::new(&view.prov_chzzk_aut).small().mask_toggle(),
                ))
                .child(labeled_input(
                    "CHZZK NID_SES",
                    Some(state.provider_chzzk_nid_ses_set),
                    Input::new(&view.prov_chzzk_ses).small().mask_toggle(),
                ))
                .child(labeled_input(
                    "KICK session_token (구독 VOD)",
                    Some(state.provider_kick_token_set),
                    Input::new(&view.prov_kick_token).small().mask_toggle(),
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pt(px(6.0))
                .border_t_1()
                .border_color(rgb(BORDER))
                .child(
                    Button::new("clear-soop-pw")
                        .danger()
                        .xsmall()
                        .disabled(state.config_busy)
                        .label("SOOP 비밀번호 삭제")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.provider_clear("SOOP"));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("clear-chzzk-auth")
                        .danger()
                        .xsmall()
                        .disabled(state.config_busy)
                        .label("CHZZK 인증정보 삭제")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.provider_clear("CHZZK"));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("clear-kick-auth")
                        .danger()
                        .xsmall()
                        .disabled(state.config_busy)
                        .label("KICK 인증정보 삭제")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.provider_clear("KICK"));
                            cx.notify();
                        })),
                ),
        );

    page.child(prov_card)
}

fn render_backup_manage_section(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut wrap = div().flex().flex_col().gap(px(12.0));

    if !state.maintenance_message.is_empty() {
        wrap = wrap.child(notice_banner(&state.maintenance_message));
    }

    // Backup policy card
    let policy_card = surface_card()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb(px(8.0))
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child("자동 백업 정책"),
                        )
                        .child(
                            Switch::new("backup-enabled-sw")
                                .checked(state.backup_enabled)
                                .label("자동 백업 사용")
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    this.controller.update(cx, |c, _| {
                                        c.state.backup_enabled = *checked;
                                    });
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("backup-refresh-btn")
                                .ghost()
                                .small()
                                .disabled(state.maintenance_busy)
                                .label("새로고침")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller
                                        .update(cx, |c, _| c.refresh_maintenance(false));
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("backup-create-btn")
                                .outline()
                                .small()
                                .disabled(state.maintenance_busy)
                                .label("지금 백업 생성")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.backup_create());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("backup-save-policy-btn")
                                .primary()
                                .small()
                                .disabled(state.maintenance_busy)
                                .label("정책 저장")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_backup_policy_inputs(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .grid()
                .grid_cols(3)
                .gap(px(10.0))
                .child(labeled_input(
                    "백업 간격 (시간)",
                    None,
                    Input::new(&view.backup_interval_input).small(),
                ))
                .child(labeled_input(
                    "최대 보관 개수",
                    None,
                    Input::new(&view.backup_keep_input).small(),
                ))
                .child(labeled_input(
                    "보관 기간 (일)",
                    None,
                    Input::new(&view.backup_retention_input).small(),
                )),
        )
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(8.0))
                .child(
                    div().flex_1().child(labeled_input(
                        if state.backup_directory_locked {
                            "백업 디렉터리 (STREAM_ARCHIVE_BACKUP_DIR로 고정됨)"
                        } else {
                            "백업 디렉터리 (기본: 패키지 외부 형제 폴더 stream-archive-backups)"
                        },
                        None,
                        Input::new(&view.backup_dir_input)
                            .small()
                            .disabled(state.backup_directory_locked),
                    )),
                )
                .when(!state.backup_directory_locked, |el| {
                    el.child(
                        Button::new("backup-pick-dir")
                            .outline()
                            .small()
                            .disabled(state.maintenance_busy)
                            .label("폴더 선택")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.backup_pick_directory());
                            })),
                    )
                }),
        );

    wrap = wrap.child(policy_card);

    // Managed backups list
    let mut table = surface_card().overflow_hidden().flex().flex_col();
    table = table.child(
        div()
            .px(px(12.0))
            .py(px(8.0))
            .bg(rgb(BG_HEADER))
            .border_b_1()
            .border_color(rgb(BORDER))
            .text_size(px(12.0))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(TEXT_PRIMARY))
            .child(format!("관리형 백업 목록 ({}개)", state.backup_rows.len())),
    );

    if state.backup_rows.is_empty() {
        table = table.child(
            div()
                .p(px(20.0))
                .text_center()
                .text_size(px(12.0))
                .text_color(rgb(TEXT_MUTED))
                .child("아직 생성된 관리형 백업이 없습니다."),
        );
    } else {
        for (idx, row) in state.backup_rows.iter().enumerate() {
            let fname = row.file_name.clone();
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
                            .gap(px(2.0))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(status_pill(&row.integrity, &row.integrity_tone, false))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(TEXT_PRIMARY))
                                            .truncate()
                                            .child(row.file_name.clone()),
                                    ),
                            )
                            .child(div().text_size(px(11.0)).text_color(rgb(TEXT_MUTED)).child(
                                format!(
                                    "{} · {} · {} · SHA256 {}",
                                    row.kind,
                                    row.created_at,
                                    row.size,
                                    row.sha256.chars().take(12).collect::<String>()
                                ),
                            )),
                    )
                    .child(
                        Button::new(("backup-restore", idx))
                            .danger()
                            .xsmall()
                            .disabled(!row.can_restore || state.maintenance_busy)
                            .label("복원")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = fname.clone();
                                this.controller.update(cx, |c, _| c.request_restore(name));
                                cx.notify();
                            })),
                    ),
            );
        }
    }

    wrap.child(table)
}

fn labeled_input(label: &str, secret_set: Option<bool>, input: impl IntoElement) -> Div {
    let mut header = div().flex().items_center().justify_between().child(
        div()
            .text_size(px(11.0))
            .text_color(rgb(TEXT_MUTED))
            .child(label.to_string()),
    );
    if let Some(is_set) = secret_set {
        header = header.child(
            div()
                .text_size(px(10.5))
                .text_color(rgb(if is_set { OK_FG } else { TEXT_MUTED }))
                .child(if is_set {
                    "저장됨 (비워두면 유지)"
                } else {
                    "미설정"
                }),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(header)
        .child(input)
}
