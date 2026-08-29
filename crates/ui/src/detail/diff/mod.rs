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
//! A row carries one cell in [`DiffViewMode::Unified`] and two in [`DiffViewMode::Split`],
//! which is the only difference between the two views: [`body::Rows`] answers how many
//! columns a body has, and every horizontal position — the content width, a cell's bounds,
//! a gutter's origin — is that column's share of the element's width. A file, hunk or
//! placeholder row keeps one full-width cell in either view, so the columns stay uniform
//! and a cell is `row * columns + column` rather than a lookup. Because a column is laid
//! out against `content_width / columns` and the width is measured over every cell, a
//! full-width header still fits in the half it is drawn in.
//!
//! Selection comes back through `gpui-base`'s window-level participant system rather than
//! from the editor. [`body`] is the element that joins it: it registers one participant and
//! declares one run per cell on screen, left before right within a row, and only the code
//! text becomes a run — the gutters and the marker are painted directly and never
//! registered, which is what keeps line
//! numbers and markers out of the clipboard. The rows scroll on both axes rather than soft-wrapping,
//! matching GitHub. `restrict_scroll_to_axis` still earns its place here, but not for the
//! reason it usually does: the container's `overflow_scroll()` puts both axes in
//! `Overflow::Scroll`, and gpui's vertical-onto-horizontal remap (`div.rs:3220-3224`,
//! `:3229-3233`) only fires when one axis is *not* `Overflow::Scroll`, so that remap is
//! unreachable here with or without the flag. What the flag does is axis-lock a precise
//! trackpad gesture through `ongoing_scroll.filter(..)` (`div.rs:3209-3216`), which is live
//! because `track_scroll` populates `ongoing_scroll` from the tracked handle (`div.rs:2165`).
//!
//! "On screen" is decided in `request_layout`, not in `paint`, because laying a row out is
//! the expensive half and nothing downstream can be narrowed without it. That ordering is
//! forced rather than chosen: every `TextLayout` accessor panics on a row that was skipped,
//! `len` and `line_height` on the cell the measure closure fills
//! (`gpui/src/elements/text.rs:935-942`) and `bounds` and `position_for_index` on that one
//! and on the cell prepaint fills as well (`:864-871`, `:930-932`). `selection_range_for_run`
//! reads `layout.len()` on *every* run it is handed, before any geometry
//! (`text_selection.rs:388`), so a row whose `StyledText` was skipped this frame cannot be
//! declared as a run at all — declaring it panics on the first scroll that has a live
//! selection.
//!
//! The copied text is therefore not read off that projection. A selection dragged past the
//! bottom edge would come back holding only the rows that happened to be on screen, and
//! nothing would report that it had been cut short. `body::DiffBody::copy_selection` derives
//! the row span from the selection's own window points instead — `body::selected_rows` — and
//! asks `body::selection_band` and `body::selected_range` for each cell's byte range, shaping
//! only the cells of the at most two rows whose ends the selection cuts through; every row
//! between them is whole. A band is a property of the row, so both cells of a row share it
//! and a cell's own column offset is what turns it into a range — which is exactly what
//! `point_in_selection_band` does to two runs that share a `y`, so the arithmetic and the
//! projection still agree cell for cell. Endpoints survive scrolling because `gpui-base`
//! stores them relative to
//! `bounds.origin`, which already carries the scroll, so a point off the top of the viewport
//! is a negative `y` rather than a lost one. Rows that *are* on screen keep the projection's
//! own range, so what is highlighted and what is copied cannot drift apart.
//!
//! What stays unwindowed is the content width: `body::DiffBody::content_width` shapes every
//! cell on every layout pass, because the horizontal scroll extent has to consider rows that
//! are not on screen or the scrollbar would resize as the view scrolls vertically, and
//! because a cell is laid out against a column width that provably exceeds every cell's
//! natural width — which is what keeps a row one line tall and `ROW_HEIGHT` true. After the
//! first frame those are hits in gpui's line-layout cache, so the cost is a hash of each
//! cell's bytes rather than a reshape.
//!
//! Copying depends on a fact `gpui-base`'s own doc comment does not state. A participant's
//! runs concatenate with no separator when `update_runs` projects them
//! (`text_selection.rs:593`); only whole participants are joined with `"\n"`
//! (`resolve_copy_items`, `:513`). So the separators are computed here, in
//! [`body::copy_text`], and published through `TextSelectionHandle::set_fallback_copy_text`
//! right after `update_runs` — which works only because that setter also clears
//! `projected_copy_text` (`:559`), the field `update_runs` just set, so `copy_item` falls
//! through to our fallback instead of the unseparated projection. `gpui-base` tracks a git
//! default branch pinned solely by `Cargo.lock`; if that clearing behaviour ever moves,
//! copying silently goes back to gluing rows together with no separator, and nothing fails
//! to say so.
//!
//! A row ends with `"\n"` and a column with `"\t"`, which makes a split copy the table the
//! reader is looking at. The alternative — a newline between the two columns as well — was
//! rejected because the band rule takes both cells of every row a selection passes through
//! whole, so a drag down one column still copies the other; newlines would interleave the
//! two sides and repeat every context line, and neither separator can yield compilable code
//! out of a two-column view. Narrowing the copy to one column was rejected for a harder
//! reason: the highlight comes from the projection, and dropping a cell the projection
//! selected is exactly the drift between clipboard and screen the paragraph above exists to
//! prevent.

mod body;
mod model;
mod pairing;
mod palette;
mod split;

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

use crate::diff_view_mode::DiffViewMode;

use body::{ROW_HEIGHT, Rows, body};
use model::rows;
use split::split_rows;

pub(super) fn render(
    patch: &Patch,
    mode: DiffViewMode,
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

    let content = match mode {
        DiffViewMode::Unified => Rows::Unified(rows(patch)),
        DiffViewMode::Split => Rows::Split(split_rows(patch)),
    };

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
                    content,
                    selection.clone(),
                    scroll.clone(),
                    theme.colors,
                    theme.mode,
                )),
        )
        .scrollbar(scroll, ScrollbarAxis::Both)
        .into_any_element()
}
