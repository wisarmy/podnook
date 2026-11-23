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
use nsh_core::element::desktop::{open_window, window as panel_window, WindowRef};
use nsh_core::keymap::When;
use nsh_core::prelude::*;
use nsh_core::{
    MacOsBackdrop, MacOsBackdropBlending, PressEvent,
    platform::HourCycle,
};
use podnook_core::db::Store;
use podnook_core::{
    hash_text, now_secs, preview_for_text, Clip, ClipKind, Snippet, SETTING_AUTO_PASTE,
    SETTING_PLAIN_PASTE,
};

use crate::theme;

mod settings;
pub use settings::settings;

thread_local! {
    /// 搜索框的 ref：面板每次呼出时把键盘焦点放进去。
    static SEARCH: std::cell::RefCell<Option<ElementRef>> = const { std::cell::RefCell::new(None) };
    /// 面板的界面状态，用于呼出时重置搜索与选中项。
    static STATE: std::cell::RefCell<Option<State>> = const { std::cell::RefCell::new(None) };
    /// 历史列表的滚动容器：删掉最后一条时把滚动条真正拉回顶部。
    static SIDEBAR: std::cell::RefCell<Option<ElementRef>> = const { std::cell::RefCell::new(None) };
    /// 历史列表每一行的 ref：选中项变化时把它滚进视野。
    static ROW_REFS: std::cell::RefCell<std::collections::HashMap<i64, ElementRef>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// 下次呼出是否回到默认选中：只有 Esc 收起时才重置，点别处消失则保留。
    static RESET_ON_SHOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn row_ref(id: i64) -> ElementRef {
    ROW_REFS.with(|refs| *refs.borrow_mut().entry(id).or_insert_with(element_ref))
}

/// 选中某条并把它带进视野（Rolling 到起点对齐：首行即顶部）。
fn select_and_reveal(state: &State, id: Option<i64>) {
    state.selected.set(id);
    if let Some(id) = id {
        reveal_row(id);
    }
}

/// 把列表滚到最顶部（分组标题也露出来），而不是只把首行对齐到可视区。
fn scroll_sidebar_to_top() {
    set_timeout(Duration::from_millis(0), || {
        SIDEBAR.with(|slot| {
            if let Some(scroll) = slot.borrow().as_ref() {
                scroll.scroll_to_offset(px(0.0), true);
            }
        });
    });
}

/// 把某一行滚进视野：延后一帧，等新布局提交后再滚。
fn reveal_row(id: i64) {
    let row = row_ref(id);
    set_timeout(Duration::from_millis(0), move || {
        row.reveal(RevealOptions::default())
    });
}

/// Esc 收起面板：下次呼出清空搜索并回到最新一条。
pub fn reset_on_next_show() {
    RESET_ON_SHOW.with(|flag| flag.set(true));
}

/// 面板每次呼出：Esc 收起时清空搜索、回到最新一条；点别处消失则保留上次的选择。
pub fn prepare_for_show() {
    let reset = RESET_ON_SHOW.with(|flag| flag.replace(false));
    // ⌘⇧V 的语义是打开剪贴板历史：除非正在编辑片段，否则每次都回到历史。
    STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            if !state.editing.get() && state.source.get() != Source::History {
                switch_source(state, Source::History);
            }
        }
    });
    STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            if reset {
                state.query.set(String::new());
                state.selected.set(None);
            }
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
    snippets: Signal<Vec<Snippet>>,
    source: Signal<Source>,
    editing: Signal<bool>,
    edit_clip: Signal<Option<i64>>,
    edit_id: Signal<Option<i64>>,
    edit_name: Signal<String>,
    edit_content: Signal<String>,
    edit_keyword: Signal<String>,
    placeholder_menu: Signal<bool>,
    placeholder_selected: Signal<usize>,
    query: Signal<String>,
    filter: Signal<Option<ClipKind>>,
    selected: Signal<Option<i64>>,
    flash: Signal<Option<String>>,
    flash_until: Signal<i64>,
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
    Section(String),
    Clip(Clip),
    Snippet(Snippet),
}

/// 面板当前浏览的来源。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    History,
    Snippets,
}

/// 来源的切换顺序：以后加新来源就加在这里。
const SOURCES: &[Source] = &[Source::History, Source::Snippets];

fn next_source(current: Source) -> Source {
    let index = SOURCES
        .iter()
        .position(|source| *source == current)
        .unwrap_or(0);
    SOURCES[(index + 1) % SOURCES.len()]
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
    Open,
    Reveal,
    Delete,
    DeleteEntries,
    DeleteAll,
    DeleteRange(i64),
    Back,
    ConfirmDeleteAll,
    SaveSnippet,
    NewSnippet,
    PinSnippet,
    DuplicateSnippet,
    PastePlain,
}

#[derive(Clone)]
struct ActionItem {
    kind: ActionKind,
    label: String,
    shortcut: &'static str,
    destructive: bool,
}

/// 删除最近…里可选的时间范围。
fn delete_ranges() -> Vec<(i64, String)> {
    vec![
        (5 * 60, t!("range.m5")),
        (15 * 60, t!("range.m15")),
        (30 * 60, t!("range.m30")),
        (60 * 60, t!("range.h1")),
        (24 * 60 * 60, t!("range.h24")),
    ]
}

