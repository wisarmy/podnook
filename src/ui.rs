//! M2：真实数据界面。
//!
//! 信息架构沿用 Raycast Clipboard History：顶部搜索 + 类型筛选，左侧
//! Pinned / Today / 更早 分组列表，右侧预览与元信息，底部操作栏。
//! 数据来自 `podnook-core` 的 SQLite，捕获线程写入后这里自动刷新。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use nsh_base::input;
use nsh_core::desktop::{WindowAttributes, WindowKind, WindowLevel, Workspaces};
use nsh_core::element::desktop::{open_window, window as panel_window, WindowRef};
use nsh_core::keymap::When;
use nsh_core::prelude::*;
use nsh_core::PressEvent;
use podnook_core::db::Store;
use podnook_core::{hash_text, now_secs, Clip, ClipKind, SETTING_AUTO_PASTE};

use crate::theme;

mod settings;
pub use settings::settings;

thread_local! {
    /// 搜索框的 ref：面板每次呼出时把键盘焦点放进去。
    static SEARCH: std::cell::RefCell<Option<ElementRef>> = const { std::cell::RefCell::new(None) };
    /// 面板的界面状态，用于呼出时重置搜索与选中项。
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
    /// 编辑内容窗口。
    static EDITOR: std::cell::RefCell<Option<WindowRef>> = const { std::cell::RefCell::new(None) };
}

/// 面板每次呼出：清空搜索、选中第一条、聚焦搜索框。
pub fn prepare_for_show() {
    STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.query.set(String::new());
            state.selected.set(None);
            state.actions_open.set(false);
            state.actions_mode.set(ActionMode::Main);
            state.actions_query.set(String::new());
            state.actions_selected.set(0);
        }
    });
    SEARCH.with(|search| {
        if let Some(search) = search.borrow().as_ref() {
            search.focus();
        }
    });
}

#[derive(Clone)]
struct State {
    clips: Signal<Vec<Clip>>,
    query: Signal<String>,
    filter: Signal<Option<ClipKind>>,
    selected: Signal<Option<i64>>,
    flash: Signal<Option<String>>,
    search: ElementRef,
    rows: Memo<Vec<Row>>,
    actions_open: Signal<bool>,
    actions_mode: Signal<ActionMode>,
    actions_query: Signal<String>,
    actions_selected: Signal<usize>,
    actions_search: ElementRef,
    save_path: Signal<String>,
    save_input: ElementRef,
}

