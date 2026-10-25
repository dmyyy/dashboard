use super::{App, Focus, projects::Project};
use nucleo::{
    Config, Nucleo, Utf32String,
    pattern::{CaseMatching, Normalization},
};
use std::sync::Arc;
use tui_input::InputRequest;

pub(super) const PROJECT_SEARCH_COLUMNS: usize = 3;
pub(super) const TARGET_SEARCH_COLUMNS: usize = 4;

impl App {
    pub fn select_next_project(&mut self) {
        if self.filtered_projects.is_empty() {
            return;
        }

        self.cursor = if self.cursor < 0 {
            0
        } else {
            (self.cursor + 1) % self.filtered_projects.len() as isize
        };
        self.refresh_targets();
    }

    pub fn go_to_top(&mut self) {
        match self.focus {
            Focus::Projects => {
                self.cursor = if self.filtered_projects.is_empty() {
                    -1
                } else {
                    0
                };
                self.refresh_targets();
            }
            Focus::RunningTargets => {
                self.running_cursor = if self.running_target_statuses().next().is_none() {
                    -1
                } else {
                    0
                };
            }
            Focus::CiRuns => {
                self.ci_cursor = if self.filtered_ci_runs.is_empty() {
                    -1
                } else {
                    0
                };
            }
            Focus::Targets => {
                self.target_cursor = if self.filtered_targets.is_empty() {
                    -1
                } else {
                    0
                };
            }
        }
    }

    pub fn go_to_bottom(&mut self) {
        match self.focus {
            Focus::Projects => {
                self.cursor = if self.filtered_projects.is_empty() {
                    -1
                } else {
                    self.filtered_projects.len() as isize - 1
                };
                self.refresh_targets();
            }
            Focus::RunningTargets => {
                let len = self.running_target_statuses().count();
                self.running_cursor = if len == 0 { -1 } else { len as isize - 1 };
            }
            Focus::CiRuns => {
                self.ci_cursor = if self.filtered_ci_runs.is_empty() {
                    -1
                } else {
                    self.filtered_ci_runs.len() as isize - 1
                };
            }
            Focus::Targets => {
                self.target_cursor = if self.filtered_targets.is_empty() {
                    -1
                } else {
                    self.filtered_targets.len() as isize - 1
                };
            }
        }
    }

    pub fn select_previous_project(&mut self) {
        if self.filtered_projects.is_empty() {
            return;
        }

        self.cursor = if self.cursor < 0 {
            self.filtered_projects.len() as isize - 1
        } else {
            (self.cursor - 1).rem_euclid(self.filtered_projects.len() as isize)
        };
        self.refresh_targets();
    }

    pub fn select_next_running_target(&mut self) {
        let len = self.running_target_statuses().count();
        if len == 0 {
            self.running_cursor = -1;
            return;
        }

        self.running_cursor = if self.running_cursor < 0 {
            0
        } else {
            (self.running_cursor + 1) % len as isize
        };
        self.sync_project_selection_to_running_target();
    }

    pub fn select_next_ci_run(&mut self) {
        if self.filtered_ci_runs.is_empty() {
            self.ci_cursor = -1;
            return;
        }

        self.ci_cursor = if self.ci_cursor < 0 {
            0
        } else {
            (self.ci_cursor + 1) % self.filtered_ci_runs.len() as isize
        };
    }

    pub fn select_next_target(&mut self) {
        if self.filtered_targets.is_empty() {
            return;
        }

        self.target_cursor = if self.target_cursor < 0 {
            0
        } else {
            (self.target_cursor + 1) % self.filtered_targets.len() as isize
        };
    }

    pub fn select_previous_running_target(&mut self) {
        let len = self.running_target_statuses().count();
        if len == 0 {
            self.running_cursor = -1;
            return;
        }

        self.running_cursor = if self.running_cursor < 0 {
            len as isize - 1
        } else {
            (self.running_cursor - 1).rem_euclid(len as isize)
        };
        self.sync_project_selection_to_running_target();
    }

    pub fn select_previous_ci_run(&mut self) {
        if self.filtered_ci_runs.is_empty() {
            self.ci_cursor = -1;
            return;
        }

        self.ci_cursor = if self.ci_cursor < 0 {
            self.filtered_ci_runs.len() as isize - 1
        } else {
            (self.ci_cursor - 1).rem_euclid(self.filtered_ci_runs.len() as isize)
        };
    }

