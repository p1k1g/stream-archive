//! Settings view owning the internal General / Manage (Backup & Restore) boundary.
//! Phase 21.9: Settings > Manage owns backup policy and operations.

use crate::{state::SettingsView, theme::*, ui::MainView};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
        switch::Switch,
    },
    prelude::FluentBuilder as _,
    *,
};

const ADVANCED_KEYS: &[&str] = &[
    "RECORD_STALL_TIMEOUT",
    "RECORD_RETRY_INTERVAL",
    "LOG_ENABLED",
    "LOG_RETENTION_DAYS",
];

/// `(setting_key, label, switch_id, default_on)` for Y/N boolean environment settings.
const YES_NO_SETTINGS: &[(&str, &str, &str, bool)] = &[
    (
        "GUI_NOTIFY_RECORD_START",
        "LIVE 녹화 시작 트레이 알림",
        "notif-live-start-switch",
        false,
    ),
    (
        "GUI_NOTIFY_RECORD_FINISH",
        "LIVE 녹화 완료 트레이 알림",
        "notif-live-finish-switch",
        true,
    ),
    (
        "GUI_NOTIFY_WARNING",
        "저장 공간 부족·채널 오류 경고 알림",
        "notif-warning-switch",
        true,
    ),
    (
        "GUI_AUTO_START_WATCHER",
        "앱 실행 시 LIVE 감시 자동 시작",
        "auto-start-watcher-switch",
        false,
    ),
    (
        "GUI_START_MINIMIZED_TRAY",
        "앱 실행 시 시스템 트레이로 최소화 시작",
        "start-minimized-tray-switch",
        false,
    ),
    (
        "LIVE_REMUX_DELETE_SOURCE",
        "Remux 성공 후 원본 .ts 자동 삭제",
        "remux-delete-source-switch",
        false,
    ),
    (
        "WEBHOOK_ENABLED",
        "외부 알림 웹훅 사용 (Discord · Telegram)",
        "webhook-enabled-switch",
        false,
    ),
    (
        "WEBHOOK_NOTIFY_LIVE",
        "웹훅 · LIVE 녹화 시작/종료 알림",
        "webhook-live-switch",
        true,
    ),
    (
        "WEBHOOK_NOTIFY_VOD",
        "웹훅 · VOD 다운로드 완료/실패 알림",
        "webhook-vod-switch",
        true,
    ),
    (
        "WEBHOOK_NOTIFY_WARNING",
        "웹훅 · 디스크 부족/채널 오류 경고 알림",
        "webhook-warning-switch",
        true,
    ),
];

/// Single source of truth for environment keys rendered as toggles/pills rather than text inputs.
pub(crate) fn is_non_text_setting_key(key: &str) -> bool {
    matches!(
        key,
        "STREAM_ARCHIVE_CLOSE_ACTION"
            | "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"
            | "FILE_NAME_PATTERN"
            | "LIVE_AUTO_REMUX"
            | "LOG_ENABLED"
    ) || YES_NO_SETTINGS.iter().any(|(name, _, _, _)| *name == key)
}

fn setting_labels(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_PRIMARY))
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(TEXT_MUTED))
                .child(description.into()),
        )
}

fn setting_switch_row(
    id: &'static str,
    title: &'static str,
    description: &str,
    checked: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut MainView, bool, &mut Context<MainView>) + 'static,
    cx: &mut Context<MainView>,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .py(px(4.0))
        .child(setting_labels(title, description.to_string()))
        .child(
            Switch::new(id)
                .disabled(disabled)
                .checked(checked)
                .on_click(cx.listener(move |this, next, _, cx| {
                    on_toggle(this, *next, cx);
                    cx.notify();
                })),
        )
}

