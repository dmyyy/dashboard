mod details;
mod dialogs;
mod format;
mod projects;
mod running;
mod widgets;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, Wrap,
    },
};
use tui_term::widget::PseudoTerminal;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, CiRun, Focus, RunProfile, TargetStatusKind};
use dialogs::{
    render_create_project_dialog, render_create_project_pending_dialog, render_delete_dialog,
    render_help_dialog,
};
use format::{
    format_ci_time, format_count, format_duration, format_size, language_label_text, truncate,
};
use running::{render_running, render_running_terminal};
use widgets::{
    centered_rect, ci_scroll_offset, project_scroll_offset, render_search_input,
    target_scroll_offset,
};

use details::{render_ci_runs, render_languages, render_metadata};
use projects::{render_projects, render_targets};
const INACTIVE_COLOR: Color = Color::Indexed(0);
const ACTIVE_COLOR: Color = Color::Magenta;
const ACTIVE_TEXT_COLOR: Color = Color::Indexed(7);
const INACTIVE_TEXT_COLOR: Color = Color::Indexed(8);
const CURSOR_COLOR: Color = Color::Indexed(0);
const SELECTED_TEXT_COLOR: Color = Color::Indexed(7);

pub(super) fn pane_border_style(focused: bool) -> Style {
    let style = Style::default().fg(if focused {
        ACTIVE_COLOR
    } else {
        INACTIVE_COLOR
    });

    if focused {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let columns = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(34),
        Constraint::Percentage(33),
    ])
    .spacing(1);

    let [left, middle, right] = frame.area().layout(&columns);

    let has_running = app.running_target_statuses().next().is_some();
    let has_ci_runs = app.project_ci_runs().is_some();
    let has_languages = app.project_languages().is_some();

    match (has_running, has_ci_runs, has_languages) {
        (false, false, false) => {
            render_metadata(frame, app, left);
        }
        _ => {
            let section_count = 1 + has_running as u32 + has_ci_runs as u32 + has_languages as u32;
            let mut constraints = vec![Constraint::Ratio(1, section_count)];
            if has_running {
                constraints.push(Constraint::Ratio(1, section_count));
            }
            if has_ci_runs {
                constraints.push(Constraint::Ratio(1, section_count));
            }
            if has_languages {
                constraints.push(Constraint::Ratio(1, section_count));
            }

            let left_chunks = Layout::vertical(constraints).split(left);
            render_metadata(frame, app, left_chunks[0]);

            let mut index = 1;
            if has_running {
                render_running(frame, app, left_chunks[index]);
                index += 1;
            }
            if has_ci_runs {
                render_ci_runs(frame, app, left_chunks[index]);
                index += 1;
            }
            if has_languages {
                render_languages(frame, app, left_chunks[index]);
            }
        }
    }

    let show_terminal =
        app.focus == Focus::RunningTargets && app.current_running_target_status().is_some();

    let show_project_search = !show_terminal
        && (!app.project_query.is_empty() || (app.focus == Focus::Projects && app.filter_mode));
    let middle_chunks = Layout::vertical([
        Constraint::Length(if show_project_search { 3 } else { 0 }),
        Constraint::Min(0),
    ])
    .split(middle);
    if show_terminal {
        let terminal_area = Rect {
            x: middle.x,
            y: middle.y,
            width: middle.width.saturating_add(1).saturating_add(right.width),
            height: middle.height,
        };
        render_running_terminal(frame, app, terminal_area);
    } else {
        render_projects(frame, app, middle_chunks[1]);
    }
    if show_project_search {
        render_search_input(
            frame,
            middle_chunks[0],
            &app.project_input,
            app.filter_mode,
            "Search projects",
        );
    }

    if !show_terminal {
        let show_target_search =
            !app.target_query.is_empty() || (app.focus == Focus::Targets && app.filter_mode);
        let right_chunks = Layout::vertical([
            Constraint::Length(if show_target_search { 3 } else { 0 }),
            Constraint::Min(0),
        ])
        .split(right);
        render_targets(frame, app, right_chunks[1]);
        if show_target_search {
            render_search_input(
                frame,
                right_chunks[0],
                &app.target_input,
                app.filter_mode,
                "Search targets",
            );
        }
    }

    if app.confirm_delete_project {
        render_delete_dialog(frame, app);
    }

    if app.creating_project {
        render_create_project_dialog(frame, app);
    }

    if app.creating_project_in_background {
        render_create_project_pending_dialog(frame);
    }

    if app.show_help {
        render_help_dialog(frame);
    }
}

#[cfg(test)]
mod format_tests {
    use super::{language_label_text, truncate};

    #[test]
    fn refactor_format_language_padding() {
        assert_eq!(language_label_text("CMake"), "△  CMake");
        assert_eq!(language_label_text("Unknown"), "   Unknown");
    }

    #[test]
    fn refactor_format_unicode_truncation() {
        assert_eq!(truncate("aé界z", 3), "aé…");
        assert_eq!(truncate("aé界z", 4), "aé界z");
    }
}
