use domain::Patch;

use super::model::{Row, file_header, hunk_header, placeholder};
use super::pairing::{SideLine, pair};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SplitRow {
    Full(Row),
    Sides {
        left: Option<SideLine>,
        right: Option<SideLine>,
    },
}

pub(super) fn split_rows(patch: &Patch) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    for file in &patch.files {
        rows.push(SplitRow::Full(file_header(file)));
        if let Some(placeholder) = placeholder(file) {
            rows.push(SplitRow::Full(placeholder));
            continue;
        }
        for hunk in &file.hunks {
            rows.push(SplitRow::Full(hunk_header(hunk)));
            rows.extend(pair(&hunk.lines).into_iter().map(|row| SplitRow::Sides {
                left: row.left,
                right: row.right,
            }));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{DiffLine, FilePatch, FileStatus, Hunk, LineOrigin};
    use std::path::PathBuf;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, content: &str) -> DiffLine {
        DiffLine {
            origin,
            old_number: old,
            new_number: new,
            content: content.to_string(),
        }
    }

    fn hunk(lines: Vec<DiffLine>) -> Hunk {
        Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            heading: String::new(),
            lines,
        }
    }

    fn file(path: &str, hunks: Vec<Hunk>, is_binary: bool) -> FilePatch {
        FilePatch {
            old_path: Some(PathBuf::from(path)),
            new_path: Some(PathBuf::from(path)),
            status: FileStatus::Modified,
            is_binary,
            hunks,
        }
    }

    fn replacement() -> Hunk {
        hunk(vec![
            line(LineOrigin::Deletion, Some(1), None, "gone"),
            line(LineOrigin::Addition, None, Some(1), "new"),
        ])
    }

    fn paths(rows: &[SplitRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                SplitRow::Full(Row::FileHeader { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_two_file_patch_yields_both_files_in_order_each_behind_its_own_header() {
        let patch = Patch {
            files: vec![
                file("src/a.rs", vec![replacement()], false),
                file("src/b.rs", vec![replacement()], false),
            ],
        };

        let rows = split_rows(&patch);

        assert_eq!(paths(&rows), vec!["src/a.rs", "src/b.rs"]);
        assert!(matches!(rows[1], SplitRow::Full(Row::HunkHeader { .. })));
        assert!(matches!(rows[2], SplitRow::Sides { .. }));
        assert_eq!(rows.len(), 6);
    }

    #[test]
    fn a_replacement_pairs_the_deletion_against_the_addition_on_one_row() {
        let patch = Patch {
            files: vec![file("src/a.rs", vec![replacement()], false)],
        };

        let rows = split_rows(&patch);

        let SplitRow::Sides { left, right } = &rows[2] else {
            panic!("the third row is the paired line");
        };
        assert_eq!(
            left.as_ref().map(|side| side.content.as_str()),
            Some("gone")
        );
        assert_eq!(
            right.as_ref().map(|side| side.content.as_str()),
            Some("new")
        );
    }

    #[test]
    fn a_pure_addition_leaves_the_left_column_empty_rather_than_collapsing_the_row() {
        let patch = Patch {
            files: vec![file(
                "src/a.rs",
                vec![hunk(vec![line(LineOrigin::Addition, None, Some(1), "new")])],
                false,
            )],
        };

        let rows = split_rows(&patch);

        let SplitRow::Sides { left, right } = &rows[2] else {
            panic!("the third row is the added line");
        };
        assert!(left.is_none());
        assert!(right.is_some());
    }

    #[test]
    fn a_pure_deletion_leaves_the_right_column_empty_rather_than_collapsing_the_row() {
        let patch = Patch {
            files: vec![file(
                "src/a.rs",
                vec![hunk(vec![line(
                    LineOrigin::Deletion,
                    Some(1),
                    None,
                    "gone",
                )])],
                false,
            )],
        };

        let rows = split_rows(&patch);

        let SplitRow::Sides { left, right } = &rows[2] else {
            panic!("the third row is the deleted line");
        };
        assert!(left.is_some());
        assert!(right.is_none());
    }

    #[test]
    fn a_binary_file_yields_a_full_width_placeholder_instead_of_columns() {
        let patch = Patch {
            files: vec![file("src/a.png", Vec::new(), true)],
        };

        let rows = split_rows(&patch);

        assert_eq!(
            rows[1],
            SplitRow::Full(Row::Placeholder {
                message: "Binary file not shown."
            })
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn a_file_with_no_hunks_yields_a_no_change_placeholder() {
        let patch = Patch {
            files: vec![file("src/a.rs", Vec::new(), false)],
        };

        let rows = split_rows(&patch);

        assert_eq!(
            rows[1],
            SplitRow::Full(Row::Placeholder {
                message: "No content changes."
            })
        );
    }

    #[test]
    fn every_hunk_of_a_file_keeps_its_own_header() {
        let patch = Patch {
            files: vec![file("src/a.rs", vec![replacement(), replacement()], false)],
        };

        let headers = split_rows(&patch)
            .iter()
            .filter(|row| matches!(row, SplitRow::Full(Row::HunkHeader { .. })))
            .count();

        assert_eq!(headers, 2);
    }
}