#[derive(Clone, PartialEq)]
enum Row {
    Section(&'static str),
    Clip(Clip),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionMode {
    Main,
    Range,
    ConfirmAll,
    SaveFile,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Paste,
    Copy,
    Edit,
    Pin,
    SaveFile,
    SaveNow,
    Delete,
    DeleteEntries,
    DeleteAll,
    DeleteRange(i64),
    Back,
    ConfirmDeleteAll,
}

#[derive(Clone)]
struct ActionItem {
    kind: ActionKind,
    label: String,
    shortcut: &'static str,
    destructive: bool,
}

const DELETE_RANGES: &[(i64, &str)] = &[
    (5 * 60, "最近 5 分钟"),
    (15 * 60, "最近 15 分钟"),
    (30 * 60, "最近 30 分钟"),
    (60 * 60, "最近 1 小时"),
    (24 * 60 * 60, "最近 24 小时"),
];

/// 整个应用的骨架。
pub fn shell() -> impl IntoElement {
    let clips = signal(Vec::<Clip>::new());
    let query = signal(String::new());
    let filter = signal(None::<ClipKind>);
    let selected = signal(None::<i64>);
    let flash = signal(None::<String>);
    let rows = computed(move || build_rows(&clips.get(), &query.get(), filter.get()));
    let search = element_ref();
    let actions_search = element_ref();
    let save_input = element_ref();
    SEARCH.with(|slot| *slot.borrow_mut() = Some(search));
    let state = State {
        clips,
        query,
        filter,
        selected,
        flash,
        search,
        rows,
        actions_open: signal(false),
        actions_mode: signal(ActionMode::Main),
        actions_query: signal(String::new()),
        actions_selected: signal(0usize),
        actions_search,
        save_path: signal(String::new()),
        save_input,
    };
    STATE.with(|slot| *slot.borrow_mut() = Some(state.clone()));

    let store = crate::history::init();
    if let Some(store) = &store {
        reload(&state, store);
    }

    // 搜索、筛选、增删之后，保证选中的仍然是可见条目。
    let sel = state.clone();
    effect(move || {
        let ids = visible_ids(&sel.rows.get());
        let current = sel.selected.get();
        if !current.is_some_and(|id| ids.contains(&id)) {
            sel.selected.set(ids.first().copied());
        }
    });

    // 操作列表过滤后，选中项不能越界。
    let act = state.clone();
    effect(move || {
        let len = action_items(&act).len();
        let selected = act.actions_selected.get();
        if len == 0 {
            if selected != 0 {
                act.actions_selected.set(0);
            }
        } else if selected >= len {
            act.actions_selected.set(len - 1);
        }
    });

    // 捕获线程写库后递增 generation，这里按需重新查库。
    let refresh = state.clone();
    let refresh_store = store.clone();
    let last_gen = signal(crate::history::generation());
    set_interval(Duration::from_millis(150), move || {
        let now = crate::history::generation();
        if now == last_gen.get() {
            return;
        }
        last_gen.set(now);
        if let Some(store) = &refresh_store {
            reload(&refresh, store);
        }
    });

    bind_keys(&state, store);
    shell_view(state)
}

fn shell_view(state: State) -> impl IntoElement {
    view()
        .flex_1()
        .bg(theme::bg())
        .text_color(theme::fg())
        .rounded(px(14.0))
        .overflow_hidden()
        .child(header(state.clone()))
        .child(
            view()
                .flex_row()
                .flex_1()
                .child(sidebar(state.clone()))
                .child(preview_pane(state.clone())),
        )
        .child(footer(state.clone()))
        .child(actions_overlay(state.clone()))
}

pub fn open_editor(clip: Clip) {
    close_editor();
    let mut attributes = WindowAttributes::default();
    attributes.activate = Some(false);
    attributes.focusable = Some(true);
    attributes.resizable = Some(true);
    attributes.close_on_escape = Some(true);
    attributes.level = Some(WindowLevel::Floating);
    attributes.workspaces = Some(Workspaces::All);
    let window = panel_window()
        .id("editor")
        .kind(WindowKind::Panel)
        .title("编辑内容")
        .default_size(size(px(640.0), px(460.0)))
        .attributes(attributes)
        .content(move || edit_content(clip.clone()));
    if let Some(window) = open_window(window) {
        window.focus();
        let refocus = window.clone();
        set_timeout(Duration::from_millis(150), move || refocus.focus());
        EDITOR.with(|slot| *slot.borrow_mut() = Some(window));
    }
}

fn close_editor() {
    EDITOR.with(|slot| {
        if let Some(window) = slot.borrow_mut().take() {
            if window.state().is_some() {
                window.close();
            }
        }
    });
}

fn edit_content(clip: Clip) -> impl IntoElement {
    let text_value = signal(clip.content.clone().unwrap_or_default());
    let notice = signal(None::<String>);
    let store = crate::history::init();

    let save_store = store;
    let save_text = text_value;
    let save_notice = notice;
    let save_id = clip.id;
    let save = move || {
        let Some(store) = save_store.as_deref() else {
            return;
        };
        match store.update_text(save_id, &save_text.get()) {
            Ok(()) => {
                crate::history::bump();
                crate::show_panel();
                close_editor();
            }
            Err(e) => {
                log::error!("update clip: {e}");
                save_notice.set(Some("保存失败：内容为空，或与已有条目重复".into()));
            }
        }
    };

    let cancel = || {
        crate::show_panel();
        close_editor();
    };
    let input_text = text_value;

    view()
        .flex_1()
        .flex_col()
        .bg(theme::bg())
        .text_color(theme::fg())
        .p_4()
        .gap_3()
        .child(
            text("编辑内容")
                .text_size(px(13.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .rounded(px(8.0))
                .bg(theme::card())
                .border_1()
                .border_color(theme::border())
                .p_3()
                .child(
                    input()
                        .multiline(true)
                        .flex_1()
                        .value(input_text)
                        .on_change(move |value: &str| input_text.set(value.to_string()))
                        .on_key_press(move |key: &str| {
                            if key == "Escape" {
                                crate::show_panel();
                                close_editor();
                            }
                        })
                        .text_size(px(13.0))
                        .text_color(theme::fg())
                        .auto_focus(true),
                ),
        )
        .child(dynamic(move || {
            if let Some(message) = notice.get() {
                text(message)
                    .text_size(px(11.0))
                    .text_color(theme::danger())
            } else {
                text("保存后更新这条历史记录")
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg())
            }
        }))
        .child(
            view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(editor_button("取消", false, cancel))
                .child(editor_button("保存", true, save)),
        )
}

fn editor_button(
    label: &'static str,
    primary: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .on_press(on_press)
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .bg(if primary {
            theme::primary()
        } else {
            theme::card()
        })
        .border_1()
        .border_color(theme::border())
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(if primary {
                    theme::on_primary()
                } else {
                    theme::fg()
                }),
        )
}

fn safe_name(preview: &str) -> String {
    let cleaned: String = preview
        .chars()
        .map(|c| {
            if matches!(c, '/' | ':' | '\\' | '\n' | '\r' | '\t') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut name: String = cleaned.trim().chars().take(60).collect();
    if name.is_empty() {
        name = "clipboard".into();
    }
    name
}

enum SaveData {
    Bytes(Vec<u8>),
    Copy(PathBuf),
}

fn default_save_path(name: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) => format!("{home}/Downloads/{name}"),
        Err(_) => name.to_string(),
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix("~/") {
            return PathBuf::from(home).join(rest);
        }
        if path == "~" {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}

fn save_target(clip: &Clip) -> Option<(String, SaveData)> {
    match clip.kind {
        ClipKind::Text | ClipKind::Link => {
            let content = clip.content.clone().unwrap_or_default();
            Some((
                format!("{}.txt", safe_name(&clip.preview)),
                SaveData::Bytes(content.into_bytes()),
            ))
        }
        ClipKind::Image => {
            let path = clip.image_path.as_deref()?;
            let bytes = std::fs::read(path).ok()?;
            Some((
                format!("{}.png", safe_name(&clip.preview)),
                SaveData::Bytes(bytes),
            ))
        }
        ClipKind::File => {
            let path = clip.file_uris.first()?;
            let source = PathBuf::from(path);
            let name = source
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| "file".to_string());
            Some((name, SaveData::Copy(source)))
        }
    }
}

fn save_now(state: &State) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let Some((_, data)) = save_target(&clip) else {
        flash(state, "这个条目无法保存为文件");
        return;
    };
    let raw = state.save_path.get();
    let path = expand_tilde(raw.trim());
    if path.as_os_str().is_empty() {
        flash(state, "保存路径不能为空");
        return;
    }
    let result = match data {
        SaveData::Bytes(bytes) => std::fs::write(&path, bytes),
        SaveData::Copy(source) => std::fs::copy(&source, &path).map(|_| ()),
    };
    match result {
        Ok(()) => {
            close_actions(state);
            flash(state, format!("已保存到 {}", path.display()));
        }
        Err(e) => {
            log::error!("save clip to {}: {e}", path.display());
            flash(state, "保存失败，检查路径后重试");
        }
    }
}

fn actions_overlay(state: State) -> impl IntoElement {
    dynamic(move || actions_overlay_inner(state.clone()))
}

fn actions_overlay_inner(state: State) -> View {
    if !state.actions_open.get() {
        return view();
    }
    let title = match state.actions_mode.get() {
        ActionMode::Main => "操作",
        ActionMode::Range => "删除最近…",
        ActionMode::ConfirmAll => "清空全部",
        ActionMode::SaveFile => "保存为文件",
    };
    let selected = state.actions_selected.get();
    let items = action_items(&state);

    let mut list = view().flex_col().gap_1().p_2().overflow_hidden();
    if items.is_empty() {
        list = list.child(
            text("没有匹配的操作")
                .p_3()
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        );
    }
    for (index, item) in items.into_iter().enumerate() {
        list = list.child(action_row(state.clone(), item, index == selected));
    }

    let save_mode = state.actions_mode.get() == ActionMode::SaveFile;
    let body = if save_mode {
        let path = state.save_path;
        let input_ref = state.save_input;
        let escape_state = state.clone();
        view()
            .flex_col()
            .p_3()
            .gap_2()
            .child(
                text("保存到")
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg()),
            )
            .child(
                input()
                    .element_ref(&input_ref)
                    .value(path)
                    .on_change(move |value: &str| path.set(value.to_string()))
                    .on_key_press(move |key: &str| {
                        if key == "Escape" {
                            close_actions(&escape_state);
                        }
                    })
                    .text_size(px(12.0))
                    .placeholder("~/Downloads/…")
                    .auto_focus(true),
            )
            .child(
                text("Enter 保存 · Esc 取消")
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg()),
            )
    } else {
        list
    };

