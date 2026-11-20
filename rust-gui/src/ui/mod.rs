//! Root GPUI workspace shell (`MainView`): Zed-inspired chrome, navigation rail,
//! persistent input bindings, scrollable page host, status bar, and modal overlays.

pub mod channels_view;
pub mod diagnostics_view;
pub mod history_view;
pub mod live_view;
pub mod queue_view;
pub mod settings_view;
pub mod vod_view;

use crate::{
    controller::AppController, state::Page, theme::*, thumbnail_adapter::png_bytes_to_render_image,
};
use gpui_kit::{
    component::{
        Disableable as _, Sizable as _, TitleBar,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        input::{Input, InputEvent, InputState},
        scroll::ScrollableElement as _,
    },
    prelude::FluentBuilder as _,
    *,
};
use std::{collections::BTreeMap, sync::Arc};

const BRAND_ICON_PNG: &[u8] = include_bytes!("../../assets/stream-archive-icon.png");
const PLATFORM_SOOP_PNG: &[u8] = include_bytes!("../../assets/platform-soop.png");
const PLATFORM_CHZZK_PNG: &[u8] = include_bytes!("../../assets/platform-chzzk.png");

pub struct ChannelInputSet {
    pub account: Entity<InputState>,
    pub name: Entity<InputState>,
    pub outdir: Entity<InputState>,
}

pub struct MainView {
    pub controller: Entity<AppController>,
    brand_icon: Option<Arc<RenderImage>>,
    soop_icon: Option<Arc<RenderImage>>,
    chzzk_icon: Option<Arc<RenderImage>>,

    pub vod_url_input: Entity<InputState>,
    pub vod_output_input: Entity<InputState>,
    pub history_search_input: Entity<InputState>,
    pub channel_inputs: Vec<ChannelInputSet>,
    pub setting_inputs: BTreeMap<String, Entity<InputState>>,

    pub prov_soop_user: Entity<InputState>,
    pub prov_soop_pass: Entity<InputState>,
    pub prov_worker_url: Entity<InputState>,
    pub prov_worker_key: Entity<InputState>,
    pub prov_chzzk_aut: Entity<InputState>,
    pub prov_chzzk_ses: Entity<InputState>,
    pub prov_kick_token: Entity<InputState>,

    pub backup_interval_input: Entity<InputState>,
    pub backup_keep_input: Entity<InputState>,
    pub backup_retention_input: Entity<InputState>,
    pub backup_dir_input: Entity<InputState>,

    live_password_input: Entity<InputState>,

    synced_settings_rev: Option<u64>,
    synced_channels_rev: Option<u64>,
    synced_provider_rev: Option<u64>,
    synced_backup_rev: Option<u64>,
    synced_backup_dir_rev: Option<u64>,
    synced_vod_output_rev: Option<u64>,

    _subscriptions: Vec<Subscription>,
    channel_subscriptions: Vec<Subscription>,
    setting_subscriptions: Vec<Subscription>,
}

