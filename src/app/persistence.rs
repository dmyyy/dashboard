use super::projects::expand_tilde;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct RecentProjects {
    pub(crate) entries: HashMap<String, u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct BookmarkedProjects {
    pub(crate) entries: HashSet<String>,
}
pub(super) fn dashboard_state_dir() -> PathBuf {
    env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| expand_tilde(PathBuf::from("~/.local/state")))
        .join("dashboard")
}

fn bookmarked_projects_path() -> PathBuf {
    dashboard_state_dir().join("bookmarked-projects.json")
}

fn recent_projects_path() -> PathBuf {
    dashboard_state_dir().join("recent-projects.json")
}

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, content)
}

pub(super) fn load_bookmarked_projects() -> BookmarkedProjects {
    load_json(&bookmarked_projects_path())
}

pub(super) fn save_bookmarked_projects(
    bookmarked_projects: &BookmarkedProjects,
) -> std::io::Result<()> {
    save_json(&bookmarked_projects_path(), bookmarked_projects)
}

pub(super) fn load_recent_projects() -> RecentProjects {
    load_json(&recent_projects_path())
}

pub(super) fn save_recent_projects(recent_projects: &RecentProjects) -> std::io::Result<()> {
    save_json(&recent_projects_path(), recent_projects)
}

#[cfg(test)]
mod tests {
    use super::{
        BookmarkedProjects, RecentProjects, bookmarked_projects_path, load_bookmarked_projects,
        load_recent_projects, recent_projects_path, save_bookmarked_projects, save_recent_projects,
    };
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            loop {
                let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "dashboard-refactor-persistence-{}-{counter}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("failed to create test directory {path:?}: {error}"),
                }
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn refactor_persistence_roundtrip_and_failures() {
        if std::env::var_os("DASHBOARD_TEST_PERSISTENCE_CHILD").is_none() {
            let state_home = TempDir::new();
            let status = Command::new(std::env::current_exe().expect("test executable"))
                .arg("refactor_persistence_roundtrip_and_failures")
                .arg("--nocapture")
                .arg("--test-threads=1")
                .env("XDG_STATE_HOME", &state_home.0)
                .env("DASHBOARD_TEST_PERSISTENCE_CHILD", "1")
                .status()
                .expect("spawn persistence child");
            assert!(status.success(), "persistence child failed");
            return;
        }

        assert!(load_bookmarked_projects().entries.is_empty());
        assert!(load_recent_projects().entries.is_empty());

        let mut bookmarks = BookmarkedProjects::default();
        bookmarks.entries.insert("/fixture/project".to_owned());
        save_bookmarked_projects(&bookmarks).expect("save bookmarks");
        assert!(
            load_bookmarked_projects()
                .entries
                .contains("/fixture/project")
        );

        let mut recent = RecentProjects::default();
        recent.entries.insert("/fixture/project".to_owned(), 42);
        save_recent_projects(&recent).expect("save recent projects");
        assert_eq!(
            load_recent_projects().entries.get("/fixture/project"),
            Some(&42)
        );

        let bookmarks_path = bookmarked_projects_path();
        let recent_path = recent_projects_path();
        assert_ne!(bookmarks_path, recent_path);
        assert_eq!(
            bookmarks_path.file_name(),
            Some(std::ffi::OsStr::new("bookmarked-projects.json"))
        );
        assert_eq!(
            recent_path.file_name(),
            Some(std::ffi::OsStr::new("recent-projects.json"))
        );

        fs::write(&bookmarks_path, "{ malformed").expect("corrupt bookmarks");
        fs::write(&recent_path, "{ malformed").expect("corrupt recents");
        assert!(load_bookmarked_projects().entries.is_empty());
        assert!(load_recent_projects().entries.is_empty());

        let state_dir = bookmarks_path.parent().expect("state directory");
        fs::remove_dir_all(state_dir).expect("remove fixture state directory");
        fs::write(state_dir, "not a directory").expect("replace state directory with file");
        assert!(save_bookmarked_projects(&bookmarks).is_err());
        assert!(save_recent_projects(&recent).is_err());
    }
}
