# GitHub-style diff view

Date: 2026-08-29
Status: implemented

## The problem

Reading a diff in gitr is hard. The reported cause is the `+`/`-` at the start of every line,
which shifts the code one column and clutters it. Investigation found a second cause the
report did not name, and it is the larger one.

`TextDecoration` backgrounds hug the glyphs of a range rather than filling the row. gpui
paints a run's background as a quad from the first glyph to the last
(`gpui/src/text_system/line.rs:689-701`), one `line_height` tall and one glyph-advance
span wide. So the addition and deletion tints, which already carry GitHub's exact light
values, do not read as bands: they follow the ragged right edge of the text, and a blank
added or deleted line gets no background at all. The `+`/`-` markers are doing most of the
work of signalling a line's kind, because the colour only does half of it.

Both causes are properties of the same decision: the diff is not a list of elements, it is
**one text document fed to a code editor** (`crates/ui/src/detail/diff.rs`).

## Why the current architecture cannot deliver the target

Three capabilities are absent from gpui-component 0.5.2 at the pinned revision
`7acfc184382d30864a688fdaa6c9ff719efc53ae`, and each one is independently fatal:

- **Gutter content is fixed.** It is always `buffer_line + 1`, right-aligned, one column
  (`crates/base/src/input/base/element.rs:2001`). There is no hook to substitute text and
  no second column. A GitHub old/new pair is unreachable.
- **No full-width row background.** The only edge-to-edge row fill is the cursor's active
  line, coloured from the theme (`element.rs:2107-2131`). Nothing is per-range or per-row.
- **No per-line element injection.** No inlay hints, line widgets, block decorations, or
  row render hook anywhere in the input engine.

The `+`/`-` are also load-bearing for the current colouring: `tree-sitter-diff` maps
additions to the theme's `string` scope and deletions to `keyword` by parsing those very
markers. Removing them from the text removes the foreground colouring with them.

There is therefore no middle path that keeps the editor. The row rendering has to be built.

## What that costs, and what it does not

The module doc on `diff.rs` records three things bought by moving to the editor. Two are
recoverable and one was overstated:

- **Virtualisation** — recoverable. `gpui_component::virtual_list` is public. The renderer
  deleted at `5cc1caa` had no virtualisation at all, so this is an improvement over the
  state we are returning to, not a regression.
- **Syntax highlighting** — recoverable and improvable. `gpui_component::highlighter::Highlighter`
  is callable directly. Today the `diff` grammar colours by *role*; per file we can colour by
  the file's actual language, which is what GitHub does. Deferred, see Out of scope.
- **Text selection** — not lost. `gpui-base` ships a window-level, cross-element document
  selection system (`crates/base/src/text_selection.rs`), already mounted in this
  application: `gpui_component::Root` renders `TextSelectionLayer` and binds copy
  (`crates/ui/src/root.rs:551,592`), and `crates/gitr/src/main.rs:209` constructs that `Root`.

The work is therefore not implementing selection. It is implementing a custom `Element` per
row type, because `register` and `update_runs` must be called from `prepaint` and `paint`.
A reference implementation exists at
`crates/base/examples/showcase/components/text_selection.rs` (359 lines).

## Decisions

**No soft wrap; horizontal scrolling instead.** This is what GitHub does. It is a product
choice, not a technical necessity — an earlier draft of this spec justified it by
virtualisation, which was wrong: `v_virtual_list` takes `Rc<Vec<Size<Pixels>>>`, one size
per item, and accepts freely varying heights. What soft wrap really costs is measurement —
every row's wrapped height depends on the viewport width and has to be computed before
anything can be placed. The choice stands on fidelity and on that cost, and can be
revisited without invalidating the rest of this design.

**The `+`/`-` marker goes, column and all.** This reverses the original decision, which was
to keep it in sixteen pixels of its own between the gutters and the code, on the grounds
that it carried the signal for anyone who reads the green and the red poorly. That argument
was made against a mock-up. Seen running, the full-width row tint — the thing this whole
design exists to make possible, and the thing the editor could not draw — turned out to
carry the signal on its own, and the marker read as leftover furniture between the line
numbers and the code. So `MARKER_WIDTH`, `MARKER_PADDING` and the glyph itself are deleted
rather than blanked, and `code_left()` is now the gutters alone: `2 × GUTTER_WIDTH` unified,
`GUTTER_WIDTH` split. The code gains the sixteen pixels.

The accessibility argument is not answered, only outweighed, and it is the reason to record
this rather than to drop the bullet. If it is ever revisited, the cheapest mitigation is not
to put the character back: it is a 2px coloured edge down the left of the row, inside the
tint. That makes *shape* carry the information — which is what the marker was really for —
without returning a glyph to the text, so it stays out of every run and every clipboard and
costs the code no width. `palette::line_background` is where the per-origin colour it would
need used to live.

**The domain layer does not change.** `DiffLine` already stores `old_number`, `new_number`,
and a `content` with the marker stripped (`crates/domain/src/patch.rs:27`, parser at
`crates/vcs/src/process/patch_parser.rs:177-220`). Both line numbers are parsed today and
rendered nowhere. Nothing in `domain` or `vcs` needs touching.

