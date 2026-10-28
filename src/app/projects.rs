use super::persistence::{RecentProjects, save_bookmarked_projects, save_recent_projects};
use super::{App, BackgroundResult, Focus, Target, shell_escape_arg, shell_escape_path};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
        if let Some(cancel) = &self.discovery_cancel {
            cancel.store(true, Ordering::Relaxed);
        }

        self.discovery_generation = self.discovery_generation.wrapping_add(1);
        self.discovering_projects = true;
        self.discovery_seen.clear();
        let generation = self.discovery_generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        let root = self.projects_root.clone();
        let recent_projects = self.recent_projects.clone();
        let tx = self.background_tx.clone();
        let completion_tx = tx.clone();

        // Runs filesystem traversal off the UI thread and publishes small batches.
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut batch = Vec::new();
                let mut emitted = 0usize;
                let completed = discover_projects_streaming(
                    &root,
                    &recent_projects,
                    &cancelled,
                    |project| {
                        batch.push(project);
                        emitted += 1;
                        if emitted == 1 || batch.len() >= 16 {
                            tx.send(BackgroundResult::ProjectsDiscovered {
                                generation,
                                projects: std::mem::take(&mut batch),
                            })
                            .is_ok()
                        } else {
                            true
                        }
                    },
                );
                if completed && !batch.is_empty() {
                    let _ = tx.send(BackgroundResult::ProjectsDiscovered {
                        generation,
                        projects: batch,
                    });
                }
                completed
            })
            .await;
            if matches!(result, Ok(true)) {
                let _ = completion_tx.send(BackgroundResult::ProjectDiscoveryFinished { generation });
            }
        });
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
    projects.sort_by(|a, b| compare_projects(a, b, recent_projects));
}

pub(super) fn compare_projects(
    a: &Project,
    b: &Project,
    recent_projects: &RecentProjects,
) -> std::cmp::Ordering {
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
        .then_with(|| a.path.cmp(&b.path))
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

#[cfg(test)]
pub(super) fn discover_projects(root: &Path) -> Vec<Project> {
    let cancelled = AtomicBool::new(false);
    let mut projects = Vec::new();
    let _ = discover_projects_streaming(
        root,
        &RecentProjects::default(),
        &cancelled,
        |project| {
            projects.push(project);
            true
        },
    );
    projects
}

// Streams validated recents before traversing the remaining project tree.
pub(super) fn discover_projects_streaming(
    root: &Path,
    recent_projects: &RecentProjects,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(Project) -> bool,
) -> bool {
    let mut emitted = HashSet::new();
    let mut recents: Vec<_> = recent_projects.entries.iter().collect();
    recents.sort_by(|(a_path, a_time), (b_path, b_time)| {
        b_time
            .cmp(a_time)
            .then_with(|| Path::new(a_path).cmp(Path::new(b_path)))
    });

    for (path, _) in recents {
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        let path = Path::new(path);
        let Some(project) = recent_project(root, path, cancelled) else {
            continue;
        };
        if emitted.insert(project.path.clone()) && !emit(project) {
            return false;
        }
    }

    let mut active_symlink_targets = fs::canonicalize(root).ok().into_iter().collect();
    visit_dirs_streaming(
        root,
        cancelled,
        &mut emitted,
        &mut active_symlink_targets,
        &mut emit,
    )
}

// Validates a persisted path with the same marker and boundary rules as scanning.
fn recent_project(root: &Path, path: &Path, cancelled: &AtomicBool) -> Option<Project> {
    if path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return None;
    }
    let root_scan = inspect_directory(root, false, cancelled)?;
    if path != root && root_scan.is_workspace {
        return None;
    }
    if path == root {
        return root_scan.project;
    }
    let relative = path.strip_prefix(root).ok()?;
    let mut ancestor = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        let name = name.to_str()?;
        if name == ".git" || should_skip_project_scan_dir(name) {
            return None;
        }
        ancestor.push(name);
        if inspect_directory(&ancestor, false, cancelled)?.is_workspace {
            return None;
        }
    }
    inspect_directory(path, false, cancelled)?.project
}

struct DirectoryScan {
    project: Option<Project>,
    is_workspace: bool,
    children: Vec<(PathBuf, bool)>,
}