    pub fn select_previous_target(&mut self) {
        if self.filtered_targets.is_empty() {
            return;
        }

        self.target_cursor = if self.target_cursor < 0 {
            self.filtered_targets.len() as isize - 1
        } else {
            (self.target_cursor - 1).rem_euclid(self.filtered_targets.len() as isize)
        };
    }
    pub(super) fn sync_project_selection_to_running_target(&mut self) {
        let Some(status) = self.current_running_target_status().cloned() else {
            return;
        };

        if let Some(index) = self.filtered_projects.iter().position(|&project_index| {
            self.projects
                .get(project_index)
                .is_some_and(|project| project.path == status.project_path)
        }) {
            self.cursor = index as isize;
            self.refresh_targets();
            if let Some(target_index) = self.filtered_targets.iter().position(|&target_index| {
                self.targets.get(target_index).is_some_and(|target| {
                    target.name == status.target_name && target.kind == status.target_kind
                })
            }) {
                self.target_cursor = target_index as isize;
            }
        }
    }

    pub(super) fn active_filter_is_non_empty(&self) -> bool {
        match self.focus {
            Focus::Projects => !self.project_query.is_empty(),
            Focus::RunningTargets => false,
            Focus::CiRuns => !self.ci_query.is_empty(),
            Focus::Targets => !self.target_query.is_empty(),
        }
    }

    pub(super) fn apply_filter_request(&mut self, request: InputRequest) {
        match self.focus {
            Focus::Projects => {
                self.project_input.handle(request);
                self.project_query = self.project_input.value().to_string();
                self.update_project_filter();
            }
            Focus::RunningTargets => {}
            Focus::CiRuns => {
                self.ci_input.handle(request);
                self.ci_query = self.ci_input.value().to_string();
                self.update_ci_filter();
            }
            Focus::Targets => {
                self.target_input.handle(request);
                self.target_query = self.target_input.value().to_string();
                self.update_target_filter();
            }
        }
    }

    pub(super) fn confirm_filter_mode(&mut self) {
        self.filter_mode = false;
        match self.focus {
            Focus::Projects => {
                self.cursor = if self.filtered_projects.is_empty() {
                    -1
                } else {
                    0
                };
                self.refresh_targets();
            }
            Focus::RunningTargets => {
                self.running_cursor = if self.running_target_statuses().next().is_none() {
                    -1
                } else {
                    0
                };
            }
            Focus::CiRuns => {
                self.ci_cursor = if self.filtered_ci_runs.is_empty() {
                    -1
                } else {
                    0
                };
            }
            Focus::Targets => {
                self.target_cursor = if self.filtered_targets.is_empty() {
                    -1
                } else {
                    0
                };
            }
        }
    }

    pub(super) fn cancel_filter_mode(&mut self) {
        self.filter_mode = false;
        match self.focus {
            Focus::Projects => {
                self.project_input.reset();
                self.project_query.clear();
                self.update_project_filter();
                self.cursor = if self.filtered_projects.is_empty() {
                    -1
                } else {
                    0
                };
                self.refresh_targets();
            }
            Focus::RunningTargets => {}
            Focus::CiRuns => {
                self.ci_input.reset();
                self.ci_query.clear();
                self.update_ci_filter();
            }
            Focus::Targets => {
                self.target_input.reset();
                self.target_query.clear();
                self.update_target_filter();
                self.target_cursor = if self.filtered_targets.is_empty() {
                    -1
                } else {
                    0
                };
                self.focus = Focus::Projects;
            }
        }
    }

    pub(super) fn clear_all_filters(&mut self) {
        self.project_input.reset();
        self.project_query.clear();
        self.update_project_filter();
        self.cursor = if self.filtered_projects.is_empty() {
            -1
        } else {
            0
        };

        self.ci_input.reset();
        self.ci_query.clear();
        self.update_ci_filter();

        self.target_input.reset();
        self.target_query.clear();
        self.update_target_filter();
        self.target_cursor = if self.filtered_targets.is_empty() {
            -1
        } else {
            0
        };
        self.ci_cursor = if self.filtered_ci_runs.is_empty() {
            -1
        } else {
            0
        };

        self.refresh_targets();
    }