**`gpui-base` becomes a direct dependency**, declared without a `rev`, matching how
`gpui-component` is declared (`Cargo.toml:26`). It is already in `Cargo.lock` at the same
revision, so this unifies rather than adding a second copy — the failure mode `CLAUDE.md`
warns about does not apply here.

## Module layout

`crates/ui/src/detail/diff.rs` becomes a directory:

| File | Owns |
|---|---|
| `diff/mod.rs` | Entry point: picks the layout for the current `DiffViewMode`. |
| `diff/model.rs` | `Row`: `FileHeader \| HunkHeader \| Line \| Placeholder`. Pure. |
| `diff/pairing.rs` | Left/right pairing for the split view. Pure. |
| `diff/body.rs` | The custom `Element`: visible-range windowing, hitbox, one selection participant, N runs, row painting, and the geometry of both views. |
| `diff/split.rs` | Side-by-side row geometry over `Vec<SplitRow>`. |
| `diff/palette.rs` | The four tint constants, moved from `decorations.rs`. |
| `crates/ui/src/diff_view_mode.rs` | `DiffViewMode`, modelled on `theme_preference.rs`. |

`model.rs` and `pairing.rs` hold the logic and carry the tests. `body.rs` holds the risk.
A separate `diff/unified.rs` was planned and not written: the unified view differs from the
split one only in its column count and the width of its gutters, so `body.rs` answers both
from `Rows` rather than arranging each in a module of its own. `split.rs` stays thin — it
arranges rows, it does not decide anything.

## Data model

Unified rendering consumes a flat `Vec<Row>` built once per patch:

```
Row::FileHeader { path, status, added, deleted }
Row::HunkHeader { text }
Row::Line { origin, old_number: Option<u32>, new_number: Option<u32>, content }
Row::Placeholder { message }
```

Split rendering consumes `Vec<SplitRow>`, where each side is an `Option<Line>` — `None`
renders as a blank, unnumbered, untinted cell.

Both are derived when the patch or the view mode changes, never per frame.

## Pairing algorithm

Within a hunk: accumulate consecutive deletions into `D` and consecutive additions into
`A`. When the run ends — at a context line or at the end of the hunk — emit `max(|D|,|A|)`
rows pairing `D[i]` with `A[i]`, padding the shorter side with `None`. A context line emits
one row carrying the same text on both sides.

This is a pure function from `&[DiffLine]` to `Vec<SplitRow>` and is where the bulk of the
tests go. Cases that must be covered: pure addition, pure deletion, equal-length
replacement, unequal-length replacement in both directions, a run at the very start of a
hunk, a run at the very end with no trailing context, and a hunk of context only.

## Selection

The body element registers one participant and declares that frame's visible rows as runs,
in `prepaint`/`paint`:

- `TextSelectionRegistration::new(hitbox, viewport)` with `.with_document_order(n)` so a drag
  across rows copies in document order, and `.with_scroll_offset(bounds.origin −
  viewport.origin)`. `gpui-base` stores an endpoint as `position − bounds.origin −
  scroll_offset` (`text_selection.rs:1336`) and resolves it back by adding both (`:806-812`),
  so what matters is the sum. Reporting the element's own bounds and no offset gives the same
  sum, and was what this was built with — but the registered *rectangle* is also the one
  `AutoScroll::compute_delta` measures the pointer against (`:1452`), and the element's bounds
  are the whole diff, not the part of it on screen, so the trigger zone sat off-screen and a
  drag past the bottom edge never scrolled. Registering the viewport and reporting the
  difference of the two origins keeps the sum identical by construction and puts the trigger
  zone where the user can reach it.
- `TextSelectionRun::new(text, layout, bounds)` — **only for the code content**. The
  gutters are painted directly and are never registered, which is what makes a copied diff
  come out as clean code with no line numbers. This is strictly better than the editor,
  which copies its markers today.
- `TextSelectionContentKey` turns out not to be needed. It is a `u64` the participant
  computes from an endpoint's content point through
  `TextSelectionHandle::resolve_content_key_with`, which `gpui-base` stores on the endpoint
  and hands straight back on `TextSelectionSnapshot::anchor().content_key()`. It is a
  channel back to the participant and nothing more: it takes no part in hit-testing, in
  `project_ranges`, or in copying. What actually makes an off-screen endpoint survive is
  that the endpoint is stored as `position − bounds.origin` and this element's
  `bounds.origin` already carries the scroll, so an endpoint above the viewport is a
  negative `y` rather than a lost one — and a row index here is `y / ROW_HEIGHT`, which is
  the identity a key would have carried anyway.

The text has to go through `StyledText` rather than `div().child("…")`, because a run needs
a `TextLayout`. This is the one structural constraint the selection system imposes on the
row.

## Virtualisation

**Not `virtual_list`.** That list produces one element per row, and the selection API runs
the other way round: a participant declares all its runs in a single
`update_runs(&[TextSelectionRun]) -> TextSelectionProjection` call, and the projection
returns one `Option<Range<usize>>` per run, in the order given. Per-row elements sharing
one handle would each overwrite the previous row's runs; a handle per row means one
`Entity` per visible line, rebuilt on every scroll.