impl MainView {
    pub fn new(
        controller: Entity<AppController>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = controller.read(cx).state.clone();
        let brand_icon = png_bytes_to_render_image(BRAND_ICON_PNG);
        let soop_icon = png_bytes_to_render_image(PLATFORM_SOOP_PNG);
        let chzzk_icon = png_bytes_to_render_image(PLATFORM_CHZZK_PNG);

        let vod_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://vod.sooplive.com/... · https://chzzk.naver.com/video/... · https://kick.com/...")
                .default_value(initial.vod_url.clone())
        });
        let vod_output_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("VOD 저장 폴더")
                .default_value(initial.vod_output_directory.clone())
        });
        let history_search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("채널명 · 방송 제목 · 파일명 검색 (Enter로 조회)")
                .default_value(initial.history_search.clone())
        });

        let prov_soop_user = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("SOOP 아이디")
                .default_value(initial.provider_soop_username.clone())
        });
        let prov_soop_pass = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("새 비밀번호 입력 (비워두면 기존 값 유지)")
                .default_value(initial.provider_soop_password.clone())
                .masked(true)
        });
        let prov_worker_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://your-worker.workers.dev/soop/url")
                .default_value(initial.provider_worker_url.clone())
        });
        let prov_worker_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("새 API_SECRET 입력 (비워두면 기존 값 유지)")
                .default_value(initial.provider_worker_api_key.clone())
                .masked(true)
        });
        let prov_chzzk_aut = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("NID_AUT 쿠키 (비워두면 기존 값 유지)")
                .default_value(initial.provider_chzzk_nid_aut.clone())
                .masked(true)
        });
        let prov_chzzk_ses = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("NID_SES 쿠키 (비워두면 기존 값 유지)")
                .default_value(initial.provider_chzzk_nid_ses.clone())
                .masked(true)
        });
        let prov_kick_token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("session_token (비워두면 기존 값 유지)")
                .default_value(initial.provider_kick_token.clone())
                .masked(true)
        });

        let backup_interval_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("24")
                .default_value(initial.backup_interval_hours.clone())
        });
        let backup_keep_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("10")
                .default_value(initial.backup_keep_count.clone())
        });
        let backup_retention_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("3")
                .default_value(initial.backup_retention_days.clone())
        });
        let backup_dir_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("백업 폴더 경로")
                .default_value(initial.backup_directory.clone())
        });

        let live_password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("방송 비밀번호 입력 후 Enter")
                .masked(true)
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe_in(&controller, window, |this, _, window, cx| {
            this.sync_inputs_from_controller(window, cx);
            cx.notify();
        }));
        subscriptions.push(cx.subscribe_in(
            &vod_url_input,
            window,
            |this, state, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    let val = state.read(cx).value().to_string();
                    this.controller.update(cx, |c, _| c.vod_edit_url(val));
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.controller.update(cx, |c, _| c.vod_analyze());
                    cx.notify();
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &vod_output_input,
            window,
            |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let val = state.read(cx).value().to_string();
                    this.controller.update(cx, |c, _| c.vod_edit_output(val));
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &history_search_input,
            window,
            |this, state, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    let val = state.read(cx).value().to_string();
                    this.controller.update(cx, |c, _| c.history_set_search(val));
                }
                InputEvent::PressEnter { .. } => {
                    this.controller.update(cx, |c, _| c.refresh_history());
                    cx.notify();
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &live_password_input,
            window,
            |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let pw = state.read(cx).value().to_string();
                    this.controller
                        .update(cx, |c, _| c.submit_live_password(pw));
                    cx.notify();
                }
            },
        ));
        for (entity, apply) in [
            (
                &prov_soop_user,
                (|s: &mut crate::state::UiState, v| s.provider_soop_username = v)
                    as fn(&mut crate::state::UiState, String),
            ),
            (&prov_soop_pass, |s, v| s.provider_soop_password = v),
            (&prov_worker_url, |s, v| s.provider_worker_url = v),
            (&prov_worker_key, |s, v| s.provider_worker_api_key = v),
            (&prov_chzzk_aut, |s, v| s.provider_chzzk_nid_aut = v),
            (&prov_chzzk_ses, |s, v| s.provider_chzzk_nid_ses = v),
            (&prov_kick_token, |s, v| s.provider_kick_token = v),
            (&backup_interval_input, |s, v| s.backup_interval_hours = v),
            (&backup_keep_input, |s, v| s.backup_keep_count = v),
            (&backup_retention_input, |s, v| s.backup_retention_days = v),
            (&backup_dir_input, |s, v| s.backup_directory = v),
        ] {
            subscriptions.push(cx.subscribe_in(
                entity,
                window,
                move |this, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let val = state.read(cx).value().to_string();
                        this.controller.update(cx, |c, _| apply(&mut c.state, val));
                    }
                },
            ));
        }

        let mut view = Self {
            controller,
            brand_icon,
            soop_icon,
            chzzk_icon,
            vod_url_input,
            vod_output_input,
            history_search_input,
            channel_inputs: Vec::new(),
            setting_inputs: BTreeMap::new(),
            prov_soop_user,
            prov_soop_pass,
            prov_worker_url,
            prov_worker_key,
            prov_chzzk_aut,
            prov_chzzk_ses,
            prov_kick_token,
            backup_interval_input,
            backup_keep_input,
            backup_retention_input,
            backup_dir_input,
            live_password_input,
            synced_settings_rev: None,
            synced_channels_rev: None,
            synced_provider_rev: None,
            synced_backup_rev: None,
            synced_backup_dir_rev: None,
            synced_vod_output_rev: None,
            _subscriptions: subscriptions,
            channel_subscriptions: Vec::new(),
            setting_subscriptions: Vec::new(),
        };
        view.sync_inputs_from_controller(window, cx);
        view
    }

    pub fn platform_icon(&self, platform: &str) -> Option<Arc<RenderImage>> {
        match platform {
            "SOOP" => self.soop_icon.clone(),
            "CHZZK" => self.chzzk_icon.clone(),
            _ => None,
        }
    }

    pub fn sync_inputs_from_controller(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.controller.read(cx).state.clone();

        if self.synced_settings_rev != Some(state.settings_revision) {
            self.synced_settings_rev = Some(state.settings_revision);
            self.setting_inputs.clear();
            self.setting_subscriptions.clear();
            for row in &state.settings_rows {
                if row.key == "STREAM_ARCHIVE_CLOSE_ACTION"
                    || row.key == "STREAM_ARCHIVE_DOWNLOAD_NOTIFICATIONS"
                {
                    continue;
                }
                let key = row.key.clone();
                let value = row.value.clone();
                let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
                let key_for_cb = key.clone();
                let sub =
                    cx.subscribe_in(&input, window, move |this, st, ev: &InputEvent, _, cx| {
                        if matches!(ev, InputEvent::Change) {
                            let next = st.read(cx).value().to_string();
                            let k = key_for_cb.clone();
                            this.controller
                                .update(cx, |c, _| c.settings_set_key(&k, next));
                            cx.notify();
                        }
                    });
                self.setting_inputs.insert(key, input);
                self.setting_subscriptions.push(sub);
            }
        }

        if self.synced_channels_rev != Some(state.channels_revision) {
            self.synced_channels_rev = Some(state.channels_revision);
            self.channel_inputs.clear();
            self.channel_subscriptions.clear();
            for (idx, row) in state.channels_rows.iter().enumerate() {
                let account = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("예: streamer_id")
                        .default_value(row.account.clone())
                });
                let name = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("자동 조회 또는 직접 입력")
                        .default_value(row.name.clone())
                });
                let outdir = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("기본 OUTPUT_DIR 사용")
                        .default_value(row.outdir.clone())
                });

                self.channel_subscriptions.push(cx.subscribe_in(
                    &account,
                    window,
                    move |this, st, ev: &InputEvent, _, cx| {
                        if matches!(ev, InputEvent::Change) {
                            let val = st.read(cx).value().to_string();
                            this.controller
                                .update(cx, |c, _| c.channels_edit(idx, "account", val));
                            cx.notify();
                        }
                    },
                ));
                self.channel_subscriptions.push(cx.subscribe_in(
                    &name,
                    window,
                    move |this, st, ev: &InputEvent, _, cx| {
                        if matches!(ev, InputEvent::Change) {
                            let val = st.read(cx).value().to_string();
                            this.controller
                                .update(cx, |c, _| c.channels_edit(idx, "name", val));
                            cx.notify();
                        }
                    },
                ));
                self.channel_subscriptions.push(cx.subscribe_in(
                    &outdir,
                    window,
                    move |this, st, ev: &InputEvent, _, cx| {
                        if matches!(ev, InputEvent::Change) {
                            let val = st.read(cx).value().to_string();
                            this.controller
                                .update(cx, |c, _| c.channels_edit(idx, "outdir", val));
                            cx.notify();
                        }
                    },
                ));

                self.channel_inputs.push(ChannelInputSet {
                    account,
                    name,
                    outdir,
                });
            }
        }

        if self.synced_provider_rev != Some(state.provider_revision) {
            self.synced_provider_rev = Some(state.provider_revision);
            self.prov_soop_user.update(cx, |s, cx| {
                s.set_value(state.provider_soop_username.clone(), window, cx)
            });
            self.prov_soop_pass.update(cx, |s, cx| {
                s.set_value(state.provider_soop_password.clone(), window, cx)
            });
            self.prov_worker_url.update(cx, |s, cx| {
                s.set_value(state.provider_worker_url.clone(), window, cx)
            });
            self.prov_worker_key.update(cx, |s, cx| {
                s.set_value(state.provider_worker_api_key.clone(), window, cx)
            });
            self.prov_chzzk_aut.update(cx, |s, cx| {
                s.set_value(state.provider_chzzk_nid_aut.clone(), window, cx)
            });
            self.prov_chzzk_ses.update(cx, |s, cx| {
                s.set_value(state.provider_chzzk_nid_ses.clone(), window, cx)
            });
            self.prov_kick_token.update(cx, |s, cx| {
                s.set_value(state.provider_kick_token.clone(), window, cx)
            });
        }

        if self.synced_backup_rev != Some(state.backup_revision) {
            self.synced_backup_rev = Some(state.backup_revision);
            self.backup_interval_input.update(cx, |s, cx| {
                s.set_value(state.backup_interval_hours.clone(), window, cx)
            });
            self.backup_keep_input.update(cx, |s, cx| {
                s.set_value(state.backup_keep_count.clone(), window, cx)
            });
            self.backup_retention_input.update(cx, |s, cx| {
                s.set_value(state.backup_retention_days.clone(), window, cx)
            });
        }

        if self.synced_backup_dir_rev != Some(state.backup_dir_revision) {
            self.synced_backup_dir_rev = Some(state.backup_dir_revision);
            self.backup_dir_input.update(cx, |s, cx| {
                s.set_value(state.backup_directory.clone(), window, cx)
            });
        }

        if self.synced_vod_output_rev != Some(state.vod_output_revision) {
            self.synced_vod_output_rev = Some(state.vod_output_revision);
            self.vod_output_input.update(cx, |s, cx| {
                s.set_value(state.vod_output_directory.clone(), window, cx)
            });
        }
    }

    pub fn open_password_modal(
        &mut self,
        target: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.live_password_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.controller
            .update(cx, |c, _| c.open_password_prompt(target, name));
        cx.notify();
    }

    pub fn save_provider_inputs(&mut self, cx: &mut Context<Self>) {
        let input = crate::controller::ProviderSaveInput {
            username: self.prov_soop_user.read(cx).value().to_string(),
            worker_url: self.prov_worker_url.read(cx).value().to_string(),
            soop_password: self.prov_soop_pass.read(cx).value().to_string(),
            worker_api_key: self.prov_worker_key.read(cx).value().to_string(),
            chzzk_nid_aut: self.prov_chzzk_aut.read(cx).value().to_string(),
            chzzk_nid_ses: self.prov_chzzk_ses.read(cx).value().to_string(),
            kick_token: self.prov_kick_token.read(cx).value().to_string(),
        };
        self.controller.update(cx, |c, _| c.provider_save(input));
        cx.notify();
    }

    pub fn save_backup_policy_inputs(&mut self, cx: &mut Context<Self>) {
        let enabled = self.controller.read(cx).state.backup_enabled;
        let interval = self.backup_interval_input.read(cx).value().to_string();
        let keep = self.backup_keep_input.read(cx).value().to_string();
        let retention = self.backup_retention_input.read(cx).value().to_string();
        let dir = self.backup_dir_input.read(cx).value().to_string();
        self.controller.update(cx, |c, _| {
            c.backup_save_policy(enabled, interval, keep, retention, dir)
        });
        cx.notify();
    }

    fn render_top_chrome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.controller.read(cx).state.clone();
        let brand_logo = if let Some(icon) = self.brand_icon.clone() {
            div()
                .size(px(18.0))
                .rounded(px(4.0))
                .overflow_hidden()
                .child(img(icon).w_full().h_full().object_fit(ObjectFit::Contain))
        } else {
            div()
                .size(px(18.0))
                .rounded(px(4.0))
                .bg(rgb(ACCENT))
                .text_color(rgb(ACCENT_INK))
                .text_size(px(11.0))
                .font_weight(FontWeight::BOLD)
                .flex()
                .items_center()
                .justify_center()
                .child("S")
        };

        TitleBar::new()
            .bg(rgb(BG_CHROME))
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .pl(px(4.0))
                    .child(brand_logo)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child("Stream Archive"),
                    )
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(1.0))
                            .rounded(px(4.0))
                            .bg(rgb(BG_RAISED))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_MUTED))
                            .child(state.app_version.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(0x4b5766))
                            .child("/"),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_MUTED))
                            .child(state.active_page.breadcrumb()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .pr(px(10.0))
                    .child(status_pill(
                        &format!("녹화 {}", state.live_recording_count),
                        if state.live_recording_count != "0" {
                            "ok"
                        } else {
                            "muted"
                        },
                        state.live_running,
                    ))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT_MUTED))
                            .child(state.desktop_status.clone()),
                    ),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let state = self.controller.read(cx).state.clone();
        let mut rail = div()
            .w(px(184.0))
            .flex_shrink_0()
            .bg(rgb(BG_CHROME))
            .border_r_1()
            .border_color(rgb(BORDER))
            .p(px(8.0))
            .flex()
            .flex_col()
            .justify_between();

        let mut nav = div().flex().flex_col().gap(px(3.0));
        for (idx, page) in Page::ALL.into_iter().enumerate() {
            let active = state.active_page == page;
            let badge = match page {
                Page::Live if state.live_recording_count != "0" => {
                    Some((format!("REC {}", state.live_recording_count), OK_FG, OK_BG))
                }
                Page::Queue
                    if state.queue_active_count != "0" || state.queue_queued_count != "0" =>
                {
                    Some((
                        format!("{}/{}", state.queue_active_count, state.queue_queued_count),
                        ACCENT,
                        BG_SELECTED,
                    ))
                }
                _ => None,
            };

            let item = div()
                .id(("nav-item", idx))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .bg(rgb(if active { BG_SELECTED } else { BG_CHROME }))
                .border_1()
                .border_color(rgb(if active { 0x265954 } else { BG_CHROME }))
                .hover(|s| {
                    if active {
                        s
                    } else {
                        s.bg(rgb(BG_SURFACE)).border_color(rgb(BORDER))
                    }
                })
                .flex()
                .items_center()
                .justify_between()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.controller.update(cx, |c, _| c.select_page(page));
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(if active {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(rgb(if active { ACCENT } else { TEXT_PRIMARY }))
                                .child(page.title_ko()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(TEXT_MUTED))
                                .child(page.as_str()),
                        ),
                )
                .when_some(badge, |el, (text, fg, bg)| {
                    el.child(
                        div()
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded(px(10.0))
                            .bg(rgb(bg))
                            .text_size(px(10.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(fg))
                            .child(text),
                    )
                });

            nav = nav.child(item);
        }

        let footer = div()
            .pt(px(8.0))
            .border_t_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(div().text_size(px(10.5)).text_color(rgb(TEXT_MUTED)).child(
                if state.close_to_tray {
                    "창 닫기: 시스템 트레이"
                } else if state.close_action == "ASK" {
                    "창 닫기: 매번 확인"
                } else {
                    "창 닫기: 앱 종료"
                },
            ))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(0x556478))
                    .child("Pure Rust · GPUI DirectX 11"),
            );

        rail = rail.child(nav).child(footer);
        rail
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> Div {
        let state = self.controller.read(cx).state.clone();
        div()
            .h(px(26.0))
            .px(px(12.0))
            .bg(rgb(BG_CHROME))
            .border_t_1()
            .border_color(rgb(BORDER))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(11.0))
            .text_color(rgb(TEXT_MUTED))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(format!(
                        "엔진: {} ({})",
                        state.live_engine, state.live_state_label
                    ))
                    .child(format!(
                        "채널 {} · 녹화 {} · 대기열 {}/{}",
                        state.live_channel_count,
                        state.live_recording_count,
                        state.queue_active_count,
                        state.queue_queued_count
                    )),
            )
            .child(if state.runtime_ready {
                "런타임 준비됨 · Canonical SQLite"
            } else if state.startup_failed {
                "런타임 초기화 오류"
            } else {
                "런타임 초기화 중..."
            })
    }

    fn render_recovery_banner(&self, message: &str, cx: &mut Context<Self>) -> Div {
        div()
            .px(px(14.0))
            .py(px(8.0))
            .bg(rgb(WARN_BG))
            .border_b_1()
            .border_color(rgb(WARN_BORDER))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(rgb(WARN_FG))
                    .child(message.to_string()),
            )
            .child(
                Button::new("dismiss-recovery-btn")
                    .outline()
                    .xsmall()
                    .label("확인")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.controller
                            .update(cx, |c, _| c.dismiss_recovery_message());
                        cx.notify();
                    })),
            )
    }

    fn render_close_dialog(&self, cx: &mut Context<Self>) -> Div {
        let state = self.controller.read(cx).state.clone();
        modal_backdrop(
            "close-dialog-backdrop",
            surface_card()
                .w(px(440.0))
                .p(px(18.0))
                .bg(rgb(BG_SURFACE))
                .border_color(rgb(BORDER_STRONG))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child("창을 닫을 때 어떻게 동작할까요?"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child("트레이로 이동하면 창 GPU 리소스를 해제하고 백그라운드에서 녹화·다운로드를 계속 유지합니다."),
                )
                .child(
                    Checkbox::new("close-remember-cb")
                        .checked(state.close_remember)
                        .label("이 선택을 기본 동작으로 기억하기")
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.controller.update(cx, |c, _| {
                                c.state.close_remember = *checked;
                            });
                            cx.notify();
                        })),
                )
                .when(!state.close_choice_error.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(ERR_FG))
                            .child(state.close_choice_error.clone()),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .pt(px(6.0))
                        .child(
                            Button::new("close-cancel-btn")
                                .ghost()
                                .small()
                                .disabled(state.close_choice_busy)
                                .label("취소")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.cancel_close_dialog());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("close-tray-btn")
                                .primary()
                                .small()
                                .disabled(state.close_choice_busy)
                                .label("트레이로 이동")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let remember = this.controller.read(cx).state.close_remember;
                                    this.controller
                                        .update(cx, |c, _| c.submit_close_choice("TRAY", remember));
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("close-exit-btn")
                                .danger()
                                .small()
                                .disabled(state.close_choice_busy)
                                .label("앱 종료")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let remember = this.controller.read(cx).state.close_remember;
                                    this.controller
                                        .update(cx, |c, _| c.submit_close_choice("EXIT", remember));
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    fn render_password_modal(&self, cx: &mut Context<Self>) -> Div {
        let name = self.controller.read(cx).state.password_prompt_name.clone();
        modal_backdrop(
            "password-modal-backdrop",
            surface_card()
                .w(px(400.0))
                .p(px(18.0))
                .bg(rgb(BG_SURFACE))
                .border_color(rgb(BORDER_STRONG))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(format!("비밀번호 방송 녹화 — {name}")),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child("현재 방송 회차에만 임시 적용되며 디스크에 저장되지 않습니다."),
                )
                .child(Input::new(&self.live_password_input).small().mask_toggle())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            Button::new("pw-cancel-btn")
                                .ghost()
                                .small()
                                .label("취소")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller
                                        .update(cx, |c, _| c.cancel_password_prompt());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("pw-submit-btn")
                                .primary()
                                .small()
                                .label("적용 후 재시도")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let pw = this.live_password_input.read(cx).value().to_string();
                                    this.controller
                                        .update(cx, |c, _| c.submit_live_password(pw));
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }

    fn render_restore_modal(&self, file_name: &str, cx: &mut Context<Self>) -> Div {
        modal_backdrop(
            "restore-modal-backdrop",
            surface_card()
                .w(px(440.0))
                .p(px(18.0))
                .bg(rgb(BG_SURFACE))
                .border_color(rgb(ERR_BORDER))
                .flex()
                .flex_col()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(px(14.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(ERR_FG))
                        .child("백업 복원을 진행할까요?"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(format!(
                            "선택한 백업({file_name})으로 SQLite 데이터베이스를 복원합니다. 복원 전 안전 백업이 자동 생성되며 진행 중인 감시·VOD 작업이 중지됩니다."
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            Button::new("restore-cancel-btn")
                                .ghost()
                                .small()
                                .label("취소")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.cancel_restore());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("restore-confirm-btn")
                                .danger()
                                .small()
                                .label("복원 실행")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.controller.update(cx, |c, _| c.confirm_restore());
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }
}

fn modal_backdrop(_id: &'static str, dialog: Div) -> Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .bg(rgba(0x070a0ee0))
        .flex()
        .items_center()
        .justify_center()
        .child(dialog)
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_inputs_from_controller(window, cx);
        let state = self.controller.read(cx).state.clone();

        let page_content = match state.active_page {
            Page::Live => live_view::render_live_page(self, cx),
            Page::Channels => channels_view::render_channels_page(self, cx),
            Page::Vod => vod_view::render_vod_page(self, cx),
            Page::Queue => queue_view::render_queue_page(self, cx),
            Page::History => history_view::render_history_page(self, cx),
            Page::Settings => settings_view::render_settings_page(self, cx),
            Page::Diagnostics => diagnostics_view::render_diagnostics_page(self, cx),
        };

        let mut workspace = div().flex_1().min_w_0().min_h_0().flex().flex_col();
        if !state.desktop_recovery_message.is_empty() {
            workspace =
                workspace.child(self.render_recovery_banner(&state.desktop_recovery_message, cx));
        }
        workspace = workspace.child(
            div()
                .id("main-scroll-pane")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .p(px(16.0))
                .child(page_content),
        );

        let mut root = div()
            .relative()
            .size_full()
            .bg(rgb(BG_CANVAS))
            .text_color(rgb(TEXT_PRIMARY))
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    let s = this.controller.read(cx).state.clone();
                    if s.close_dialog_visible {
                        this.controller.update(cx, |c, _| c.cancel_close_dialog());
                        cx.notify();
                    } else if s.password_prompt_target.is_some() {
                        this.controller
                            .update(cx, |c, _| c.cancel_password_prompt());
                        cx.notify();
                    } else if s.restore_confirm_file.is_some() {
                        this.controller.update(cx, |c, _| c.cancel_restore());
                        cx.notify();
                    }
                }
            }))
            .child(self.render_top_chrome(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(workspace),
            )
            .child(self.render_status_bar(cx));

        if state.close_dialog_visible {
            root = root.child(self.render_close_dialog(cx));
        } else if state.password_prompt_target.is_some() {
            root = root.child(self.render_password_modal(cx));
        } else if let Some(file_name) = state.restore_confirm_file.as_deref() {
            root = root.child(self.render_restore_modal(file_name, cx));
        }

        root
    }
}
