//! M0 的静态界面骨架：先把信息架构与视觉定下来，数据与交互在 M1/M2 接入。
//!
//! 布局对齐 Raycast Clipboard History：
//! 顶部搜索 + 类型筛选，左侧分组历史列表，右侧预览与元信息，底部操作栏。

use nsh_core::prelude::*;

use crate::theme;

/// 整个应用的骨架。
pub fn shell() -> impl IntoElement {
    view()
        .flex_1()
        .bg(theme::bg())
        .text_color(theme::fg())
        .child(header())
        .child(view().flex_row().flex_1().child(sidebar()).child(preview()))
        .child(footer())
}

fn header() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .border_b(px(1.0))
        .border_b_color(theme::border())
        .child(search_box())
        .child(type_filter())
}

fn search_box() -> impl IntoElement {
    view()
        .flex_1()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
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
            text("搜索剪贴板历史…")
                .text_size(px(14.0))
                .text_color(theme::muted_fg()),
        )
}

fn type_filter() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(8.0))
        .bg(theme::card())
        .border_1()
        .border_color(theme::border())
        .child(text("全部类型").text_size(px(13.0)))
        .child(
            text("⌄")
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        )
}

fn sidebar() -> impl IntoElement {
    view()
        .w(px(theme::SIDEBAR_WIDTH))
        .flex_col()
        .bg(theme::surface())
        .border_r(px(1.0))
        .border_r_color(theme::border())
        .child(
            scroll().flex_1().child(
                view()
                    .flex_col()
                    .py_2()
                    .child(section_label("已固定"))
                    .child(clip_row("T", "13592613640", false))
                    .child(section_label("今天"))
                    .child(clip_row("↗", "https://nsh.dev/", false))
                    .child(clip_row("↗", "https://gpui-kit.com/", false))
                    .child(clip_row("T", "astryx", false))
                    .child(clip_row("T", "升级当作一次有意的迁移，用它的 a…", false))
                    .child(clip_row("T", "确版本（0.4.1，不用 ^），升级当…", false))
                    .child(clip_row("T", "仓/日志这些关键面保持现状；", false))
                    .child(clip_row("↗", "https://github.com/meta/astryx", false))
                    .child(clip_row("◫", "截屏 2026-11-11 21.31.01", true)),
            ),
        )
}

fn section_label(label: &'static str) -> impl IntoElement {
    view().px_4().pt_4().pb_2().child(
        text(label)
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::muted_fg()),
    )
}

fn clip_row(icon: &'static str, title: &'static str, selected: bool) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .mx_2()
        .px_2()
        .py_2()
        .rounded(px(8.0))
        .bg(if selected {
            theme::accent()
        } else {
            theme::surface()
        })
        .child(icon_box(icon))
        .child(
            view().flex_1().child(
                text(title)
                    .truncate()
                    .text_size(px(14.0))
                    .text_color(if selected {
                        theme::accent_fg()
                    } else {
                        theme::fg()
                    }),
            ),
        )
}

fn icon_box(icon: &'static str) -> impl IntoElement {
    view()
        .size(px(26.0))
        .items_center()
        .justify_center()
        .rounded(px(7.0))
        .bg(theme::card())
        .border_1()
        .border_color(theme::border())
        .child(
            text(icon)
                .text_size(px(12.0))
                .text_color(theme::muted_fg()),
        )
}

fn preview() -> impl IntoElement {
    view()
        .flex_1()
        .flex_col()
        .child(
            view()
                .flex_1()
                .p_6()
                .child(
                    view()
                        .flex_1()
                        .w_full()
                        .flex_col()
                        .gap_3()
                        .child(
                            view()
                                .flex_1()
                                .w_full()
                                .items_center()
                                .justify_center()
                                .rounded(px(10.0))
                                .bg(theme::card())
                                .border_1()
                                .border_color(theme::border())
                                .child(
                                    text("图片预览")
                                        .text_size(px(13.0))
                                        .text_color(theme::muted_fg()),
                                ),
                        )
                        .child(
                            text("截屏 2026-11-11 21.31.01")
                                .text_size(px(13.0))
                                .text_color(theme::muted_fg()),
                        ),
                ),
        )
        .child(information())
}

fn information() -> impl IntoElement {
    view()
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
        .child(info_row("来源", "Safari"))
        .child(info_row("类型", "图片"))
        .child(info_row("尺寸", "1699 × 334"))
        .child(info_row("大小", "225 KB"))
        .child(info_row("复制于", "今天 21:31"))
}

fn info_row(label: &'static str, value: &'static str) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            text(label)
                .text_size(px(13.0))
                .text_color(theme::muted_fg()),
        )
        .child(text(value).text_size(px(13.0)).text_color(theme::fg()))
}

fn footer() -> impl IntoElement {
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
                .child(
                    view()
                        .size(px(8.0))
                        .rounded_full()
                        .bg(theme::primary()),
                )
                .child(
                    text("剪贴板历史")
                        .text_size(px(12.0))
                        .text_color(theme::muted_fg()),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_4()
                .child(hint("粘贴到 Safari", "↵"))
                .child(hint("操作", "⌘K")),
        )
}

fn hint(label: &'static str, key: &'static str) -> impl IntoElement {
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
                    text(key)
                        .text_size(px(11.0))
                        .text_color(theme::muted_fg()),
                ),
        )
}
