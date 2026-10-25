use super::{App, Focus};
use crate::event::AppEvent;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::InputRequest;

impl App {
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        if self.confirm_delete_project {
            match key_event.code {
                KeyCode::Char('y') => self.delete_selected_project()?,
                KeyCode::Char('n') | KeyCode::Esc => self.confirm_delete_project = false,
                _ => {}
            }
            return Ok(());
        }

        if self.creating_project {
            match key_event.code {
                KeyCode::Esc => self.cancel_create_project(),
                KeyCode::Enter => self.confirm_create_project()?,
                KeyCode::Backspace
                | KeyCode::Char(_)
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Delete => self.handle_create_project_input(key_event),
                _ => {}
            }
            return Ok(());
        }

        if self.show_help {
            if matches!(key_event.code, KeyCode::Char('?') | KeyCode::Esc) {
                self.show_help = false;
            }
            return Ok(());
        }

        if self.filter_mode {
            match key_event.code {
                KeyCode::Esc => self.cancel_filter_mode(),
                KeyCode::Enter => self.confirm_filter_mode(),
                KeyCode::Backspace
                | KeyCode::Char(_)
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Delete => self.handle_filter_input(key_event),
                _ => {}
            }
            return Ok(());
        }

        if self.pending_g {
            self.pending_g = false;
            match key_event.code {
                KeyCode::Char('g') => {
                    self.go_to_top();
                    return Ok(());
                }
                KeyCode::Char('e') => {
                    self.go_to_bottom();
                    return Ok(());
                }
                _ => {}
            }
        }

        if key_event.code == KeyCode::Backspace && self.active_filter_is_non_empty() {
            self.handle_filter_backspace();
            return Ok(());
        }

