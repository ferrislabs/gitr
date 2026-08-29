//! Renders a `Patch` as rows painted by one custom element, instead of feeding a
//! reconstructed unified diff to a code editor.
//!
//! The editor gave selection, syntax highlighting and virtualised scrolling for free, and
//! it also fixed three things this view needs to control. Its gutter is always
//! `buffer_line + 1` in a single column, so a GitHub old/new pair is unreachable; it has no
//! full-width row background outside the cursor line, so an addition's tint hugs the glyphs
//! and a blank added line gets none at all; and it has no per-line element hook to work
//! around either. The `+`/`-` markers were carrying the signal the colour only half
//! carried; a full-width tint carries it whole, so this view draws no markers at all. All
//! three follow from the diff being one text document, so the rows are built here instead —
//! see the design note under `docs/superpowers/specs/`.
//!
//! A row carries one cell in [`DiffViewMode::Unified`] and two in [`DiffViewMode::Split`],
//! which is the only difference between the two views: [`body::Rows`] answers how many
//! columns a body has, and every horizontal position — the content width, a cell's bounds,
//! a gutter's origin — is that column's share of the element's width. A file, separator or
//! placeholder row keeps one full-width cell in either view, so the columns stay uniform
//! and a cell is `row * columns + column` rather than a lookup. Because a column is laid
//! out against `content_width / columns` and the width is measured over every cell, a
//! full-width header still fits in the half it is drawn in.
//!
//! A file header is the one row that is not placed that way, because it is the one row that
//! is not document. The code rows scroll horizontally because they *are* the thing being
//! read; a header describes what is being read and belongs to the frame around it. So its
//! chevron, pastille and path pin to the viewport's left edge and its bar and count to the
//! viewport's right edge, inset by `gpui_component::scroll::Scrollbar::width()` so the count
//! never sits under the overlay scrollbar. Only the background band still spans the element,
//! which is what keeps a scrolled header reading as one continuous row rather than as a
//! label floating over the code. The viewport is unmeasured on the first frame, exactly as
//! it is for [`body::row_window`], and the fallback has the same shape: for that one frame
//! the header falls back to the element's own edges and nothing is elided, and the second
//! frame corrects it.
//!
//! Pinning is what bounds the path, and the bound is a single number per frame:
//! `body::header_budget` is the viewport less the furniture at both ends, computed in
//! `request_layout` beside the visible range. A path wider than it is elided from the
//! *left* — `…detail/diff/split.rs` — because the tail of a path is what names the file and
//! the head is the part that can be lost. The elided string is the one that becomes the run,
//! so what is highlighted and what is copied still cannot drift apart. The price is the
//! other half of that trade: copying a header whose path is too long for the panel yields
//! the elided form, not the whole path.
//!
//! Selection comes back through `gpui-base`'s window-level participant system rather than
//! from the editor. [`body`] is the element that joins it: it registers one participant and
//! declares one run per cell on screen, left before right within a row, and only the code
//! text — or, on a file header, the path alone — becomes a run. Everything else a row draws
//! is painted directly and never registered: a line's two gutters, and a header's disclosure
//! chevron, status pastille, change bar and change count. That is what keeps line numbers and
//! a file's statistics out of the clipboard, and it is why copying a header yields a bare
//! path. The code rows scroll on both axes rather than
//! soft-wrapping,
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
//! stores them relative to `bounds.origin + scroll_offset`, and the participant reports the
//! two so that their sum is this element's own origin, which already carries the scroll: a
//! point off the top of the viewport is then a negative `y` rather than a lost one. Rows
//! that *are* on screen keep the projection's
//! own range, so what is highlighted and what is copied cannot drift apart. Select All is
//! the one selection with no window points to derive anything from — it is participant-local
//! (`set_local_selection`), so `copy_selection` answers the whole document for it directly
//! and every visible cell is highlighted whole.
//!
//! What stays unwindowed is the content width: `body::DiffBody::content_width` shapes every
//! cell on every layout pass, because the horizontal scroll extent has to consider rows that
//! are not on screen or the scrollbar would resize as the view scrolls vertically, and
//! because a cell is laid out against a column width that provably exceeds every cell's
//! natural width — which is what keeps a row one line tall and `ROW_HEIGHT` true. That is
//! affordable only because the text it measures is not rebuilt with it: [`DiffContent`]
//! holds the rows, one [`gpui::SharedString`] per cell, and the total changes of the patch's
//! largest file, derived by [`content`] when the patch, the view mode or the set of collapsed
//! files changes and handed to the element behind an `Rc` thereafter. So after the first
//! frame each cell is a hit in gpui's line-layout cache and the steady-state cost is a hash
//! of bytes that are already there, with no allocation and no reshape. A header row is the
//! exception in one respect: its elision binary-searches the tail against the shaped width,
//! so it allocates a handful of candidates per header per frame. They are the same
//! candidates every frame, so they settle into the same cache, and there are at most a
//! screenful of headers — but it is a search, not a lookup. That last number is
//! the scale every header's change bar is drawn against, and it belongs to the derivation for
//! the same reason the strings do: a bar is a share of the widest file in the patch, so
//! sizing one from the element would mean walking every file of the patch on every frame.
//! Collapsing is applied in that same derivation — the body rows of a collapsed file are
//! never emitted — so the element windows, paints and selects over a shorter list and knows
//! nothing of collapsing beyond mapping a click to a row and drawing a disclosure chevron at
//! the header's own left edge, which keeps that chevron out of every run and every copy.
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

use std::collections::HashSet;
use std::rc::Rc;

use domain::Patch;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_base::TextSelectionHandle;
use gpui_component::{
    ActiveTheme as _,
    scroll::{ScrollableElement as _, ScrollbarAxis},
};

use crate::diff_view_mode::DiffViewMode;

use body::{ROW_HEIGHT, Rows, body, cell_strings};
use model::{max_changes, rows};
use split::split_rows;

pub(super) type Collapsed = HashSet<usize>;

pub(super) type ToggleFile = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

pub(super) struct DiffContent {
    rows: Rows,
    strings: Vec<SharedString>,
    max_changes: usize,
}

impl DiffContent {
    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(super) fn height(&self) -> Pixels {
        px(self.rows.len() as f32 * ROW_HEIGHT)
    }
}

pub(super) fn content(patch: &Patch, mode: DiffViewMode, collapsed: &Collapsed) -> DiffContent {
    let rows = match mode {
        DiffViewMode::Unified => Rows::Unified(rows(patch, collapsed)),
        DiffViewMode::Split => Rows::Split(split_rows(patch, collapsed)),
    };
    let strings = cell_strings(&rows);
    DiffContent {
        rows,
        strings,
        max_changes: max_changes(patch),
    }
}

pub(super) fn render(
    content: Option<&Rc<DiffContent>>,
    select_all: bool,
    selection: &TextSelectionHandle,
    scroll: &ScrollHandle,
    toggle_file: ToggleFile,
    cx: &App,
) -> AnyElement {
    let Some(content) = content.filter(|content| !content.is_empty()) else {
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child("This commit changes nothing.")
            .into_any_element();
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
                    Rc::clone(content),
                    select_all,
                    selection.clone(),
                    scroll.clone(),
                    toggle_file,
                    theme.colors,
                    theme.mode,
                )),
        )
        .scrollbar(scroll, ScrollbarAxis::Both)
        .into_any_element()
}