fn setting_pill_options(
    id_prefix: &'static str,
    options: &[(&'static str, &'static str)],
    current_upper: &str,
    default_value: &str,
    busy: bool,
    idx: usize,
    cx: &mut Context<MainView>,
) -> Div {
    let mut pills = div().flex().items_center().gap(px(4.0));
    for (p_idx, (val, label)) in options.iter().enumerate() {
        let active = if current_upper.is_empty() {
            *val == default_value
        } else {
            current_upper == *val
        };
        let next_value = (*val).to_string();
        pills = pills.child(
            filter_pill((id_prefix, p_idx), label, active).when(!busy, |el| {
                el.on_click(cx.listener(move |this, _, _, cx| {
                    let v = next_value.clone();
                    this.controller.update(cx, |c, _| c.settings_edit(idx, v));
                    cx.notify();
                }))
            }),
        );
    }
    pills
}

fn setting_text_row(
    idx: usize,
    key: &str,
    description: &str,
    input: &Entity<InputState>,
    busy: bool,
    pickable: bool,
    cx: &mut Context<MainView>,
) -> Div {
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
                        .child(key.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(TEXT_MUTED))
                        .child(description.to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(input).small().disabled(busy)),
                )
                .when(pickable, |el| {
                    el.child(
                        Button::new(("setting-pick", idx))
                            .outline()
                            .small()
                            .disabled(busy)
                            .label("찾아보기")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.controller.update(cx, |c, _| c.settings_pick(idx));
                            })),
                    )
                }),
        )
}

