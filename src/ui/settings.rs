//! 设置窗口的内容。

use std::rc::Rc;

use nsh_base::input;
use nsh_core::element::View;
use nsh_core::prelude::*;
use podnook_core::{
    SETTING_AUTO_PASTE, SETTING_HOTKEY, SETTING_LAUNCH_AT_LOGIN, SETTING_RETENTION_COUNT,
    SETTING_RETENTION_DAYS,
};

use nsh_platform_macos::objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};

use podnook_core::SETTING_PLAIN_PASTE;

use crate::{login_item, settings as prefs, theme};

/// 设置窗口的状态：信号与元素引用跨重绘保留，语言切换只重建视图。
#[derive(Clone)]
struct SettingsState {
    store: Option<std::sync::Arc<podnook_core::db::Store>>,
    hotkey: Signal<String>,
    recording: Signal<bool>,
    success: Signal<Option<String>>,
    count: Signal<i64>,
    days: Signal<i64>,
    auto_paste: Signal<bool>,
    plain_paste: Signal<bool>,
    login: Signal<bool>,
    login_label: Signal<String>,
    ignored: Signal<Vec<String>>,
    new_bundle: Signal<String>,
    picker: Signal<bool>,
    bundle_input: ElementRef,
    notice: Signal<Option<String>>,
    confirm_clear: Signal<bool>,
    trusted: Signal<bool>,
    language: Signal<crate::locale::Choice>,
}

impl SettingsState {
    fn new(
        store: Option<std::sync::Arc<podnook_core::db::Store>>,
        config: &prefs::Settings,
    ) -> Self {
        let language = signal(crate::locale::current(store.as_deref()));
        Self {
            store,
            hotkey: signal(config.hotkey.clone()),
            recording: signal(false),
            success: signal(None::<String>),
            count: signal(config.retention_count),
            days: signal(config.retention_days),
            auto_paste: signal(config.auto_paste),
            plain_paste: signal(config.plain_paste),
            login: signal(login_item::status().is_on()),
            login_label: signal(login_item::status().label().to_string()),
            ignored: signal(config.ignored_bundles.clone()),
            new_bundle: signal(String::new()),
            picker: signal(false),
            bundle_input: element_ref(),
            notice: signal(None::<String>),
            confirm_clear: signal(false),
            trusted: signal(crate::paste::is_trusted()),
            language,
        }
    }
}

pub fn settings() -> impl IntoElement {
    let store = crate::history::init();
    let config = prefs::load(store.as_deref());
    let state = SettingsState::new(store, &config);
    start_polling(&state);
    // Esc 收起设置而不是关窗：窗口与它的状态留着，下次打开直接复用。
    on_key("escape", |_| crate::hide_settings());
    dynamic(move || settings_ui(state.clone()))
}

/// 一次性的轮询与清理；不随语言重绘重复注册。
fn start_polling(state: &SettingsState) {
    let store_for_poll = state.store.clone();
    let recording = state.recording;
    let hotkey = state.hotkey;
    let success = state.success;
    let notice = state.notice;
    let trusted = state.trusted;

    set_interval(std::time::Duration::from_millis(100), move || {
        if !recording.get() {
            return;
        }
        let Some(result) = crate::hotkey_recorder::take_result() else {
            return;
        };
        recording.set(false);
        match result {
            Some(key) => {
                let previous = hotkey.get();
                let key_for_result = key.clone();
                let previous_for_result = previous.clone();
                let hotkey_signal = hotkey;
                let success_signal = success;
                let notice_signal = notice;
                let store_for_result = store_for_poll.clone();
                if !crate::register_hotkey_with(&key, move |ok| {
                    if ok {
                        hotkey_signal.set(key_for_result.clone());
                        if let Some(store) = &store_for_result {
                            prefs::write(store, SETTING_HOTKEY, &key_for_result);
                        }
                        success_signal.set(Some(key_for_result.clone()));
                        let clear = success_signal;
                        set_timeout(std::time::Duration::from_millis(1200), move || {
                            clear.set(None)
                        });
                        show_notice(notice_signal, t!("notice.applied"));
                    } else {
                        hotkey_signal.set(previous_for_result.clone());
                        crate::register_hotkey(&previous_for_result);
                        success_signal.set(None);
                    }
                }) {
                    crate::register_hotkey(&previous);
                    show_notice(notice, t!("notice.invalid_hotkey"));
                }
            }
            None => {
                crate::register_hotkey(&hotkey.get());
                show_notice(notice, t!("notice.cancelled"));
            }
        }
    });

    // 系统设置里已授权后，这里自动刷新。
    set_interval(std::time::Duration::from_millis(1000), move || {
        let now = crate::paste::is_trusted();
        if trusted.get() != now {
            trusted.set(now);
        }
    });
    cleanup(move || {
        crate::hotkey_recorder::cancel();
        crate::register_hotkey(&hotkey.get());
    });
}