    let backdrop_state = state.clone();
    let search_ref = state.actions_search;
    let query = state.actions_query;
    let change_state = state.clone();
    let escape_state = state.clone();

    view()
        .absolute()
        .inset(px(0.0))
        .z_index(40)
        .child(
            view()
                .absolute()
                .inset(px(0.0))
                .on_press(move || close_actions(&backdrop_state)),
        )
        .child(
            view()
                .absolute()
                .top(px(64.0))
                .right(px(20.0))
                .w(px(360.0))
                .max_h(px(520.0))
                .z_index(41)
                .flex_col()
                .rounded(px(12.0))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::border())
                .shadow_lg()
                .overflow_hidden()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px_4()
                        .py_3()
                        .border_b(px(1.0))
                        .border_b_color(theme::border())
                        .child(text(title).text_size(px(12.0)).text_color(theme::muted_fg()))
                        .child(text("⌘K 关闭").text_size(px(11.0)).text_color(theme::muted_fg())),
                )
                .child(body)
                .child(if save_mode {
                    view()
                } else {
                    view()
                        .px_3()
                        .py_2()
                        .border_t(px(1.0))
                        .border_t_color(theme::border())
                        .child(
                            input()
                                .element_ref(&search_ref)
                                .value(query)
                                .on_change(move |value: &str| {
                                    change_state.actions_query.set(value.to_string());
                                    change_state.actions_selected.set(0);
                                })
                                .on_key_press(move |key: &str| {
                                    if key == "Escape" {
                                        close_actions(&escape_state);
                                    }
                                })
                                .placeholder("搜索操作…")
                                .text_size(px(12.0)),
                        )
                }),
        )
}

fn action_row(state: State, item: ActionItem, selected: bool) -> View {
    let store = crate::history::init();
    let kind = item.kind;
    let label = item.label;
    let shortcut = item.shortcut;
    let destructive = item.destructive;

    let mut row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .py_2()
        .rounded(px(8.0))
        .bg(move || {
            if selected {
                theme::accent()
            } else {
                theme::surface()
            }
        })
        .on_press(move || {
            let store = store.clone();
            run_action(&state, store.as_deref(), kind);
        })
        .child(
            text(label)
                .text_size(px(13.0))
                .text_color(if destructive {
                    theme::danger()
                } else {
                    theme::fg()
                }),
        );
    if !shortcut.is_empty() {
        row = row.child(
            text(key_label(shortcut))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        );
    }
    row
}

// ---------------------------------------------------------------- 数据

fn reload(state: &State, store: &Store) {
    match store.list(1000) {
        Ok(clips) => state.clips.set(clips),
        Err(e) => log::error!("list history: {e}"),
    }
}

