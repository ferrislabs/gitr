//! Saving and restoring window preferences across launches: the dock layout, the theme
//! preference, and the project list.
//!
//! [`DockAreaState`], [`ThemePreference`] and [`ProjectList`] are all `Serialize`/
//! `Deserialize` and carry no gpui entities, so the round trip through this module is
//! ordinary JSON on disk — no different in kind from any other config file. Dock layout
//! saves are debounced by [`crate::workspace::Workspace`], since `DockEvent::LayoutChanged`
//! fires on every drag frame while a panel is being resized; a theme preference change or a
//! project being added or switched is a rare, deliberate action, so each saves immediately.
//!
//! A failure here is a logged inconvenience, not a crash: nothing in this module panics,
//! and every fallible function returns a `Result` for the caller to log and move past.

use std::path::{Path, PathBuf};

use gpui_component::dock::DockAreaState;

use crate::diff_view_mode::DiffViewMode;
use crate::project::ProjectList;
use crate::theme_preference::ThemePreference;

const APPLICATION_SUPPORT_DIR: &str = "Library/Application Support/gitr";
const DOCK_LAYOUT_FILE: &str = "dock-layout.json";
const THEME_PREFERENCE_FILE: &str = "theme-preference.json";
const DIFF_VIEW_MODE_FILE: &str = "diff-view-preference.json";
const PROJECTS_FILE: &str = "projects.json";
const REMOTE_CACHE_DIR: &str = "remotes";

pub fn application_support_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(APPLICATION_SUPPORT_DIR))
}

/// Where the dock layout lives for the signed-in user, or `None` if `$HOME` is unset.
///
/// Not cached: resolving `$HOME` is one `env::var_os` call, cheap enough to redo on every
/// save rather than risk a stale value if the process environment ever changed.
pub fn dock_layout_path() -> Option<PathBuf> {
    Some(application_support_dir()?.join(DOCK_LAYOUT_FILE))
}

/// Persists `state` to `path`, creating its parent directory if it does not exist yet.
///
/// Blocking: call this from `cx.background_executor()`, never on the frame thread.
pub fn save_to(path: &Path, state: &DockAreaState) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Reads and parses the dock layout at `path`.
///
/// Blocking, and meant to run once at startup before the first frame — matching how
/// `crates/story/examples/dock.rs` in the gpui-component checkout loads its own layout.
pub fn load_from(path: &Path) -> anyhow::Result<DockAreaState> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Saves `state` to the user's application support directory.
pub fn save(state: &DockAreaState) -> anyhow::Result<()> {
    let path = dock_layout_path().ok_or_else(|| anyhow::anyhow!("$HOME is not set"))?;
    save_to(&path, state)
}

/// Loads the dock layout from the user's application support directory, if one exists
/// and parses cleanly. Any failure — missing file, unreadable JSON, a shape from a much
/// older version — is treated the same way: fall back to the default layout.
pub fn load() -> Option<DockAreaState> {
    load_from(&dock_layout_path()?).ok()
}

/// Where the theme preference lives for the signed-in user, or `None` if `$HOME` is
/// unset. See [`dock_layout_path`] — same directory, same not-cached reasoning.
pub fn theme_preference_path() -> Option<PathBuf> {
    Some(application_support_dir()?.join(THEME_PREFERENCE_FILE))
}

/// Persists `preference` to `path`, creating its parent directory if it does not exist
/// yet. Blocking: call this from `cx.background_executor()`, never on the frame thread.
pub fn save_theme_preference_to(path: &Path, preference: &ThemePreference) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(preference)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Reads and parses the theme preference at `path`. Blocking, meant to run once at
/// startup before the first frame — same shape as [`load_from`].
pub fn load_theme_preference_from(path: &Path) -> anyhow::Result<ThemePreference> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Saves `preference` to the user's application support directory.
pub fn save_theme_preference(preference: &ThemePreference) -> anyhow::Result<()> {
    let path = theme_preference_path().ok_or_else(|| anyhow::anyhow!("$HOME is not set"))?;
    save_theme_preference_to(&path, preference)
}

/// Loads the theme preference from the user's application support directory, if one
/// exists and parses cleanly. Any failure falls back to `None`, mirroring [`load`].
pub fn load_theme_preference() -> Option<ThemePreference> {
    load_theme_preference_from(&theme_preference_path()?).ok()
}