The diff body is therefore **one custom `Element`** that computes its own visible range and
paints the rows in it, registering a single participant and declaring that frame's visible
rows as N runs in one call. Virtualisation is kept; it is hand-rolled rather than borrowed.

This is more code than delegating to a list, and it is the single largest piece of work in
this design. It is also not optional: it follows from the shape of the only selection API
available.

## Toggle and persistence

`DiffViewMode { Unified, Split }`, defaulting to `Unified`. Persisted to
`diff-view-preference.json` through the `save_to`/`load_from` plus `save`/`load` pair that
`persistence.rs` already uses for the theme, so the pure half stays testable without
touching the disk. Surfaced as a segmented control in the detail panel's existing tab row
(`detail/mod.rs:164-187`).

## Edge cases

- Binary file, rename with no content change, and an empty commit keep their three existing
  placeholder messages.
- `\ No newline at end of file` is already swallowed by the parser. GitHub renders a marker
  for it; v1 does not.
- Lines beyond 10 000 characters skip highlighting in gpui-component. With no soft wrap they
  simply scroll.
- A file that is pure addition renders an entirely blank left column in split view. That is
  correct and should not be special-cased into a single-column view.

## Testing

- `model.rs` and `pairing.rs`: pure unit tests, the bulk of the suite.
- The contrast reasoning in `decorations.rs:20-35` moves to `palette.rs` with its tests
  intact. Those values were chosen against two bars — legibility of text on the band, and
  visibility of the band against the theme background — and that reasoning must survive the
  move.
- `format::unified_diff_text_with_line_ranges`, `DiffLineRanges`, `write_file` and the
  private path helpers that exist only to serve them become dead. Delete them along with
  their tests. The case guarded by
  `line_ranges_are_read_from_the_marker_column_not_the_lines_own_content` does not
  disappear; it becomes trivially correct once no marked text is reconstructed.
- No element-tree tests, consistent with the rest of the crate.
- Manual verification in the running app is required and must not be skipped: nothing here
  proves a pixel.

## Out of scope

Deliberately excluded from v1, each worth its own change:

- Syntax highlighting of code by the file's language.
- Word-level intra-line highlighting of what actually changed.
- Expandable context beyond the hunk.
- Per-file collapsing.

## Risks and order of work

The custom `Element` is the risk, not the algorithm. Build it first, against the unified
view alone, and prove three things before writing any split-view code: that a drag selects
across row boundaries, that a copy yields code without gutters or markers, and that both
survive scrolling. A bad surprise then arrives before the second view is written rather
than after.

Second risk: the coordinate handling. `TextLayout::bounds`, `line_height`, `len`,
`position_for_index` and `index_for_position` all panic when called before the text has been
laid out, but not on the same cell: `len` and `line_height` need only the one the measure
closure fills (`text.rs:935-942`), while `bounds`, `position_for_index` and
`index_for_position` need the one prepaint fills as well (`:830-837`, `:864-871`,
`:930-932`). All five are safe from `paint` and from nowhere earlier — the distinction
matters only when deciding which rows may be skipped. This is where hand-rolled windowing
bites, and the
two obvious moves are both wrong. Narrowing layout while keeping a run per row panics on the
first scroll with a live selection, because `selection_range_for_run` reads `layout.len()` on
every run it is handed. Narrowing the runs to match avoids the panic and silently truncates a
copy to whatever rows are on screen.

The way out is that the copy does not have to come from the projection. `update_runs` covers
the window and drives only the highlight; the copied text is derived from the selection's own
window points, which are scroll-invariant, so the row span is arithmetic and only the two rows
whose ends the selection cuts through need shaping at all — a row between them is whole by
construction, and answering `0..len` for it must short-circuit before the shaper is reached
rather than after, or the saving is only notional. A row already on screen keeps the
projection's range, so the highlight and the clipboard cannot disagree. What stays unwindowed
is the content-width measurement, which must consider every row or the horizontal scroll
extent moves as the view scrolls vertically.

## Files touched

- New: `crates/ui/src/detail/diff/` (six files), `crates/ui/src/diff_view_mode.rs`,
  `docs/superpowers/specs/2026-08-29-github-style-diff-view-design.md`.
- Rewritten: `crates/ui/src/detail/mod.rs` — `pending_diff`, `diff_editor` and
  `diff_decorations` all disappear, along with the flush block that pushed text into the
  editor.
- Reduced: `crates/ui/src/detail/format.rs` — the text reconstruction goes. `hunk_heading`
  stays, and so do `abbreviate`, `format_timestamp` and `escape_markdown`, which serve
  `metadata.rs` and are unrelated to this change. Note that `file_header` and `diff_stat`
  do **not** exist in this file today — they lived only in the renderer deleted at
  `5cc1caa` and have to be written again, recoverable from
  `git show 5cc1caa^:crates/ui/src/detail/format.rs`.
- Deleted: `crates/ui/src/detail/decorations.rs`, superseded by `diff/palette.rs`.
- Extended: `crates/ui/src/persistence.rs`, `Cargo.toml`.
