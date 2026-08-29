//! Renders a `Patch` as rows painted by one custom element, instead of feeding a
//! reconstructed unified diff to a code editor.
//!
//! The editor gave selection, syntax highlighting and virtualised scrolling for free, and
//! it also fixed three things this view needs to control. Its gutter is always
//! `buffer_line + 1` in a single column, so a GitHub old/new pair is unreachable; it has no
//! full-width row background outside the cursor line, so an addition's tint hugs the glyphs
//! and a blank added line gets none at all; and it has no per-line element hook to work
//! around either. The `+`/`-` markers were carrying the signal the colour only half
//! carried. All three follow from the diff being one text document, so the rows are built
//! here instead — see the design note under `docs/superpowers/specs/`.
//!
//! Selection comes back through `gpui-base`'s window-level participant system rather than
//! from the editor. [`body`] is the element that joins it: it registers one participant and
//! declares one run per row, and only the code text becomes a run — the gutters and the
//! marker are painted directly and never registered, which is what keeps line numbers and
//! markers out of the clipboard. The rows scroll on both axes rather than soft-wrapping,
//! matching GitHub. `restrict_scroll_to_axis` still earns its place here, but not for the
//! reason it usually does: the container's `overflow_scroll()` puts both axes in
//! `Overflow::Scroll`, and gpui's vertical-onto-horizontal remap (`div.rs:3220-3224`,
//! `:3229-3233`) only fires when one axis is *not* `Overflow::Scroll`, so that remap is
//! unreachable here with or without the flag. What the flag does is axis-lock a precise
//! trackpad gesture through `ongoing_scroll.filter(..)` (`div.rs:3209-3216`), which is live
//! because `track_scroll` populates `ongoing_scroll` from the tracked handle (`div.rs:2165`).
//!
//! Copying depends on a fact `gpui-base`'s own doc comment does not state. A participant's
//! runs concatenate with no separator when `update_runs` projects them
//! (`text_selection.rs:593`); only whole participants are joined with `"\n"`
//! (`resolve_copy_items`, `:513`). So the row separator is computed here, in
//! [`body::copy_text`], and published through `TextSelectionHandle::set_fallback_copy_text`
//! right after `update_runs` — which works only because that setter also clears
//! `projected_copy_text` (`:559`), the field `update_runs` just set, so `copy_item` falls
//! through to our fallback instead of the unseparated projection. `gpui-base` tracks a git
//! default branch pinned solely by `Cargo.lock`; if that clearing behaviour ever moves,
//! copying silently goes back to gluing rows together with no separator, and nothing fails
//! to say so.

mod body;
mod model;
#[allow(dead_code)]
mod pairing;
mod palette;

use domain::Patch;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_base::TextSelectionHandle;
use gpui_component::{
    ActiveTheme as _,
    scroll::{ScrollableElement as _, ScrollbarAxis},
};

use body::{ROW_HEIGHT, body};
use model::rows;

pub(super) fn render(
    patch: &Patch,
    selection: &TextSelectionHandle,
    scroll: &ScrollHandle,
    cx: &App,
) -> AnyElement {
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

    let theme = cx.theme();
    div()
        .relative()
        .size_full()
        .child(
            div()
                .id("detail-diff-scroll")
                .size_full()
                .overflow_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(scroll)
                .font_family(theme.mono_font_family.clone())
                .text_size(theme.mono_font_size)
                .line_height(px(ROW_HEIGHT))
                .child(body(
                    rows(patch),
                    selection.clone(),
                    scroll.clone(),
                    theme.colors,
                    theme.mode,
                )),
        )
        .scrollbar(scroll, ScrollbarAxis::Both)
        .into_any_element()
}