fn selected_clip(state: &State) -> Option<Clip> {
    let id = state.selected.get()?;
    state.clips.get().into_iter().find(|clip| clip.id == id)
}

fn visible_ids(rows: &[Row]) -> Vec<i64> {
    rows.iter()
        .filter_map(|row| match row {
            Row::Clip(clip) => Some(clip.id),
            Row::Section(_) => None,
        })
        .collect()
}

fn build_rows(clips: &[Clip], query: &str, filter: Option<ClipKind>) -> Vec<Row> {
    let q = query.trim().to_lowercase();
    let today = local_today();
    let mut pinned = Vec::new();
    let mut today_rows = Vec::new();
    let mut older = Vec::new();

    for clip in clips {
        if filter.is_some_and(|kind| kind != clip.kind) {
            continue;
        }
        if !q.is_empty() && !matches_query(clip, &q) {
            continue;
        }
        if clip.pinned {
            pinned.push(clip.clone());
        } else if local_date(clip.last_copied_at) == Some(today) {
            today_rows.push(clip.clone());
        } else {
            older.push(clip.clone());
        }
    }

    let mut rows = Vec::new();
    push_group(&mut rows, "已固定", pinned);
    push_group(&mut rows, "今天", today_rows);
    push_group(&mut rows, "更早", older);
    rows
}

fn push_group(rows: &mut Vec<Row>, label: &'static str, group: Vec<Clip>) {
    if group.is_empty() {
        return;
    }
    rows.push(Row::Section(label));
    rows.extend(group.into_iter().map(Row::Clip));
}

fn matches_query(clip: &Clip, q: &str) -> bool {
    let mut hay = String::new();
    if let Some(content) = &clip.content {
        hay.push_str(content);
    }
    hay.push_str(&clip.preview);
    hay.push_str(&clip.file_uris.join(" "));
    if let Some(app) = &clip.source_app {
        hay.push_str(app);
    }
    hay.push_str(clip.kind.label());
    hay.to_lowercase().contains(q)
}

fn local_date(secs: i64) -> Option<jiff::civil::Date> {
    Timestamp::from_second(secs)
        .ok()
        .map(|ts| ts.to_zoned(TimeZone::system()).date())
}

fn local_today() -> jiff::civil::Date {
    Timestamp::now().to_zoned(TimeZone::system()).date()
}

fn format_time(secs: i64) -> String {
    let Ok(ts) = Timestamp::from_second(secs) else {
        return String::new();
    };
    let zoned = ts.to_zoned(TimeZone::system());
    if zoned.date() == local_today() {
        zoned.strftime("%H:%M").to_string()
    } else {
        zoned.strftime("%m-%d %H:%M").to_string()
    }
}

fn human_size(bytes: i64) -> String {
    let bytes = bytes.max(0);
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------- 交互

fn bind_keys(state: &State, store: Option<Arc<Store>>) {
    // 搜索框一直有焦点，方向键、回车等必须仍然可用。
    let always = || KeyOptions {
        when: When::Always,
        ..KeyOptions::default()
    };

    let s = state.clone();
    on_key_with("down", always(), move |_| {
        if s.actions_open.get() {
            move_action(&s, 1);
        } else {
            move_selection(&s, 1);
        }
    });
    let s = state.clone();
    on_key_with("up", always(), move |_| {
        if s.actions_open.get() {
            move_action(&s, -1);
        } else {
            move_selection(&s, -1);
        }
    });
    let s = state.clone();
    let db = store.clone();
    on_key_with("enter", always(), move |_| {
        if s.actions_open.get() {
            if s.actions_mode.get() == ActionMode::SaveFile {
                save_now(&s);
            } else {
                let items = action_items(&s);
                if let Some(item) = items.get(s.actions_selected.get()).cloned() {
                    let store = crate::history::init();
                    run_action(&s, store.as_deref(), item.kind);
                }
            }
        } else {
            activate_selected(&s, db.as_deref(), true);
        }
    });
    let s = state.clone();
    let db = store.clone();
    on_key_with("mod+enter", always(), move |_| {
        activate_selected(&s, db.as_deref(), false)
    });
    let s = state.clone();
    on_key_with("mod+e", always(), move |_| {
        if let Some(clip) = selected_clip(&s) {
            if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
                s.actions_open.set(false);
                open_editor(clip);
            }
        }
    });
    let s = state.clone();
    on_key_with("alt+shift+mod+s", always(), move |_| {
        if selected_clip(&s).is_some() {
            enter_save_mode(&s);
        }
    });
    // Raycast 对齐：固定是 ⇧⌘P；⌘. 留给 Append to Clipboard。
    let s = state.clone();
    let db = store.clone();
    on_key_with("shift+mod+p", always(), move |_| {
        toggle_pin(&s, db.as_deref());
        if s.actions_open.get() {
            close_actions(&s);
        }
    });
    let s = state.clone();
    on_key_with("mod+p", always(), move |_| cycle_filter(&s));
    // Raycast 对齐：⌃X 删除当前条目。
    let s = state.clone();
    let db = store.clone();
    on_key_with("ctrl+x", always(), move |_| {
        delete_selected(&s, db.as_deref());
        if s.actions_open.get() {
            close_actions(&s);
        }
    });
    // ⇧⌘X 打开「删除最近…」，⌃⇧X 打开「清空全部」确认。
    let s = state.clone();
    on_key_with("shift+mod+x", always(), move |_| {
        enter_action_mode(&s, ActionMode::Range)
    });
    let s = state.clone();
    on_key_with("ctrl+shift+x", always(), move |_| {
        enter_action_mode(&s, ActionMode::ConfirmAll)
    });
    let s = state.clone();
    on_key_with("mod+k", always(), move |_| {
        if s.actions_open.get() {
            close_actions(&s);
        } else {
            enter_action_mode(&s, ActionMode::Main);
        }
    });
    let s = state.clone();
    on_key_with("escape", always(), move |_| {
        if s.actions_open.get() {
            close_actions(&s);
        } else {
            crate::hide_panel();
        }
    });
    on_key("mod+,", |_| crate::open_settings());
}