/// 整个应用的骨架。
pub fn shell() -> impl IntoElement {
    let clips = signal(Vec::<Clip>::new());
    let snippets = signal(Vec::<Snippet>::new());
    let source = signal(Source::History);
    let query = signal(String::new());
    let filter = signal(None::<ClipKind>);
    let selected = signal(None::<i64>);
    let flash = signal(None::<String>);
    let rows = computed(move || {
        build_rows(
            &clips.get(),
            &snippets.get(),
            source.get(),
            &query.get(),
            filter.get(),
        )
    });
    let search = element_ref();
    let actions_search = element_ref();
    let save_input = element_ref();
    SEARCH.with(|slot| *slot.borrow_mut() = Some(search));
    let state = State {
        clips,
        snippets,
        source,
        editing: signal(false),
        edit_clip: signal(None::<i64>),
        edit_id: signal(None::<i64>),
        edit_name: signal(String::new()),
        edit_content: signal(String::new()),
        edit_keyword: signal(String::new()),
        placeholder_menu: signal(false),
        placeholder_selected: signal(0usize),
        query,
        filter,
        selected,
        flash,
        flash_until: signal(0i64),
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
        let rows = sel.rows.get();
        let ids = visible_ids(&rows);
        let next = match sel.selected.get() {
            // 呼出或首次加载：默认落在最新的未固定条目（Raycast 行为）。
            None => default_selection(&rows),
            Some(id) if !ids.contains(&id) => ids.first().copied(),
            Some(id) => Some(id),
        };
        if next != sel.selected.get() {
            select_and_reveal(&sel, next);
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
    let flash_state = state.flash;
    let flash_until = state.flash_until;
    let refresh_store = store.clone();
    let last_gen = signal(crate::history::generation());
    set_interval(Duration::from_millis(150), move || {
        if flash_state.get().is_some() && now_secs() >= flash_until.get() {
            flash_state.set(None);
        }
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
    dynamic(move || shell_view(state.clone()))
}

pub fn glass_backdrop() -> MacOsBackdrop {
    let mut backdrop = MacOsBackdrop::default();
    backdrop.blending = MacOsBackdropBlending::BehindWindow;
    backdrop
}

fn shell_view(state: State) -> impl IntoElement {
    let root = view()
        .flex_1()
        .macos_backdrop(glass_backdrop())
        .bg(theme::glass_bg())
        .text_color(theme::fg())
        .rounded(px(14.0))
        .overflow_hidden();
    if state.editing.get() {
        return root.child(snippet_form(state.clone()));
    }
    if state.edit_clip.get().is_some() {
        return root.child(clip_form(state.clone()));
    }
    root.child(header(state.clone()))
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

/// 片段编辑态：整块替换列表，和 Raycast 的 Update Snippet 一样。
fn snippet_form(state: State) -> impl IntoElement {
    let name = state.edit_name;
    let content = state.edit_content;
    let keyword = state.edit_keyword;
    let menu = state.placeholder_menu;
    let selected = state.placeholder_selected;
    let cancel_state = state.clone();
    let back_state = state.clone();
    let save_state = state.clone();
    view()
        .flex_1()
        .flex_col()
        .p_6()
        .gap_4()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .on_press(move |_event: &PressEvent| cancel_snippet_editor(&back_state))
                        .px_2()
                        .py_1()
                        .rounded(px(7.0))
                        .bg(theme::card())
                        .border_1()
                        .border_color(theme::border())
                        .cursor_pointer()
                        .child(
                            text("←")
                                .text_size(px(13.0))
                                .text_color(theme::fg()),
                        ),
                )
                .child(
                    text(t!("snippet.title"))
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                ),
        )
        .child(
            view()
                .flex_col()
                .gap_2()
                .child(
                    text(t!("snippet.name"))
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                )
                .child(
                    input()
                        .test_id("snippet-name")
                        .value(name)
                        .on_change(move |value: &str| name.set(value.to_string()))
                        .placeholder(t!("snippet.name"))
                        .text_size(px(13.0))
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(theme::glass_card())
                        .border_1()
                        .border_color(theme::border())
                        .auto_focus(true),
                ),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .gap_2()
                .child(
                    text(t!("snippet.body"))
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                )
                .child(
                    view()
                        .relative()
                        .flex_1()
                        .flex_col()
                        .rounded(px(8.0))
                        .bg(theme::glass_card())
                        .border_1()
                        .border_color(theme::border())
                        .p_3()
                        .child(
                            input()
                                .test_id("snippet-body")
                                .multiline(true)
                                .flex_1()
                                .value(content)
                                .on_change(move |value: &str| {
                                    content.set(value.to_string());
                                    if value.ends_with('{') {
                                        selected.set(0);
                                        menu.set(true);
                                    } else if menu.get() && placeholder_query(value).is_some() {
                                        selected.set(0);
                                    } else {
                                        menu.set(false);
                                    }
                                })
                                .text_size(px(13.0))
                                .text_color(theme::fg()),
                        ),
                )
                .child(placeholder_menu(state.clone())),
        )
        .child(
            view()
                .flex_col()
                .gap_2()
                .child(
                    text(t!("snippet.keyword"))
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                )
                .child(
                    input()
                        .test_id("snippet-keyword")
                        .value(keyword)
                        .on_change(move |value: &str| keyword.set(value.to_string()))
                        .placeholder(t!("snippet.keyword_placeholder"))
                        .text_size(px(13.0))
                        .px_3()
                        .py_2()
                        .rounded(px(8.0))
                        .bg(theme::glass_card())
                        .border_1()
                        .border_color(theme::border()),
                ),
        )
        .child(
            text(t!("snippet.hint"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(editor_button(t!("editor.cancel"), false, move || {
                    cancel_snippet_editor(&cancel_state)
                }))
                .child(editor_button(t!("snippet.save"), true, move || {
                    save_snippet(&save_state)
                })),
        )
}

/// 进入剪贴板条目的编辑态：正文复用 `edit_content`，行为与片段编辑一致。
fn start_clip_editor(state: &State, clip: Clip) {
    state.editing.set(false);
    state.placeholder_menu.set(false);
    state.edit_clip.set(Some(clip.id));
    state.edit_content
        .set(clip.content.clone().unwrap_or_default());
    state.editing.set(false);
    state.actions_open.set(false);
}

fn cancel_clip_editor(state: &State) {
    state.edit_clip.set(None);
}

fn save_clip_editor(state: &State) {
    let Some(id) = state.edit_clip.get() else {
        return;
    };
    let Some(store) = crate::history::init() else {
        return;
    };
    match store.update_text(id, &state.edit_content.get()) {
        Ok(()) => {
            state.edit_clip.set(None);
            crate::history::bump();
            reload(state, store.as_ref());
        }
        Err(e) => {
            log::error!("update clip: {e}");
            flash(state, t!("editor.duplicate"));
        }
    }
}

fn clip_form(state: State) -> impl IntoElement {
    let content = state.edit_content;
    let back_state = state.clone();
    let cancel_state = state.clone();
    let save_state = state.clone();
    view()
        .flex_1()
        .flex_col()
        .p_6()
        .gap_4()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .on_press(move |_event: &PressEvent| cancel_clip_editor(&back_state))
                        .px_2()
                        .py_1()
                        .rounded(px(7.0))
                        .bg(theme::card())
                        .border_1()
                        .border_color(theme::border())
                        .cursor_pointer()
                        .child(
                            text("←")
                                .text_size(px(13.0))
                                .text_color(theme::fg()),
                        ),
                )
                .child(
                    text(t!("editor.title"))
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                ),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .rounded(px(8.0))
                .bg(theme::glass_card())
                .border_1()
                .border_color(theme::border())
                .p_3()
                .child(
                    input()
                        .test_id("clip-body")
                        .multiline(true)
                        .flex_1()
                        .value(content)
                        .on_change(move |value: &str| content.set(value.to_string()))
                        .text_size(px(13.0))
                        .text_color(theme::fg())
                        .auto_focus(true),
                ),
        )
        .child(
            text(t!("editor.hint"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            view()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(editor_button(t!("editor.cancel"), false, move || {
                    cancel_clip_editor(&cancel_state)
                }))
                .child(editor_button(t!("editor.save"), true, move || {
                    save_clip_editor(&save_state)
                })),
        )
}

/// 进入片段编辑态：`existing` 为 `Some` 时编辑它，`content` 为预填正文。
fn start_snippet_editor(state: &State, existing: Option<Snippet>, content: Option<String>) {
    state.edit_id.set(existing.as_ref().map(|snippet| snippet.id));
    state.edit_name.set(
        existing
            .as_ref()
            .map(|snippet| snippet.name.clone())
            .unwrap_or_default(),
    );
    state.edit_content.set(
        content
            .or_else(|| existing.as_ref().map(|snippet| snippet.content.clone()))
            .unwrap_or_default(),
    );
    state.edit_keyword.set(
        existing
            .as_ref()
            .map(|snippet| snippet.keyword.clone())
            .unwrap_or_default(),
    );
    state.actions_open.set(false);
    state.editing.set(true);
}

fn cancel_snippet_editor(state: &State) {
    state.editing.set(false);
}

fn save_snippet(state: &State) {
    let Some(store) = crate::history::init() else {
        return;
    };
    let body = state.edit_content.get();
    if body.trim().is_empty() {
        flash(state, t!("snippet.empty"));
        return;
    }
    let name = state.edit_name.get();
    let name = if name.trim().is_empty() {
        preview_for_text(&body)
    } else {
        name.trim().to_string()
    };
    let keyword = state.edit_keyword.get();
    let now = now_secs();
    let result = match state.edit_id.get() {
        Some(id) => store.update_snippet(id, &name, &body, &keyword, now),
        None => store.create_snippet(&name, &body, &keyword, now).map(|_| ()),
    };
    match result {
        Ok(()) => {
            state.editing.set(false);
            crate::history::bump();
            reload(state, store.as_ref());
        }
        Err(e) => {
            log::error!("save snippet: {e}");
            flash(state, t!("snippet.failed"));
        }
    }
}

fn editor_button(
    label: String,
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

/// 语言无关的保存文件名时间戳。
fn stamp(secs: i64) -> String {
    Timestamp::from_second(secs)
        .map(|ts| {
            ts.to_zoned(TimeZone::system())
                .strftime("%Y%m%d-%H%M%S")
                .to_string()
        })
        .unwrap_or_else(|_| "clip".to_string())
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

fn open_selected(state: &State) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    match clip.kind {
        ClipKind::Link => {
            if let Some(url) = &clip.content {
                crate::paste::open_external(url);
            }
        }
        ClipKind::File => {
            if let Some(path) = clip.file_uris.first() {
                let _ = nsh_core::element::open_path(path);
            }
        }
        _ => flash(state, t!("flash.cannot_open")),
    }
}

fn reveal_selected(state: &State) {
    let Some(clip) = selected_clip(state) else {
        return;
    };
    if clip.kind == ClipKind::File {
        if let Some(path) = clip.file_uris.first() {
            let _ = nsh_core::element::reveal_path(path);
        }
    } else {
        flash(state, t!("flash.reveal_files_only"));
    }
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
                format!("podnook-{}.png", stamp(clip.created_at)),
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
        flash(state, t!("flash.cannot_save"));
        return;
    };
    let raw = state.save_path.get();
    let path = expand_tilde(raw.trim());
    if path.as_os_str().is_empty() {
        flash(state, t!("flash.empty_path"));
        return;
    }
    let result = match data {
        SaveData::Bytes(bytes) => std::fs::write(&path, bytes),
        SaveData::Copy(source) => std::fs::copy(&source, &path).map(|_| ()),
    };
    match result {
        Ok(()) => {
            close_actions(state);
            flash(state, t!("flash.saved_to", path = path.display()));
        }
        Err(e) => {
            log::error!("save clip to {}: {e}", path.display());
            flash(state, t!("flash.save_failed"));
        }
    }
}

fn actions_overlay(state: State) -> impl IntoElement {
    dynamic(move || {
        if state.actions_open.get() {
            actions_overlay_inner(state.clone()).into_element()
        } else {
            view().into_element()
        }
    })
}

fn actions_overlay_inner(state: State) -> View {
    let selected = state.actions_selected.get();
    let items = action_items(&state);

    let mut list = view().flex_col().p_2().overflow_hidden();
    if items.is_empty() {
        list = list.child(
            text(t!("panel.no_actions"))
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
                text(t!("panel.save_to"))
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
                text(t!("panel.save_keys"))
                    .text_size(px(11.0))
                    .text_color(theme::muted_fg()),
            )
    } else {
        list
    };

    let backdrop_state = state.clone();
    let backdrop_menu_state = state.clone();
    let search_ref = state.actions_search;
    let query = state.actions_query;
    let change_state = state.clone();
    let escape_state = state.clone();

    view()
        .absolute()
        .inset(px(0.0))
        .z_index(40)
        .pointer_events(PointerEvents::BoxNone)
        .child(
            view()
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .right(px(0.0))
                .bottom(px(44.0))
                .bg(theme::bg().opacity(0.01))
                .on_press(move || close_actions(&backdrop_state))
                .on_context_menu(move |_event: &nsh_core::ContextMenuEvent| {
                    close_actions(&backdrop_menu_state)
                }),
        )
        .child(
            view()
                .absolute()
                .bottom(px(52.0))
                .right(px(16.0))
                .w(px(440.0))
                .max_h(px(520.0))
                .z_index(41)
                .flex_col()
                .rounded(px(12.0))
                .bg(theme::card())
                .border_1()
                .border_color(theme::border())
                .shadow_md()
                .overflow_hidden()
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
                                .placeholder(t!("panel.search_actions"))
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
                theme::card()
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
    match store.list_snippets() {
        Ok(snippets) => state.snippets.set(snippets),
        Err(e) => log::error!("list snippets: {e}"),
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
            Row::Snippet(snippet) => Some(snippet.id),
            Row::Section(_) => None,
        })
        .collect()
}

/// 呼出时默认选中的条目：第一条未固定的（最新的捕获）；全部固定时取第一条。
fn default_selection(rows: &[Row]) -> Option<i64> {
    let mut first = None;
    for row in rows {
        let id = match row {
            Row::Clip(clip) => {
                first.get_or_insert(clip.id);
                if !clip.pinned {
                    return Some(clip.id);
                }
                clip.id
            }
            Row::Snippet(snippet) => snippet.id,
            Row::Section(_) => continue,
        };
        first.get_or_insert(id);
    }
    first
}

fn build_rows(
    clips: &[Clip],
    snippets: &[Snippet],
    source: Source,
    query: &str,
    filter: Option<ClipKind>,
) -> Vec<Row> {
    let q = query.trim().to_lowercase();
    if source == Source::Snippets {
        let matched: Vec<Snippet> = snippets
            .iter()
            .filter(|snippet| q.is_empty() || snippet_matches(snippet, &q))
            .cloned()
            .collect();
        let (pinned, rest): (Vec<Snippet>, Vec<Snippet>) =
            matched.into_iter().partition(|snippet| snippet.pinned);
        let mut rows = Vec::new();
        if !pinned.is_empty() {
            rows.push(Row::Section(t!("group.pinned")));
            rows.extend(pinned.into_iter().map(Row::Snippet));
        }
        if !rest.is_empty() {
            rows.push(Row::Section(t!("source.snippets")));
            rows.extend(rest.into_iter().map(Row::Snippet));
        }
        return rows;
    }
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
    push_group(&mut rows, t!("group.pinned"), pinned);
    push_group(&mut rows, t!("group.today"), today_rows);
    push_group(&mut rows, t!("group.earlier"), older);
    rows
}

fn snippet_matches(snippet: &Snippet, q: &str) -> bool {
    snippet.name.to_lowercase().contains(q)
        || snippet.content.to_lowercase().contains(q)
        || snippet.keyword.to_lowercase().contains(q)
}

fn push_group(rows: &mut Vec<Row>, label: String, group: Vec<Clip>) {
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
    hay.push_str(&kind_label(clip.kind));
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
    let h24 = uses_24_hour_clock();
    let time = if h24 {
        zoned.strftime("%H:%M").to_string()
    } else {
        zoned.strftime("%I:%M %p").to_string()
    };
    if zoned.date() == local_today() {
        return time;
    }
    if h24 {
        format!("{} {time}", zoned.strftime("%m-%d"))
    } else {
        format!("{} {time}", zoned.strftime("%b %d"))
    }
}

fn word_count(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for c in text.chars() {
        let cjk = matches!(
            c as u32,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
        );
        if cjk {
            count += 1;
            in_word = false;
        } else if c.is_alphanumeric() || c == '_' {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    count
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

fn kind_label(kind: ClipKind) -> String {
    match kind {
        ClipKind::Text => t!("kind.text"),
        ClipKind::Link => t!("kind.link"),
        ClipKind::Image => t!("kind.image"),
        ClipKind::File => t!("kind.file"),
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// 列表里的单行标题：文本用入库时的预览，图片/文件按当前语言现算。
fn clip_title(clip: &Clip) -> String {
    match clip.kind {
        ClipKind::Image => match (clip.image_w, clip.image_h) {
            (Some(w), Some(h)) => t!("preview.image_size", w = w, h = h),
            _ => t!("preview.image"),
        },
        ClipKind::File => match clip.file_uris.as_slice() {
            [] => t!("preview.file"),
            [one] => file_name(one),
            [first, rest @ ..] => {
                let others = rest.len();
                if others == 1 {
                    t!("preview.files_more_one", first = file_name(first))
                } else {
                    t!(
                        "preview.files_more",
                        first = file_name(first),
                        total = others + 1,
                        others = others
                    )
                }
            }
        },
        ClipKind::Text | ClipKind::Link => clip.preview.clone(),
    }
}

fn uses_24_hour_clock() -> bool {
    matches!(
        nsh_core::element::locale_settings().hour_cycle,
        HourCycle::H23 | HourCycle::H24
    )
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
        if s.editing.get() && s.placeholder_menu.get() {
            let len = filtered_placeholders(&s.edit_content.get()).len();
            if len > 0 {
                let current = s.placeholder_selected.get().min(len - 1);
                s.placeholder_selected.set((current + 1).min(len - 1));
            }
            return;
        }
        if s.actions_open.get() {
            move_action(&s, 1);
        } else {
            move_selection(&s, 1);
        }
    });
    let s = state.clone();
    on_key_with("up", always(), move |_| {
        if s.editing.get() && s.placeholder_menu.get() {
            let current = s.placeholder_selected.get();
            s.placeholder_selected.set(current.saturating_sub(1));
            return;
        }
        if s.actions_open.get() {
            move_action(&s, -1);
        } else {
            move_selection(&s, -1);
        }
    });
    let s = state.clone();
    let db = store.clone();
    let enter_options = || KeyOptions {
        when: When::Always,
        pass_through: true,
        ..KeyOptions::default()
    };
    on_key_with("enter", enter_options(), move |_| {
        if s.edit_clip.get().is_some() {
            return;
        }
        if s.editing.get() && s.placeholder_menu.get() {
            insert_placeholder(&s, s.placeholder_selected.get());
            return;
        }
        if s.editing.get() {
            save_snippet(&s);
            return;
        }
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
            let plain = prefer_plain(db.as_deref());
            activate_selected(&s, db.as_deref(), true, plain);
        }
    });
    let s = state.clone();
    let db = store.clone();
    on_key_with("mod+enter", always(), move |_| {
        if s.edit_clip.get().is_some() {
            save_clip_editor(&s);
            return;
        }
        if s.editing.get() && s.placeholder_menu.get() {
            insert_placeholder(&s, s.placeholder_selected.get());
            return;
        }
        if s.editing.get() {
            save_snippet(&s);
            return;
        }
        activate_selected(&s, db.as_deref(), false, false)
    });
    let s = state.clone();
    on_key_with("mod+e", always(), move |_| {
        if s.source.get() == Source::Snippets {
            if let Some(snippet) = selected_snippet(&s) {
                s.actions_open.set(false);
                start_snippet_editor(&s, Some(snippet), None);
            }
            return;
        }
        if let Some(clip) = selected_clip(&s) {
            if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
                s.actions_open.set(false);
                start_clip_editor(&s, clip);
            }
        }
    });
    let s = state.clone();
    on_key_with("alt+shift+mod+s", always(), move |_| {
        if selected_clip(&s).is_some() {
            enter_save_mode(&s);
        }
    });
    let s = state.clone();
    let db = store.clone();
    on_key_with("shift+enter", always(), move |_| {
        activate_selected(&s, db.as_deref(), true, true);
    });
    // Raycast 对齐：固定是 ⇧⌘P；⌘. 留给 Append to Clipboard。
    let s = state.clone();
    let db = store.clone();
    on_key_with("shift+mod+p", always(), move |_| {
        if s.source.get() == Source::Snippets {
            toggle_snippet_pin(&s, db.as_deref());
        } else {
            toggle_pin(&s, db.as_deref());
        }
        if s.actions_open.get() {
            close_actions(&s);
        }
    });
    let s = state.clone();
    let duplicate = state.clone();
    let duplicate_db = store.clone();
    on_key_with("mod+d", always(), move |_| {
        if duplicate.source.get() == Source::Snippets {
            duplicate_snippet(&duplicate, duplicate_db.as_deref());
        }
    });
    let create = state.clone();
    on_key_with("mod+n", always(), move |_| {
        if create.source.get() == Source::Snippets {
            start_snippet_editor(&create, None, None);
        }
    });
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
        if s.edit_clip.get().is_some() {
            cancel_clip_editor(&s);
            return;
        }
        if s.editing.get() && s.placeholder_menu.get() {
            s.placeholder_menu.set(false);
            return;
        }
        if s.editing.get() {
            cancel_snippet_editor(&s);
        } else if s.actions_open.get() {
            close_actions(&s);
        } else {
            reset_on_next_show();
            crate::hide_panel();
        }
    });
    // 面板自己的那份：设置窗口没开、主窗口不是 key 时也能用。
    // ⌘S：历史里把选中条目存成片段；片段模式下新建。
    let save_state = state.clone();
    on_key("mod+s", move |_| {
        if save_state.edit_clip.get().is_some() {
            save_clip_editor(&save_state);
        } else if save_state.editing.get() {
            save_snippet(&save_state);
        } else if save_state.source.get() == Source::History {
            let content = selected_clip(&save_state)
                .and_then(|clip| clip.content)
                .unwrap_or_default();
            start_snippet_editor(&save_state, None, Some(content));
        }
    });

    let cycle = state.clone();
    on_key("shift+tab", move |_| {
        if cycle.editing.get() || cycle.edit_clip.get().is_some() {
            return;
        }
        let next = next_source(cycle.source.get());
        switch_source(&cycle, next);
    });

    let source_1 = state.clone();
    on_key("mod+1", move |_| switch_source(&source_1, Source::History));
    let source_2 = state.clone();
    on_key("mod+2", move |_| switch_source(&source_2, Source::Snippets));

    on_key("mod+,", |_| crate::open_settings());
}

fn switch_source(state: &State, source: Source) {
    if state.source.get() == source {
        return;
    }
    state.source.set(source);
    state.query.set(String::new());
    state.filter.set(None);
    state.selected.set(None);
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
    // 等内容淡出后再重置模式并把焦点还给主搜索，避免关闭动画里内容跳变。
    let state = state.clone();
    set_timeout(Duration::from_millis(240), move || {
        if state.actions_open.get() {
            return;
        }
        state.actions_mode.set(ActionMode::Main);
        state.actions_query.set(String::new());
        state.actions_selected.set(0);
        state.search.focus();
    });
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
    if state.source.get() == Source::Snippets {
        let pinned = selected_snippet(state).is_some_and(|snippet| snippet.pinned);
        let mut items = vec![
            ActionItem {
                kind: ActionKind::Paste,
                label: t!("action.paste"),
                shortcut: "enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Copy,
                label: t!("action.copy"),
                shortcut: "mod+enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::PinSnippet,
                label: if pinned {
                    t!("action.unpin")
                } else {
                    t!("action.pin")
                },
                shortcut: "shift+mod+p",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Edit,
                label: t!("action.edit_snippet"),
                shortcut: "mod+e",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::DuplicateSnippet,
                label: t!("action.duplicate_snippet"),
                shortcut: "mod+d",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::NewSnippet,
                label: t!("action.new_snippet"),
                shortcut: "mod+n",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Delete,
                label: t!("action.delete"),
                shortcut: "ctrl+x",
                destructive: true,
            },
        ];
        let query = state.actions_query.get().trim().to_lowercase();
        if !query.is_empty() {
            items.retain(|item| item.label.to_lowercase().contains(&query));
        }
        return items;
    }

    let pin_label = match selected_clip(state) {
        Some(clip) if clip.pinned => t!("action.unpin"),
        _ => t!("action.pin"),
    };

    let editable = matches!(
        selected_clip(state).map(|clip| clip.kind),
        Some(ClipKind::Text) | Some(ClipKind::Link)
    );

    let mut items = match state.actions_mode.get() {
        ActionMode::Range => delete_ranges()
            .into_iter()
            .map(|(seconds, label)| ActionItem {
                kind: ActionKind::DeleteRange(seconds),
                label,
                shortcut: "",
                destructive: true,
            })
            .collect::<Vec<_>>(),
        ActionMode::ConfirmAll => vec![
            ActionItem {
                kind: ActionKind::ConfirmDeleteAll,
                label: t!("action.clear_all_confirm"),
                shortcut: "enter",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::Back,
                label: t!("editor.cancel"),
                shortcut: "escape",
                destructive: false,
            },
        ],
        ActionMode::SaveFile => vec![
            ActionItem {
                kind: ActionKind::SaveNow,
                label: t!("editor.save"),
                shortcut: "enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Back,
                label: t!("editor.cancel"),
                shortcut: "escape",
                destructive: false,
            },
        ],
        ActionMode::Main => vec![
            ActionItem {
                kind: ActionKind::Paste,
                label: t!("action.paste"),
                shortcut: "enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Copy,
                label: t!("action.copy"),
                shortcut: "mod+enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::PastePlain,
                label: t!("action.paste_plain"),
                shortcut: "shift+enter",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Edit,
                label: t!("action.edit"),
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
                label: t!("action.save_file"),
                shortcut: "alt+shift+mod+s",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::SaveSnippet,
                label: t!("action.save_snippet"),
                shortcut: "mod+s",
                destructive: false,
            },
            ActionItem {
                kind: ActionKind::Delete,
                label: t!("action.delete"),
                shortcut: "ctrl+x",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::DeleteEntries,
                label: t!("action.delete_recent"),
                shortcut: "shift+mod+x",
                destructive: true,
            },
            ActionItem {
                kind: ActionKind::DeleteAll,
                label: t!("action.clear_all"),
                shortcut: "ctrl+shift+x",
                destructive: true,
            },
        ],
    };

    if state.actions_mode.get() == ActionMode::Range {
        items.push(ActionItem {
            kind: ActionKind::Back,
            label: t!("action.back"),
            shortcut: "escape",
            destructive: false,
        });
    }

    if !editable {
        items.retain(|item| item.kind != ActionKind::Edit);
    }

    if state.actions_mode.get() == ActionMode::Main {
        let kind = selected_clip(state).map(|clip| clip.kind);
        if matches!(kind, Some(ClipKind::Link) | Some(ClipKind::File)) {
            items.insert(
                2,
                ActionItem {
                    kind: ActionKind::Open,
                    label: if kind == Some(ClipKind::Link) {
                        t!("action.open_link")
                    } else {
                        t!("action.open_file")
                    },
                    shortcut: "mod+o",
                    destructive: false,
                },
            );
        }
        if kind == Some(ClipKind::File) {
            items.insert(
                3,
                ActionItem {
                    kind: ActionKind::Reveal,
                    label: t!("action.reveal"),
                    shortcut: "mod+shift+o",
                    destructive: false,
                },
            );
        }
    }

    let query = state.actions_query.get().trim().to_lowercase();
    if !query.is_empty() {
        items.retain(|item| item.label.to_lowercase().contains(&query));
    }
    items
}

fn range_label(seconds: i64) -> String {
    match seconds {
        300 => t!("range.m5"),
        900 => t!("range.m15"),
        1800 => t!("range.m30"),
        3600 => t!("range.h1"),
        86400 => t!("range.h24"),
        _ => t!("range.any"),
    }
}

fn run_action(state: &State, store: Option<&Store>, kind: ActionKind) {
    match kind {
        ActionKind::Paste => {
            state.actions_open.set(false);
            let plain = prefer_plain(store);
            activate_selected(state, store, true, plain);
        }
        ActionKind::PastePlain => {
            state.actions_open.set(false);
            activate_selected(state, store, true, true);
        }
        ActionKind::Copy => {
            state.actions_open.set(false);
            activate_selected(state, store, false, false);
        }
        ActionKind::Edit => {
            if state.source.get() == Source::Snippets {
                if let Some(snippet) = selected_snippet(state) {
                    state.actions_open.set(false);
                    start_snippet_editor(state, Some(snippet), None);
                }
                return;
            }
            if let Some(clip) = selected_clip(state) {
                if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
                    state.actions_open.set(false);
                    start_clip_editor(state, clip);
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
        ActionKind::Open => {
            open_selected(state);
            close_actions(state);
        }
        ActionKind::Reveal => {
            reveal_selected(state);
            close_actions(state);
        }
        ActionKind::Pin => {
            toggle_pin(state, store);
            close_actions(state);
        }
        ActionKind::Delete => {
            delete_selected(state, store);
            close_actions(state);
        }
        ActionKind::SaveSnippet => {
            let content = selected_clip(state)
                .and_then(|clip| clip.content)
                .unwrap_or_default();
            close_actions(state);
            start_snippet_editor(state, None, Some(content));
        }
        ActionKind::NewSnippet => {
            close_actions(state);
            start_snippet_editor(state, None, None);
        }
        ActionKind::PinSnippet => {
            toggle_snippet_pin(state, store);
            close_actions(state);
        }
        ActionKind::DuplicateSnippet => {
            duplicate_snippet(state, store);
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
                    let next = default_selection(&state.rows.get());
                    select_and_reveal(state, next);
                    close_actions(state);
                    flash(
                        state,
                        t!(
                            "flash.deleted_recent",
                            range = range_label(seconds),
                            count = count
                        ),
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
                    let next = default_selection(&state.rows.get());
                    select_and_reveal(state, next);
                    close_actions(state);
                    flash(state, t!("flash.cleared", count = count));
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

fn activate_selected(state: &State, store: Option<&Store>, paste: bool, plain: bool) {
    if state.source.get() == Source::Snippets {
        activate_snippet(state, store, paste);
        return;
    }
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let ok = match clip.kind {
        ClipKind::Text | ClipKind::Link => match &clip.content {
            Some(text) => {
                #[cfg(target_os = "macos")]
                {
                    let formats = if plain {
                        Vec::new()
                    } else {
                        store
                            .and_then(|store| store.formats_for(clip.id).ok())
                            .unwrap_or_default()
                    };
                    if formats.is_empty() {
                        set_clipboard(text.clone());
                    } else {
                        crate::clipboard::write_text_formats(text, &formats);
                    }
                }
                #[cfg(not(target_os = "macos"))]
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
        let _ = store.touch_usage(clip.id, now_secs());
        crate::history::bump();
    }

    if paste {
        #[cfg(target_os = "macos")]
        {
            if crate::paste::is_trusted() {
                crate::hide_panel();
                crate::paste::paste_after_hide();
            } else {
                crate::paste::prompt_trust_once();
                flash(state, t!("flash.copied_no_paste"));
            }
            return;
        }
    }

    crate::hide_panel();
    flash(state, t!("flash.copied"));
}

fn toggle_snippet_pin(state: &State, store: Option<&Store>) {
    let Some(snippet) = selected_snippet(state) else {
        return;
    };
    let Some(store) = store else { return };
    if let Err(e) = store.set_snippet_pinned(snippet.id, !snippet.pinned) {
        log::error!("pin snippet {}: {e}", snippet.id);
        return;
    }
    crate::history::bump();
    reload(state, store);
    flash(
        state,
        if snippet.pinned {
            t!("flash.unpinned")
        } else {
            t!("flash.pinned")
        },
    );
}

fn duplicate_snippet(state: &State, store: Option<&Store>) {
    let Some(snippet) = selected_snippet(state) else {
        return;
    };
    let Some(store) = store else { return };
    let name = format!("{} {}", snippet.name, t!("snippet.copy_suffix"));
    let Ok(id) = store.create_snippet(&name, &snippet.content, &snippet.keyword, now_secs()) else {
        return;
    };
    crate::history::bump();
    reload(state, store);
    if let Some(copy) = state.snippets.get().into_iter().find(|item| item.id == id) {
        start_snippet_editor(state, Some(copy), None);
    }
}

fn prefer_plain(store: Option<&Store>) -> bool {
    store
        .map(|store| crate::settings::read_bool(store, SETTING_PLAIN_PASTE, false))
        .unwrap_or(false)
}

fn selected_snippet(state: &State) -> Option<Snippet> {
    let id = state.selected.get()?;
    state
        .snippets
        .get()
        .into_iter()
        .find(|snippet| snippet.id == id)
}

fn activate_snippet(state: &State, store: Option<&Store>, paste: bool) {
    let Some(snippet) = selected_snippet(state) else {
        return;
    };
    let text = crate::snippet_text::expand(&snippet.content);
    set_clipboard(text.clone());

    let auto_paste = store
        .map(|store| crate::settings::read_bool(store, SETTING_AUTO_PASTE, true))
        .unwrap_or(true);
    if let Some(store) = store {
        store.mark_self_write(hash_text(&text));
        let _ = store.touch_snippet(snippet.id, now_secs());
        crate::history::bump();
    }

    if paste && auto_paste {
        #[cfg(target_os = "macos")]
        {
            if crate::paste::is_trusted() {
                crate::hide_panel();
                crate::paste::paste_after_hide();
            } else {
                crate::paste::prompt_trust_once();
                flash(state, t!("flash.copied_no_paste"));
            }
            return;
        }
    }

    crate::hide_panel();
    flash(state, t!("flash.copied"));
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
    flash(
        state,
        if clip.pinned {
            t!("flash.unpinned")
        } else {
            t!("flash.pinned")
        },
    );
}

fn delete_selected(state: &State, store: Option<&Store>) {
    if state.source.get() == Source::Snippets {
        let Some(snippet) = selected_snippet(state) else {
            return;
        };
        let Some(store) = store else { return };
        let previous_rows = state.rows.get();
        if let Err(e) = store.delete_snippet(snippet.id) {
            log::error!("delete snippet {}: {e}", snippet.id);
            return;
        }
        crate::history::bump();
        reload(state, store);
        let rows = state.rows.get();
        let (next, jump_to_top) = selection_after_delete(&previous_rows, &rows, snippet.id);
        state.selected.set(next);
        if jump_to_top {
            if let Some(id) = next {
                reveal_row(id);
            }
        }
        flash(state, t!("flash.deleted"));
        return;
    }
    let Some(clip) = selected_clip(state) else {
        return;
    };
    let Some(store) = store else { return };
    let previous_rows = state.rows.get();
    if let Err(e) = store.delete(clip.id) {
        log::error!("delete {}: {e}", clip.id);
        return;
    }
    crate::history::bump();
    reload(state, store);
    let rows = state.rows.get();
    let (next, jump_to_top) = selection_after_delete(&previous_rows, &rows, clip.id);
    state.selected.set(next);
    // 只有删掉最后一条、选中项跳到列表另一端时才滚动；其余删除不动滚动位置。
    if jump_to_top {
        scroll_sidebar_to_top();
    }
    flash(state, t!("flash.deleted"));
}

/// 删除后选中的条目，以及是否要滚回顶部。删最后一条时选中第一条（不分固定
/// 与否）并回到顶部；其余情况选中下一条，不动滚动位置。
fn selection_after_delete(
    rows_before: &[Row],
    rows_after: &[Row],
    deleted: i64,
) -> (Option<i64>, bool) {
    let before = visible_ids(rows_before);
    let after = visible_ids(rows_after);
    let index = before.iter().position(|id| *id == deleted);
    if index.is_some_and(|index| index + 1 == before.len()) {
        return (after.first().copied(), true);
    }
    (index.and_then(|index| after.get(index).copied()), false)
}

fn flash(state: &State, message: impl Into<String>) {
    state.flash.set(Some(message.into()));
    state.flash_until.set(now_secs() + 2);
}

// ---------------------------------------------------------------- 顶栏

fn header(state: State) -> impl IntoElement {
    dynamic(move || header_inner(state.clone()))
}

fn header_inner(state: State) -> impl IntoElement {
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
        .child(dynamic({
            let state = state.clone();
            move || {
                if state.source.get() == Source::Snippets {
                    view().into_element()
                } else {
                    filter_bar(state.clone()).into_element()
                }
            }
        }))
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
        .bg(theme::glass_card())
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
                .placeholder(move || if state.source.get() == Source::Snippets { t!("panel.search_snippets") } else { t!("panel.search") })
                .placeholder_color(rgb(0x8A8178))
                .text_color(theme::fg())
                .text_size(px(14.0))
                // 输入框聚焦时先吃掉 Esc 并失焦；在这里直接处理，
                // 避免要按两次。操作面板打开时 Esc 先关面板。
                .on_key_press({
                    let state = state.clone();
                    move |key: &str| {
                        if key == "Escape" {
                            if state.editing.get() {
                                cancel_snippet_editor(&state);
                            } else if state.actions_open.get() {
                                close_actions(&state);
                            } else {
                                reset_on_next_show();
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
        .child(filter_pill(state.clone(), None, t!("filter.all")))
        .child(filter_pill(state.clone(), Some(ClipKind::Text), t!("kind.text")))
        .child(filter_pill(state.clone(), Some(ClipKind::Link), t!("kind.link")))
        .child(filter_pill(state.clone(), Some(ClipKind::Image), t!("kind.image")))
        .child(filter_pill(state, Some(ClipKind::File), t!("kind.file")))
}


fn filter_pill(state: State, kind: Option<ClipKind>, label: String) -> impl IntoElement {
    let filter = state.filter;
    view()
        .on_press(move || filter.set(kind))
        .px_3()
        .py_2()
        .rounded(px(8.0))
        .bg(move || {
            if filter.get() == kind {
                theme::glass_accent()
            } else {
                theme::glass_card()
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
    Section(String),
    Clip(i64),
    Snippet(i64),
}

fn sidebar(state: State) -> impl IntoElement {
    let rows = state.rows;
    let selected = state.selected;
    let scroll_ref = element_ref();
    SIDEBAR.with(|slot| *slot.borrow_mut() = Some(scroll_ref));
    view()
        .w(px(theme::SIDEBAR_WIDTH))
        .flex_col()
        .bg(theme::glass_surface())
        .border_r(px(1.0))
        .border_r_color(theme::border())
        .child(
            scroll().element_ref(&scroll_ref).flex_1().child(
                view().flex_col().py_2().child(each(
                    move || rows.get(),
                    |row| match row {
                        Row::Section(label) => RowKey::Section(label.clone()),
                        Row::Clip(clip) => RowKey::Clip(clip.id),
                        Row::Snippet(snippet) => RowKey::Snippet(snippet.id),
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
                                .element_ref(&row_ref(id))
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .mx_2()
                                .px_2()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(move || {
                                    if selected.get() == Some(id) {
                                        theme::glass_accent()
                                    } else {
                                        AdaptiveColor::TRANSPARENT
                                    }
                                })
                                .on_press(move |event: &PressEvent| {
                                    selected.set(Some(id));
                                    if event.clicks >= 2 {
                                        if let Some(store) = crate::history::init() {
                                            let plain = prefer_plain(Some(store.as_ref()));
                                            activate_selected(&row_state, Some(store.as_ref()), true, plain);
                                        }
                                    }
                                })
                                .on_context_menu({
                                    let menu_state = state.clone();
                                    move |_event: &nsh_core::ContextMenuEvent| {
                                        selected.set(Some(id));
                                        enter_action_mode(&menu_state, ActionMode::Main);
                                    }
                                })
                                .child(icon_box(&clip))
                                .child(
                                    view().flex_1().child(
                                        text({
                                            let clip = clip.clone();
                                            move || clip_title(&clip)
                                        })
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
                        Row::Snippet(snippet) => {
                            let id = snippet.id;
                            let row_state = state.clone();
                            let snippets = state.snippets;
                            let lookup = move |id: i64| {
                                snippets
                                    .get()
                                    .into_iter()
                                    .find(|candidate| candidate.id == id)
                            };
                            view()
                                .element_ref(&row_ref(id))
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .mx_2()
                                .px_2()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(move || {
                                    if selected.get() == Some(id) {
                                        theme::glass_accent()
                                    } else {
                                        AdaptiveColor::TRANSPARENT
                                    }
                                })
                                .on_press(move |event: &PressEvent| {
                                    selected.set(Some(id));
                                    if event.clicks >= 2 {
                                        if let Some(store) = crate::history::init() {
                                            let plain = prefer_plain(Some(store.as_ref()));
                                            activate_selected(&row_state, Some(store.as_ref()), true, plain);
                                        }
                                    }
                                })
                                .on_context_menu({
                                    let menu_state = state.clone();
                                    move |_event: &nsh_core::ContextMenuEvent| {
                                        selected.set(Some(id));
                                        enter_action_mode(&menu_state, ActionMode::Main);
                                    }
                                })
                                .child(
                                    view()
                                        .size(px(26.0))
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(7.0))
                                        .bg(theme::card())
                                        .border_1()
                                        .border_color(theme::border())
                                        .child(
                                            text("¶")
                                                .text_size(px(12.0))
                                                .text_color(theme::muted_fg()),
                                        ),
                                )
                                .child(
                                    view()
                                        .flex_1()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            view()
                                                .flex_row()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    text(move || {
                                                        lookup(id)
                                                            .map(|current| current.name)
                                                            .unwrap_or_default()
                                                    })
                                                    .truncate()
                                                    .text_size(px(13.0))
                                                    .text_color(move || {
                                                        if selected.get() == Some(id) {
                                                            theme::accent_fg()
                                                        } else {
                                                            theme::fg()
                                                        }
                                                    }),
                                                )
                                                .child(
                                                    dynamic(move || {
                                                        let keyword = lookup(id)
                                                            .map(|current| current.keyword)
                                                            .unwrap_or_default();
                                                        if keyword.is_empty() {
                                                            return view().into_element();
                                                        }
                                                        view()
                                                            .px_1()
                                                            .py(px(1.0))
                                                            .rounded(px(4.0))
                                                            .bg(theme::glass_card())
                                                            .border_1()
                                                            .border_color(theme::border())
                                                            .child(
                                                                text(keyword).text_size(px(10.0)).text_color(
                                                                    theme::muted_fg(),
                                                                ),
                                                            )
                                                            .into_element()
                                                    }),
                                                ),
                                        )
                                        .child(
                                            text(move || {
                                                lookup(id)
                                                    .map(|current| preview_for_text(&current.content))
                                                    .unwrap_or_default()
                                            })
                                            .truncate()
                                                .text_size(px(11.0))
                                                .text_color(theme::muted_fg()),
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

/// 一个可插入的占位符。
#[derive(Clone)]
struct PlaceholderItem {
    token: String,
    label: String,
    sample: String,
}

fn placeholder_items() -> Vec<PlaceholderItem> {
    vec![
        PlaceholderItem {
            token: "{time}".into(),
            label: t!("snippet.time"),
            sample: crate::snippet_text::expand("{time}"),
        },
        PlaceholderItem {
            token: "{date}".into(),
            label: t!("snippet.date"),
            sample: crate::snippet_text::expand("{date}"),
        },
        PlaceholderItem {
            token: "{datetime}".into(),
            label: t!("snippet.datetime"),
            sample: crate::snippet_text::expand("{datetime}"),
        },
        PlaceholderItem {
            token: "{weekday}".into(),
            label: t!("snippet.weekday"),
            sample: crate::snippet_text::expand("{weekday}"),
        },
        PlaceholderItem {
            token: "{date format=\"yyyy-MM-dd\"}".into(),
            label: t!("snippet.custom_format"),
            sample: "{date format=\"yyyy-MM-dd\"}".into(),
        },
    ]
}

/// `{` 之后还没闭合的那段就是筛选词；没有就返回 `None`。
fn placeholder_query(content: &str) -> Option<String> {
    let at = content.rfind('{')?;
    let tail = &content[at + 1..];
    if tail.contains('}') || tail.chars().count() > 24 {
        return None;
    }
    Some(tail.to_string())
}

fn filtered_placeholders(content: &str) -> Vec<PlaceholderItem> {
    let query = placeholder_query(content)
        .unwrap_or_default()
        .to_lowercase();
    placeholder_items()
        .into_iter()
        .filter(|item| {
            query.is_empty()
                || item.label.to_lowercase().contains(&query)
                || item.token.to_lowercase().contains(&query)
        })
        .collect()
}

fn insert_placeholder(state: &State, index: usize) {
    let content = state.edit_content.get();
    let items = filtered_placeholders(&content);
    if items.is_empty() {
        return;
    }
    let item = &items[index.min(items.len() - 1)];
    let head = match content.rfind('{') {
        Some(at) => &content[..at],
        None => content.as_str(),
    };
    state.edit_content.set(format!("{head}{}", item.token));
    state.placeholder_menu.set(false);
}

/// 输入 `{` 时弹出：时间类占位符 + 自定义格式；输入文字筛选，方向键选择。
fn placeholder_menu(state: State) -> impl IntoElement {
    dynamic(move || {
        let content = state.edit_content.get();
        if !state.placeholder_menu.get() || placeholder_query(&content).is_none() {
            return view().into_element();
        }
        let items = filtered_placeholders(&content);
        if items.is_empty() {
            return view().into_element();
        }
        let selected = state.placeholder_selected.get().min(items.len() - 1);

        let mut list = view()
            .absolute()
            .top(px(56.0))
            .left(px(8.0))
            .right(px(8.0))
            .z_index(30)
            .flex_col()
            .p_2()
            .rounded(px(10.0))
            .bg(theme::card())
            .border_1()
            .border_color(theme::border())
            .shadow_md();

        for (index, item) in items.into_iter().enumerate() {
            let row_state = state.clone();
            let active = index == selected;
            list = list.child(
                view()
                    .on_press(move |_event: &PressEvent| insert_placeholder(&row_state, index))
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .bg(if active {
                        theme::glass_accent()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    })
                    .child(
                        text(item.label.clone())
                            .text_size(px(12.0))
                            .text_color(if active {
                                theme::accent_fg()
                            } else {
                                theme::fg()
                            }),
                    )
                    .child(
                        text(item.sample.clone())
                            .text_size(px(11.0))
                            .text_color(theme::muted_fg()),
                    ),
            );
        }
        list.into_element()
    })
}

fn preview_pane(state: State) -> impl IntoElement {
    view()
        .flex_1()
        .flex_col()
        .child(dynamic(move || preview_content(state.clone())))
}

fn preview_content(state: State) -> impl IntoElement {
    let mut root = view().flex_1().flex_col();
    if state.source.get() == Source::Snippets {
        return root.child(snippet_preview(state.clone()));
    }
    match selected_clip(&state) {
        None => root.items_center().justify_center().child(
            text(t!("panel.empty"))
                .text_size(px(13.0))
                .text_color(theme::muted_fg()),
        ),
        Some(clip) => root
            .child(view().flex_1().p_6().child(preview_body(clip.clone())))
            .child(information(clip)),
    }
}

fn snippet_preview(state: State) -> impl IntoElement {
    let mut root = view().flex_1().flex_col();
    match selected_snippet(&state) {
        None => {
            root = root.items_center().justify_center().child(
                text(t!("panel.empty_snippets"))
                    .text_size(px(13.0))
                    .text_color(theme::muted_fg()),
            );
        }
        Some(snippet) => {
            root = root
                .gap_4()
                .child(
                    view().px_6().pt_6().child(
                        text(snippet.name.clone())
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::fg()),
                    ),
                )
                .child(
                    scroll().flex_1().child(
                        view().px_6().py_2().child(
                            view()
                                .flex_row()
                                .flex_wrap()
                                .children(preview_runs(&snippet.content)),
                        ),
                    ),
                )
                ;
        }
    }
    root
}

/// 分段渲染时文本节点会吞掉首尾空格，段与段之间用一个固定宽度的空白单元补上。
fn preview_runs(content: &str) -> Vec<nsh_core::element::AnyElement> {
    let mut runs = Vec::new();
    for piece in crate::snippet_text::render(content) {
        match piece {
            crate::snippet_text::Piece::Value(value) => {
                runs.push(text_(value).text_color(theme::primary()).into_element());
            }
            crate::snippet_text::Piece::Text(text) => {
                let mut body = text.as_str();
                if body.starts_with(' ') {
                    runs.push(view().size(px(5.0)).into_element());
                    body = body.trim_start_matches(' ');
                }
                let trailing = body.ends_with(' ');
                let body = body.trim_end_matches(' ');
                if !body.is_empty() {
                    runs.push(text_(body.to_string()).into_element());
                }
                if trailing {
                    runs.push(view().size(px(5.0)).into_element());
                }
            }
        }
    }
    runs
}

fn text_(content: String) -> nsh_core::element::Text {
    text(content)
        .selectable(true)
        .text_size(px(14.0))
        .text_color(theme::fg())
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
                                text(t!(
                                    "panel.truncated_note",
                                    size = human_size(
                                        clip.content.as_ref().map_or(0, |c| c.len() as i64)
                                    )
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
                            t!("preview.file_many", count = clip.file_uris.len())
                        } else {
                            t!("preview.file_one")
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
    let source = clip
        .source_app
        .clone()
        .unwrap_or_else(|| t!("meta.unknown"));
    let mut info = view()
        .flex_col()
        .gap_3()
        .px_6()
        .py_5()
        .border_t(px(1.0))
        .border_t_color(theme::border())
        .child(
            text(t!("meta.info"))
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted_fg()),
        )
        .child(info_row(t!("meta.source"), source))
        .child(info_row(t!("meta.kind"), kind_label(clip.kind)))
        .child(info_row(t!("meta.copies"), clip.copy_count.to_string()));

    if clip.kind == ClipKind::Image {
        if let (Some(w), Some(h)) = (clip.image_w, clip.image_h) {
            info = info.child(info_row(t!("meta.dimensions"), format!("{w} × {h}")));
        }
    }
    if clip.kind == ClipKind::File {
        info = info.child(info_row(
            t!("meta.files"),
            clip.file_uris.len().to_string(),
        ));
    }
    if matches!(clip.kind, ClipKind::Text | ClipKind::Link) {
        if let Some(content) = &clip.content {
            info = info
                .child(info_row(
                    t!("meta.characters"),
                    content.chars().count().to_string(),
                ))
                .child(info_row(t!("meta.words"), word_count(content).to_string()));
        }
    }
    if !clip.file_uris.is_empty() {
        info = info.child(info_row(t!("meta.path"), clip.file_uris.join("\n")));
    }
    info.child(info_row(t!("meta.size"), human_size(clip.byte_size)))
        .child(info_row(
            t!("meta.copied_at"),
            format_time(clip.last_copied_at),
        ))
}

fn info_row(label: String, value: String) -> impl IntoElement {
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

/// 底栏的来源图标：剪贴板。
fn clipboard_icon() -> View {
    view().size(px(14.0)).items_center().justify_center().child(
        view()
            .relative()
            .w(px(11.0))
            .h(px(12.0))
            .rounded(px(2.5))
            .border_1()
            .border_color(theme::muted_fg())
            .child(
                view()
                    .absolute()
                    .top(px(-1.5))
                    .left(px(3.0))
                    .w(px(6.0))
                    .h(px(2.5))
                    .rounded(px(1.0))
                    .bg(theme::muted_fg()),
            ),
    )
}

/// 底栏的来源图标：片段（三条短线）。
fn snippet_icon() -> View {
    view().size(px(14.0)).flex_col().justify_center().gap(px(2.0)).children([
        view().w(px(10.0)).h(px(1.5)).rounded(px(0.75)).bg(theme::muted_fg()),
        view().w(px(10.0)).h(px(1.5)).rounded(px(0.75)).bg(theme::muted_fg()),
        view().w(px(6.0)).h(px(1.5)).rounded(px(0.75)).bg(theme::muted_fg()),
    ])
}

fn footer(state: State) -> impl IntoElement {
    let state = state.clone();
    dynamic(move || footer_inner(state.clone()))
}

fn footer_inner(state: State) -> impl IntoElement {
    let snippets = state.source.get() == Source::Snippets;
    let total = if snippets {
        state.snippets.get().len()
    } else {
        state.clips.get().len()
    };
    let visible = visible_ids(&state.rows.get()).len();
    let label = if snippets {
        t!("footer.snippets")
    } else {
        t!("footer.history")
    };
    let count = if visible == total {
        total.to_string()
    } else {
        format!("{visible} / {total}")
    };
    let mut right = view().flex_row().items_center().gap_4();
    if let Some(message) = state.flash.get() {
        right = right.child(
            text(message)
                .text_size(px(12.0))
                .text_color(theme::primary()),
        );
    }
    right = right.child(actions_button(state.clone()));

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .px_4()
        .py_2()
        .border_t(px(1.0))
        .border_t_color(theme::border())
        .child({
            let toggle_state = state.clone();
            let icon_source = state.source;
            view()
                .test_id("source-toggle")
                .on_press(move |_event: &PressEvent| {
                    let next = if toggle_state.source.get() == Source::Snippets {
                        Source::History
                    } else {
                        Source::Snippets
                    };
                    switch_source(&toggle_state, next);
                })
                .cursor_pointer()
                .flex_row()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded(px(6.0))
                .child(dynamic(move || {
                    if icon_source.get() == Source::Snippets {
                        snippet_icon().into_element()
                    } else {
                        clipboard_icon().into_element()
                    }
                }))
                // 标签与计数挨在一起，计数做成下标，像脚注标记。
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(2.0))
                        .child(
                            text(label)
                                .text_size(px(12.0))
                                .text_color(theme::muted_fg()),
                        )
                        .child(view().translate_y(px(2.5)).child(
                            text(count)
                                .text_size(px(8.0))
                                .text_color(theme::muted_fg()),
                        )),
                )
                .child(
                    text("⇄")
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                )
        })
        .child(right)
}

fn actions_button(state: State) -> impl IntoElement {
    view()
        .on_press(move || {
            if state.actions_open.get() {
                close_actions(&state);
            } else {
                enter_action_mode(&state, ActionMode::Main);
            }
        })
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded(px(6.0))
        .bg(theme::glass_card())
        .border_1()
        .border_color(theme::border())
        .child(
            text(t!("panel.actions"))
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        )
        .child(
            text(key_label("mod+k"))
                .text_size(px(11.0))
                .text_color(theme::muted_fg()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: i64, pinned: bool) -> Clip {
        Clip {
            id,
            kind: ClipKind::Text,
            content: Some("x".into()),
            preview: "x".into(),
            hash: String::new(),
            image_path: None,
            image_w: None,
            image_h: None,
            image_bytes: None,
            file_uris: Vec::new(),
            source_app: None,
            source_bundle: None,
            byte_size: 1,
            truncated: false,
            created_at: 0,
            last_copied_at: 0,
            pinned,
            copy_count: 1,
        }
    }

    #[test]
    fn default_selection_skips_pinned() {
        let rows = vec![
            Row::Section("Pinned".into()),
            Row::Clip(clip(1, true)),
            Row::Clip(clip(2, true)),
            Row::Section("Today".into()),
            Row::Clip(clip(3, false)),
            Row::Clip(clip(4, false)),
        ];
        assert_eq!(default_selection(&rows), Some(3));
    }

    #[test]
    fn default_selection_falls_back_to_the_first_pinned() {
        let rows = vec![
            Row::Section("Pinned".into()),
            Row::Clip(clip(1, true)),
            Row::Clip(clip(2, true)),
        ];
        assert_eq!(default_selection(&rows), Some(1));
    }

    #[test]
    fn default_selection_is_empty_without_clips() {
        assert_eq!(default_selection(&[]), None);
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    fn clip(id: i64, pinned: bool) -> Clip {
        Clip {
            id,
            kind: ClipKind::Text,
            content: Some("x".into()),
            preview: "x".into(),
            hash: String::new(),
            image_path: None,
            image_w: None,
            image_h: None,
            image_bytes: None,
            file_uris: Vec::new(),
            source_app: None,
            source_bundle: None,
            byte_size: 1,
            truncated: false,
            created_at: 0,
            last_copied_at: 0,
            pinned,
            copy_count: 1,
        }
    }

    fn rows(ids: &[(i64, bool)]) -> Vec<Row> {
        let mut rows = vec![Row::Section("Pinned".into())];
        for (id, pinned) in ids {
            if !pinned {
                rows.push(Row::Section("Today".into()));
                break;
            }
            rows.push(Row::Clip(clip(*id, *pinned)));
        }
        rows.extend(
            ids.iter()
                .filter(|(_, pinned)| !pinned)
                .map(|(id, pinned)| Row::Clip(clip(*id, *pinned))),
        );
        rows
    }

    #[test]
    fn deleting_selects_the_next_entry() {
        let before = rows(&[(1, true), (2, false), (3, false), (4, false)]);
        let after = rows(&[(1, true), (3, false), (4, false)]);
        assert_eq!(selection_after_delete(&before, &after, 2), (Some(3), false));
    }

    #[test]
    fn deleting_the_last_entry_selects_the_newest() {
        let before = rows(&[(1, true), (2, false), (3, false)]);
        let after = rows(&[(1, true), (2, false)]);
        assert_eq!(selection_after_delete(&before, &after, 3), (Some(1), true));
    }

    #[test]
    fn deleting_the_only_unpinned_entry_selects_the_first_pinned() {
        let before = rows(&[(1, true), (2, false)]);
        let after = rows(&[(1, true)]);
        assert_eq!(selection_after_delete(&before, &after, 2), (Some(1), true));
    }
}