    pub(super) fn refresh_targets(&mut self) {
        let Some(project) = self.current_project() else {
            self.targets.clear();
            self.filtered_targets.clear();
            self.target_cursor = -1;
            self.filtered_ci_runs.clear();
            self.ci_cursor = -1;
            return;
        };

        let path = project.path.clone();
        if let Some(targets) = self.target_cache.get(&path) {
            self.targets = targets.clone();
            self.rebuild_target_matcher();
        } else {
            self.targets.clear();
            self.filtered_targets.clear();
            self.target_cursor = -1;
            self.request_targets(path);
        }
        self.update_ci_filter();
    }
    pub(super) fn rebuild_project_matcher(&mut self) {
        self.project_matcher.restart(true);
        for project in self.projects.clone() {
            self.inject_project(project);
        }
        self.update_project_filter();
    }

    // Adds streamed projects without resetting pending fuzzy work.
    pub(super) fn inject_project(&self, project: Project) {
        self.project_matcher.injector().push(project, |project, cols| {
            cols[0] = Utf32String::from(
                format!("{} {}", project.name, project.path.display()).as_str(),
            );
            cols[1] = Utf32String::from(project.name.as_str());
            cols[2] = Utf32String::from(project.path.to_string_lossy().as_ref());
        });
    }

    pub(super) fn rebuild_target_matcher(&mut self) {
        self.target_matcher.restart(true);
        let injector = self.target_matcher.injector();
        for target in self.targets.clone() {
            injector.push(target, |target, cols| {
                cols[0] = Utf32String::from(
                    format!("{} {} {}", target.kind, target.name, target.path).as_str(),
                );
                cols[1] = Utf32String::from(target.kind.as_str());
                cols[2] = Utf32String::from(target.name.as_str());
                cols[3] = Utf32String::from(target.path.as_str());
            });
        }
        self.update_target_filter();
    }

    pub(super) fn update_project_filter(&mut self) {
        let queries = scoped_queries(&self.project_query, &["name", "path"]);
        for (column, query) in queries.iter().enumerate() {
            self.project_matcher.pattern.reparse(
                column,
                query,
                CaseMatching::Smart,
                Normalization::Smart,
                false,
            );
        }
        let _ = self.project_matcher.tick(10);
        self.sync_filtered_projects();
    }

    pub(super) fn update_ci_filter(&mut self) {
        let query = self.ci_query.to_ascii_lowercase();
        self.filtered_ci_runs = self
            .project_ci_runs()
            .map(|ci| {
                ci.runs
                    .iter()
                    .enumerate()
                    .filter(|(_, run)| {
                        query.is_empty()
                            || run.branch.to_ascii_lowercase().contains(&query)
                            || run.title.to_ascii_lowercase().contains(&query)
                            || run.status.to_ascii_lowercase().contains(&query)
                            || run.created_at.to_ascii_lowercase().contains(&query)
                    })
                    .map(|(index, _)| index)
                    .collect()
            })
            .unwrap_or_default();
        self.running_cursor = if self.running_target_statuses().next().is_none() {
            -1
        } else {
            self.running_cursor
                .clamp(0, self.running_target_statuses().count() as isize - 1)
        };
        self.ci_cursor = if self.filtered_ci_runs.is_empty() {
            -1
        } else {
            self.ci_cursor
                .clamp(0, self.filtered_ci_runs.len() as isize - 1)
        };
    }

    pub(super) fn update_target_filter(&mut self) {
        let queries = scoped_queries(&self.target_query, &["kind", "name", "path"]);
        for (column, query) in queries.iter().enumerate() {
            self.target_matcher.pattern.reparse(
                column,
                query,
                CaseMatching::Smart,
                Normalization::Smart,
                false,
            );
        }
        let _ = self.target_matcher.tick(10);
        self.sync_filtered_targets();
    }

    pub(super) fn sync_filtered_projects(&mut self) {
        let selected_path = self.current_project().map(|project| project.path.clone());
        if self.project_query.is_empty() {
            self.filtered_projects = (0..self.projects.len()).collect();
            self.filtered_projects.sort_by(|&a, &b| {
                super::projects::compare_projects(
                    &self.projects[a],
                    &self.projects[b],
                    &self.recent_projects,
                )
            });
        } else {
            self.filtered_projects = self
                .project_matcher
                .snapshot()
                .matched_items(..)
                .map(|item| item.data.clone())
                .filter_map(|project| {
                    self.projects
                        .iter()
                        .position(|candidate| candidate.path == project.path)
                })
                .collect();
        }

        if let Some(selected_path) = selected_path {
            self.cursor = self
                .filtered_projects
                .iter()
                .position(|&project_index| {
                    self.projects
                        .get(project_index)
                        .is_some_and(|project| project.path == selected_path)
                })
                .map_or(-1, |index| index as isize);
        } else {
            self.cursor = if self.filtered_projects.is_empty() { -1 } else { 0 };
        }
        if self.cursor < 0 && !self.filtered_projects.is_empty() {
            self.cursor = 0;
        }
        if self.filtered_projects.is_empty() {
            self.targets.clear();
            self.filtered_targets.clear();
            self.target_cursor = -1;
        }
    }