fn enter_action_mode(state: &State, mode: ActionMode) {
    state.actions_mode.set(mode);
    state.actions_query.set(String::new());
    state.actions_selected.set(0);
    if !state.actions_open.get() {
        state.actions_open.set(true);
    }
    let search = state.actions_search;
    set_timeout(Duration::from_millis(30), move || search.focus());
}

fn enter_save_mode(state: &State) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let Some((name, _)) = save_target(&clip) else {
        return;
    };
    state.save_path.set(default_save_path(&name));
    state.actions_mode.set(ActionMode::SaveFile);
    state.actions_query.set(String::new());
    state.actions_selected.set(0);
    if !state.actions_open.get() {
        state.actions_open.set(true);
    }
    let input = state.save_input;
    set_timeout(Duration::from_millis(30), move || input.focus());
}

fn close_actions(state: &State) {
    state.actions_open.set(false);
    state.actions_mode.set(ActionMode::Main);
    state.actions_query.set(String::new());
    state.actions_selected.set(0);
    let search = state.search;
    set_timeout(Duration::from_millis(30), move || search.focus());
}

fn move_action(state: &State, delta: isize) {
    let len = action_items(state).len();
    if len == 0 {
        return;
    }
    let current = state.actions_selected.get().min(len - 1) as isize;
    let next = (current + delta).clamp(0, len as isize - 1) as usize;
    state.actions_selected.set(next);
}

fn action_items(state: &State) -> Vec<ActionItem> {
    let pin_label = match selected_clip(state) {
        Some(clip) if clip.pinned => "取消固定",
        _ => "固定",
    };

    let editable = matches!(
        selected_clip(state).map(|clip| clip.kind),
        Some(ClipKind::Text) | Some(ClipKind::Link)
    );

    let mut items = match state.actions_mode.get() {
        ActionMode::Range => DELETE_RANGES
            .iter()
            .map(|(seconds, label)| ActionItem {
                kind: ActionKind::DeleteRange(*seconds),
                label: (*label).to_string(),
                shortcut: "",
                destructive: true,
            })
            .collect::<Vec<_>>(),
        ActionMode::ConfirmAll => vec![
            ActionItem {
                kind: ActionKind::ConfirmDeleteAll,
                label: "确认清空全部历史（含固定）".into(),
                shortcut: "enter",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::Back,
                label: "取消".into(),
                shortcut: "escape",
                destructive: false,
            },
        ],
        ActionMode::SaveFile => vec![
            ActionItem {
                kind: ActionKind::SaveNow,
                label: "保存".into(),
                shortcut: "enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Back,
                label: "取消".into(),
                shortcut: "escape",
                destructive: false,
            },
        ],
        ActionMode::Main => vec![
            ActionItem {
                kind: ActionKind::Paste,
                label: "粘贴".into(),
                shortcut: "enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Copy,
                label: "仅复制".into(),
                shortcut: "mod+enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Edit,
                label: "编辑内容".into(),
                shortcut: "mod+e",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Pin,
                label: pin_label.into(),
                shortcut: "shift+mod+p",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::SaveFile,
                label: "保存为文件…".into(),
                shortcut: "alt+shift+mod+s",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Delete,
                label: "删除".into(),
                shortcut: "ctrl+x",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::DeleteEntries,
                label: "删除最近…".into(),
                shortcut: "shift+mod+x",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::DeleteAll,
                label: "清空全部…".into(),
                shortcut: "ctrl+shift+x",
                destructive: true,
            },
        ],
    };

    if state.actions_mode.get() == ActionMode::Range {
        items.push(ActionItem {
            kind: ActionKind::Back,
            label: "返回".into(),
            shortcut: "escape",
            destructive: false,
        });
    }

    if !editable {
        items.retain(|item| item.kind != ActionKind::Edit);
    }

    let query = state.actions_query.get().trim().to_lowercase();
    if !query.is_empty() {
        items.retain(|item| item.label.to_lowercase().contains(&query));
    }
    items
}

fn range_label(seconds: i64) -> &'static str {
    match seconds {
        300 => "5 分钟",
        900 => "15 分钟",
        1800 => "30 分钟",
        3600 => "1 小时",
        86400 => "24 小时",
        _ => "时间范围",
    }
}