fn settings_ui(state: SettingsState) -> View {
    let SettingsState {
        store,
        hotkey,
        recording,
        success,
        count,
        days,
        auto_paste,
        plain_paste,
        login,
        login_label,
        ignored,
        new_bundle,
        picker,
        bundle_input,
        notice,
        confirm_clear,
        trusted,
        language,
    } = state;
    let language_store = store.clone();

    // 呼出热键：点字段直接录制；录制提示用字段上方的浮层。
    let start_recording = move || {
        // 设置窗口是非激活浮层；录制前主动拿键盘，本地按键监听才能收到事件。
        let window = nsh_core::element::desktop::current_window();
        window.focus();
        set_timeout(std::time::Duration::from_millis(40), move || window.focus());
        success.set(None);
        recording.set(true);
        crate::suspend_hotkey();
        crate::hotkey_recorder::start();
    };
    let record_control = view()
        .relative()
        .child(
            view()
                .on_press(start_recording)
                .cursor_pointer()
                .px_3()
                .py_2()
                .rounded(px(7.0))
                .bg(theme::surface())
                .border_1()
                .border_color(move || {
                    if recording.get() || success.get().is_some() {
                        theme::primary()
                    } else {
                        theme::border()
                    }
                })
                .child(dynamic(move || {
                    text(key_label(&hotkey.get()))
                        .text_size(px(12.0))
                        .text_color(theme::fg())
                })),
        )
        .child(dynamic(move || {
            if !recording.get() && success.get().is_none() {
                return view();
            }
            let saved = success.get();
            let mut popover = view()
                .absolute()
                .top(px(40.0))
                .right(px(0.0))
                .w(px(190.0))
                .z_index(50)
                .rounded(px(12.0))
                .bg(theme::card())
                .border_1()
                .border_color(theme::border())
                .shadow_lg()
                .px_4()
                .py_3()
                .flex_col()
                .items_center()
                .gap_1();
            let dismiss_state = success;
            let dismiss = move || {
                let was_success = dismiss_state.get().is_some();
                crate::hotkey_recorder::cancel();
                recording.set(false);
                dismiss_state.set(None);
                if !was_success {
                    crate::register_hotkey(&hotkey.get());
                    show_notice(notice, t!("notice.cancelled"));
                }
            };
            popover = popover.child(
                view().w_full().flex_row().justify_end().child(
                    view()
                        .on_press(dismiss)
                        .cursor_pointer()
                        .child(
                            text("×")
                                .text_size(px(12.0))
                                .text_color(theme::muted_fg()),
                        ),
                ),
            );
            if let Some(key) = saved {
                popover = popover
                    .child(
                        view()
                            .px_3()
                            .py_1()
                            .rounded(px(6.0))
                            .bg(theme::success().opacity(0.18))
                            .child(
                                text(key_label(&key))
                                    .text_size(px(13.0))
                                    .text_color(theme::success()),
                            ),
                    )
                    .child(
                        text(t!("settings.hotkey_set"))
                            .text_size(px(13.0))
                            .text_color(theme::fg()),
                    );
            } else {
                popover = popover
                    .child(
                        text(t!("settings.hotkey_example"))
                            .text_size(px(12.0))
                            .text_color(theme::muted_fg()),
                    )
                    .child(
                        text(t!("settings.recording"))
                            .text_size(px(13.0))
                            .text_color(theme::fg()),
                    );
            }
            popover
        }));

    // 保留条数
    let mut count_row = view().flex_row().items_center().gap_2();
    for value in [500_i64, 1000, 5000] {
        let store = store.clone();
        count_row = count_row.child(choice(
            t!("settings.items", value = value),
            move || count.get() == value,
            move || {
                count.set(value);
                if let Some(store) = &store {
                    prefs::write(store, SETTING_RETENTION_COUNT, value);
                }
                show_notice(notice, t!("notice.saved"));
            },
        ));
    }

    // 保留天数
    let mut days_row = view().flex_row().items_center().gap_2();
    for value in [7_i64, 30, 90, 365] {
        let store = store.clone();
        days_row = days_row.child(choice(
            t!("settings.days", value = value),
            move || days.get() == value,
            move || {
                days.set(value);
                if let Some(store) = &store {
                    prefs::write(store, SETTING_RETENTION_DAYS, value);
                }
                show_notice(notice, t!("notice.saved"));
            },
        ));
    }

    let store_for_auto = store.clone();
    let auto_toggle = toggle_control(auto_paste, move |on| {
        auto_paste.set(on);
        if let Some(store) = &store_for_auto {
            prefs::write_bool(store, SETTING_AUTO_PASTE, on);
        }
        show_notice(
            notice,
            if on {
                t!("notice.auto_paste_on")
            } else {
                t!("notice.auto_paste_off")
            },
        );
    });

    let store_for_login = store.clone();
    let login_row = view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(dynamic(move || {
            text(login_label.get())
                .text_size(px(12.0))
                .text_color(theme::muted_fg())
        }))
        .child(toggle_control(login, move |on| {
            match login_item::set_enabled(on) {
                Ok(status) => {
                    login.set(status.is_on());
                    login_label.set(status.label().to_string());
                    if let Some(store) = &store_for_login {
                        prefs::write_bool(store, SETTING_LAUNCH_AT_LOGIN, status.is_on());
                    }
                    show_notice(notice, t!("notice.login_updated"));
                }
                Err(error) => {
                    login.set(!on);
                    show_notice(notice, t!("notice.login_failed", error = error));
                }
            }
        }));

    let store_for_plain = store.clone();
    let plain_toggle = toggle_control(plain_paste, move |on| {
        plain_paste.set(on);
        if let Some(store) = &store_for_plain {
            prefs::write_bool(store, SETTING_PLAIN_PASTE, on);
        }
        show_notice(notice, t!("notice.saved"));
    });

    let permission_row = view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(dynamic(move || {
            text(if trusted.get() {
                t!("settings.granted")
            } else {
                t!("settings.not_granted")
            })
                .text_size(px(12.0))
                .text_color(if trusted.get() {
                    theme::primary()
                } else {
                    theme::muted_fg()
                })
        }))
        .child(choice(t!("settings.request"), || false, || {
            // 系统设置会盖在浮层下面；先收起自己的设置窗口。
            crate::hide_settings();
            crate::paste::request_trust();
        }))
        .child(choice(t!("settings.open_settings"), || false, || {
            crate::hide_settings();
            crate::paste::open_accessibility_settings()
        }));

    // 清空历史（两步确认）
    let store_for_clear = store.clone();
    let clear_control = dynamic(move || {
        if confirm_clear.get() {
            choice(t!("settings.clear_confirm"), || true, {
                let store = store_for_clear.clone();
                move || {
                    if let Some(store) = &store {
                        if let Err(error) = store.clear_history() {
                            log::error!("clear history: {error}");
                        }
                        crate::history::bump();
                    }
                    confirm_clear.set(false);
                    show_notice(notice, t!("notice.cleared"));
                }
            })
        } else {
            choice(t!("settings.clear"), || false, move || {
                confirm_clear.set(true);
                show_notice(notice, t!("notice.confirm_again"));
            })
        }
    });

    // 忽略的 App
    let store_for_add = store.clone();
    let add_ignored = move |bundle: String| {
        ignore_bundle(ignored, store_for_add.clone(), notice, bundle);
    };
    let add_row = view()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_3()
        .child(
            input()
                .flex_1()
                .element_ref(&bundle_input)
                .value(new_bundle)
                .on_change(move |value: &str| new_bundle.set(value.to_string()))
                .placeholder("com.apple.Terminal")
                .text_size(px(12.0)),
        )
        .child(choice(t!("settings.add"), || false, move || {
            let bundle = new_bundle.get().trim().to_string();
            if bundle.is_empty() {
                // 设置窗口是非激活浮层，输入框点击不一定拿到键盘；
                // 这里明确把窗口和输入框都聚焦。
                let window = nsh_core::element::desktop::current_window();
                window.focus();
                let input = bundle_input;
                set_timeout(std::time::Duration::from_millis(60), move || input.focus());
                show_notice(notice, t!("notice.bundle_required"));
                return;
            }
            add_ignored(bundle);
            new_bundle.set(String::new());
        }))
        .child(choice(t!("settings.choose_app"), || false, move || {
            picker.set(!picker.get())
        }));

    let store_for_picker = store.clone();
    let picker_rows = dynamic(move || {
        if !picker.get() {
            return view();
        }
        let current = ignored.get();
        let apps: Vec<(String, String)> = running_apps()
            .into_iter()
            .filter(|(_, bundle)| !current.iter().any(|item| item == bundle))
            .collect();
        let mut rows = view()
            .flex_col()
            .border_t(px(1.0))
            .border_t_color(theme::border());
        if apps.is_empty() {
            rows = rows.child(view().px_4().py_3().child(
                text(t!("settings.no_running_apps"))
                    .text_size(px(12.0))
                    .text_color(theme::muted_fg()),
            ));
        }
        for (name, bundle) in apps {
            let add_bundle = bundle.clone();
            let store = store_for_picker.clone();
            rows = rows.child(
                view()
                    .on_press(move || {
                        ignore_bundle(ignored, store.clone(), notice, add_bundle.clone());
                        picker.set(false);
                    })
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .border_b(px(1.0))
                    .border_b_color(theme::border())
                    .child(text(name).text_size(px(12.0)))
                    .child(
                        text(bundle)
                            .truncate()
                            .text_size(px(11.0))
                            .text_color(theme::muted_fg()),
                    ),
            );
        }
        rows.child(view().px_4().py_2().child(
            text(t!("settings.running_apps_note"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ))
    });

    let ignored_rows = dynamic(move || {
        let mut rows = view().flex_col();
        let bundles = ignored.get();
        if bundles.is_empty() {
            rows = rows.child(
                view()
                    .px_4()
                    .py_3()
                    .child(
                        text(t!("settings.no_ignored"))
                            .text_size(px(12.0))
                            .text_color(theme::muted_fg()),
                    ),
            );
        }
        for bundle in bundles {
            let remove = bundle.clone();
            let store = store.clone();
            rows = rows.child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .border_b(px(1.0))
                    .border_b_color(theme::border())
                    .child(text(bundle).truncate().text_size(px(12.0)))
                    .child(choice(t!("settings.remove"), || false, move || {
                        let mut list = ignored.get();
                        list.retain(|item| item != &remove);
                        ignored.set(list.clone());
                        if let Some(store) = &store {
                            prefs::save_ignored(store, &list);
                        }
                        show_notice(notice, t!("notice.removed"));
                    })),
            );
        }
        rows
    });

    let ignored_card = view()
        .flex_col()
        .child(add_row)
        .child(picker_rows)
        .child(ignored_rows)
        .child(
            view().px_4().py_2().child(
                text(t!("settings.bundle_note"))
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg()),
            ),
        );

    let about_rows = view()
        .flex_col()
        .child(setting_row(
            t!("settings.version"),
            text(format!("v{}", env!("CARGO_PKG_VERSION")))
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        ))
        .child(setting_row(
            t!("settings.repository"),
            choice("GitHub", || false, || {
                crate::paste::open_external("https://github.com/wisarmy/podnook")
            }),
        ))
        .child(view().px_4().py_3().child(
            text(t!("settings.local_first"))
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        ));

    let history_rows = view()
        .flex_col()
        .child(setting_row(t!("settings.keep_count"), count_row))
        .child(setting_row(t!("settings.keep_days"), days_row))
        .child(setting_row(t!("settings.clear"), clear_control));

    let store_for_language = language_store.clone();
    let mut language_row = view().flex_row().items_center().gap_2();
    for (value, label) in [
        (
            crate::locale::Choice::System,
            t!("settings.language_system"),
        ),
        (crate::locale::Choice::Chinese, t!("settings.language_zh")),
        (crate::locale::Choice::English, t!("settings.language_en")),
    ] {
        let store_for_language = store_for_language.clone();
        language_row = language_row.child(choice(
            label,
            move || language.get() == value,
            move || {
                language.set(value);
                crate::locale::set(store_for_language.as_deref(), value);
            },
        ));
    }

    let paste_rows = view()
        .flex_col()
        .child(setting_row(t!("settings.auto_paste"), auto_toggle))
        .child(view().px_4().pb_3().child(
            text(t!("settings.auto_paste_note"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ))
        .child(setting_row(t!("settings.prefer_plain"), plain_toggle))
        .child(view().px_4().pb_3().child(
            text(t!("settings.prefer_plain_note"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ));

    let login_rows = view().flex_col().child(setting_row(t!("settings.launch_at_login"), login_row)).child(
        view().px_4().pb_3().child(
            text(t!("settings.login_note"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ),
    );

    let permission_rows = view()
        .flex_col()
        .child(setting_row(t!("settings.accessibility"), permission_row))
        .child(view().px_4().pb_3().child(
            text(t!("settings.permission_note"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ));

    view()
        .flex_1()
        .macos_backdrop(super::glass_backdrop())
        .bg(theme::glass_bg())
        .text_color(theme::fg())
        .child(scroll().flex_1().child(
            view()
                .flex_col()
                .p_6()
                .gap_5()
                .child(
                    view()
                        .z_index(move || if recording.get() || success.get().is_some() { 20 } else { 0 })
                        .child(settings_section(
                            t!("settings.summon"),
                            view()
                                .flex_col()
                                .child(setting_row(t!("settings.hotkey"), record_control)),
                        )),
                )
                .child(settings_section(t!("settings.history"), history_rows))
                .child(settings_section(t!("settings.paste"), paste_rows))
                .child(settings_section(t!("settings.login_section"), login_rows))
                .child(settings_section(t!("settings.permissions"), permission_rows))
                .child(settings_section(
                    t!("settings.language"),
                    view()
                        .flex_col()
                        .child(setting_row(t!("settings.language_row"), language_row)),
                ))
                .child(settings_section(t!("settings.ignored"), ignored_card))
                .child(settings_section(t!("settings.about"), about_rows))
                .child(
                    view().h(px(18.0)).child(dynamic(move || match notice.get() {
                        Some(message) => text(message)
                            .text_size(px(12.0))
                            .text_color(theme::primary()),
                        None => text("").text_size(px(12.0)),
                    })),
                ),
        ))
}

fn settings_section(title: String, rows: View) -> View {
    view()
        .flex_col()
        .gap_2()
        .child(
            view().pl(px(2.0)).child(
                text(title)
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted_fg()),
            ),
        )
        .child(
            view()
                .flex_col()
                .rounded(px(10.0))
                .bg(theme::glass_card())
                .border_1()
                .border_color(theme::border())
                .child(rows),
        )
}

fn setting_row(label: String, control: impl IntoElement) -> View {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_3()
        .border_b(px(1.0))
        .border_b_color(theme::border())
        .child(text(label).text_size(px(13.0)))
        .child(control)
}

fn choice(
    label: impl Into<String>,
    selected: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    let label = label.into();
    let selected: Rc<dyn Fn() -> bool> = Rc::new(selected);
    let selected_style = selected.clone();
    let selected_text = selected.clone();
    view()
        .on_press(move || on_press())
        .px_3()
        .py_1()
        .rounded(px(7.0))
        .bg(move || {
            if selected_style() {
                theme::glass_accent()
            } else {
                theme::glass_card()
            }
        })
        .border_1()
        .border_color(theme::border())
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(move || {
                    if selected_text() {
                        theme::accent_fg()
                    } else {
                        theme::muted_fg()
                    }
                }),
        )
}

fn toggle_control(on: Signal<bool>, on_change: impl Fn(bool) + 'static) -> View {
    view()
        .on_press(move || on_change(!on.get()))
        .w(px(46.0))
        .h(px(26.0))
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(move || {
            if on.get() {
                theme::primary()
            } else {
                theme::surface()
            }
        })
        .child(
            text(move || if on.get() { t!("login.on") } else { t!("login.off") })
                .text_size(px(11.0))
                .text_color(move || {
                    if on.get() {
                        theme::on_primary()
                    } else {
                        theme::muted_fg()
                    }
                }),
        )
}

fn ignore_bundle(
    ignored: Signal<Vec<String>>,
    store: Option<std::sync::Arc<podnook_core::db::Store>>,
    notice: Signal<Option<String>>,
    bundle: String,
) {
    let bundle = bundle.trim().to_string();
    if bundle.is_empty() {
        return;
    }
    let mut list = ignored.get();
    if list.iter().any(|item| item == &bundle) {
        show_notice(notice, t!("notice.already_listed"));
        return;
    }
    list.push(bundle.clone());
    ignored.set(list.clone());
    if let Some(store) = &store {
        prefs::save_ignored(store, &list);
    }
    show_notice(notice, t!("notice.added", bundle = bundle));
}

fn running_apps() -> Vec<(String, String)> {
    let workspace = NSWorkspace::sharedWorkspace();
    let mut out = Vec::new();
    for app in workspace.runningApplications().iter() {
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let Some(name) = app.localizedName() else {
            continue;
        };
        let Some(bundle) = app.bundleIdentifier() else {
            continue;
        };
        let name = name.to_string();
        let bundle = bundle.to_string();
        if bundle.is_empty() || bundle == "com.podnook.app" {
            continue;
        }
        out.push((name, bundle));
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out.dedup_by(|a, b| a.1 == b.1);
    out
}

fn show_notice(notice: Signal<Option<String>>, message: impl Into<String>) {
    notice.set(Some(message.into()));
    set_timeout(std::time::Duration::from_millis(1600), move || {
        notice.set(None)
    });
}