    pub(super) fn sync_filtered_targets(&mut self) {
        self.filtered_targets = self
            .target_matcher
            .snapshot()
            .matched_items(..)
            .map(|item| item.data.clone())
            .filter_map(|target| {
                self.targets.iter().position(|candidate| {
                    candidate.kind == target.kind
                        && candidate.name == target.name
                        && candidate.path == target.path
                })
            })
            .collect();

        self.target_cursor = if self.filtered_targets.is_empty() {
            -1
        } else {
            self.target_cursor
                .clamp(0, self.filtered_targets.len() as isize - 1)
        };
    }
}

pub(super) fn new_matcher<T: Sync + Send + 'static>(columns: usize) -> Nucleo<T> {
    Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), columns as u32)
}

fn scoped_queries(query: &str, columns: &[&str]) -> Vec<String> {
    let mut queries = vec![String::new(); columns.len() + 1];
    let mut active_column = 0;

    for token in query.split_whitespace() {
        if let Some(column) = token
            .strip_prefix('%')
            .and_then(|prefix| matching_column(prefix, columns))
        {
            active_column = column + 1;
        } else {
            if !queries[active_column].is_empty() {
                queries[active_column].push(' ');
            }
            queries[active_column].push_str(token);
        }
    }

    queries
}

fn matching_column(prefix: &str, columns: &[&str]) -> Option<usize> {
    if prefix.is_empty() {
        return None;
    }

    let mut matches = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.starts_with(prefix));
    let index = matches.next()?.0;
    matches.next().is_none().then_some(index)
}

#[cfg(test)]
mod tests {
    use crate::app::{Target, projects::Project, test_support::empty_app};
    use std::path::PathBuf;
    fn target(name: &str) -> Target {
        Target {
            package_name: "fixture".to_owned(),
            kind: "bin".to_owned(),
            name: name.to_owned(),
            path: format!("src/{name}.rs"),
            description: None,
            required_features: Vec::new(),
        }
    }

    #[test]
    fn refactor_input_navigation_boundaries() {
        let mut app = empty_app();
        app.targets = vec![target("first"), target("second")];
        app.filtered_targets = vec![1, 0];
        app.target_cursor = -1;

        app.select_next_target();
        assert_eq!(
            app.current_target().map(|target| target.name.as_str()),
            Some("second")
        );
        app.select_previous_target();
        assert_eq!(
            app.current_target().map(|target| target.name.as_str()),
            Some("first")
        );

        app.target_cursor = 7;
        app.filtered_targets.clear();
        app.select_next_target();
        assert_eq!(app.target_cursor, 7);

        app.ci_cursor = 7;
        app.filtered_ci_runs.clear();
        app.select_next_ci_run();
        assert_eq!(app.ci_cursor, -1);
    }

    #[test]
    fn filters_projects_and_targets_by_scoped_columns() {
        let mut app = empty_app();
        app.projects = vec![
            Project {
                name: "dashboard".to_owned(),
                path: PathBuf::from("/work/dashboard"),
                has_git_dir: true,
                is_workspace: false,
            },
            Project {
                name: "website".to_owned(),
                path: PathBuf::from("/work/site"),
                has_git_dir: true,
                is_workspace: false,
            },
        ];
        app.rebuild_project_matcher();
        app.project_query = "%p dashboard".to_owned();
        app.update_project_filter();
        assert_eq!(app.filtered_projects, vec![0]);

        app.targets = vec![
            Target {
                package_name: "fixture".to_owned(),
                kind: "bin".to_owned(),
                name: "dashboard".to_owned(),
                path: "src/main.rs".to_owned(),
                description: None,
                required_features: Vec::new(),
            },
            Target {
                package_name: "fixture".to_owned(),
                kind: "test".to_owned(),
                name: "dashboard".to_owned(),
                path: "tests/dashboard.rs".to_owned(),
                description: None,
                required_features: Vec::new(),
            },
        ];
        app.rebuild_target_matcher();
        app.target_query = "%k bin %n dashboard %p src".to_owned();
        app.update_target_filter();
        assert_eq!(app.filtered_targets, vec![0]);
    }
}