pub fn render_settings_page(view: &MainView, cx: &mut Context<MainView>) -> Div {
    let state = view.controller.read(cx).state.clone();
    let mut page = div().flex().flex_col().gap(px(12.0)).w_full();

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

    if !state.settings_message.is_empty() {
        page = page.child(notice_banner(&state.settings_message));
    }

    let mut env_card = surface_card()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(card_header(
            "런타임 · 도구 · 데스크톱 동작",
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
        ));

    for (idx, row) in state.settings_rows.iter().enumerate() {
        if ADVANCED_KEYS.contains(&row.key.as_str()) {
            continue;
        }
        let current = row.value.trim().to_ascii_uppercase();
        if row.key == "STREAM_ARCHIVE_CLOSE_ACTION" {
            let pills = setting_pill_options(
                "close-act",
                &[
                    ("EXIT", "프로그램 종료"),
                    ("TRAY", "시스템 트레이로 이동"),
                    ("ASK", "매번 물어보기"),
                ],
                &current,
                "EXIT",
                state.settings_busy,
                idx,
                cx,
            );
            env_card = env_card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .py(px(4.0))
                    .child(setting_labels(
                        "창 닫기 동작 (STREAM_ARCHIVE_CLOSE_ACTION)",
                        row.description.clone(),
                    ))
                    .child(pills),
            );
            continue;
        }

        if row.key == "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS" {
            let checked = !row.value.eq_ignore_ascii_case("false");
            env_card = env_card.child(setting_switch_row(
                "notif-switch",
                "VOD 다운로드 완료·실패 트레이 알림",
                &row.description,
                checked,
                state.settings_busy,
                move |this, next, cx| {
                    let v = if next { "true" } else { "false" }.to_string();
                    this.controller.update(cx, |c, _| c.settings_edit(idx, v));
                },
                cx,
            ));
            continue;
        }

        if row.key == "FILE_NAME_PATTERN" {
            let example = match current.as_str() {
                "TIME_TITLE" => "예: 261124_180000_방송제목_채널명.ts",
                "BJ_TITLE" => "예: 261124_채널명_방송제목.ts",
                "TITLE_NUMBER" => "예: 261124_방송제목_01_채널명.ts",
                _ => "예: 261124_180000_채널명.ts",
            };
            let pills = setting_pill_options(
                "fn-pattern",
                &[
                    ("LEGACY", "기본 (날짜_시간_채널)"),
                    ("TIME_TITLE", "날짜_시간_제목_채널"),
                    ("BJ_TITLE", "날짜_채널_제목"),
                    ("TITLE_NUMBER", "날짜_제목_번호_채널"),
                ],
                &current,
                "LEGACY",
                state.settings_busy,
                idx,
                cx,
            );
            env_card = env_card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .py(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child("LIVE 녹화 파일명 패턴 (FILE_NAME_PATTERN)"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(ACCENT))
                                    .child(example),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_MUTED))
                            .child(row.description.clone()),
                    )
                    .child(pills),
            );
            continue;
        }

        if row.key == "LIVE_AUTO_REMUX" {
            let pills = setting_pill_options(
                "remux-fmt",
                &[
                    ("OFF", "사용 안 함 (.ts 유지)"),
                    ("MP4", "MP4 (무손실 -c copy)"),
                    ("MKV", "MKV (무손실 -c copy)"),
                ],
                &current,
                "OFF",
                state.settings_busy,
                idx,
                cx,
            );
            env_card = env_card.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .py(px(4.0))
                    .child(setting_labels(
                        "LIVE 녹화 후 무손실 자동 Remux (LIVE_AUTO_REMUX)",
                        row.description.clone(),
                    ))
                    .child(pills),
            );
            continue;
        }

        if let Some((_, title, switch_id, default_on)) = YES_NO_SETTINGS
            .iter()
            .find(|(key, _, _, _)| *key == row.key)
        {
            let checked = if *default_on {
                !current.eq_ignore_ascii_case("N")
            } else {
                current.eq_ignore_ascii_case("Y")
            };
            env_card = env_card.child(setting_switch_row(
                switch_id,
                title,
                &row.description,
                checked,
                state.settings_busy,
                move |this, next, cx| {
                    let v = if next { "Y" } else { "N" }.to_string();
                    this.controller.update(cx, |c, _| c.settings_edit(idx, v));
                },
                cx,
            ));
            continue;
        }

        if let Some(input_entity) = view.setting_inputs.get(&row.key) {
            env_card = env_card.child(setting_text_row(
                idx,
                &row.key,
                &row.description,
                input_entity,
                state.settings_busy,
                row.pickable,
                cx,
            ));
        }
    }

    env_card = env_card.child(
        div()
            .pt(px(8.0))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(setting_switch_row(
                "windows-startup-switch",
                "Windows 로그인 시 자동 실행 (시작프로그램 · 즉시 적용)",
                "현재 Windows 계정의 시작프로그램(HKCU Run)에 StreamArchive.exe를 등록하거나 해제합니다.",
                state.windows_startup_enabled,
                state.windows_startup_busy,
                |this, next, cx| {
                    this.controller
                        .update(cx, |c, _| c.set_windows_startup(next));
                },
                cx,
            )),
    );

    let mut adv_section = div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .pt(px(8.0))
        .border_t_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(setting_labels(
                    "고급 설정 (녹화 정체 감지 · 재시도 · 로그 보관)",
                    "RECORD_STALL_TIMEOUT · RECORD_RETRY_INTERVAL · LOG_ENABLED · LOG_RETENTION_DAYS",
                ))
                .child(
                    Button::new("settings-advanced-toggle")
                        .ghost()
                        .xsmall()
                        .label(if state.settings_advanced_open {
                            "접기"
                        } else {
                            "펼치기"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller
                                .update(cx, |c, _| c.toggle_settings_advanced());
                            cx.notify();
                        })),
                ),
        );

    if state.settings_advanced_open {
        for (idx, row) in state.settings_rows.iter().enumerate() {
            if !ADVANCED_KEYS.contains(&row.key.as_str()) {
                continue;
            }
            if row.key == "LOG_ENABLED" {
                let checked = !row.value.trim().eq_ignore_ascii_case("N");
                adv_section = adv_section.child(setting_switch_row(
                    "log-enabled-switch",
                    "일별 파일 로그 기록 (LOG_ENABLED)",
                    &row.description,
                    checked,
                    state.settings_busy,
                    move |this, next, cx| {
                        let v = if next { "Y" } else { "N" }.to_string();
                        this.controller.update(cx, |c, _| c.settings_edit(idx, v));
                    },
                    cx,
                ));
                continue;
            }
            if let Some(input_entity) = view.setting_inputs.get(&row.key) {
                adv_section = adv_section.child(setting_text_row(
                    idx,
                    &row.key,
                    &row.description,
                    input_entity,
                    state.settings_busy,
                    false,
                    cx,
                ));
            }
        }
    }

    page = page.child(env_card.child(adv_section));

    if !state.config_message.is_empty() {
        page = page.child(notice_banner(&state.config_message));
    }

    let mut provider_grid = div().grid().grid_cols(2).gap(px(10.0));
    for (label, secret_set, entity, masked) in [
        ("SOOP 사용자명", None, &view.prov_soop_user, false),
        (
            "SOOP 비밀번호",
            Some(state.provider_soop_password_set),
            &view.prov_soop_pass,
            true,
        ),
        (
            "Cloudflare Worker URL (/soop/url 포함)",
            None,
            &view.prov_worker_url,
            false,
        ),
        (
            "Cloudflare Worker API Key (API_SECRET)",
            Some(state.provider_worker_api_key_set),
            &view.prov_worker_key,
            true,
        ),
        (
            "CHZZK NID_AUT",
            Some(state.provider_chzzk_nid_aut_set),
            &view.prov_chzzk_aut,
            true,
        ),
        (
            "CHZZK NID_SES",
            Some(state.provider_chzzk_nid_ses_set),
            &view.prov_chzzk_ses,
            true,
        ),
        (
            "KICK session_token (구독 VOD)",
            Some(state.provider_kick_token_set),
            &view.prov_kick_token,
            true,
        ),
        (
            "Discord Webhook URL (외부 알림)",
            Some(state.provider_discord_webhook_set),
            &view.prov_discord_webhook,
            true,
        ),
        (
            "Telegram Bot Token (외부 알림)",
            Some(state.provider_telegram_bot_token_set),
            &view.prov_telegram_token,
            true,
        ),
        (
            "Telegram Chat ID (-100... 또는 @채널명)",
            None,
            &view.prov_telegram_chat_id,
            false,
        ),
    ] {
        let input = Input::new(entity)
            .small()
            .disabled(state.config_busy)
            .when(masked, |el| el.mask_toggle());
        provider_grid = provider_grid.child(labeled_input(label, secret_set, input));
    }

    let mut clear_bar = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .pt(px(6.0))
        .border_t_1()
        .border_color(rgb(BORDER));
    for (btn_id, provider_key, label) in [
        ("clear-soop-pw", "SOOP", "SOOP 비밀번호 삭제"),
        ("clear-chzzk-auth", "CHZZK", "CHZZK 인증정보 삭제"),
        ("clear-kick-auth", "KICK", "KICK 인증정보 삭제"),
        ("clear-webhook-auth", "WEBHOOK", "웹훅 인증정보 삭제"),
    ] {
        clear_bar = clear_bar.child(
            Button::new(btn_id)
                .danger()
                .xsmall()
                .disabled(state.config_busy)
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.controller
                        .update(cx, |c, _| c.provider_clear(provider_key));
                    cx.notify();
                })),
        );
    }

    let prov_card = surface_card()
        .p(px(14.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(card_header(
            "서비스 연결 · 암호화 인증정보 (Windows DPAPI)",
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
                    Button::new("prov-test-webhook")
                        .outline()
                        .small()
                        .disabled(state.config_busy)
                        .label("웹훅 테스트 발송")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.controller.update(cx, |c, _| c.provider_test_webhook());
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
        ))
        .child(provider_grid)
        .child(clear_bar);

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
                                .disabled(state.maintenance_busy)
                                .checked(state.backup_enabled)
                                .label("자동 백업 사용")
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    this.controller.update(cx, |c, _| {
                                        if !c.state.maintenance_busy {
                                            c.state.backup_enabled = *checked;
                                        }
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
                    Input::new(&view.backup_interval_input)
                        .small()
                        .disabled(state.maintenance_busy),
                ))
                .child(labeled_input(
                    "최대 보관 개수",
                    None,
                    Input::new(&view.backup_keep_input)
                        .small()
                        .disabled(state.maintenance_busy),
                ))
                .child(labeled_input(
                    "보관 기간 (일)",
                    None,
                    Input::new(&view.backup_retention_input)
                        .small()
                        .disabled(state.maintenance_busy),
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
                            .disabled(state.backup_directory_locked || state.maintenance_busy),
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