fn run_action(state: &State, store: Option<&Store>, kind: ActionKind) {
    match kind {
        ActionKind::Paste => {
            state.actions_open.set(false);
            activate_selected(state, store, true);
        }
        ActionKind::Copy => {
            state.actions_open.set(false);
            activate_selected(state, store, false);
        }
        ActionKind::Edit => {
            if let Some(clip) = selected_clip(state) {
                if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
                    state.actions_open.set(false);
                    open_editor(clip);
                }
            }
        }
        ActionKind::SaveFile => {
            if let Some(clip) = selected_clip(state) {
                if save_target(&clip).is_some() {
                    enter_save_mode(state);
                }
            }
        }
        ActionKind::SaveNow => save_now(state),
        ActionKind::Pin => {
            toggle_pin(state, store);
            close_actions(state);
        }
        ActionKind::Delete => {
            delete_selected(state, store);
            close_actions(state);
        }
        ActionKind::DeleteEntries => enter_action_mode(state, ActionMode::Range),
        ActionKind::DeleteAll => enter_action_mode(state, ActionMode::ConfirmAll),
        ActionKind::Back => {
            if state.actions_mode.get() == ActionMode::SaveFile {
                close_actions(state);
            } else {
                enter_action_mode(state, ActionMode::Main);
            }
        }
        ActionKind::DeleteRange(seconds) => {
            let Some(store) = store else {
                return;
            };
            let cutoff = now_secs() - seconds;
            match store.delete_since(cutoff) {
                Ok(count) => {
                    crate::history::bump();
                    reload(state, store);
                    close_actions(state);
                    flash(
                        state,
                        format!("已删除最近 {} 的 {count} 条（固定项保留）", range_label(seconds)),
                    );
                }
                Err(e) => log::error!("delete since: {e}"),
            }
        }
        ActionKind::ConfirmDeleteAll => {
            let Some(store) = store else {
                return;
            };
            match store.clear_history() {
                Ok(count) => {
                    crate::history::bump();
                    reload(state, store);
                    close_actions(state);
                    flash(state, format!("已清空 {count} 条历史"));
                }
                Err(e) => log::error!("clear history: {e}"),
            }
        }
    }
}

fn cycle_filter(state: &State) {
    let next = match state.filter.get() {
        None => Some(ClipKind::Text),
        Some(ClipKind::Text) => Some(ClipKind::Link),
        Some(ClipKind::Link) => Some(ClipKind::Image),
        Some(ClipKind::Image) => Some(ClipKind::File),
        Some(ClipKind::File) => None,
    };
    state.filter.set(next);
}

fn move_selection(state: &State, delta: isize) {
    let ids = visible_ids(&state.rows.get());
    if ids.is_empty() {
        state.selected.set(None);
        return;
    }
    let next = match state.selected.get().and_then(|id| ids.iter().position(|x| *x == id)) {
        Some(current) => (current as isize + delta).clamp(0, ids.len() as isize - 1) as usize,
        None => 0,
    };
    state.selected.set(Some(ids[next]));
}

fn activate_selected(state: &State, store: Option<&Store>, paste: bool) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let ok = match clip.kind {
        ClipKind::Text | ClipKind::Link => match &clip.content {
            Some(text) => {
                set_clipboard(text.clone());
                true
            }
            None => false,
        },
        ClipKind::Image => match &clip.image_path {
            Some(path) => match std::fs::read(path) {
                Ok(bytes) => {
                    let data = format!("data:image/png;base64,{}", BASE64.encode(bytes));
                    set_clipboard_image(data);
                    true
                }
                Err(e) => {
                    log::error!("read image {path}: {e}");
                    false
                }
            },
            None => false,
        },
        ClipKind::File => {
            #[cfg(target_os = "macos")]
            {
                crate::clipboard::write_files(&clip.file_uris)
            }
            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }
    };
    if !ok {
        return;
    }

    // 设置里关掉「自动粘回」时，回车等同于仅复制。
    let auto_paste = store
        .map(|store| crate::settings::read_bool(store, SETTING_AUTO_PASTE, true))
        .unwrap_or(true);
    let paste = paste && auto_paste;

    // 写回剪贴板的可能是截断后的文本；自身写入的指纹要按写进去的内容算。
    let written_hash = match clip.kind {
        ClipKind::Text | ClipKind::Link => clip
            .content
            .as_deref()
            .map(hash_text)
            .unwrap_or_else(|| clip.hash.clone()),
        _ => clip.hash.clone(),
    };
    if let Some(store) = store {
        store.mark_self_write(written_hash);
    }

    if paste {
        #[cfg(target_os = "macos")]
        {
            if crate::paste::is_trusted() {
                crate::hide_panel();
                crate::paste::paste_after_hide();
            } else {
                crate::paste::prompt_trust_once();
                flash(state, "已复制 · 开启辅助功能后可自动粘贴");
            }
            return;
        }
    }

    crate::hide_panel();
    flash(state, "已复制");
}

fn toggle_pin(state: &State, store: Option<&Store>) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let Some(store) = store else { return };
    if let Err(e) = store.set_pinned(clip.id, !clip.pinned) {
        log::error!("pin {}: {e}", clip.id);
        return;
    }
    crate::history::bump();
    reload(state, store);
    flash(state, if clip.pinned { "已取消固定" } else { "已固定" });
}

fn delete_selected(state: &State, store: Option<&Store>) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let Some(store) = store else { return };
    if let Err(e) = store.delete(clip.id) {
        log::error!("delete {}: {e}", clip.id);
        return;
    }
    crate::history::bump();
    reload(state, store);
    flash(state, "已删除");
}

