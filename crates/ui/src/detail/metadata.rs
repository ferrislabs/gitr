//! Renders the commit metadata header (subject, identifier, parents and author) and,
//! separately, the commit message body — split because [`render_description`] answers
//! `None` for a commit that has nothing beyond its subject line, which renders no row
//! rather than an empty one. Both scroll together inside the General tab's single scroll
//! region; see `detail::general_tab` for where the two are recombined.
//!
//! Every value here goes through [`gpui_component::text::markdown`] rather than a plain
//! `div`, which is what makes the commit's identifier — the thing most worth copying in
//! this panel — selectable.

use domain::{Commit, Parents};
use gpui::{
    AnyElement, App, ElementId, HighlightStyle, IntoElement, ParentElement as _, SharedString,
    StyleRefinement, Styled as _, div, px,
};
use gpui_component::{ActiveTheme as _, text, text::TextViewStyle};

use super::format::{abbreviate, escape_markdown, format_timestamp};

const LABEL_WIDTH: f32 = 84.;
const BADGE_RADIUS: f32 = 4.;

pub(super) fn render_header(commit: &Commit, cx: &App) -> impl IntoElement {
    let mono = cx.theme().mono_font_family.clone();

    let mut rows = vec![
        row("Subject", selectable("subject", &commit.summary, cx), cx),
        row(
            "ID",
            div()
                .flex()
                .child(mono_badge(
                    ElementId::Name("id".into()),
                    commit.id.to_string(),
                    mono.clone(),
                    cx,
                ))
                .into_any_element(),
            cx,
        ),
    ];

    if let Some(parents) = parent_badges(&commit.parents, mono, cx) {
        rows.push(row("Parents", parents, cx));
    }

    rows.push(row(
        "Author",
        selectable("author", &author_line(commit), cx),
        cx,
    ));

    div().flex().flex_col().gap_1().p_3().children(rows)
}

/// The commit message body, if it has one beyond the subject line — `None` renders
/// nothing rather than an empty scroll-region row.
///
/// Unlike [`selectable`], the body is not escaped: it is prose the committer wrote, and
/// [`gpui_component::text::markdown`] rendering it as Markdown (lists, emphasis, code
/// spans) is the point, not a hazard to guard against.
pub(super) fn render_description(commit: &Commit, cx: &App) -> Option<AnyElement> {
    let body = commit.body.trim();
    if body.is_empty() {
        return None;
    }

    Some(
        div()
            .flex_shrink_0()
            .px_3()
            .py_3()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(
                text::markdown(body.to_string())
                    .selectable(true)
                    .style(body_style(cx)),
            )
            .into_any_element(),
    )
}

/// Gives a fenced block in the commit body the app's own code rendering.
///
/// `TextViewStyle::default()` pins `highlight_theme` to the light one whatever the app is
/// set to, so a fenced block in a commit message rendered light-on-dark until this passed
/// the active theme through. `is_dark` travels with it because the highlighter picks
/// fallback colours from it, not from the theme it was handed.
fn body_style(cx: &App) -> TextViewStyle {
    let theme = cx.theme();

    let code_block = StyleRefinement::default()
        .bg(theme.secondary)
        .rounded(px(BADGE_RADIUS))
        .border_1()
        .border_color(theme.border);

    let mut style = TextViewStyle::default()
        .code_block(code_block)
        .inline_code(HighlightStyle {
            background_color: Some(theme.foreground.opacity(0.08)),
            color: Some(theme.foreground),
            ..Default::default()
        });
    style.highlight_theme = theme.highlight_theme.clone();
    style.is_dark = theme.mode.is_dark();
    style
}

fn selectable(id: &'static str, value: &str, cx: &App) -> AnyElement {
    text::TextView::markdown(id, escape_markdown(value))
        .selectable(true)
        .text_color(cx.theme().foreground)
        .into_any_element()
}

fn row(label: &'static str, value: impl IntoElement, cx: &App) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .text_sm()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
        .into_any_element()
}

fn author_line(commit: &Commit) -> String {
    let signature = &commit.author;
    format!(
        "{} <{}>    {}",
        signature.name,
        signature.email,
        format_timestamp(signature.time)
    )
}

fn parent_badges(parents: &Parents, mono: SharedString, cx: &App) -> Option<AnyElement> {
    if parents.is_empty() {
        return None;
    }

    let badges = parents.iter().enumerate().map(move |(index, parent)| {
        mono_badge(
            ElementId::NamedInteger("parent".into(), index as u64),
            abbreviate(parent),
            mono.clone(),
            cx,
        )
    });

    Some(
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(badges)
            .into_any_element(),
    )
}

/// An object id as a badge whose text is still selectable.
///
/// A `Tag` would be the obvious component and is the wrong one: it renders its child as
/// plain text, so the identifier — the thing most worth copying out of this panel — could
/// be read and not taken. The badge is therefore a styled `div` wrapping the same
/// selectable text view the other rows use.
fn mono_badge(id: ElementId, value: String, mono: SharedString, cx: &App) -> impl IntoElement {
    let foreground = cx.theme().foreground;

    div()
        .px_1p5()
        .rounded(px(BADGE_RADIUS))
        .bg(foreground.opacity(0.06))
        .border_1()
        .border_color(foreground.opacity(0.18))
        .child(
            text::TextView::markdown(id, value)
                .selectable(true)
                .font_family(mono)
                .text_color(foreground),
        )
}
