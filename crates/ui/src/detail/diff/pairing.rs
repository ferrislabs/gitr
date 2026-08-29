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

#[cfg(test)]
mod tests {
    use super::*;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, content: &str) -> DiffLine {
        DiffLine {
            origin,
            old_number: old,
            new_number: new,
            content: content.to_string(),
        }
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
        assert_eq!(
            rows[0].left.as_ref().map(|s| s.content.as_str()),
            Some("keep")
        );
        assert_eq!(
            rows[0].right.as_ref().map(|s| s.content.as_str()),
            Some("keep")
        );
    }

    #[test]
    fn an_equal_length_replacement_pairs_line_for_line() {
        let rows = pair(&[
            deletion(1, "a"),
            deletion(2, "b"),
            addition(1, "x"),
            addition(2, "y"),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().map(|s| s.content.as_str()), Some("a"));
        assert_eq!(
            rows[0].right.as_ref().map(|s| s.content.as_str()),
            Some("x")
        );
        assert_eq!(rows[1].left.as_ref().map(|s| s.content.as_str()), Some("b"));
        assert_eq!(
            rows[1].right.as_ref().map(|s| s.content.as_str()),
            Some("y")
        );
    }

    #[test]
    fn more_additions_than_deletions_pads_the_left() {
        let rows = pair(&[deletion(1, "a"), addition(1, "x"), addition(2, "y")]);
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1].left.is_none(),
            "the extra addition has nothing to pair with"
        );
        assert_eq!(
            rows[1].right.as_ref().map(|s| s.content.as_str()),
            Some("y")
        );
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
        assert_eq!(
            rows[1].left.as_ref().map(|s| s.content.as_str()),
            Some("keep")
        );
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