        match key_event.code {
            KeyCode::Esc if self.active_filter_is_non_empty() => self.filter_mode = true,
            KeyCode::Esc => {
                self.pending_g = false;
                self.clear_all_filters()
            }
            KeyCode::Char('q') if self.focus == Focus::RunningTargets => {
                self.close_selected_running_target()?
            }
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('a') if self.focus == Focus::Projects => self.start_create_project(),
            KeyCode::Char('b') if key_event.modifiers.is_empty() => {
                self.toggle_selected_project_bookmark()
            }
            KeyCode::Char('c')
                if self.focus == Focus::Projects && key_event.modifiers.is_empty() =>
            {
                self.clean_selected_project()
            }
            KeyCode::Char('d') if self.focus == Focus::Projects => {
                if self.current_project().is_some() {
                    self.confirm_delete_project = true;
                }
            }
            KeyCode::Char('r' | 'R') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.rescan_selected_project()
            }
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('/') => match self.focus {
                Focus::Projects => {
                    self.filter_mode = true;
                    self.cursor = if self.filtered_projects.is_empty() {
                        -1
                    } else {
                        0
                    };
                    self.refresh_targets();
                }
                Focus::RunningTargets | Focus::CiRuns => {}
                Focus::Targets => {
                    self.filter_mode = true;
                    self.target_cursor = if self.filtered_targets.is_empty() {
                        -1
                    } else {
                        0
                    };
                }
            },
            KeyCode::Right | KeyCode::Char('l') => {
                self.focus = match self.focus {
                    Focus::CiRuns => {
                        if self.running_target_statuses().next().is_some() {
                            if self.running_cursor < 0 {
                                self.running_cursor = 0;
                            }
                            Focus::RunningTargets
                        } else {
                            Focus::Projects
                        }
                    }
                    Focus::RunningTargets => Focus::Projects,
                    Focus::Projects => Focus::Targets,
                    Focus::Targets => Focus::Targets,
                };
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = match self.focus {
                    Focus::Projects if self.running_target_statuses().next().is_some() => {
                        if self.running_cursor < 0 {
                            self.running_cursor = 0;
                        }
                        Focus::RunningTargets
                    }
                    Focus::Projects if self.project_ci_runs().is_some() => {
                        if self.ci_cursor < 0 {
                            self.ci_cursor = 0;
                        }
                        Focus::CiRuns
                    }
                    Focus::Projects => Focus::Projects,
                    Focus::RunningTargets if self.project_ci_runs().is_some() => {
                        if self.ci_cursor < 0 {
                            self.ci_cursor = 0;
                        }
                        Focus::CiRuns
                    }
                    Focus::RunningTargets => Focus::RunningTargets,
                    Focus::CiRuns => Focus::CiRuns,
                    Focus::Targets => Focus::Projects,
                };
            }
            KeyCode::Char('J') | KeyCode::Down if self.focus == Focus::RunningTargets => {
                if self.project_ci_runs().is_some() {
                    if self.ci_cursor < 0 {
                        self.ci_cursor = 0;
                    }
                    self.focus = Focus::CiRuns;
                } else {
                    self.select_next_running_target();
                }
            }
            KeyCode::Char('K') | KeyCode::Up if self.focus == Focus::CiRuns => {
                if self.running_target_statuses().next().is_some() {
                    if self.running_cursor < 0 {
                        self.running_cursor = 0;
                    }
                    self.focus = Focus::RunningTargets;
                } else {
                    self.select_previous_ci_run();
                }
            }
            KeyCode::Char('j') => match self.focus {
                Focus::Projects => self.select_next_project(),
                Focus::RunningTargets => self.select_next_running_target(),
                Focus::CiRuns => self.select_next_ci_run(),
                Focus::Targets => self.select_next_target(),
            },
            KeyCode::Char('k') => match self.focus {
                Focus::Projects => self.select_previous_project(),
                Focus::RunningTargets => self.select_previous_running_target(),
                Focus::CiRuns => self.select_previous_ci_run(),
                Focus::Targets => self.select_previous_target(),
            },
            KeyCode::Down => match self.focus {
                Focus::Projects => self.select_next_project(),
                Focus::RunningTargets => self.select_next_running_target(),
                Focus::CiRuns => self.select_next_ci_run(),
                Focus::Targets => self.select_next_target(),
            },
            KeyCode::Up => match self.focus {
                Focus::Projects => self.select_previous_project(),
                Focus::RunningTargets => self.select_previous_running_target(),
                Focus::CiRuns => self.select_previous_ci_run(),
                Focus::Targets => self.select_previous_target(),
            },
            KeyCode::Enter => match self.focus {
                Focus::Projects => self.open_selected_project(false)?,
                Focus::RunningTargets => self.rerun_selected_running_target()?,
                Focus::CiRuns => self.open_selected_ci_run()?,
                Focus::Targets => self.run_selected_target()?,
            },
            KeyCode::Char('O') if self.focus == Focus::Projects => {
                self.open_selected_project(true)?
            }
            KeyCode::Char('e') if self.focus == Focus::Targets => self.edit_selected_target()?,
            _ => {}
        }
        if self.focus == Focus::RunningTargets {
            self.sync_project_selection_to_running_target();
        }
        Ok(())
    }
    pub(super) fn handle_filter_input(&mut self, key_event: KeyEvent) {
        let request = match key_event.code {
            KeyCode::Backspace if key_event.modifiers == KeyModifiers::ALT => {
                Some(InputRequest::DeletePrevWord)
            }
            KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
            KeyCode::Left => Some(InputRequest::GoToPrevChar),
            KeyCode::Right => Some(InputRequest::GoToNextChar),
            KeyCode::Home => Some(InputRequest::GoToStart),
            KeyCode::End => Some(InputRequest::GoToEnd),
            KeyCode::Delete => Some(InputRequest::DeleteNextChar),
            KeyCode::Char(c)
                if key_event.modifiers.is_empty() || key_event.modifiers == KeyModifiers::SHIFT =>
            {
                Some(InputRequest::InsertChar(c))
            }
            _ => None,
        };

        let Some(request) = request else {
            return;
        };

        self.apply_filter_request(request);
    }

    fn handle_filter_backspace(&mut self) {
        self.apply_filter_request(InputRequest::DeletePrevChar);
    }

    pub(super) fn handle_create_project_input(&mut self, key_event: KeyEvent) {
        let request = match key_event.code {
            KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
            KeyCode::Left => Some(InputRequest::GoToPrevChar),
            KeyCode::Right => Some(InputRequest::GoToNextChar),
            KeyCode::Home => Some(InputRequest::GoToStart),
            KeyCode::End => Some(InputRequest::GoToEnd),
            KeyCode::Delete => Some(InputRequest::DeleteNextChar),
            KeyCode::Char(c)
                if key_event.modifiers.is_empty() || key_event.modifiers == KeyModifiers::SHIFT =>
            {
                Some(InputRequest::InsertChar(c))
            }
            _ => None,
        };

        if let Some(request) = request {
            self.create_project_input.handle(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Focus;
    use crate::app::test_support::empty_app;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn refactor_input_editing_parity() {
        for focus in [Focus::Projects, Focus::Targets, Focus::CiRuns] {
            let mut app = empty_app();
            app.focus = focus;

            for key_event in [
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
                KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT),
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            ] {
                app.handle_filter_input(key_event);
            }

            let (input, query) = match focus {
                Focus::Projects => (&app.project_input, &app.project_query),
                Focus::Targets => (&app.target_input, &app.target_query),
                Focus::CiRuns => (&app.ci_input, &app.ci_query),
                Focus::RunningTargets => unreachable!(),
            };
            assert_eq!(input.value(), "Za");
            assert_eq!(query, "Za");
        }

        let mut app = empty_app();
        for key_event in [
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        ] {
            app.handle_create_project_input(key_event);
        }
        assert_eq!(app.create_project_input.value(), "Za");
    }

    #[test]
    fn alt_backspace_deletes_previous_search_word() {
        let mut app = empty_app();
        app.filter_mode = true;

        for c in "first second".chars() {
            app.handle_key_events(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key_events(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT))
            .unwrap();

        assert_eq!(app.project_input.value(), "first ");
        assert_eq!(app.project_query, "first ");
    }

    #[test]
    fn escape_edits_then_clears_target_and_project_queries_in_order() {
        let mut app = empty_app();
        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);

        app.filter_mode = true;
        for c in "project".chars() {
            app.handle_key_events(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key_events(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        app.focus = Focus::Targets;
        app.filter_mode = true;
        for c in "target".chars() {
            app.handle_key_events(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
                .unwrap();
        }
        app.handle_key_events(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .unwrap();

        app.handle_key_events(escape).unwrap();
        assert!(app.filter_mode);
        assert_eq!(app.focus, Focus::Targets);
        assert_eq!(app.target_query, "target");

        app.handle_key_events(escape).unwrap();
        assert!(!app.filter_mode);
        assert_eq!(app.focus, Focus::Projects);
        assert!(app.target_query.is_empty());
        assert_eq!(app.project_query, "project");

        app.handle_key_events(escape).unwrap();
        assert!(app.filter_mode);
        assert_eq!(app.focus, Focus::Projects);
        assert_eq!(app.project_query, "project");

        app.handle_key_events(escape).unwrap();
        assert!(!app.filter_mode);
        assert!(app.project_query.is_empty());
    }
}
