# GitHub-style diff view — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the editor-backed diff with a rendered one that reads like GitHub — old/new line-number gutters, a full-width tint per line, the `+`/`-` in its own column, unified and side-by-side views, and text selection that crosses rows.

**Architecture:** The diff stops being a text document fed to `EditorState` and becomes a derived `Vec<Row>` painted by one custom `Element`. That element does its own visible-range windowing and declares every visible row's text as runs of a single selection participant, because `gpui-base`'s selection API projects one range per run from one `update_runs` call. Row derivation and left/right pairing are pure functions and carry the tests; the element carries the risk.

**Tech Stack:** Rust 2024, gpui (zed default branch), gpui-component 0.5.2 @ `7acfc18`, `gpui-base` (new direct dependency, same git source, no `rev`).

**Spec:** `docs/superpowers/specs/2026-08-29-github-style-diff-view-design.md`

## Global Constraints

- Crates are imported unprefixed: `use domain::…`, never `use gitr_domain::…`.
- `cargo test -p gitr-domain -p gitr-graph` is the fast loop; `cargo test --workspace` is the exit check.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` must pass at every commit.
- `[workspace.lints.clippy]` sets `todo = "deny"` and `dbg_macro = "deny"`. No `todo!()` scaffolding — every task compiles for real.
- Do not add comments to the code. Doc comments (`///`) on new functions and fields included. Reasoning goes in the commit message.
- Commit convention: `<type>(<subject>): <imperative message>`. Never add an AI co-author trailer.
- Never pin `gpui` or any gpui-component crate to a `rev`. `Cargo.lock` is the pin.
- Cargo holds one lock per target directory — do not run `clippy` and `test` concurrently.
- `domain` and `vcs` are not touched by any task in this plan.

---

### Task 1: Row model

Derives the flat row list the unified view paints. Pure — no gpui, no window.

**Files:**
- Create: `crates/ui/src/detail/diff/model.rs`
- Create: `crates/ui/src/detail/diff/mod.rs`
- Delete: `crates/ui/src/detail/diff.rs` (its body moves to `mod.rs` unchanged for now)

**Interfaces:**
- Consumes: `domain::{DiffLine, FilePatch, FileStatus, LineOrigin, Patch}`.
- Produces: `pub(super) enum Row`, `pub(super) fn rows(patch: &Patch) -> Vec<Row>`. Task 4 paints `Row`; Task 2 reuses `file_stat`.

- [ ] **Step 1: Turn the module into a directory**

```bash
mkdir -p crates/ui/src/detail/diff
git mv crates/ui/src/detail/diff.rs crates/ui/src/detail/diff/mod.rs
```

Then add to the top of `crates/ui/src/detail/diff/mod.rs`:

```rust
mod model;
```

- [ ] **Step 2: Write the failing tests**

