use crate::{
    event::{AppEvent, Event, EventHandler},
    ui,
};
use cargo_metadata::{MetadataCommand, Target as CargoTarget};
use nucleo::Nucleo;
use persistence::{
    BookmarkedProjects, RecentProjects, load_bookmarked_projects, load_recent_projects,
    save_bookmarked_projects, save_recent_projects,
};
use portable_pty::{CommandBuilder as PtyCommandBuilder, PtySize, native_pty_system};
use projects::{Project, projects_root};
use providers::{
    CiRunsData, LanguageStat, LanguagesData, ProjectMetadataSummary, discover_ci_runs,
    discover_languages, discover_project_metadata, git_status,
};
use providers::{normalize_github_remote, shell_url_encode};
use ratatui::{
    DefaultTerminal,
    style::{Color, Style},
};
use ratatui_cheese::spinner::{Spinner, SpinnerState, SpinnerType};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
    time::Instant,
};
use tokio::sync::{Semaphore, mpsc};
use tui_input::Input;

mod background;
mod input;
mod navigation;
mod persistence;
mod projects;
mod providers;
mod runtime;
mod targets;
#[cfg(test)]
mod test_support;
pub(crate) use background::BackgroundResult;
pub(crate) use providers::CiRun;
pub(crate) use runtime::{
    ProcessStats, RunProfile, RunningSession, TargetStatus, TargetStatusKind,
};
use runtime::{read_process_stats, shell_escape_arg, shell_escape_path, target_runtime_key};
use targets::{Target, discover_targets, load_target_descriptions, select_target_kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Projects,
    RunningTargets,
    CiRuns,
    Targets,
}

pub struct App {
    pub running: bool,
    pub confirm_delete_project: bool,
    pub show_help: bool,
    pub creating_project: bool,
    pub renaming_project: bool,
    pub creating_project_in_background: bool,
    pub counter: u8,
    pub cursor: isize,
    pub target_cursor: isize,
    pub running_cursor: isize,
    pub ci_cursor: isize,
    pub targets: Vec<Target>,
    pub target_cache: HashMap<PathBuf, Vec<Target>>,
    pub metadata_cache: HashMap<PathBuf, ProjectMetadataSummary>,
    pub ci_runs_cache: HashMap<PathBuf, Option<CiRunsData>>,
    pub languages_cache: HashMap<PathBuf, Option<LanguagesData>>,
    pub git_status_cache: HashMap<PathBuf, String>,
    pub size_cache: HashMap<PathBuf, u64>,
    pub loading_metadata: HashSet<PathBuf>,
    pub loading_git_statuses: HashSet<PathBuf>,
    pub loading_sizes: HashSet<PathBuf>,
    pub loading_ci_runs: HashSet<PathBuf>,
    pub loading_languages: HashSet<PathBuf>,
    pub loading_targets: HashSet<PathBuf>,
    pub focus: Focus,
    pub filter_mode: bool,
    pub project_query: String,
    pub ci_query: String,
    pub target_query: String,
    pub project_input: Input,
    pub ci_input: Input,
    pub target_input: Input,
    pub create_project_input: Input,
    pub rename_project_input: Input,
    pub filtered_projects: Vec<usize>,
    pub filtered_ci_runs: Vec<usize>,
    pub filtered_targets: Vec<usize>,
    pub project_matcher: Nucleo<Project>,
    pub target_matcher: Nucleo<Target>,
    pub target_statuses: Vec<TargetStatus>,
    pub running_sessions: Vec<RunningSession>,
    pub spinner: Spinner,
    pub spinner_state: SpinnerState,
    pub spinner_last_tick: Instant,
    pub git_status_scan_index: usize,
    pub pending_g: bool,
    pub projects_root: PathBuf,
    pub projects: Vec<Project>,
    // Stores project paths whose linked worktree rows are hidden.
    pub minimized_worktree_projects: HashSet<PathBuf>,
    pub recent_projects: RecentProjects,
    pub bookmarked_projects: BookmarkedProjects,
    pub project_colors: HashMap<String, String>,
    // Tracks the last generated project accent hue for distinct consecutive colors.
    pub project_color_hue: f64,
    pub discovering_projects: bool,
    pub discovery_generation: u64,
    pub discovery_cancel: Option<Arc<AtomicBool>>,
    pub discovery_seen: HashSet<PathBuf>,
    pub recently_discovered: HashMap<PathBuf, u64>,
    pub mark_new_discoveries: bool,
    pub background_tx: mpsc::UnboundedSender<BackgroundResult>,
    pub background_rx: mpsc::UnboundedReceiver<BackgroundResult>,
    pub background_limiter: Arc<Semaphore>,
    pub size_limiter: Arc<Semaphore>,
    pub events: EventHandler,
}

