//! 设置窗口的内容。

use std::rc::Rc;

use nsh_base::input;
use nsh_core::element::View;
use nsh_core::prelude::*;
use podnook_core::{
    SETTING_AUTO_PASTE, SETTING_HOTKEY, SETTING_LAUNCH_AT_LOGIN, SETTING_RETENTION_COUNT,
    SETTING_RETENTION_DAYS,
};

use crate::{login_item, settings as prefs, theme};

pub fn settings() -> impl IntoElement {
    let store = crate::history::init();
    let config = prefs::load(store.as_deref());

    let hotkey = signal(config.hotkey.clone());
    let recording = signal(false);
    let count = signal(config.retention_count);
    let days = signal(config.retention_days);
    let auto_paste = signal(config.auto_paste);
    let login = signal(login_item::status().is_on());
    let login_label = signal(login_item::status().label().to_string());
    let ignored = signal(config.ignored_bundles.clone());
    let new_bundle = signal(String::new());
    let notice = signal(None::<String>);
    let confirm_clear = signal(false);
    let trusted = signal(crate::paste::is_trusted());

    // 呼出热键：按下组合键录制
    let record_control = view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(dynamic(move || {
            if recording.get() {
                text("请按下组合键…")
                    .text_size(px(12.0))
                    .text_color(theme::primary())
            } else {
                text(key_label(&hotkey.get()))
                    .text_size(px(12.0))
                    .text_color(theme::muted_fg())
            }
        }))
        .child(dynamic(move || {
            if recording.get() {
                choice("取消", || true, move || {
                    crate::hotkey_recorder::cancel();
                    recording.set(false);
                    crate::register_hotkey(&hotkey.get());
                    show_notice(notice, "已取消");
                })
            } else {
                choice("录制", || false, move || {
                    recording.set(true);
                    crate::suspend_hotkey();
                    crate::hotkey_recorder::start();
                    show_notice(notice, "请按下组合键；Esc 取消");
                })
            }
        }));

    let store_for_poll = store.clone();
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
                if crate::register_hotkey(&key) {
                    hotkey.set(key.clone());
                    if let Some(store) = &store_for_poll {
                        prefs::write(store, SETTING_HOTKEY, &key);
                    }
                    show_notice(notice, "已应用");
                } else {
                    crate::register_hotkey(&hotkey.get());
                    show_notice(notice, "组合键无效");
                }
            }
            None => {
                crate::register_hotkey(&hotkey.get());
                show_notice(notice, "已取消");
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

    // 保留条数
    let mut count_row = view().flex_row().items_center().gap_2();
    for value in [500_i64, 1000, 5000] {
        let store = store.clone();
        count_row = count_row.child(choice(
            format!("{value} 条"),
            move || count.get() == value,
            move || {
                count.set(value);
                if let Some(store) = &store {
                    prefs::write(store, SETTING_RETENTION_COUNT, value);
                }
                show_notice(notice, "已保存");
            },
        ));
    }

    // 保留天数
    let mut days_row = view().flex_row().items_center().gap_2();
    for value in [7_i64, 30, 90, 365] {
        let store = store.clone();
        days_row = days_row.child(choice(
            format!("{value} 天"),
            move || days.get() == value,
            move || {
                days.set(value);
                if let Some(store) = &store {
                    prefs::write(store, SETTING_RETENTION_DAYS, value);
                }
                show_notice(notice, "已保存");
            },
        ));
    }

    let store_for_auto = store.clone();
    let auto_toggle = toggle_control(auto_paste, move |on| {
        auto_paste.set(on);
        if let Some(store) = &store_for_auto {
            prefs::write_bool(store, SETTING_AUTO_PASTE, on);
        }
        show_notice(notice, if on { "已开启自动粘回" } else { "已关闭：回车仅复制" });
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
                    show_notice(notice, "已更新登录项");
                }
                Err(error) => {
                    login.set(!on);
                    show_notice(notice, format!("登录项失败：{error}"));
                }
            }
        }));

    let permission_row = view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(dynamic(move || {
            text(if trusted.get() { "已授权" } else { "未授权" })
                .text_size(px(12.0))
                .text_color(if trusted.get() {
                    theme::primary()
                } else {
                    theme::muted_fg()
                })
        }))
        .child(choice("刷新", || false, move || {
            trusted.set(crate::paste::is_trusted());
        }))
        .child(choice("打开系统设置", || false, || {
            crate::paste::open_accessibility_settings()
        }));

    // 清空历史（两步确认）
    let store_for_clear = store.clone();
    let clear_control = dynamic(move || {
        if confirm_clear.get() {
            choice("确认清空", || true, {
                let store = store_for_clear.clone();
                move || {
                    if let Some(store) = &store {
                        if let Err(error) = store.clear_history() {
                            log::error!("clear history: {error}");
                        }
                        crate::history::bump();
                    }
                    confirm_clear.set(false);
                    show_notice(notice, "已清空历史");
                }
            })
        } else {
            choice("清空历史", || false, move || {
                confirm_clear.set(true);
                show_notice(notice, "再按一次确认");
            })
        }
    });

    // 忽略的 App
    let store_for_add = store.clone();
    let add_row = view()
        .flex_row()
        .items_center()
        .gap_2()
        .px_4()
        .py_3()
        .child(
            input()
                .flex_1()
                .value(new_bundle)
                .on_change(move |value: &str| new_bundle.set(value.to_string()))
                .placeholder("com.apple.Terminal")
                .text_size(px(12.0)),
        )
        .child(choice("添加", || false, move || {
            let bundle = new_bundle.get().trim().to_string();
            if bundle.is_empty() {
                return;
            }
            let mut list = ignored.get();
            if list.iter().any(|item| item == &bundle) {
                show_notice(notice, "已经在列表里");
                return;
            }
            list.push(bundle);
            ignored.set(list.clone());
            if let Some(store) = &store_for_add {
                prefs::save_ignored(store, &list);
            }
            new_bundle.set(String::new());
            show_notice(notice, "已添加");
        }));

    let ignored_rows = dynamic(move || {
        let mut rows = view().flex_col();
        let bundles = ignored.get();
        if bundles.is_empty() {
            rows = rows.child(
                view()
                    .px_4()
                    .py_3()
                    .child(
                        text("还没有忽略任何 App")
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
                    .child(choice("移除", || false, move || {
                        let mut list = ignored.get();
                        list.retain(|item| item != &remove);
                        ignored.set(list.clone());
                        if let Some(store) = &store {
                            prefs::save_ignored(store, &list);
                        }
                        show_notice(notice, "已移除");
                    })),
            );
        }
        rows
    });

    let ignored_card = view()
        .flex_col()
        .child(add_row)
        .child(ignored_rows)
        .child(
            view().px_4().py_2().child(
                text("按 bundle id 忽略；例如 com.apple.Terminal")
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg()),
            ),
        );

    let about_rows = view()
        .flex_col()
        .child(setting_row(
            "版本",
            text(format!("v{}", env!("CARGO_PKG_VERSION")))
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        ))
        .child(setting_row(
            "仓库",
            choice("GitHub", || false, || {
                crate::paste::open_external("https://github.com/wisarmy/podnook")
            }),
        ))
        .child(view().px_4().py_3().child(
            text("本地优先 · 数据不上传")
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        ));

    let history_rows = view()
        .flex_col()
        .child(setting_row("保留条数", count_row))
        .child(setting_row("保留天数", days_row))
        .child(setting_row("清空历史", clear_control));

    let paste_rows = view()
        .flex_col()
        .child(setting_row("自动粘回", auto_toggle))
        .child(view().px_4().pb_3().child(
            text("关闭后，回车只把条目复制回剪贴板，不模拟 ⌘V。")
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ));

    let login_rows = view().flex_col().child(setting_row("开机自启", login_row)).child(
        view().px_4().pb_3().child(
            text("使用 macOS 登录项；开发用的裸 runner 不支持，打包后的 .app 才有效。")
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ),
    );

    let permission_rows = view()
        .flex_col()
        .child(setting_row("辅助功能", permission_row))
        .child(view().px_4().pb_3().child(
            text("若系统设置已开启但这里仍显示未授权：完全退出 App 后重新打开；重新打包会改变 ad-hoc 签名，需要移除旧的辅助功能条目再添加。")
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        ));

    view()
        .flex_1()
        .bg(theme::bg())
        .text_color(theme::fg())
        .child(scroll().flex_1().child(
            view()
                .flex_col()
                .p_6()
                .gap_5()
                .child(settings_section(
                    "呼出",
                    view()
                        .flex_col()
                        .child(setting_row("呼出热键", record_control))
                        .child(view().px_4().pb_3().child(
                            text("点「录制」后直接按下组合键；至少包含 mod / ctrl / alt，Esc 取消。")
                                .text_size(px(11.0))
                                .text_color(theme::muted_fg()),
                        )),
                ))
                .child(settings_section("历史", history_rows))
                .child(settings_section("粘贴", paste_rows))
                .child(settings_section("登录项", login_rows))
                .child(settings_section("权限", permission_rows))
                .child(settings_section("忽略的 App", ignored_card))
                .child(settings_section("关于", about_rows))
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

fn settings_section(title: &'static str, rows: View) -> View {
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
                .bg(theme::card())
                .border_1()
                .border_color(theme::border())
                .overflow_hidden()
                .child(rows),
        )
}

fn setting_row(label: &'static str, control: impl IntoElement) -> View {
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
                theme::accent()
            } else {
                theme::card()
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
            text(move || if on.get() { "开" } else { "关" })
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

fn show_notice(notice: Signal<Option<String>>, message: impl Into<String>) {
    notice.set(Some(message.into()));
    set_timeout(std::time::Duration::from_millis(1600), move || {
        notice.set(None)
    });
}
