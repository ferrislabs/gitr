use domain::Patch;

use super::Collapsed;
use super::model::{Row, file_header, placeholder};
use super::pairing::{SideLine, pair};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SplitRow {
    Full(Row),
    Sides {
        left: Option<SideLine>,
        right: Option<SideLine>,
    },
}

pub(super) fn split_rows(patch: &Patch, collapsed: &Collapsed) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    for (file, patch) in patch.files.iter().enumerate() {
        rows.push(SplitRow::Full(file_header(
            file,
            patch,
            collapsed.contains(&file),
        )));
        if collapsed.contains(&file) {
            continue;
        }
        if let Some(placeholder) = placeholder(patch) {
            rows.push(SplitRow::Full(placeholder));
            continue;
        }
        for (position, hunk) in patch.hunks.iter().enumerate() {
            if position > 0 {
                rows.push(SplitRow::Full(Row::Separator));
            }
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

    fn moved(old: &str, new: &str, status: FileStatus) -> FilePatch {
        FilePatch {
            old_path: Some(PathBuf::from(old)),
            new_path: Some(PathBuf::from(new)),
            status,
            is_binary: false,
            hunks: Vec::new(),
        }
    }

    fn paths(rows: &[SplitRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                SplitRow::Full(Row::FileHeader { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    fn expanded() -> Collapsed {
        Collapsed::new()
    }

    fn collapsed(files: impl IntoIterator<Item = usize>) -> Collapsed {
        files.into_iter().collect()
    }

    #[test]
    fn a_two_file_patch_yields_both_files_in_order_each_behind_its_own_header() {
        let patch = Patch {
            files: vec![
                file("src/a.rs", vec![replacement()], false),
                file("src/b.rs", vec![replacement()], false),
            ],
        };

        let rows = split_rows(&patch, &expanded());

        assert_eq!(paths(&rows), vec!["src/a.rs", "src/b.rs"]);
        assert!(matches!(rows[1], SplitRow::Sides { .. }));
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn a_replacement_pairs_the_deletion_against_the_addition_on_one_row() {
        let patch = Patch {
            files: vec![file("src/a.rs", vec![replacement()], false)],
        };

        let rows = split_rows(&patch, &expanded());

        let SplitRow::Sides { left, right } = &rows[1] else {
            panic!("the second row is the paired line");
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

        let rows = split_rows(&patch, &expanded());

        let SplitRow::Sides { left, right } = &rows[1] else {
            panic!("the second row is the added line");
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

        let rows = split_rows(&patch, &expanded());

        let SplitRow::Sides { left, right } = &rows[1] else {
            panic!("the second row is the deleted line");
        };
        assert!(left.is_some());
        assert!(right.is_none());
    }

    #[test]
    fn a_binary_file_yields_a_full_width_placeholder_instead_of_columns() {
        let patch = Patch {
            files: vec![file("src/a.png", Vec::new(), true)],
        };

        let rows = split_rows(&patch, &expanded());

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

        let rows = split_rows(&patch, &expanded());

        assert_eq!(
            rows[1],
            SplitRow::Full(Row::Placeholder {
                message: "No content changes."
            })
        );
    }

    #[test]
    fn a_rename_with_no_content_change_still_names_both_paths() {
        let patch = Patch {
            files: vec![moved(
                "src/old.rs",
                "src/new.rs",
                FileStatus::Renamed { similarity: 87 },
            )],
        };

        let rows = split_rows(&patch, &expanded());

        assert_eq!(paths(&rows), vec!["src/old.rs \u{2192} src/new.rs"]);
        assert_eq!(
            rows[0],
            SplitRow::Full(Row::FileHeader {
                file: 0,
                path: "src/old.rs \u{2192} src/new.rs".to_string(),
                status: FileStatus::Renamed { similarity: 87 },
                added: 0,
                deleted: 0,
                collapsed: false,
            })
        );
        assert_eq!(
            rows[1],
            SplitRow::Full(Row::Placeholder {
                message: "No content changes."
            })
        );
    }

    #[test]
    fn a_copy_carries_its_own_status_into_the_header_row() {
        let patch = Patch {
            files: vec![moved(
                "src/old.rs",
                "src/copy.rs",
                FileStatus::Copied { similarity: 100 },
            )],
        };

        let rows = split_rows(&patch, &expanded());

        assert_eq!(
            rows[0],
            SplitRow::Full(Row::FileHeader {
                file: 0,
                path: "src/old.rs \u{2192} src/copy.rs".to_string(),
                status: FileStatus::Copied { similarity: 100 },
                added: 0,
                deleted: 0,
                collapsed: false,
            })
        );
    }

    #[test]
    fn two_hunks_of_a_file_are_parted_by_a_single_separator() {
        let patch = Patch {
            files: vec![file("src/a.rs", vec![replacement(), replacement()], false)],
        };

        let rows = split_rows(&patch, &expanded());

        assert!(matches!(rows[0], SplitRow::Full(Row::FileHeader { .. })));
        assert!(matches!(rows[1], SplitRow::Sides { .. }));
        assert_eq!(rows[2], SplitRow::Full(Row::Separator));
        assert!(matches!(rows[3], SplitRow::Sides { .. }));
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn a_collapsed_file_emits_its_header_and_nothing_else() {
        let patch = Patch {
            files: vec![file("src/a.rs", vec![replacement(), replacement()], false)],
        };

        let rows = split_rows(&patch, &collapsed([0]));

        assert_eq!(rows.len(), 1);
        assert!(matches!(
            rows[0],
            SplitRow::Full(Row::FileHeader {
                collapsed: true,
                ..
            })
        ));
        assert!(!rows.contains(&SplitRow::Full(Row::Separator)));
    }

    #[test]
    fn collapsing_one_file_leaves_the_others_whole() {
        let patch = Patch {
            files: vec![
                file("src/a.rs", vec![replacement()], false),
                file("src/b.rs", vec![replacement()], false),
                file("src/c.rs", vec![replacement()], false),
            ],
        };

        let rows = split_rows(&patch, &collapsed([1]));

        assert_eq!(paths(&rows), vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
        assert_eq!(rows.len(), 5);
        assert!(matches!(rows[1], SplitRow::Sides { .. }));
        assert!(matches!(
            rows[2],
            SplitRow::Full(Row::FileHeader {
                file: 1,
                collapsed: true,
                ..
            })
        ));
        assert!(matches!(rows[4], SplitRow::Sides { .. }));
    }

    #[test]
    fn a_collapsed_binary_file_drops_its_placeholder_too() {
        let patch = Patch {
            files: vec![file("src/a.png", Vec::new(), true)],
        };
        assert_eq!(split_rows(&patch, &collapsed([0])).len(), 1);
    }
}