// Inspects direct entries once so recent probing and tree walking agree.
fn inspect_directory(
    dir: &Path,
    collect_children: bool,
    cancelled: &AtomicBool,
) -> Option<DirectoryScan> {
    let entries = fs::read_dir(dir).ok()?;
    let mut has_git_dir = false;
    let mut has_cargo_toml = false;
    let mut children = Vec::new();

    for entry in entries.flatten() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let path = entry.path();
        let is_dir = file_type.is_dir() || (file_type.is_symlink() && path.is_dir());
        if is_dir {
            if name == ".git" {
                has_git_dir = true;
            } else if !name
                .to_str()
                .is_some_and(should_skip_project_scan_dir)
                && collect_children
            {
                children.push((path, file_type.is_symlink()));
            }
            continue;
        }
        if name == "Cargo.toml" {
            has_cargo_toml = true;
        }
    }

    let is_workspace = has_cargo_toml && is_workspace_root(dir);
    let project = (has_git_dir || has_cargo_toml)
        .then(|| {
            dir.file_name().and_then(|name| name.to_str()).map(|name| Project {
                name: name.to_owned(),
                path: dir.to_path_buf(),
                has_git_dir,
                is_workspace,
            })
        })
        .flatten();

    Some(DirectoryScan {
        project,
        is_workspace,
        children,
    })
}

// Workspace roots are project rows but terminate recursive discovery.
fn is_workspace_root(dir: &Path) -> bool {
    fs::read_to_string(dir.join("Cargo.toml"))
        .is_ok_and(|contents| contents.contains("[workspace]"))
}

// Keeps lexical aliases visible while preventing a symlink from recursing into an ancestor.
fn visit_dirs_streaming(
    dir: &Path,
    cancelled: &AtomicBool,
    emitted: &mut HashSet<PathBuf>,
    active_symlink_targets: &mut HashSet<PathBuf>,
    emit: &mut impl FnMut(Project) -> bool,
) -> bool {
    if cancelled.load(Ordering::Relaxed) {
        return false;
    }
    let Some(scan) = inspect_directory(dir, true, cancelled) else {
        return !cancelled.load(Ordering::Relaxed);
    };
    if let Some(project) = scan.project {
        if emitted.insert(project.path.clone()) && !emit(project) {
            return false;
        }
    }
    if scan.is_workspace {
        return true;
    }

    for (child, is_symlink) in scan.children {
        if cancelled.load(Ordering::Relaxed) {
            return false;
        }
        let canonical = if is_symlink {
            let Ok(canonical) = fs::canonicalize(&child) else {
                continue;
            };
            if !active_symlink_targets.insert(canonical.clone()) {
                continue;
            }
            Some(canonical)
        } else {
            None
        };
        let keep_scanning = visit_dirs_streaming(
            &child,
            cancelled,
            emitted,
            active_symlink_targets,
            emit,
        );
        if let Some(canonical) = canonical {
            active_symlink_targets.remove(&canonical);
        }
        if !keep_scanning {
            return false;
        }
    }
    true
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

        let project_path = project.path.clone();
        let project_name = shell_escape_arg(&project.name);
        let cwd = shell_escape_path(&project_path);
        let pane_focus = if no_focus { " --no-focus" } else { "" };
        // Start a fresh OMP session in a floating pane while preserving an interactive shell in the tab.
        let tab_command =
            format!("zellij action new-pane --floating{pane_focus} --cwd {cwd} -- omp; exec fish");
        let tab_options = if no_focus { " --no-focus" } else { "" };
        let command = format!(
            "zellij action new-tab -n {project_name} --cwd {cwd}{tab_options} -- sh -c {}",
            shell_escape_arg(&tab_command)
        );

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

    pub fn open_selected_vibe_tab(&self) -> color_eyre::Result<()> {
        let Some(project) = self.current_project() else {
            return Ok(());
        };

        let cwd = shell_escape_path(&project.path);
        let tab_name = shell_escape_arg(&format!("-{}", project.name));
        // Start the Vibe layout in a tab rooted at the selected project.
        let command = format!(
            "zellij action new-tab --layout ~/.config/zellij/layouts/vibe.kdl --cwd {cwd} --name {tab_name}"
        );

        Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
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
