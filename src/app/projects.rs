use super::persistence::{RecentProjects, save_bookmarked_projects, save_recent_projects};
use super::{App, BackgroundResult, Focus, Target, shell_escape_arg, shell_escape_path};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone)]
pub(crate) struct Project {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) has_git_dir: bool,
    pub(crate) is_workspace: bool,
}

impl App {
    pub(super) fn start_create_project(&mut self) {
        self.creating_project = true;
        self.create_project_input.reset();
    }

    pub(super) fn cancel_create_project(&mut self) {
        self.creating_project = false;
        self.create_project_input.reset();
    }

    pub(super) fn confirm_create_project(&mut self) -> color_eyre::Result<()> {
        let value = self.create_project_input.value().trim().to_string();
        if value.is_empty() {
            self.cancel_create_project();
            return Ok(());
        }

        fs::create_dir_all(&self.projects_root)?;

        let created_project_path = if looks_like_git_url(&value) {
            cloned_project_path(&self.projects_root, &value)
        } else {
            Some(self.projects_root.join(&value))
        };

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        let projects_root = self.projects_root.clone();
        self.cancel_create_project();
        self.creating_project_in_background = true;

        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };

            let value = value.clone();
            let projects_root = projects_root.clone();
            let project_path = created_project_path.clone();
            let status = tokio::task::spawn_blocking(move || {
                let mut command = if looks_like_git_url(&value) {
                    let mut command = Command::new("git");
                    command.args(["clone", &value]);
                    command
                } else {
                    let mut command = Command::new("cargo");
                    command.args(["new", &value]);
                    command
                };

                command
                    .current_dir(&projects_root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
            })
            .await;

            if matches!(status, Ok(Ok(exit)) if exit.success()) {
                let _ = tx.send(BackgroundResult::ProjectCreated(project_path));
            } else {
                let _ = tx.send(BackgroundResult::ProjectCreated(None));
            }
        });

        Ok(())
    }

    pub(super) fn reload_projects(&mut self) {
        let selected_path = self.current_project().map(|project| project.path.clone());
        self.projects = discover_projects(&self.projects_root);
        sort_projects(&mut self.projects, &self.recent_projects);
        self.rebuild_project_matcher();

        if let Some(selected_path) = selected_path {
            if let Some(index) = self.filtered_projects.iter().position(|&project_index| {
                self.projects
                    .get(project_index)
                    .is_some_and(|project| project.path == selected_path)
            }) {
                self.cursor = index as isize;
                self.refresh_targets();
                return;
            }
        }

        self.cursor = if self.filtered_projects.is_empty() {
            -1
        } else {
            0
        };
        self.refresh_targets();
    }

    pub(super) fn toggle_selected_project_bookmark(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };

        let key = project.path.to_string_lossy().to_string();
        if !self.bookmarked_projects.entries.insert(key.clone()) {
            self.bookmarked_projects.entries.remove(&key);
        }
        let _ = save_bookmarked_projects(&self.bookmarked_projects);
    }

    pub(super) fn clean_selected_project(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };

        if !project.path.join("Cargo.toml").exists() {
            return;
        }

        let path = project.path.clone();
        self.size_cache.remove(&path);
        self.loading_sizes.remove(&path);

        let tx = self.background_tx.clone();
        let limiter = self.background_limiter.clone();
        tokio::spawn(async move {
            let Ok(_permit) = limiter.acquire_owned().await else {
                return;
            };

            let work_path = path.clone();
            let cleaned = tokio::task::spawn_blocking(move || {
                Command::new("cargo")
                    .arg("clean")
                    .current_dir(&work_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);

            if cleaned {
                let _ = tx.send(BackgroundResult::ProjectCleaned(path));
            }
        });
    }

    pub(super) fn rescan_selected_project(&mut self) {
        let Some(project) = self.current_project() else {
            return;
        };

        let path = project.path.clone();
        self.metadata_cache.remove(&path);
        self.ci_runs_cache.remove(&path);
        self.languages_cache.remove(&path);
        self.git_status_cache.remove(&path);
        self.size_cache.remove(&path);
        self.target_cache.remove(&path);
        self.loading_metadata.remove(&path);
        self.loading_ci_runs.remove(&path);
        self.loading_languages.remove(&path);
        self.loading_git_statuses.remove(&path);
        self.loading_sizes.remove(&path);
        self.loading_targets.remove(&path);

        self.refresh_targets();
        self.request_metadata(path.clone());
        self.request_ci_runs(path.clone());
        self.request_languages(path.clone());
        self.request_git_status(path.clone());
        self.request_size(path);
    }

    pub(super) fn delete_selected_project(&mut self) -> color_eyre::Result<()> {
        let Some(project) = self.current_project() else {
            self.confirm_delete_project = false;
            return Ok(());
        };

        let project_path = project.path.clone();
        fs::remove_dir_all(&project_path)?;

        let key = project_path.to_string_lossy().to_string();
        self.bookmarked_projects.entries.remove(&key);
        self.recent_projects.entries.remove(&key);
        let _ = save_bookmarked_projects(&self.bookmarked_projects);
        let _ = save_recent_projects(&self.recent_projects);

        self.projects.retain(|project| project.path != project_path);
        self.metadata_cache.remove(&project_path);
        self.ci_runs_cache.remove(&project_path);
        self.languages_cache.remove(&project_path);
        self.git_status_cache.remove(&project_path);
        self.size_cache.remove(&project_path);
        self.target_cache.remove(&project_path);
        self.loading_metadata.remove(&project_path);
        self.loading_ci_runs.remove(&project_path);
        self.loading_languages.remove(&project_path);
        self.loading_git_statuses.remove(&project_path);
        self.loading_sizes.remove(&project_path);
        self.loading_targets.remove(&project_path);
        let removed_keys: HashSet<String> = self
            .target_statuses
            .iter()
            .filter(|status| status.project_path == project_path)
            .map(|status| status.key.clone())
            .collect();
        self.target_statuses
            .retain(|status| status.project_path != project_path);
        self.running_sessions
            .retain(|session| !removed_keys.contains(&session.key));

        self.confirm_delete_project = false;
        self.rebuild_project_matcher();
        if self.filtered_projects.is_empty() {
            self.cursor = -1;
            self.targets.clear();
            self.filtered_targets.clear();
            self.target_cursor = -1;
            self.filtered_ci_runs.clear();
            self.ci_cursor = -1;
        } else {
            self.cursor = self
                .cursor
                .clamp(0, self.filtered_projects.len() as isize - 1);
            self.refresh_targets();
        }

        Ok(())
    }

    pub(super) fn mark_project_opened(&mut self, project_path: &Path) {
        let key = project_path.to_string_lossy().into_owned();
        self.recent_projects
            .entries
            .insert(key, unix_timestamp_now());
        let _ = save_recent_projects(&self.recent_projects);

        let selected_path = self.current_project().map(|project| project.path.clone());
        sort_projects(&mut self.projects, &self.recent_projects);
        self.rebuild_project_matcher();

        if let Some(selected_path) = selected_path {
            if let Some(index) = self.filtered_projects.iter().position(|&project_index| {
                self.projects
                    .get(project_index)
                    .is_some_and(|project| project.path == selected_path)
            }) {
                self.cursor = index as isize;
            }
        }
    }
}
pub(super) fn format_timestamp(timestamp: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp as i64, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| "—".to_string())
}