/// Where the diff view mode preference lives for the signed-in user, or `None` if `$HOME` is
/// unset. See [`dock_layout_path`] — same directory, same not-cached reasoning.
pub fn diff_view_mode_path() -> Option<PathBuf> {
    Some(application_support_dir()?.join(DIFF_VIEW_MODE_FILE))
}

/// Persists `mode` to `path`, creating its parent directory if it does not exist
/// yet. Blocking: call this from `cx.background_executor()`, never on the frame thread.
pub fn save_diff_view_mode_to(path: &Path, mode: &DiffViewMode) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(mode)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Reads and parses the diff view mode preference at `path`. Blocking, meant to run once at
/// startup before the first frame — same shape as [`load_from`].
pub fn load_diff_view_mode_from(path: &Path) -> anyhow::Result<DiffViewMode> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Saves `mode` to the user's application support directory.
pub fn save_diff_view_mode(mode: &DiffViewMode) -> anyhow::Result<()> {
    let path = diff_view_mode_path().ok_or_else(|| anyhow::anyhow!("$HOME is not set"))?;
    save_diff_view_mode_to(&path, mode)
}

/// Loads the diff view mode preference from the user's application support directory, if one
/// exists and parses cleanly. Any failure falls back to `None`, mirroring [`load`].
pub fn load_diff_view_mode() -> Option<DiffViewMode> {
    load_diff_view_mode_from(&diff_view_mode_path()?).ok()
}

/// Where the project list lives for the signed-in user, or `None` if `$HOME` is unset.
/// See [`dock_layout_path`] — same directory, same not-cached reasoning.
pub fn project_list_path() -> Option<PathBuf> {
    Some(application_support_dir()?.join(PROJECTS_FILE))
}

/// Persists `list` to `path`, creating its parent directory if it does not exist yet.
/// Blocking: call this from `cx.background_executor()`, never on the frame thread —
/// except at startup, before any executor exists to spawn onto, which is the one place
/// [`crates/gitr/src/main.rs`] calls this directly.
pub fn save_project_list_to(path: &Path, list: &ProjectList) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(list)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Reads and parses the project list at `path`. Blocking, same shape as [`load_from`].
pub fn load_project_list_from(path: &Path) -> anyhow::Result<ProjectList> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Saves `list` to the user's application support directory.
pub fn save_project_list(list: &ProjectList) -> anyhow::Result<()> {
    let path = project_list_path().ok_or_else(|| anyhow::anyhow!("$HOME is not set"))?;
    save_project_list_to(&path, list)
}

/// Loads the project list from the user's application support directory, if one exists
/// and parses cleanly. Any failure — missing file, unreadable JSON, a shape from an older
/// version — falls back to `None`, mirroring [`load`]; the caller treats that the same as
/// an empty list, never as an error to surface.
pub fn load_project_list() -> Option<ProjectList> {
    load_project_list_from(&project_list_path()?).ok()
}