fn flash(state: &State, message: impl Into<String>) {
    let flash = state.flash;
    flash.set(Some(message.into()));
    set_timeout(Duration::from_millis(1400), move || flash.set(None));
}

// ---------------------------------------------------------------- 顶栏

fn header(state: State) -> impl IntoElement {
    view()
        .window_drag(true)
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .border_b(px(1.0))
        .border_b_color(theme::border())
        .child(search_box(state.clone()))
        .child(filter_bar(state))
}

fn search_box(state: State) -> impl IntoElement {
    let query = state.query;
    let search = state.search;
    view()
        .flex_1()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded(px(8.0))
        .bg(theme::card())
        .border_1()
        .border_color(theme::border())
        .child(
            text("⌕")
                .text_size(px(16.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            input()
                .flex_1()
                .element_ref(&search)
                .value(query)
                .on_change(move |value: &str| query.set(value.to_string()))
                .placeholder("搜索剪贴板历史…")
                .placeholder_color(rgb(0x8A8178))
                .text_color(theme::fg())
                .text_size(px(14.0))
                // 输入框聚焦时先吃掉 Esc 并失焦；在这里直接处理，
                // 避免要按两次。操作面板打开时 Esc 先关面板。
                .on_key_press({
                    let state = state.clone();
                    move |key: &str| {
                        if key == "Escape" {
                            if state.actions_open.get() {
                                close_actions(&state);
                            } else {
                                crate::hide_panel();
                            }
                        }
                    }
                })
                .auto_focus(true),
        )
}

fn filter_bar(state: State) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(filter_pill(state.clone(), None, "全部"))
        .child(filter_pill(state.clone(), Some(ClipKind::Text), "文本"))
        .child(filter_pill(state.clone(), Some(ClipKind::Link), "链接"))
        .child(filter_pill(state.clone(), Some(ClipKind::Image), "图片"))
        .child(filter_pill(state, Some(ClipKind::File), "文件"))
}

fn filter_pill(state: State, kind: Option<ClipKind>, label: &'static str) -> impl IntoElement {
    let filter = state.filter;
    view()
        .on_press(move || filter.set(kind))
        .px_3()
        .py_2()
        .rounded(px(8.0))
        .bg(move || {
            if filter.get() == kind {
                theme::accent()
            } else {
                theme::card()
            }
        })
        .border_1()
        .border_color(theme::border())
        .child(text(label).text_size(px(12.0)).text_color(move || {
            if filter.get() == kind {
                theme::accent_fg()
            } else {
                theme::muted_fg()
            }
        }))
}

// ---------------------------------------------------------------- 列表

#[derive(Clone, PartialEq, Eq, Hash)]
enum RowKey {
    Section(&'static str),
    Clip(i64),
}

fn sidebar(state: State) -> impl IntoElement {
    let rows = state.rows;
    let selected = state.selected;
    view()
        .w(px(theme::SIDEBAR_WIDTH))
        .flex_col()
        .bg(theme::surface())
        .border_r(px(1.0))
        .border_r_color(theme::border())
        .child(
            scroll().flex_1().child(
                view().flex_col().py_2().child(each(
                    move || rows.get(),
                    |row| match row {
                        Row::Section(label) => RowKey::Section(label),
                        Row::Clip(clip) => RowKey::Clip(clip.id),
                    },
                    move |row, _index| match row.get() {
                        Row::Section(label) => view().px_4().pt_4().pb_2().child(
                            text(label)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::muted_fg()),
                        ),
                        Row::Clip(clip) => {
                            let id = clip.id;
                            let row_state = state.clone();
                            view()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .mx_2()
                                .px_2()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(move || {
                                    if selected.get() == Some(id) {
                                        theme::accent()
                                    } else {
                                        theme::surface()
                                    }
                                })
                                .on_press(move |event: &PressEvent| {
                                    selected.set(Some(id));
                                    if event.clicks >= 2 {
                                        if let Some(store) = crate::history::init() {
                                            activate_selected(&row_state, Some(store.as_ref()), true);
                                        }
                                    }
                                })
                                .child(icon_box(&clip))
                                .child(
                                    view().flex_1().child(
                                        text(clip.preview.clone())
                                            .truncate()
                                            .text_size(px(14.0))
                                            .text_color(move || {
                                                if selected.get() == Some(id) {
                                                    theme::accent_fg()
                                                } else {
                                                    theme::fg()
                                                }
                                            }),
                                    ),
                                )
                        }
                    },
                ))),
        )
}

fn icon_box(clip: &Clip) -> impl IntoElement {
    let base = view()
        .size(px(26.0))
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .bg(theme::card())
        .border_1()
        .border_color(theme::border());

    if clip.kind == ClipKind::Image {
        if let Some(path) = &clip.image_path {
            return base.overflow_hidden().child(
                image(path.clone())
                    .w(px(26.0))
                    .h(px(26.0))
                    .object_fit(ResizeMode::Cover),
            );
        }
    }

    let glyph = match clip.kind {
        ClipKind::Text => "T",
        ClipKind::Link => "↗",
        ClipKind::Image => "◫",
        ClipKind::File => "▤",
    };
    base.child(
        text(glyph)
            .text_size(px(12.0))
            .text_color(theme::muted_fg()),
    )
}

// ---------------------------------------------------------------- 预览

fn preview_pane(state: State) -> impl IntoElement {
    view()
        .flex_1()
        .flex_col()
        .child(dynamic(move || preview_content(state.clone())))
}

fn preview_content(state: State) -> impl IntoElement {
    let Some(clip) = selected_clip(&state) else {
        return view()
            .flex_1()
            .items_center()
            .justify_center()
            .child(
                text("复制点什么，就会出现在这里")
                    .text_size(px(13.0))
                    .text_color(theme::muted_fg()),
            );
    };
    view()
        .flex_1()
        .flex_col()
        .child(view().flex_1().p_6().child(preview_body(clip.clone())))
        .child(information(clip.clone()))
}

fn preview_body(clip: Clip) -> impl IntoElement {
    let mut body = view().flex_1().w_full();
    match clip.kind {
        ClipKind::Text | ClipKind::Link => {
            let text_value = clip.content.clone().unwrap_or_default();
            let truncated = clip.truncated;
            body = body.child(
                scroll().flex_1().child(
                    view()
                        .p_2()
                        .gap_2()
                        .child(
                            text(text_value)
                                .text_size(px(14.0))
                                .text_color(theme::fg())
                                .selectable(true),
                        )
                        .when(truncated, |v| {
                            v.child(
                                text(format!(
                                    "内容过长，仅保存了前 {}。",
                                    human_size(clip.content.as_ref().map_or(0, |c| c.len() as i64))
                                ))
                                .text_size(px(12.0))
                                .text_color(theme::muted_fg()),
                            )
                        }),
                ),
            );
        }
        ClipKind::Image => {
            if let Some(path) = &clip.image_path {
                body = body.child(
                    view()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .child(
                            image(path.clone())
                                .object_fit(ResizeMode::Contain)
                                .w_full()
                                .h_full(),
                        ),
                );
            }
        }
        ClipKind::File => {
            body = body.child(
                view()
                    .flex_1()
                    .gap_2()
                    .child(
                        text(if clip.file_uris.len() > 1 {
                            format!("{} 个文件", clip.file_uris.len())
                        } else {
                            "1 个文件".to_string()
                        })
                        .text_size(px(13.0))
                        .text_color(theme::muted_fg()),
                    )
                    .children(clip.file_uris.iter().map(|path| {
                        text(path.clone())
                            .text_size(px(14.0))
                            .text_color(theme::fg())
                    })),
            );
        }
    }
    body
}

fn information(clip: Clip) -> impl IntoElement {
    let source = clip.source_app.clone().unwrap_or_else(|| "未知".to_string());
    let mut info = view()
        .flex_col()
        .gap_3()
        .px_6()
        .py_5()
        .border_t(px(1.0))
        .border_t_color(theme::border())
        .child(
            text("信息")
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted_fg()),
        )
        .child(info_row("来源", source))
        .child(info_row("类型", clip.kind.label().to_string()));

    if clip.kind == ClipKind::Image {
        if let (Some(w), Some(h)) = (clip.image_w, clip.image_h) {
            info = info.child(info_row("尺寸", format!("{w} × {h}")));
        }
    }
    if clip.kind == ClipKind::File {
        info = info.child(info_row("文件数", clip.file_uris.len().to_string()));
    }
    if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
        if let Some(content) = &clip.content {
            info = info.child(info_row("字符数", content.chars().count().to_string()));
        }
    }
    if !clip.file_uris.is_empty() {
        info = info.child(info_row("路径", clip.file_uris.join("\n")));
    }
    info.child(info_row("大小", human_size(clip.byte_size)))
        .child(info_row("复制于", format_time(clip.last_copied_at)))
}

