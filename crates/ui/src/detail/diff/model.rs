use domain::{DiffLine, FilePatch, LineOrigin, Patch};

use crate::detail::format;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    FileHeader {
        path: String,
        stat: String,
    },
    HunkHeader {
        text: String,
    },
    Line {
        origin: LineOrigin,
        old_number: Option<u32>,
        new_number: Option<u32>,
        content: String,
    },
    Placeholder {
        message: &'static str,
    },
}

pub(super) fn rows(patch: &Patch) -> Vec<Row> {
    let mut rows = Vec::new();
    for file in &patch.files {
        rows.push(Row::FileHeader {
            path: file
                .display_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
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
        rows.push(Row::Placeholder {
            message: "Binary file not shown.",
        });
        return;
    }
    if file.hunks.is_empty() {
        rows.push(Row::Placeholder {
            message: "No content changes.",
        });
        return;
    }
    for hunk in &file.hunks {
        rows.push(Row::HunkHeader {
            text: format::hunk_heading(hunk),
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{FileStatus, Hunk};
    use std::path::PathBuf;

    fn line(origin: LineOrigin, old: Option<u32>, new: Option<u32>, content: &str) -> DiffLine {
        DiffLine {
            origin,
            old_number: old,
            new_number: new,
            content: content.to_string(),
        }
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
        Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            heading: String::new(),
            lines,
        }
    }

    #[test]
    fn a_modified_file_yields_a_header_a_hunk_header_and_one_row_per_line() {
        let patch = Patch {
            files: vec![file(
                vec![hunk(vec![
                    line(LineOrigin::Context, Some(1), Some(1), "keep"),
                    line(LineOrigin::Deletion, Some(2), None, "gone"),
                    line(LineOrigin::Addition, None, Some(2), "new"),
                ])],
                false,
            )],
        };

        let rows = rows(&patch);

        assert!(matches!(rows[0], Row::FileHeader { .. }));
        assert!(matches!(rows[1], Row::HunkHeader { .. }));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn a_line_row_carries_both_numbers_and_the_bare_content() {
        let patch = Patch {
            files: vec![file(
                vec![hunk(vec![line(
                    LineOrigin::Deletion,
                    Some(7),
                    None,
                    "-not a marker",
                )])],
                false,
            )],
        };

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
        let patch = Patch {
            files: vec![file(Vec::new(), true)],
        };
        let rows = rows(&patch);
        assert_eq!(
            rows[1],
            Row::Placeholder {
                message: "Binary file not shown."
            }
        );
    }

    #[test]
    fn a_file_with_no_hunks_yields_a_no_change_placeholder() {
        let patch = Patch {
            files: vec![file(Vec::new(), false)],
        };
        let rows = rows(&patch);
        assert_eq!(
            rows[1],
            Row::Placeholder {
                message: "No content changes."
            }
        );
    }

    #[test]
    fn every_file_contributes_its_own_header() {
        let patch = Patch {
            files: vec![file(Vec::new(), true), file(Vec::new(), true)],
        };
        let headers = rows(&patch)
            .iter()
            .filter(|r| matches!(r, Row::FileHeader { .. }))
            .count();
        assert_eq!(headers, 2);
    }
}
