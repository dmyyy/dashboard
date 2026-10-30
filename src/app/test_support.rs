use super::{
    App, BookmarkedProjects, RecentProjects,
    navigation::{PROJECT_SEARCH_COLUMNS, TARGET_SEARCH_COLUMNS, new_matcher},
};
use crate::event::EventHandler;
use ratatui::style::{Color, Style};
use ratatui_cheese::spinner::{Spinner, SpinnerState, SpinnerType};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use tokio::sync::{Semaphore, mpsc};
use tui_input::Input;

pub(crate) fn empty_app() -> App {
    let (background_tx, background_rx) = mpsc::unbounded_channel();

    App {
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
        focus: super::Focus::Projects,
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
        project_matcher: new_matcher(PROJECT_SEARCH_COLUMNS),
        target_matcher: new_matcher(TARGET_SEARCH_COLUMNS),
        target_statuses: Vec::new(),
        running_sessions: Vec::new(),
        spinner: Spinner::default().style(Style::default().fg(Color::Yellow)),
        spinner_state: SpinnerState::new(SpinnerType::Moon),
        spinner_last_tick: Instant::now(),
        git_status_scan_index: 0,
        pending_g: false,
        projects_root: PathBuf::from("/__dashboard_test_projects__"),
        projects: Vec::new(),
        recent_projects: RecentProjects::default(),
        bookmarked_projects: BookmarkedProjects::default(),
        project_colors: HashMap::new(),
        project_color_hue: 0.0,
        discovering_projects: false,
        discovery_generation: 0,
        discovery_cancel: None,
        discovery_seen: HashSet::new(),
        background_tx,
        background_rx,
        background_limiter: Arc::new(Semaphore::new(8)),
        size_limiter: Arc::new(Semaphore::new(1)),
        events: EventHandler::for_test(),
    }
}