/// Where a remote project's bare partial clone lands, or `None` if `$HOME` is unset. See
/// [`dock_layout_path`] — same directory, same not-cached reasoning.
///
/// This is a cache gitr fills and reads on its own, not a location the user ever browses
/// or is asked to pick — [`crate::project::remote_cache_dir`] derives a specific clone's
/// directory underneath it from the URL alone.
pub fn remote_cache_root() -> Option<PathBuf> {
    Some(application_support_dir()?.join(REMOTE_CACHE_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Axis, px};
    use gpui_component::dock::{PanelInfo, PanelState};

    use crate::project::Project;

    fn sample_state() -> DockAreaState {
        DockAreaState {
            version: Some(3),
            center: PanelState {
                panel_name: "Split".to_string(),
                children: vec![
                    PanelState {
                        panel_name: "HistorySeam".to_string(),
                        children: Vec::new(),
                        info: PanelInfo::tabs(0),
                    },
                    PanelState {
                        panel_name: "DetailSeam".to_string(),
                        children: Vec::new(),
                        info: PanelInfo::tabs(0),
                    },
                ],
                info: PanelInfo::stack(vec![px(600.), px(240.)], Axis::Vertical),
            },
            left_dock: None,
            right_dock: None,
            bottom_dock: None,
        }
    }

    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gitr-persistence-test-{}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn round_trips_a_nested_layout_through_disk() {
        let path = scratch_path("round-trip.json");
        let state = sample_state();

        save_to(&path, &state).expect("save must succeed against a writable temp path");
        let loaded = load_from(&path).expect("load must succeed right after save");

        assert_eq!(loaded, state);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        let path = scratch_path("does-not-exist.json");
        assert!(load_from(&path).is_err());
    }

    #[test]
    fn saving_creates_missing_parent_directories() {
        let path = scratch_path("nested-dir").join("dock-layout.json");
        let state = sample_state();

        save_to(&path, &state).expect("save must create its own parent directory");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn round_trips_a_theme_preference_through_disk() {
        let path = scratch_path("theme-round-trip.json");

        save_theme_preference_to(&path, &ThemePreference::Dark)
            .expect("save must succeed against a writable temp path");
        let loaded = load_theme_preference_from(&path).expect("load must succeed right after save");

        assert_eq!(loaded, ThemePreference::Dark);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_theme_preference_file_is_an_error_not_a_panic() {
        let path = scratch_path("theme-does-not-exist.json");
        assert!(load_theme_preference_from(&path).is_err());
    }

    #[test]
    fn a_corrupt_theme_preference_file_is_an_error_not_a_panic() {
        let path = scratch_path("theme-corrupt.json");
        std::fs::write(&path, b"not json").expect("must be able to write the scratch file");

        assert!(load_theme_preference_from(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_theme_preference_file_falls_back_to_the_default_preference() {
        let path = scratch_path("theme-default-fallback-missing.json");
        let preference = load_theme_preference_from(&path).unwrap_or_default();
        assert_eq!(preference, ThemePreference::default());
    }

    #[test]
    fn a_corrupt_theme_preference_file_falls_back_to_the_default_preference() {
        let path = scratch_path("theme-default-fallback-corrupt.json");
        std::fs::write(&path, b"{ not json").expect("must be able to write the scratch file");

        let preference = load_theme_preference_from(&path).unwrap_or_default();
        assert_eq!(preference, ThemePreference::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saving_a_theme_preference_creates_missing_parent_directories() {
        let path = scratch_path("theme-nested-dir").join("theme-preference.json");

        save_theme_preference_to(&path, &ThemePreference::Light)
            .expect("save must create its own parent directory");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_diff_view_mode_round_trips_through_a_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("diff-view-preference.json");
        save_diff_view_mode_to(&path, &DiffViewMode::Split).expect("save");
        assert_eq!(
            load_diff_view_mode_from(&path).expect("load"),
            DiffViewMode::Split
        );
    }

    fn sample_project_list() -> ProjectList {
        let a = Project::local(PathBuf::from("/repos/a"));
        let b = Project::local(PathBuf::from("/repos/b"));
        ProjectList {
            active: Some(b.source.clone()),
            projects: vec![a, b],
        }
    }

    #[test]
    fn round_trips_a_project_list_through_disk() {
        let path = scratch_path("projects-round-trip.json");
        let list = sample_project_list();

        save_project_list_to(&path, &list).expect("save must succeed against a writable temp path");
        let loaded = load_project_list_from(&path).expect("load must succeed right after save");

        assert_eq!(loaded, list);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_project_list_file_is_an_error_not_a_panic() {
        let path = scratch_path("projects-does-not-exist.json");
        assert!(load_project_list_from(&path).is_err());
    }

    #[test]
    fn a_corrupt_project_list_file_is_an_error_not_a_panic() {
        let path = scratch_path("projects-corrupt.json");
        std::fs::write(&path, b"not json").expect("must be able to write the scratch file");

        assert!(load_project_list_from(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_or_corrupt_project_list_file_falls_back_to_an_empty_list() {
        let missing = scratch_path("projects-fallback-missing.json");
        assert_eq!(
            load_project_list_from(&missing).unwrap_or_default(),
            ProjectList::default()
        );

        let corrupt = scratch_path("projects-fallback-corrupt.json");
        std::fs::write(&corrupt, b"{ not json").expect("must be able to write the scratch file");
        assert_eq!(
            load_project_list_from(&corrupt).unwrap_or_default(),
            ProjectList::default()
        );
        let _ = std::fs::remove_file(&corrupt);
    }

    #[test]
    fn saving_a_project_list_creates_missing_parent_directories() {
        let path = scratch_path("projects-nested-dir").join("projects.json");

        save_project_list_to(&path, &sample_project_list())
            .expect("save must create its own parent directory");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