fn unix_timestamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn sort_projects(projects: &mut [Project], recent_projects: &RecentProjects) {
    projects.sort_by(|a, b| {
        let a_key = a.path.to_string_lossy();
        let b_key = b.path.to_string_lossy();
        let a_recent = recent_projects.entries.get(a_key.as_ref()).copied();
        let b_recent = recent_projects.entries.get(b_key.as_ref()).copied();

        b_recent
            .cmp(&a_recent)
            .then_with(|| match (a_recent, b_recent) {
                (None, None) => b.has_git_dir.cmp(&a.has_git_dir),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub(super) fn projects_root() -> PathBuf {
    env::var("PROJECTS_DIR")
        .map(PathBuf::from)
        .map(expand_tilde)
        .unwrap_or_else(|_| expand_tilde(PathBuf::from("~/code")))
}

pub(super) fn expand_tilde(path: PathBuf) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str == "~" || path_str.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(path_str.trim_start_matches("~/"));
        }
    }
    path
}

pub(super) fn discover_projects(root: &Path) -> Vec<Project> {
    let mut projects = Vec::new();
    let mut seen = HashSet::new();
    visit_dirs(root, &mut projects, &mut seen);
    projects
}

fn visit_dirs(dir: &Path, projects: &mut Vec<Project>, seen: &mut HashSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut has_git_dir = false;
    let mut has_cargo_toml = false;
    let mut child_dirs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                if name == ".git" {
                    has_git_dir = true;
                    continue;
                }

                if should_skip_project_scan_dir(name) {
                    continue;
                }
            }

            child_dirs.push(path);
            continue;
        }

        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            has_cargo_toml = true;
        }
    }

    let is_workspace = has_cargo_toml && is_workspace_root(dir);

    if (has_git_dir || has_cargo_toml) && seen.insert(dir.to_path_buf()) {
        if let Some(name) = dir.file_name().and_then(|name| name.to_str()) {
            projects.push(Project {
                name: name.to_string(),
                path: dir.to_path_buf(),
                has_git_dir,
                is_workspace,
            });
        }
    }

    if is_workspace {
        return;
    }

    for child in child_dirs {
        visit_dirs(&child, projects, seen);
    }
}

