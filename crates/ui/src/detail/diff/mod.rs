//! Renders a `Patch` inside a single readonly code editor, instead of one hand-built
//! `div` per diff line.
//!
//! A hand-built row per line means every element for the whole patch is constructed on
//! every render, selects nothing, and highlights nothing beyond a full-width background
//! tint. Feeding [`super::format::unified_diff_text`] into a real
//! [`gpui_component::input::Editor`] gets text selection, `tree-sitter-diff` syntax
//! highlighting, and virtualised scrolling that only lays out the lines actually on
//! screen — for free, from the editor.
//!
//! One [`Editor`] holds the whole patch rather than one per file: a diff reads as a
//! single continuous document (this is how `git diff` and a GitHub raw patch view both
//! present it), a single scrollbar matches the rest of the panel, and it avoids creating
//! and tearing down one [`EditorState`] entity per file on every commit selection. The
//! trade-off is that per-file collapsing isn't available; nothing in this panel asks for
//! it.
//!
//! [`super::DetailPanel`] owns the [`EditorState`] entity and is the one that pushes new
//! text into it — this module only builds the element each render, exactly like the rest
//! of the panel's view functions.

#[allow(dead_code)]
mod model;
#[allow(dead_code)]
mod pairing;
#[allow(dead_code)]
mod palette;

use domain::Patch;
use gpui::{AnyElement, App, Entity, IntoElement, ParentElement as _, Styled as _, div};
use gpui_component::{
    ActiveTheme as _,
    input::{Editor, EditorState},
};

pub(super) fn render(patch: &Patch, diff_editor: &Entity<EditorState>, cx: &App) -> AnyElement {
    if patch.files.is_empty() {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child("This commit changes nothing.")
            .into_any_element();
    }

    Editor::new(diff_editor)
        .appearance(false)
        .bordered(false)
        .readonly(true)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(cx.theme().mono_font_size)
        .size_full()
        .into_any_element()
}