Create `crates/ui/src/detail/diff/model.rs` containing only this test module plus the `use super::*;` it needs:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use domain::{FileStatus, Hunk};
    use std::path::PathBuf;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, content: &str) -> DiffLine {
        DiffLine { origin, old_number: old, new_number: new, content: content.to_string() }
    }

    fn file(hunks: Vec<Hunk>, is_binary: bool) -> FilePatch {
        FilePatch {
            old_path: Some(PathBuf::from("src/main.rs")),
            new_path: Some(PathBuf::from("src/main.rs")),
            status: FileStatus::Modified,
            is_binary,
            hunks,
        }
    }

    fn hunk(lines: Vec<DiffLine>) -> Hunk {
        Hunk { old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, heading: String::new(), lines }
    }

    #[test]
    fn a_modified_file_yields_a_header_a_hunk_header_and_one_row_per_line() {
        let patch = Patch { files: vec![file(vec![hunk(vec![
            line(LineOrigin::Context, Some(1), Some(1), "keep"),
            line(LineOrigin::Deletion, Some(2), None, "gone"),
            line(LineOrigin::Addition, None, Some(2), "new"),
        ])], false)] };

        let rows = rows(&patch);

        assert!(matches!(rows[0], Row::FileHeader { .. }));
        assert!(matches!(rows[1], Row::HunkHeader { .. }));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn a_line_row_carries_both_numbers_and_the_bare_content() {
        let patch = Patch { files: vec![file(vec![hunk(vec![
            line(LineOrigin::Deletion, Some(7), None, "-not a marker"),
        ])], false)] };

        let rows = rows(&patch);

        assert_eq!(
            rows[2],
            Row::Line {
                origin: LineOrigin::Deletion,
                old_number: Some(7),
                new_number: None,
                content: "-not a marker".to_string(),
            },
            "content is stored bare by the parser and must not be re-marked here"
        );
    }

    #[test]
    fn a_binary_file_yields_a_placeholder_instead_of_lines() {
        let patch = Patch { files: vec![file(Vec::new(), true)] };
        let rows = rows(&patch);
        assert_eq!(rows[1], Row::Placeholder { message: "Binary file not shown." });
    }

    #[test]
    fn a_file_with_no_hunks_yields_a_no_change_placeholder() {
        let patch = Patch { files: vec![file(Vec::new(), false)] };
        let rows = rows(&patch);
        assert_eq!(rows[1], Row::Placeholder { message: "No content changes." });
    }

    #[test]
    fn every_file_contributes_its_own_header() {
        let patch = Patch { files: vec![file(Vec::new(), true), file(Vec::new(), true)] };
        let headers = rows(&patch).iter().filter(|r| matches!(r, Row::FileHeader { .. })).count();
        assert_eq!(headers, 2);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gitr-ui --lib detail::diff::model`
Expected: FAIL to compile — `Row` and `rows` are not defined.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/ui/src/detail/diff/model.rs`:

```rust
use domain::{DiffLine, FilePatch, LineOrigin, Patch};

use crate::detail::format;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    FileHeader { path: String, stat: String },
    HunkHeader { text: String },
    Line {
        origin: LineOrigin,
        old_number: Option<u32>,
        new_number: Option<u32>,
        content: String,
    },
    Placeholder { message: &'static str },
}

pub(super) fn rows(patch: &Patch) -> Vec<Row> {
    let mut rows = Vec::new();
    for file in &patch.files {
        rows.push(Row::FileHeader {
            path: file.display_path(),
            stat: file_stat(file),
        });
        push_body(&mut rows, file);
    }
    rows
}

pub(super) fn file_stat(file: &FilePatch) -> String {
    format!("+{} \u{2212}{}", file.added_lines(), file.deleted_lines())
}

fn push_body(rows: &mut Vec<Row>, file: &FilePatch) {
    if file.is_binary {
        rows.push(Row::Placeholder { message: "Binary file not shown." });
        return;
    }
    if file.hunks.is_empty() {
        rows.push(Row::Placeholder { message: "No content changes." });
        return;
    }
    for hunk in &file.hunks {
        rows.push(Row::HunkHeader { text: format::hunk_heading(hunk) });
        rows.extend(hunk.lines.iter().map(line_row));
    }
}

fn line_row(line: &DiffLine) -> Row {
    Row::Line {
        origin: line.origin,
        old_number: line.old_number,
        new_number: line.new_number,
        content: line.content.clone(),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gitr-ui --lib detail::diff::model`
Expected: PASS, 5 tests.

- [ ] **Step 6: Check `display_path` returns a `String`**

Run: `grep -n "fn display_path" -A 6 crates/domain/src/patch.rs`
If it returns `&Path` or `Option<&Path>` rather than `String`, adjust the `path:` field construction to `.display().to_string()`. Do not change `domain`.

- [ ] **Step 7: Lint, format, commit**

```bash
cargo fmt --all
cargo clippy -p gitr-ui --all-targets -- -D warnings
git add crates/ui/src/detail/diff/
git commit -m "feat(diff): derive a flat row model from a patch"
```

---

### Task 2: Side-by-side pairing

The only real algorithm in this plan. Pure.

**Files:**
- Create: `crates/ui/src/detail/diff/pairing.rs`
- Modify: `crates/ui/src/detail/diff/mod.rs` — add `mod pairing;`

**Interfaces:**
- Consumes: `domain::{DiffLine, LineOrigin}`.
- Produces: `pub(super) struct SplitRow { pub left: Option<SideLine>, pub right: Option<SideLine> }`, `pub(super) struct SideLine { pub number: Option<u32>, pub origin: LineOrigin, pub content: String }`, `pub(super) fn pair(lines: &[DiffLine]) -> Vec<SplitRow>`. Task 7 paints these.

- [ ] **Step 1: Write the failing tests**

Create `crates/ui/src/detail/diff/pairing.rs` with this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, content: &str) -> DiffLine {
        DiffLine { origin, old_number: old, new_number: new, content: content.to_string() }
    }

    fn deletion(number: u32, content: &str) -> DiffLine {
        line(LineOrigin::Deletion, Some(number), None, content)
    }

    fn addition(number: u32, content: &str) -> DiffLine {
        line(LineOrigin::Addition, None, Some(number), content)
    }

    fn context(number: u32, content: &str) -> DiffLine {
        line(LineOrigin::Context, Some(number), Some(number), content)
    }

    #[test]
    fn a_context_line_is_the_same_on_both_sides() {
        let rows = pair(&[context(1, "keep")]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left.as_ref().map(|s| s.content.as_str()), Some("keep"));
        assert_eq!(rows[0].right.as_ref().map(|s| s.content.as_str()), Some("keep"));
    }

    #[test]
    fn an_equal_length_replacement_pairs_line_for_line() {
        let rows = pair(&[deletion(1, "a"), deletion(2, "b"), addition(1, "x"), addition(2, "y")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().map(|s| s.content.as_str()), Some("a"));
        assert_eq!(rows[0].right.as_ref().map(|s| s.content.as_str()), Some("x"));
        assert_eq!(rows[1].left.as_ref().map(|s| s.content.as_str()), Some("b"));
        assert_eq!(rows[1].right.as_ref().map(|s| s.content.as_str()), Some("y"));
    }

    #[test]
    fn more_additions_than_deletions_pads_the_left() {
        let rows = pair(&[deletion(1, "a"), addition(1, "x"), addition(2, "y")]);
        assert_eq!(rows.len(), 2);
        assert!(rows[1].left.is_none(), "the extra addition has nothing to pair with");
        assert_eq!(rows[1].right.as_ref().map(|s| s.content.as_str()), Some("y"));
    }

    #[test]
    fn more_deletions_than_additions_pads_the_right() {
        let rows = pair(&[deletion(1, "a"), deletion(2, "b"), addition(1, "x")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].left.as_ref().map(|s| s.content.as_str()), Some("b"));
        assert!(rows[1].right.is_none());
    }

    #[test]
    fn a_pure_addition_leaves_the_left_side_empty() {
        let rows = pair(&[addition(1, "x"), addition(2, "y")]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.left.is_none()));
    }

    #[test]
    fn a_pure_deletion_leaves_the_right_side_empty() {
        let rows = pair(&[deletion(1, "a")]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].right.is_none());
    }

    #[test]
    fn a_run_is_flushed_when_a_context_line_ends_it() {
        let rows = pair(&[deletion(1, "a"), addition(1, "x"), context(2, "keep")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].left.as_ref().map(|s| s.content.as_str()), Some("keep"));
    }

    #[test]
    fn a_run_at_the_very_end_is_flushed_without_trailing_context() {
        let rows = pair(&[context(1, "keep"), deletion(2, "a")]);
        assert_eq!(rows.len(), 2, "the trailing run must not be dropped");
        assert_eq!(rows[1].left.as_ref().map(|s| s.content.as_str()), Some("a"));
    }

    #[test]
    fn an_empty_hunk_yields_no_rows() {
        assert!(pair(&[]).is_empty());
    }

    #[test]
    fn a_side_line_keeps_its_own_number() {
        let rows = pair(&[deletion(7, "a"), addition(9, "x")]);
        assert_eq!(rows[0].left.as_ref().and_then(|s| s.number), Some(7));
        assert_eq!(rows[0].right.as_ref().and_then(|s| s.number), Some(9));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gitr-ui --lib detail::diff::pairing`
Expected: FAIL to compile — `pair`, `SplitRow`, `SideLine` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/ui/src/detail/diff/pairing.rs`:

```rust
use domain::{DiffLine, LineOrigin};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SideLine {
    pub number: Option<u32>,
    pub origin: LineOrigin,
    pub content: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SplitRow {
    pub left: Option<SideLine>,
    pub right: Option<SideLine>,
}

pub(super) fn pair(lines: &[DiffLine]) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    let mut deletions: Vec<&DiffLine> = Vec::new();
    let mut additions: Vec<&DiffLine> = Vec::new();

    for line in lines {
        match line.origin {
            LineOrigin::Deletion => deletions.push(line),
            LineOrigin::Addition => additions.push(line),
            LineOrigin::Context => {
                flush(&mut rows, &mut deletions, &mut additions);
                rows.push(SplitRow {
                    left: Some(side(line, line.old_number)),
                    right: Some(side(line, line.new_number)),
                });
            }
        }
    }
    flush(&mut rows, &mut deletions, &mut additions);
    rows
}

fn flush(rows: &mut Vec<SplitRow>, deletions: &mut Vec<&DiffLine>, additions: &mut Vec<&DiffLine>) {
    let paired = deletions.len().max(additions.len());
    for index in 0..paired {
        rows.push(SplitRow {
            left: deletions.get(index).map(|line| side(line, line.old_number)),
            right: additions.get(index).map(|line| side(line, line.new_number)),
        });
    }
    deletions.clear();
    additions.clear();
}

fn side(line: &DiffLine, number: Option<u32>) -> SideLine {
    SideLine {
        number,
        origin: line.origin,
        content: line.content.clone(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gitr-ui --lib detail::diff::pairing`
Expected: PASS, 10 tests.

- [ ] **Step 5: Register the module**

Add `mod pairing;` to `crates/ui/src/detail/diff/mod.rs`. It is unused until Task 7, so add `#[allow(dead_code)]` above the `mod pairing;` line and delete that attribute in Task 7.

- [ ] **Step 6: Lint, format, commit**

```bash
cargo fmt --all
cargo clippy -p gitr-ui --all-targets -- -D warnings
git add crates/ui/src/detail/diff/
git commit -m "feat(diff): pair deletions with additions for a side-by-side view"
```

---

### Task 3: Row palette

Moves the four tint constants and their contrast reasoning out of `decorations.rs`, and adds the foreground colours the rows need. `decorations.rs` is still alive at the end of this task; Task 4 deletes it.

**Files:**
- Create: `crates/ui/src/detail/diff/palette.rs`
- Modify: `crates/ui/src/detail/diff/mod.rs` — add `mod palette;`

**Interfaces:**
- Consumes: `gpui_component::{ThemeColor, ThemeMode}`, `domain::LineOrigin`.
- Produces: `pub(super) struct LineColors { pub background: Option<Hsla>, pub foreground: Hsla }`, `pub(super) fn line_colors(origin: LineOrigin, mode: ThemeMode, theme: &ThemeColor) -> LineColors`.

- [ ] **Step 1: Write the failing tests**

Create `crates/ui/src/detail/diff/palette.rs` with this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_line_has_no_background() {
        let theme = ThemeColor::light();
        assert!(line_colors(LineOrigin::Context, ThemeMode::Light, &theme).background.is_none());
    }

    #[test]
    fn an_addition_and_a_deletion_do_not_share_a_background() {
        let theme = ThemeColor::light();
        let added = line_colors(LineOrigin::Addition, ThemeMode::Light, &theme).background;
        let deleted = line_colors(LineOrigin::Deletion, ThemeMode::Light, &theme).background;
        assert!(added.is_some() && deleted.is_some());
        assert_ne!(added, deleted);
    }

    #[test]
    fn dark_mode_does_not_reuse_the_light_pair() {
        let theme = ThemeColor::dark();
        let light = line_colors(LineOrigin::Addition, ThemeMode::Light, &theme).background;
        let dark = line_colors(LineOrigin::Addition, ThemeMode::Dark, &theme).background;
        assert_ne!(light, dark);
    }

    #[test]
    fn every_band_is_distinguishable_from_the_background_it_sits_on() {
        for (mode, theme) in [(ThemeMode::Light, ThemeColor::light()), (ThemeMode::Dark, ThemeColor::dark())] {
            for origin in [LineOrigin::Addition, LineOrigin::Deletion] {
                let band = line_colors(origin, mode, &theme).background.expect("a band");
                let distance = crate::theme_palette::rendered_distance(theme.background, band, theme.background);
                assert!(distance > 0.0, "{origin:?} in {mode:?} must not vanish into the background");
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gitr-ui --lib detail::diff::palette`
Expected: FAIL to compile — `line_colors` is not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/ui/src/detail/diff/palette.rs`. The four constants and their reasoning come verbatim from `crates/ui/src/detail/decorations.rs:17-35` — copy the doc comments on the two dark constants across unchanged, they record contrast measurements that must not be lost:

```rust
use domain::LineOrigin;
use gpui::{Hsla, rgb};
use gpui_component::{ThemeColor, ThemeMode};

const LIGHT_ADDITION_BACKGROUND: u32 = 0xdafbe1;
const LIGHT_DELETION_BACKGROUND: u32 = 0xffebe9;
const DARK_ADDITION_BACKGROUND: u32 = 0x355a40;
const DARK_DELETION_BACKGROUND: u32 = 0x5f3c45;

pub(super) struct LineColors {
    pub background: Option<Hsla>,
    pub foreground: Hsla,
}

pub(super) fn line_colors(origin: LineOrigin, mode: ThemeMode, theme: &ThemeColor) -> LineColors {
    let background = match (origin, mode.is_dark()) {
        (LineOrigin::Context, _) => None,
        (LineOrigin::Addition, false) => Some(rgb(LIGHT_ADDITION_BACKGROUND).into()),
        (LineOrigin::Addition, true) => Some(rgb(DARK_ADDITION_BACKGROUND).into()),
        (LineOrigin::Deletion, false) => Some(rgb(LIGHT_DELETION_BACKGROUND).into()),
        (LineOrigin::Deletion, true) => Some(rgb(DARK_DELETION_BACKGROUND).into()),
    };
    LineColors { background, foreground: theme.foreground }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gitr-ui --lib detail::diff::palette`
Expected: PASS, 4 tests.

If `rendered_distance` is not `pub(crate)`, widen it in `crates/ui/src/theme_palette.rs` to `pub(crate)` rather than duplicating it.

- [ ] **Step 5: Lint, format, commit**

```bash
cargo fmt --all
cargo clippy -p gitr-ui --all-targets -- -D warnings
git add crates/ui/src/detail/diff/ crates/ui/src/theme_palette.rs
git commit -m "feat(diff): give rows their own palette, carrying the contrast reasoning over"
```

---

### Task 4: The selectable diff body

The risk. One custom `Element` that paints every row and declares their text as runs of one selection participant. No windowing yet — Task 5 adds it — so this task must be verified on a small commit.

**Files:**
- Modify: `Cargo.toml` — add `gpui-base` to `[workspace.dependencies]`
- Modify: `crates/ui/Cargo.toml` — add `gpui-base.workspace = true`
- Create: `crates/ui/src/detail/diff/body.rs`
- Modify: `crates/ui/src/detail/diff/mod.rs` — render `body`, drop the `Editor`
- Modify: `crates/ui/src/detail/mod.rs` — drop `pending_diff`, `diff_editor`, `diff_decorations`
- Delete: `crates/ui/src/detail/decorations.rs`
- Modify: `crates/ui/src/detail/format.rs` — delete `DiffLineRanges`, `unified_diff_text_with_line_ranges`, `write_file`, and the path helpers that only served them, plus their tests

**Interfaces:**
- Consumes: `Row` and `rows` (Task 1), `LineColors` and `line_colors` (Task 3).
- Produces: `pub(super) struct DiffBody`, `pub(super) fn body(rows: Vec<Row>, selection: TextSelectionHandle, theme: ThemeColor, mode: ThemeMode) -> DiffBody`. Task 5 adds windowing inside it; Task 7 adds a split variant beside it.

- [ ] **Step 1: Add the dependency**

In `Cargo.toml`, after the `gpui-component-assets` line:

```toml
gpui-base = { git = "https://github.com/longbridge/gpui-component" }
```

No `rev` — `gpui-component` is declared the same way, and Cargo unifies two git sources only when the reference matches exactly. In `crates/ui/Cargo.toml`, under `[dependencies]`, after `gpui-component.workspace = true`:

```toml
gpui-base.workspace = true
```

- [ ] **Step 2: Verify the dependency unified rather than duplicated**

Run: `cargo tree -p gitr-ui -i gpui-base 2>&1 | head -20`
Expected: exactly one `gpui-base v0.5.2` at source `#7acfc18…`. If two appear, stop — the reference does not match and every trait from one copy will fail to apply to the other.

- [ ] **Step 3: Write the element**

Create `crates/ui/src/detail/diff/body.rs`. This is adapted from the reference at
`~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/7acfc18/crates/base/examples/showcase/components/text_selection.rs`,
whose `PlainSelectableText` is the only worked example of a custom element joining the selection system. Read it before writing this. The two departures from it are that the participant declares many runs rather than one, and that each run is one row's code text.

```rust
use std::ops::Range;

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    IntoElement, LayoutId, Pixels, Point, SharedString, StyledText, Window, fill, point, px, size,
};
use gpui_base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use gpui_component::{ThemeColor, ThemeMode};

use super::model::Row;
use super::palette::line_colors;

const GUTTER_WIDTH: f32 = 44.;
const MARKER_WIDTH: f32 = 16.;
const ROW_HEIGHT: f32 = 18.;

pub(super) struct DiffBody {
    rows: Vec<Row>,
    selection: TextSelectionHandle,
    theme: ThemeColor,
    mode: ThemeMode,
    texts: Vec<StyledText>,
}

pub(super) fn body(
    rows: Vec<Row>,
    selection: TextSelectionHandle,
    theme: ThemeColor,
    mode: ThemeMode,
) -> DiffBody {
    let texts = rows.iter().map(|row| StyledText::new(row_text(row))).collect();
    DiffBody { rows, selection, theme, mode, texts }
}

fn row_text(row: &Row) -> SharedString {
    match row {
        Row::FileHeader { path, stat } => format!("{path}  {stat}").into(),
        Row::HunkHeader { text } => text.clone().into(),
        Row::Line { content, .. } => content.clone().into(),
        Row::Placeholder { message } => (*message).into(),
    }
}
```

The `Element` impl follows the reference's shape exactly — `type RequestLayoutState = ();`, `type PrepaintState = Hitbox;`, `id` and `source_location` returning `None`. In `prepaint`, lay out each `StyledText` at its row's bounds, insert one hitbox over the whole body, and register one participant:

```rust
self.selection.register(
    TextSelectionRegistration::new(hitbox.clone(), bounds)
        .with_document_order(0)
        .with_text_bounds(row_bounds.clone()),
    window,
    cx,
);
```

In `paint`, build one run per row, in document order, and project:

```rust
let runs: Vec<TextSelectionRun> = self
    .texts
    .iter()
    .enumerate()
    .map(|(index, text)| {
        TextSelectionRun::new(row_text(&self.rows[index]), text.layout().clone(), row_bounds[index])
            .with_document_order(index as u64)
    })
    .collect();
let projection = self.selection.update_runs(&runs, cx);
```

Then, per row: paint the tint quad across the full `bounds.size.width`, paint the two gutter numbers and the marker, paint the selection quads for `projection.ranges()[index]` if `Some`, and finally paint the row's `StyledText`.

Only the code text becomes a run. The gutters and the marker are painted directly and are never registered, which is what keeps them out of the clipboard.

**`TextLayout::bounds`, `line_height`, `len`, `position_for_index` and `index_for_position` panic if called before layout.** They are safe in `paint` and nowhere earlier.

Copy `selection_quad_bounds` verbatim from the reference — it is a ready-made three-rectangle start/middle/end helper.

- [ ] **Step 4: Render it, and tear out the editor**

In `crates/ui/src/detail/diff/mod.rs`, replace the `Editor::new(...)` body of `render` with the new element, keeping the empty-patch early return unchanged. In `crates/ui/src/detail/mod.rs`, delete the `pending_diff`, `diff_editor` and `diff_decorations` fields, the flush block at the top of `Render::render`, and the `EditorState`/`TextDecorationCollection` imports. `DetailPanel` gains one `TextSelectionHandle`, built in `new`:

```rust
let selection = TextSelectionHandle::new("", cx);
selection.refresh_window_on_change(window, cx).detach();
```

Without that subscription the selection changes but nothing repaints.

`set_detail` now derives rows directly — it needs no window, which is the whole reason `pending_diff` existed:

```rust
pub fn set_detail(&mut self, detail: LoadState<Arc<CommitDetail>>, cx: &mut Context<Self>) {
    self.detail = detail;
    cx.notify();
}
```

Delete `crates/ui/src/detail/decorations.rs` and its `mod decorations;` line. In `format.rs`, delete `DiffLineRanges`, `unified_diff_text_with_line_ranges`, `write_file`, `git_path`, `git_header_path`, `side_path`, and every test naming them. Keep `abbreviate`, `format_timestamp`, `escape_markdown` and `hunk_heading`. Update the module doc on `detail/mod.rs`, which describes the editor and the staging that no longer exist.

- [ ] **Step 5: Stop the code from wrapping**

The spec calls for horizontal scrolling rather than soft wrap, and nothing so far enforces
it: `StyledText` wraps to the bounds it is given. Lay each row's text out against a width
wide enough that it never wraps — the widest row's measured width, not the viewport's — and
put the body inside a horizontally scrollable parent in `diff/mod.rs`:

```rust
div()
    .id("detail-diff-scroll")
    .size_full()
    .overflow_x_scroll()
    .restrict_scroll_to_axis()
    .child(body(...))
```

`restrict_scroll_to_axis` is not optional. It defaults to `false`, and with it unset a
scrollable-x element treats a vertical delta as horizontal whenever its own y overflow is
not `Scroll` — a vertical gesture over the diff would scroll it sideways and never scroll it
down.

If a row wraps anyway, the fixed `ROW_HEIGHT` Task 5 relies on is wrong and the arithmetic
windowing breaks. Confirm no row wraps before moving on.

- [ ] **Step 6: Build and lint**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
Expected: green. The tests deleted from `format.rs` are gone; nothing else should fail.

- [ ] **Step 7: Verify in the running app — this task is not done without it**

gitr is single-instance, so a running installed binary will swallow the launch and you will test the old build. Check first:

```bash
ps -eo pid,command | grep "[g]itr" | grep -v cargo
```

If an instance is running, quit it, then:

```bash
cargo run -p gitr_gui -- .
```

Select a commit, open the Diffs tab, and confirm all five: the code starts at the same column on every line; added and deleted lines carry a band that reaches the full width, including blank ones; both gutters show the file's own line numbers, not a running document count; a drag selects across row boundaries; and Cmd-C yields code with no line numbers and no `+`/`-`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(diff): render the diff as rows with selectable code"
```

---

### Task 5: Windowing

Task 4 builds a `StyledText` per row for the whole patch. On a large commit that is thousands of laid-out lines per frame. This task limits the work to the rows on screen.

**Files:**
- Modify: `crates/ui/src/detail/diff/body.rs`

**Interfaces:**
- Consumes: `DiffBody` from Task 4.
- Produces: no new public names. `body()` keeps its signature.

- [ ] **Step 1: Establish the baseline**

Run the app on a repository with a large commit and note that scrolling is slow. `zed-industries/zed` has commits touching hundreds of files. Record what you saw — this is the before.

- [ ] **Step 2: Compute the visible range in `prepaint`**

Row height is fixed at `ROW_HEIGHT`, so the range is arithmetic, not measurement:

```rust
let first = (scroll_offset.y / px(ROW_HEIGHT)).floor().max(0.) as usize;
let visible = (bounds.size.height / px(ROW_HEIGHT)).ceil() as usize + 1;
let range = first..(first + visible).min(self.rows.len());
```

Lay out `StyledText` only for `range`, and store `range` on the element so `paint` uses the same one.

- [ ] **Step 3: Keep document order absolute**

Runs must carry their index in the whole patch, not in the visible window, or a selection dragged past the edge reorders on copy:

```rust
.with_document_order(range.start as u64 + offset_in_window as u64)
```

- [ ] **Step 4: Report the scroll offset to the selection layer**

```rust
TextSelectionRegistration::new(hitbox.clone(), bounds)
    .with_scroll_offset(scroll_offset)
```

Without it, hit-testing maps window points into the wrong content position as soon as the body is scrolled.

- [ ] **Step 5: Verify**

Run the app again on the same large commit. Scrolling should be smooth. Then re-check the two selection behaviours from Task 4 Step 7 **while scrolled down**, and drag a selection from a visible row past the bottom edge to confirm document order survives.

- [ ] **Step 6: Lint, format, commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add crates/ui/src/detail/diff/body.rs
git commit -m "feat(diff): lay out only the rows on screen"
```

---

### Task 6: The view-mode preference

Pure and testable, and independent of Tasks 4 and 5 — it can be built in parallel with them.

**Files:**
- Create: `crates/ui/src/diff_view_mode.rs`
- Modify: `crates/ui/src/lib.rs` — add `pub mod diff_view_mode;`
- Modify: `crates/ui/src/persistence.rs`

**Interfaces:**
- Produces: `pub enum DiffViewMode { Unified, Split }` with `ALL`, `index`, `from_index`, `label`; `persistence::{save_diff_view_mode, load_diff_view_mode, save_diff_view_mode_to, load_diff_view_mode_from}`.

- [ ] **Step 1: Read the pattern to copy**

Run: `cat crates/ui/src/theme_preference.rs` and `grep -n "theme_preference" crates/ui/src/persistence.rs`. `DiffViewMode` mirrors `ThemePreference` — a small serde enum with a `Default`, and a `_to`/`_from` pair beside the `save`/`load` pair so the disk-free half stays testable.

- [ ] **Step 2: Write the failing tests**

In `crates/ui/src/diff_view_mode.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_unified() {
        assert_eq!(DiffViewMode::default(), DiffViewMode::Unified);
    }

    #[test]
    fn every_mode_round_trips_through_its_index() {
        for mode in DiffViewMode::ALL {
            assert_eq!(DiffViewMode::from_index(mode.index()), mode);
        }
    }

    #[test]
    fn an_out_of_range_index_falls_back_to_the_default() {
        assert_eq!(DiffViewMode::from_index(99), DiffViewMode::default());
    }
}
```

And in `crates/ui/src/persistence.rs`'s test module:

```rust
#[test]
fn a_diff_view_mode_round_trips_through_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("diff-view-preference.json");
    save_diff_view_mode_to(&path, &DiffViewMode::Split).expect("save");
    assert_eq!(load_diff_view_mode_from(&path).expect("load"), DiffViewMode::Split);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gitr-ui --lib diff_view_mode persistence`
Expected: FAIL to compile.

- [ ] **Step 4: Write the implementation**

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffViewMode {
    #[default]
    Unified,
    Split,
}

impl DiffViewMode {
    pub const ALL: [DiffViewMode; 2] = [Self::Unified, Self::Split];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unified => "Unified",
            Self::Split => "Split",
        }
    }
}
```

In `persistence.rs`, add `const DIFF_VIEW_MODE_FILE: &str = "diff-view-preference.json";` beside the other file constants, and copy the four theme-preference functions, substituting the type and the constant.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gitr-ui --lib diff_view_mode persistence`
Expected: PASS.

- [ ] **Step 6: Lint, format, commit**

```bash
cargo fmt --all
cargo clippy -p gitr-ui --all-targets -- -D warnings
git add crates/ui/src/diff_view_mode.rs crates/ui/src/lib.rs crates/ui/src/persistence.rs
git commit -m "feat(diff): persist the chosen diff view mode"
```

---

### Task 7: Side-by-side view and the toggle

**Files:**
- Create: `crates/ui/src/detail/diff/split.rs`
- Modify: `crates/ui/src/detail/diff/body.rs`, `crates/ui/src/detail/diff/mod.rs`, `crates/ui/src/detail/mod.rs`
- Modify: `crates/ui/src/detail/diff/pairing.rs` — remove the `#[allow(dead_code)]` added in Task 2

**Interfaces:**
- Consumes: `pair`, `SplitRow`, `SideLine` (Task 2); `DiffViewMode` (Task 6); `DiffBody` (Tasks 4-5).
- Produces: `pub(super) fn split_rows(patch: &Patch) -> Vec<SplitRow>`.

- [ ] **Step 1: Write the failing test for split row derivation**

In `crates/ui/src/detail/diff/split.rs`, a test asserting that a two-file patch yields the rows of both files in order, with each file's header row present. Follow Task 1's fixtures.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p gitr-ui --lib detail::diff::split`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `split_rows`**

Walk `patch.files`, emit the same `Row::FileHeader` and `Row::Placeholder` cases as Task 1, and for each hunk call `pairing::pair(&hunk.lines)`.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p gitr-ui --lib detail::diff::split`

- [ ] **Step 5: Paint two columns**

Extend `DiffBody` to take an enum of either `Vec<Row>` or `Vec<SplitRow>`. In the split case each row paints gutter, marker and code twice, at `bounds.size.width / 2.` — and declares **two** runs, left then right, so document order runs left-to-right within a row.

- [ ] **Step 6: Add the toggle**

In `crates/ui/src/detail/mod.rs`, add a second `TabBar::new("diff-view-mode").segmented().small()` beside the existing `detail-tabs` bar, rendered only when `selected_tab == DetailTab::Diff`. Its `on_click` sets the mode, calls `persistence::save_diff_view_mode` on a background executor, and `cx.notify()`. Load the saved mode in `DetailPanel::new`.

- [ ] **Step 7: Verify in the running app**

Confirm: the toggle switches views and survives a restart; a pure addition shows an empty left column rather than collapsing to one; a drag across the split view copies left-then-right within a row and top-to-bottom across rows.

- [ ] **Step 8: Full check and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git add -A
git commit -m "feat(diff): add a side-by-side view behind a persisted toggle"
```

---

## Notes for whoever executes this

Tasks 1, 2, 3 and 6 are pure and carry real tests. Tasks 4, 5 and 7 touch gpui and are verified by running the app — this crate has no element-tree tests and this plan does not add the machinery for them.

Task 4 is where this plan is most likely to be wrong. Its code is adapted from a reference example, not from something compiled while writing this. Read
`crates/base/examples/showcase/components/text_selection.rs` in the gpui-component checkout in full before starting it, and treat the snippets here as the shape rather than the letter. If the participant/run model turns out not to work as described, stop and re-open the spec's Virtualisation section rather than working around it in the element.
