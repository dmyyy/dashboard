use super::projects::{Project, directory_size, sort_projects};
use super::providers::{
    CiRunsData, LanguagesData, ProjectMetadataSummary, discover_ci_runs, discover_languages,
    discover_project_metadata, git_status,
};
use super::targets::discover_targets;
use super::{App, Target};
use std::path::PathBuf;

#[derive(Debug)]
pub enum BackgroundResult {
    Metadata(PathBuf, ProjectMetadataSummary),
    GitStatus(PathBuf, String),
    Size(PathBuf, u64),
    CiRuns(PathBuf, Option<CiRunsData>),
    Languages(PathBuf, Option<LanguagesData>),
    Targets(PathBuf, Vec<Target>),
    ProjectCreated(Option<PathBuf>),
    ProjectCleaned(PathBuf),
    ProjectsDiscovered {
        generation: u64,
        projects: Vec<Project>,
    },
    ProjectDiscoveryFinished {
        generation: u64,
    },
}
impl App {
    pub(super) fn drain_background_results(&mut self) -> bool {
        let mut projects_changed = false;
        for _ in 0..64 {
            let Ok(result) = self.background_rx.try_recv() else {
                break;
            };
            match result {
                BackgroundResult::Metadata(path, metadata) => {
                    self.loading_metadata.remove(&path);
                    self.metadata_cache.insert(path, metadata);
                }
                BackgroundResult::GitStatus(path, git_status) => {
                    self.loading_git_statuses.remove(&path);
                    self.git_status_cache.insert(path, git_status);
                }
                BackgroundResult::Size(path, size) => {
                    self.loading_sizes.remove(&path);
                    self.size_cache.insert(path, size);
                }
                BackgroundResult::CiRuns(path, ci_runs) => {
                    let is_current_project = self
                        .current_project()
                        .is_some_and(|project| project.path == path);
                    self.loading_ci_runs.remove(&path);
                    self.ci_runs_cache.insert(path, ci_runs);
                    if is_current_project {
                        self.update_ci_filter();
                    }
                }
                BackgroundResult::Languages(path, languages) => {
                    self.loading_languages.remove(&path);
                    self.languages_cache.insert(path, languages);
                }
                BackgroundResult::Targets(path, targets) => {
                    let is_current_project = self
                        .current_project()
                        .is_some_and(|project| project.path == path);
                    self.loading_targets.remove(&path);
                    self.target_cache.insert(path, targets.clone());
                    if is_current_project {
                        self.targets = targets;
                        self.rebuild_target_matcher();
                    }
                }
                BackgroundResult::ProjectCreated(project_path) => {
                    self.creating_project_in_background = false;
                    if let Some(project_path) = project_path.as_deref() {
                        self.mark_project_opened(project_path);
                    }
                    self.reload_projects();
                }
                BackgroundResult::ProjectCleaned(path) => {
                    self.size_cache.remove(&path);
                    self.loading_sizes.remove(&path);
                    self.request_size(path);
                }
                BackgroundResult::ProjectsDiscovered {
                    generation,
                    projects,
                } if generation == self.discovery_generation => {
                    for project in projects {
                        self.discovery_seen.insert(project.path.clone());
                        if let Some(existing) = self
                            .projects
                            .iter_mut()
                            .find(|existing| existing.path == project.path)
                        {
                            *existing = project;
                            continue;
                        }
                        self.inject_project(project.clone());
                        self.projects.push(project);
                        projects_changed = true;
                    }
                }
                BackgroundResult::ProjectDiscoveryFinished { generation }
                    if generation == self.discovery_generation =>
                {
                    let selected_path = self.current_project().map(|project| project.path.clone());
                    self.discovering_projects = false;
                    self.projects
                        .retain(|project| self.discovery_seen.contains(&project.path));
                    sort_projects(&mut self.projects, &self.recent_projects);
                    self.rebuild_project_matcher();
                    if let Some(selected_path) = selected_path {
                        self.cursor = self
                            .filtered_projects
                            .iter()
                            .position(|&index| {
                                self.projects
                                    .get(index)
                                    .is_some_and(|project| project.path == selected_path)
                            })
                            .map_or(-1, |index| index as isize);
                    }
                    projects_changed = true;
                }
                BackgroundResult::ProjectsDiscovered { .. }
                | BackgroundResult::ProjectDiscoveryFinished { .. } => {}
            }
        }
        projects_changed
    }