fn info_row(label: &'static str, value: String) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            text(label)
                .text_size(px(13.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            text(value)
                .truncate()
                .text_size(px(13.0))
                .text_color(theme::fg()),
        )
}

// ---------------------------------------------------------------- 底栏

fn footer(state: State) -> impl IntoElement {
    let state = state.clone();
    dynamic(move || footer_inner(state.clone()))
}

fn footer_inner(state: State) -> impl IntoElement {
    let total = state.clips.get().len();
    let visible = visible_ids(&state.rows.get()).len();
    let count = if visible == total {
        format!("剪贴板历史 · {total} 条")
    } else {
        format!("剪贴板历史 · {visible} / {total} 条")
    };
    let mut right = view().flex_row().items_center().gap_4();
    right = match state.flash.get() {
        Some(message) => right.child(
            text(message)
                .text_size(px(12.0))
                .text_color(theme::primary()),
        ),
        None => right
            .child(hint("粘贴", key_label("enter")))
            .child(hint("操作", key_label("mod+k")))
            .child(hint("固定", key_label("shift+mod+p")))
            .child(hint("筛选", key_label("mod+p")))
            .child(hint("删除", key_label("ctrl+x"))),
    };

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .px_4()
        .py_2()
        .border_t(px(1.0))
        .border_t_color(theme::border())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(view().size(px(8.0)).rounded_full().bg(theme::primary()))
                .child(
                    text(count)
                        .text_size(px(12.0))
                        .text_color(theme::muted_fg()),
                ),
        )
        .child(right)
}

fn hint(label: &'static str, key: String) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            view()
                .px_2()
                .py_1()
                .rounded(px(5.0))
                .bg(theme::card())
                .border_1()
                .border_color(theme::border())
                .child(
                    text(key.to_string())
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                ),
        )
}