impl Default for App {
    fn default() -> Self {
        let projects_root = projects_root();
        let recent_projects = load_recent_projects();
        let bookmarked_projects = load_bookmarked_projects();
        let (background_tx, background_rx) = mpsc::unbounded_channel();

        Self {
            running: true,
            confirm_delete_project: false,
            show_help: false,
            creating_project: false,
            renaming_project: false,
            creating_project_in_background: false,
            counter: 0,
            cursor: -1,
            target_cursor: -1,
            running_cursor: -1,
            ci_cursor: -1,
            targets: Vec::new(),
            target_cache: HashMap::new(),
            metadata_cache: HashMap::new(),
            ci_runs_cache: HashMap::new(),
            languages_cache: HashMap::new(),
            git_status_cache: HashMap::new(),
            size_cache: HashMap::new(),
            loading_metadata: HashSet::new(),
            loading_git_statuses: HashSet::new(),
            loading_sizes: HashSet::new(),
            loading_ci_runs: HashSet::new(),
            loading_languages: HashSet::new(),
            loading_targets: HashSet::new(),
            focus: Focus::Projects,
            filter_mode: false,
            project_query: String::new(),
            ci_query: String::new(),
            target_query: String::new(),
            project_input: Input::default(),
            ci_input: Input::default(),
            target_input: Input::default(),
            create_project_input: Input::default(),
            rename_project_input: Input::default(),
            filtered_projects: Vec::new(),
            filtered_ci_runs: Vec::new(),
            filtered_targets: Vec::new(),
            project_matcher: navigation::new_matcher(navigation::PROJECT_SEARCH_COLUMNS),
            target_matcher: navigation::new_matcher(navigation::TARGET_SEARCH_COLUMNS),
            target_statuses: Vec::new(),
            running_sessions: Vec::new(),
            spinner: Spinner::default().style(Style::default().fg(Color::Yellow)),
            spinner_state: SpinnerState::new(SpinnerType::Moon),
            spinner_last_tick: Instant::now(),
            git_status_scan_index: 0,
            pending_g: false,
            projects_root,
            projects: Vec::new(),
            minimized_worktree_projects: HashSet::new(),
            recent_projects,
            bookmarked_projects,
            project_colors: HashMap::new(),
            project_color_hue: 0.0,
            discovering_projects: false,
            discovery_generation: 0,
            discovery_cancel: None,
            discovery_seen: HashSet::new(),
            recently_discovered: HashMap::new(),
            mark_new_discoveries: false,
            background_tx,
            background_rx,
            background_limiter: Arc::new(Semaphore::new(8)),
            size_limiter: Arc::new(Semaphore::new(1)),
            events: EventHandler::new(),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.reload_projects();
        while self.running {
            terminal.draw(|frame| ui::render(frame, &mut self))?;
            match self.events.next().await? {
                Event::Tick => self.tick(),
                Event::Crossterm(event) => match event {
                    crossterm::event::Event::Key(key_event)
                        if key_event.kind == crossterm::event::KeyEventKind::Press =>
                    {
                        self.handle_key_events(key_event)?
                    }
                    _ => {}
                },
                Event::App(app_event) => match app_event {
                    AppEvent::Increment => self.increment_counter(),
                    AppEvent::Decrement => self.decrement_counter(),
                    AppEvent::Quit => self.quit(),
                },
            }
        }
        Ok(())
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let dt = now - self.spinner_last_tick;
        self.spinner_last_tick = now;
        self.spinner_state.tick(dt);
        self.refresh_target_status();
        if self.focus == Focus::RunningTargets && self.running_target_statuses().next().is_none() {
            self.focus = if self.project_ci_runs().is_some() {
                Focus::CiRuns
            } else {
                Focus::Projects
            };
        }

        let selected_path = self.current_project().map(|project| project.path.clone());
        let projects_changed = self.drain_background_results();
        let project_status = self.project_matcher.tick(0);
        if projects_changed || project_status.changed {
            self.sync_filtered_projects();
        }
        if selected_path != self.current_project().map(|project| project.path.clone()) {
            self.refresh_targets();
        }

        let target_status = self.target_matcher.tick(0);
        if target_status.changed {
            self.sync_filtered_targets();
        }

        self.request_current_project_metadata();
        self.request_current_project_ci_runs();
        self.request_current_project_languages();
        self.request_visible_project_data();
        self.request_visible_git_statuses();
        if !self.discovering_projects {
            self.request_next_git_status_batch(4);
        }
    }

    pub fn quit(&mut self) {
        if let Some(cancel) = &self.discovery_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.running = false;
    }

    pub fn increment_counter(&mut self) {
        self.counter = self.counter.saturating_add(1);
    }

    pub fn decrement_counter(&mut self) {
        self.counter = self.counter.saturating_sub(1);
    }

    pub fn visible_targets(&self) -> impl Iterator<Item = &Target> {
        self.filtered_targets
            .iter()
            .filter_map(|&index| self.targets.get(index))
    }
}

#[cfg(test)]
mod project_tests {
    use super::projects::{Project, discover_projects, discover_projects_streaming, sort_projects};
    use super::test_support::empty_app;
    use super::{BackgroundResult, RecentProjects};
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    };

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            loop {
                let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "dashboard-refactor-projects-{}-{counter}",
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
    fn refactor_projects_scan_boundaries() {
        let root = TempDir::new();
        fs::create_dir_all(root.0.join("git-project/.git")).expect("git marker");
        fs::create_dir_all(root.0.join("cargo-project")).expect("cargo project");
        fs::write(root.0.join("cargo-project/Cargo.toml"), "[package]").expect("cargo manifest");
        fs::create_dir_all(root.0.join("workspace/member")).expect("workspace member");
        fs::write(root.0.join("workspace/Cargo.toml"), "[workspace]").expect("workspace manifest");
        fs::write(root.0.join("workspace/member/Cargo.toml"), "[package]")
            .expect("member manifest");
        // A linked worktree uses a .git file rather than a .git directory.
        fs::create_dir_all(root.0.join("workspace/.worktrees/feature"))
            .expect("worktree directory");
        fs::write(
            root.0.join("workspace/.worktrees/feature/.git"),
            "gitdir: /fixture/worktrees/feature",
        )
        .expect("worktree git marker");
        fs::create_dir_all(root.0.join("target/hidden")).expect("target directory");
        fs::write(root.0.join("target/hidden/Cargo.toml"), "[package]").expect("hidden manifest");

        let projects = discover_projects(&root.0);
        let names: std::collections::HashSet<_> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(
            names,
            std::collections::HashSet::from([
                "git-project",
                "cargo-project",
                "workspace",
                "feature"
            ])
        );
        assert!(
            projects
                .iter()
                .any(|project| project.name == "git-project" && project.has_git_dir)
        );
        assert!(
            projects
                .iter()
                .any(|project| project.name == "workspace" && project.is_workspace)
        );
        assert!(
            projects
                .iter()
                .any(|project| project.name == "feature" && project.is_worktree())
        );
        assert!(discover_projects(&root.0.join("missing")).is_empty());
    }

    #[test]
    fn orders_projects_opened_on_the_same_day_by_millisecond_recency() {
        let root = TempDir::new();
        for name in ["older", "newer", "git-a", "plain-a", "plain-b"] {
            fs::create_dir(root.0.join(name)).expect("project directory");
            fs::write(root.0.join(name).join("Cargo.toml"), "[package]").expect("project manifest");
        }
        fs::create_dir(root.0.join("git-a/.git")).expect("git marker");
        let mut projects = discover_projects(&root.0);
        let mut entries = HashMap::new();
        entries.insert(
            root.0.join("older").to_string_lossy().into_owned(),
            1_700_000_000_001,
        );
        entries.insert(
            root.0.join("newer").to_string_lossy().into_owned(),
            1_700_000_000_002,
        );
        sort_projects(&mut projects, &RecentProjects { entries });
        assert_eq!(
            projects
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            ["newer", "older", "git-a", "plain-a", "plain-b"]
        );
    }

    #[test]
    fn streams_recent_projects_before_tree_scan() {
        let root = TempDir::new();
        for name in ["older", "newer", "undiscovered"] {
            fs::create_dir(root.0.join(name)).expect("project directory");
            fs::write(root.0.join(name).join("Cargo.toml"), "[package]").expect("project manifest");
        }
        let recents = RecentProjects {
            entries: HashMap::from([
                (root.0.join("older").to_string_lossy().into_owned(), 10),
                (root.0.join("newer").to_string_lossy().into_owned(), 20),
            ]),
        };
        let cancelled = AtomicBool::new(false);
        let mut streamed = Vec::new();

        assert!(discover_projects_streaming(
            &root.0,
            &recents,
            &cancelled,
            |project| {
                streamed.push(project.name);
                true
            },
        ));
        assert_eq!(&streamed[..2], ["newer", "older"]);
        assert!(streamed.contains(&"undiscovered".to_string()));
    }

    #[test]
    fn streams_discovery_batches_without_waiting_for_completion() {
        let mut app = empty_app();
        app.discovery_generation = 7;
        app.discovering_projects = true;
        let newer = PathBuf::from("/projects/newer");
        let older = PathBuf::from("/projects/older");
        app.recent_projects.entries = HashMap::from([
            (newer.to_string_lossy().into_owned(), 20),
            (older.to_string_lossy().into_owned(), 10),
        ]);
        app.background_tx
            .send(BackgroundResult::ProjectsDiscovered {
                generation: 7,
                projects: vec![
                    Project {
                        name: "older".to_string(),
                        path: older,
                        has_git_dir: false,
                        is_workspace: false,
                    },
                    Project {
                        name: "newer".to_string(),
                        path: newer,
                        has_git_dir: false,
                        is_workspace: false,
                    },
                ],
            })
            .expect("discovery result");

        assert!(app.drain_background_results());
        app.sync_filtered_projects();
        assert_eq!(
            app.visible_projects()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>(),
            ["newer", "older"]
        );
        assert!(app.discovering_projects);
    }

    #[test]
    fn keeps_selected_project_when_streaming_reorders_rows() {
        let mut app = empty_app();
        app.discovery_generation = 1;
        let older = PathBuf::from("/projects/older");
        let newer = PathBuf::from("/projects/newer");
        app.projects = vec![
            Project {
                name: "older".to_string(),
                path: older.clone(),
                has_git_dir: false,
                is_workspace: false,
            },
            Project {
                name: "newer".to_string(),
                path: newer.clone(),
                has_git_dir: false,
                is_workspace: false,
            },
        ];
        app.recent_projects.entries = HashMap::from([
            (older.to_string_lossy().into_owned(), 10),
            (newer.to_string_lossy().into_owned(), 20),
        ]);
        app.sync_filtered_projects();
        app.cursor = 1;
        let latest = PathBuf::from("/projects/latest");
        app.recent_projects
            .entries
            .insert(latest.to_string_lossy().into_owned(), 30);
        app.background_tx
            .send(BackgroundResult::ProjectsDiscovered {
                generation: 1,
                projects: vec![Project {
                    name: "latest".to_string(),
                    path: latest,
                    has_git_dir: false,
                    is_workspace: false,
                }],
            })
            .expect("discovery result");

        assert!(app.drain_background_results());
        app.sync_filtered_projects();
        assert_eq!(
            app.current_project().map(|project| &project.path),
            Some(&older)
        );
    }

    #[test]
    fn ignores_stale_discovery_batches() {
        let mut app = empty_app();
        app.discovery_generation = 2;
        app.background_tx
            .send(BackgroundResult::ProjectsDiscovered {
                generation: 1,
                projects: vec![Project {
                    name: "stale".to_string(),
                    path: PathBuf::from("/projects/stale"),
                    has_git_dir: false,
                    is_workspace: false,
                }],
            })
            .expect("stale result");

        assert!(!app.drain_background_results());
        assert!(app.projects.is_empty());
    }
}

#[cfg(test)]
mod target_tests {
    use super::load_target_descriptions;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn refactor_targets_description_precedence() {
        let path = std::env::temp_dir().join(format!(
            "dashboard-target-descriptions-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("test directory");
        let manifest = path.join("Cargo.toml");
        fs::write(
            &manifest,
            r#"
[[example]]
name = "demo"
description = " direct "
[[example]]
name = "fallback"
description = " direct fallback "
[[example]]
name = "empty"
description = "   "
[package.metadata.example.demo]
description = " override "
[package.metadata.example.fallback]
description = "   "
"#,
        )
        .expect("manifest");

        let descriptions = load_target_descriptions(&manifest);
        assert_eq!(
            descriptions.get(&("example".into(), "demo".into())),
            Some(&Some("override".into()))
        );
        assert_eq!(
            descriptions.get(&("example".into(), "fallback".into())),
            Some(&Some("direct fallback".into()))
        );
        assert_eq!(
            descriptions.get(&("example".into(), "empty".into())),
            Some(&None)
        );
        assert!(load_target_descriptions(&path.join("missing")).is_empty());
        fs::write(&manifest, "{ malformed").expect("malformed manifest");
        assert!(load_target_descriptions(&manifest).is_empty());
        fs::remove_dir_all(path).expect("remove test directory");
    }
}

#[cfg(test)]
mod provider_tests {
    use super::{normalize_github_remote, shell_url_encode};

    #[test]
    fn refactor_providers_github_url_handling() {
        assert_eq!(
            normalize_github_remote("git@github.com:owner/repo.git"),
            Some("https://github.com/owner/repo".to_owned())
        );
        assert_eq!(
            normalize_github_remote("https://github.com/owner/repo.git"),
            Some("https://github.com/owner/repo".to_owned())
        );
        assert_eq!(
            normalize_github_remote("https://gitlab.com/owner/repo.git"),
            None
        );
        assert_eq!(shell_url_encode("feature/a b"), "feature%2Fa%20b");
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::{
        ProcessStats, Project, RunProfile, Target, TargetStatus, TargetStatusKind,
        shell_escape_arg, shell_escape_path,
    };
    use crate::app::test_support::empty_app;
    use std::path::{Path, PathBuf};

    #[test]
    fn refactor_runtime_shell_quoting() {
        assert_eq!(shell_escape_arg("a'b"), "'a'\\''b'");
        assert_eq!(shell_escape_path(Path::new("a b")), "'a b'");
    }

    #[test]
    fn refactor_runtime_status_identity() {
        let project = Project {
            name: "one".into(),
            path: PathBuf::from("/one"),
            has_git_dir: false,
            is_workspace: false,
        };
        let target = Target {
            package_name: "pkg".into(),
            kind: "bin".into(),
            name: "app".into(),
            path: "src/main.rs".into(),
            description: None,
            required_features: Vec::new(),
        };
        let mut app = empty_app();
        for (path, kind) in [
            ("/other", TargetStatusKind::Running),
            ("/one", TargetStatusKind::Failed),
        ] {
            app.target_statuses.push(TargetStatus {
                kind,
                project_name: "project".into(),
                project_path: PathBuf::from(path),
                target_name: "app".into(),
                target_kind: "bin".into(),
                key: path.into(),
                profile: RunProfile::Debug,
                pid: None,
                started_at: None,
                stats: ProcessStats::default(),
            });
        }
        assert_eq!(
            app.target_status_for(&project, &target)
                .map(|status| &status.kind),
            Some(&TargetStatusKind::Failed)
        );
        app.running_cursor = -1;
        assert!(app.current_running_target_status().is_none());
        app.running_cursor = 2;
        assert!(app.current_running_target_status().is_none());
    }
}