    pub(super) fn request_current_project_metadata(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };
        self.request_metadata(project.path.clone());
    }

    pub(super) fn request_current_project_ci_runs(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };
        self.request_ci_runs(project.path.clone());
    }

    pub(super) fn request_current_project_languages(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };
        self.request_languages(project.path.clone());
    }

    pub(super) fn request_visible_project_data(&mut self) {
        if self.discovering_projects || !self.loading_sizes.is_empty() {
            return;
        }
        if let Some(project) = self.current_project() {
            if !self.size_cache.contains_key(&project.path) {
                self.request_size(project.path.clone());
                return;
            }
        }
        let next_path = {
            self.visible_projects()
                .map(|project| project.path.clone())
                .find(|path| !self.size_cache.contains_key(path))
        };
        if let Some(path) = next_path {
            self.request_size(path);
        }
    }

    pub(super) fn request_visible_git_statuses(&mut self) {
        if self.discovering_projects {
            return;
        }
        let visible_paths: Vec<_> = self
            .visible_projects()
            .take(16)
            .map(|project| project.path.clone())
            .collect();

        for path in visible_paths {
            self.request_git_status(path);
        }
    }


    pub(super) fn request_next_git_status_batch(&mut self, batch_size: usize) {
        if self.projects.is_empty() {
            return;
        }

        for _ in 0..batch_size {
            let index = self.git_status_scan_index % self.projects.len();
            self.git_status_scan_index = self.git_status_scan_index.wrapping_add(1);
            if let Some(project) = self.projects.get(index) {
                self.request_git_status(project.path.clone());
            }
        }
    }

    pub(super) fn request_metadata(&mut self, path: PathBuf) {
        if self.metadata_cache.contains_key(&path) || !self.loading_metadata.insert(path.clone()) {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let metadata =
                tokio::task::spawn_blocking(move || discover_project_metadata(&work_path))
                    .await
                    .unwrap_or_else(|_| ProjectMetadataSummary::default());
            let _ = tx.send(BackgroundResult::Metadata(path, metadata));
        });
    }

    pub(super) fn request_git_status(&mut self, path: PathBuf) {
        if self.git_status_cache.contains_key(&path)
            || !self.loading_git_statuses.insert(path.clone())
        {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let git_status = tokio::task::spawn_blocking(move || git_status(&work_path))
                .await
                .unwrap_or_else(|_| "UNTRACKED".to_string());
            let _ = tx.send(BackgroundResult::GitStatus(path, git_status));
        });
    }

    pub(super) fn request_size(&mut self, path: PathBuf) {
        if self.size_cache.contains_key(&path) || !self.loading_sizes.insert(path.clone()) {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.size_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let size = tokio::task::spawn_blocking(move || directory_size(&work_path))
                .await
                .unwrap_or(0);
            let _ = tx.send(BackgroundResult::Size(path, size));
        });
    }

    pub(super) fn request_ci_runs(&mut self, path: PathBuf) {
        if self.ci_runs_cache.contains_key(&path) || !self.loading_ci_runs.insert(path.clone()) {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let ci_runs = tokio::task::spawn_blocking(move || discover_ci_runs(&work_path))
                .await
                .ok()
                .flatten();
            let _ = tx.send(BackgroundResult::CiRuns(path, ci_runs));
        });
    }

    pub(super) fn request_languages(&mut self, path: PathBuf) {
        if self.languages_cache.contains_key(&path) || !self.loading_languages.insert(path.clone())
        {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let languages = tokio::task::spawn_blocking(move || discover_languages(&work_path))
                .await
                .ok()
                .flatten();
            let _ = tx.send(BackgroundResult::Languages(path, languages));
        });
    }

    pub(super) fn request_targets(&mut self, path: PathBuf) {
        if self.target_cache.contains_key(&path) || !self.loading_targets.insert(path.clone()) {
            return;
        }

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };
            let work_path = path.clone();
            let targets = tokio::task::spawn_blocking(move || discover_targets(&work_path))
                .await
                .unwrap_or_default();
            let _ = tx.send(BackgroundResult::Targets(path, targets));
        });
    }
}