fn is_workspace_root(dir: &Path) -> bool {
    fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .is_some_and(|cargo_toml| cargo_toml.contains("[workspace]"))
}

fn should_skip_project_scan_dir(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".direnv"
            | ".devenv"
            | ".jj"
            | ".venv"
            | "venv"
            | "dist"
            | "build"
            | ".next"
            | ".turbo"
    )
}

pub(super) fn directory_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };

    if metadata.is_file() {
        return metadata.len();
    }

    if !metadata.is_dir() {
        return 0;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    entries
        .flatten()
        .map(|entry| directory_size(&entry.path()))
        .sum()
}
fn looks_like_git_url(value: &str) -> bool {
    value.starts_with("git@")
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.ends_with(".git")
        || value.contains("github.com/")
}

fn cloned_project_path(projects_root: &Path, value: &str) -> Option<PathBuf> {
    let repo = value
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()?
        .trim_end_matches(".git");

    if repo.is_empty() {
        None
    } else {
        Some(projects_root.join(repo))
    }
}

impl App {
    pub fn is_bookmarked(&self, project: &Project) -> bool {
        let key = project.path.to_string_lossy();
        self.bookmarked_projects.entries.contains(key.as_ref())
    }

    pub fn project_last_opened(&self, project: &Project) -> String {
        let key = project.path.to_string_lossy();
        self.recent_projects
            .entries
            .get(key.as_ref())
            .copied()
            .map(format_timestamp)
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn cached_project_size_bytes(&self, project: &Project) -> Option<u64> {
        self.size_cache.get(&project.path).copied()
    }
    pub fn open_selected_project(&mut self, no_focus: bool) -> color_eyre::Result<()> {
        let Some(project) = self.current_project() else {
            return Ok(());
        };

        // add test comment

        let project_path = project.path.clone();
        let project_name = shell_escape_arg(&project.name);
        // Create the tab directly so Zellij starts its initial terminal normally.
        let command = if no_focus {
            format!(
                "zellij action new-tab -n {project_name} --cwd {cwd} --no-focus",
                cwd = shell_escape_path(&project_path)
            )
        } else {
            format!(
                "zellij action new-tab -n {project_name} --cwd {cwd}",
                cwd = shell_escape_path(&project_path)
            )
        };

        Command::new("sh")
            .arg("-c")
            .arg(command)
            // Detach stdio so zellij's informational output does not get rendered
            // into the dashboard TUI when opening a project in a new tab.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        self.mark_project_opened(&project_path);
        Ok(())
    }
    pub fn current_project(&self) -> Option<&Project> {
        let visible_index = usize::try_from(self.cursor).ok()?;
        let project_index = *self.filtered_projects.get(visible_index)?;
        self.projects.get(project_index)
    }

    pub fn current_target(&self) -> Option<&Target> {
        let visible_index = usize::try_from(self.target_cursor).ok()?;
        let target_index = *self.filtered_targets.get(visible_index)?;
        self.targets.get(target_index)
    }

    pub fn current_target_description(&self) -> Option<&str> {
        if self.focus != Focus::Targets {
            return None;
        }
        self.current_target()?.description.as_deref()
    }

    pub fn targets_loading(&self) -> bool {
        self.current_project()
            .is_some_and(|project| self.loading_targets.contains(&project.path))
    }

    pub fn visible_projects(&self) -> impl Iterator<Item = &Project> {
        self.filtered_projects
            .iter()
            .filter_map(|&index| self.projects.get(index))
    }
}
