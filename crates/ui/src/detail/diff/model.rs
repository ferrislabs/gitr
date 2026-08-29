use std::fmt::Write as _;

use domain::{DiffLine, FilePatch, FileStatus, Hunk, LineOrigin, Patch};

use crate::detail::format;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    FileHeader {
        path: String,
        status: FileStatus,
        added: usize,
        deleted: usize,
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
        rows.push(file_header(file));
        push_body(&mut rows, file);
    }
    rows
}

pub(super) fn file_header(file: &FilePatch) -> Row {
    Row::FileHeader {
        path: header_path(file),
        status: file.status.clone(),
        added: file.added_lines(),
        deleted: file.deleted_lines(),
    }
}

pub(super) fn header_line(path: &str, status: &FileStatus, added: usize, deleted: usize) -> String {
    let mut text = path.to_string();
    if let Some(label) = status_label(status) {
        let _ = write!(text, "  {label}");
    }
    let _ = write!(text, "  +{added} \u{2212}{deleted}");
    text
}

fn header_path(file: &FilePatch) -> String {
    let moved = matches!(
        file.status,
        FileStatus::Renamed { .. } | FileStatus::Copied { .. }
    );
    match (&file.old_path, &file.new_path) {
        (Some(old), Some(new)) if moved && old != new => {
            format!("{} \u{2192} {}", old.display(), new.display())
        }
        _ => file
            .display_path()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    }
}

fn status_label(status: &FileStatus) -> Option<String> {
    match status {
        FileStatus::Modified => None,
        FileStatus::Added => Some("added".to_string()),
        FileStatus::Deleted => Some("deleted".to_string()),
        FileStatus::Renamed { similarity } => Some(format!("renamed {similarity}%")),
        FileStatus::Copied { similarity } => Some(format!("copied {similarity}%")),
        FileStatus::TypeChanged => Some("type changed".to_string()),
    }
}

pub(super) fn placeholder(file: &FilePatch) -> Option<Row> {
    if file.is_binary {
        return Some(Row::Placeholder {
            message: "Binary file not shown.",
        });
    }
    if file.hunks.is_empty() {
        return Some(Row::Placeholder {
            message: "No content changes.",
        });
    }
    None
}

pub(super) fn hunk_header(hunk: &Hunk) -> Row {
    Row::HunkHeader {
        text: format::hunk_heading(hunk),
    }
}

fn push_body(rows: &mut Vec<Row>, file: &FilePatch) {
    if let Some(placeholder) = placeholder(file) {
        rows.push(placeholder);
        return;
    }
    for hunk in &file.hunks {
        rows.push(hunk_header(hunk));
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

    fn moved(old: &str, new: &str, status: FileStatus) -> FilePatch {
        FilePatch {
            old_path: Some(PathBuf::from(old)),
            new_path: Some(PathBuf::from(new)),
            status,
            is_binary: false,
            hunks: Vec::new(),
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
    fn a_rename_keeps_both_paths_and_its_similarity() {
        let file = moved(
            "src/old.rs",
            "src/new.rs",
            FileStatus::Renamed { similarity: 87 },
        );
        let Row::FileHeader {
            path,
            status,
            added,
            deleted,
        } = file_header(&file)
        else {
            panic!("a file yields a header row");
        };

        assert_eq!(path, "src/old.rs \u{2192} src/new.rs");
        assert_eq!(status, FileStatus::Renamed { similarity: 87 });
        assert_eq!(
            header_line(&path, &status, added, deleted),
            "src/old.rs \u{2192} src/new.rs  renamed 87%  +0 \u{2212}0"
        );
    }

    #[test]
    fn a_copy_reads_as_a_copy_rather_than_as_a_rename() {
        let file = moved(
            "src/old.rs",
            "src/copy.rs",
            FileStatus::Copied { similarity: 100 },
        );
        let Row::FileHeader {
            path,
            status,
            added,
            deleted,
        } = file_header(&file)
        else {
            panic!("a file yields a header row");
        };

        assert_eq!(path, "src/old.rs \u{2192} src/copy.rs");
        assert_eq!(
            header_line(&path, &status, added, deleted),
            "src/old.rs \u{2192} src/copy.rs  copied 100%  +0 \u{2212}0"
        );
    }

    #[test]
    fn a_rename_that_only_changed_the_content_shows_one_path() {
        let file = moved(
            "src/a.rs",
            "src/a.rs",
            FileStatus::Renamed { similarity: 100 },
        );
        let Row::FileHeader { path, .. } = file_header(&file) else {
            panic!("a file yields a header row");
        };
        assert_eq!(path, "src/a.rs");
    }

    #[test]
    fn a_modified_file_carries_no_status_label() {
        let patch = Patch {
            files: vec![file(
                vec![hunk(vec![line(LineOrigin::Addition, None, Some(1), "new")])],
                false,
            )],
        };
        let Row::FileHeader {
            path,
            status,
            added,
            deleted,
        } = rows(&patch).remove(0)
        else {
            panic!("the first row is the file header");
        };

        assert_eq!(
            header_line(&path, &status, added, deleted),
            "src/main.rs  +1 \u{2212}0"
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
