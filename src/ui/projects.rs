use super::format::format_size;
use super::widgets::{project_scroll_offset, render_search_input, target_scroll_offset};
use super::{
    ACTIVE_COLOR, ACTIVE_TEXT_COLOR, App, CURSOR_COLOR, Focus, INACTIVE_COLOR, INACTIVE_TEXT_COLOR,
    SELECTED_TEXT_COLOR, pane_border_style,
};
use nucleo::{Config, Matcher, Utf32String, pattern::MultiPattern};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table,
    },
};
pub(super) fn render_projects(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Projects && !app.filter_mode;
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let visible_rows = inner.height.saturating_sub(1) as usize;
    let scroll_offset = project_scroll_offset(app, visible_rows);
    let title = if app.discovering_projects {
        "Projects (discovering…)"
    } else {
        "Projects"
    };

    if app.filtered_projects.is_empty() {
        let message = if app.discovering_projects {
            "Discovering projects…"
        } else {
            "No matching projects"
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(
                    Block::bordered()
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .border_type(BorderType::Rounded)
                        .border_style(pane_border_style(focused)),
                )
                .fg(if focused {
                    ACTIVE_COLOR
                } else {
                    INACTIVE_COLOR
                }),
            area,
        );
    } else {
        let visible_projects: Vec<_> = app
            .visible_projects()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_rows)
            .map(|(index, project)| (index, project.clone()))
            .collect();

        let rows: Vec<Row> = visible_projects
            .into_iter()
            .map(|(index, project)| {
                let is_selected = focused && app.cursor == index as isize;
                let base_style = if is_selected {
                    Style::default()
                        .fg(SELECTED_TEXT_COLOR)
                        .bg(CURSOR_COLOR)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(if focused {
                        ACTIVE_TEXT_COLOR
                    } else {
                        INACTIVE_TEXT_COLOR
                    })
                };
                let name_style = if is_selected {
                    base_style
                } else if focused {
                    base_style.fg(ACTIVE_TEXT_COLOR)
                } else {
                    base_style.fg(INACTIVE_TEXT_COLOR)
                };
                let cached_size = app.cached_project_size_bytes(&project);
                let size_text = cached_size
                    .map(format_size)
                    .unwrap_or_else(|| "…".to_string());
                let size_style = if is_selected {
                    base_style
                } else if cached_size.is_some_and(|size| size >= 5 * 1024 * 1024 * 1024) {
                    base_style.fg(Color::Indexed(9))
                } else if cached_size.is_some_and(|size| size >= 1024 * 1024 * 1024) {
                    base_style.fg(Color::Indexed(1))
                } else {
                    base_style
                };

                let last_opened = app.project_last_opened(&project);
                let bookmark = if app.is_bookmarked(&project) {
                    "🌟"
                } else {
                    ""
                };
                let matched = matched_lines(
                    &[project.name.clone(), project.path.display().to_string()],
                    &app.project_matcher.pattern,
                    base_style,
                );
                Row::new(vec![
                    Cell::from(bookmark),
                    Cell::from(matched[0].clone()).style(name_style),
                    Cell::from(size_text).style(size_style),
                    Cell::from(last_opened),
                ])
                .style(base_style)
            })
            .collect();

        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(2),
                    Constraint::Fill(1),
                    Constraint::Length(10),
                    Constraint::Length(12),
                ],
            )
            .header(
                Row::new(vec!["", "Name", "Size", "Last Opened"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::bordered()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded)
                    .border_style(pane_border_style(focused)),
            )
            .fg(if focused {
                ACTIVE_COLOR
            } else {
                INACTIVE_COLOR
            }),
            area,
        );
    }

    if !app.filtered_projects.is_empty() {
        let mut scrollbar_state =
            ScrollbarState::new(app.filtered_projects.len()).position(scroll_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

pub(super) fn render_targets(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Targets && !app.filter_mode;
    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    });
    let visible_rows = inner.height.saturating_sub(1) as usize;
    let target_offset = target_scroll_offset(app, visible_rows);

    if app.targets_loading() {
        frame.render_widget(
            Paragraph::new("Loading targets…")
                .block(
                    Block::bordered()
                        .title("Targets")
                        .title_alignment(Alignment::Center)
                        .border_type(BorderType::Rounded)
                        .border_style(pane_border_style(focused)),
                )
                .fg(if focused {
                    ACTIVE_COLOR
                } else {
                    INACTIVE_COLOR
                }),
            area,
        );
    } else if app.filtered_targets.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching targets")
                .block(
                    Block::bordered()
                        .title("Targets")
                        .title_alignment(Alignment::Center)
                        .border_type(BorderType::Rounded)
                        .border_style(pane_border_style(focused)),
                )
                .fg(if focused {
                    ACTIVE_COLOR
                } else {
                    INACTIVE_COLOR
                }),
            area,
        );
    } else {
        let visible_targets: Vec<_> = app
            .visible_targets()
            .enumerate()
            .skip(target_offset)
            .take(visible_rows)
            .map(|(index, target)| (index, target.clone()))
            .collect();

        let rows: Vec<Row> = visible_targets
            .into_iter()
            .map(|(index, target)| {
                let is_selected = focused && app.target_cursor == index as isize;
                let style = if is_selected {
                    Style::default()
                        .fg(SELECTED_TEXT_COLOR)
                        .bg(CURSOR_COLOR)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(if focused {
                        ACTIVE_TEXT_COLOR
                    } else {
                        INACTIVE_TEXT_COLOR
                    })
                };
                let grouped_path = if index > 0
                    && app
                        .targets
                        .get(index - 1)
                        .is_some_and(|prev| prev.path == target.path)
                {
                    String::new()
                } else {
                    target.path.clone()
                };
                let matched = matched_lines(
                    &[
                        target.kind.clone(),
                        target.name.clone(),
                        target.path.clone(),
                    ],
                    &app.target_matcher.pattern,
                    style,
                );
                Row::new(vec![
                    Cell::from(matched[1].clone()),
                    Cell::from(matched[0].clone()),
                    if grouped_path.is_empty() {
                        Cell::from(grouped_path)
                    } else {
                        Cell::from(matched[2].clone())
                    },
                ])
                .style(style)
            })
            .collect();

        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(20),
                    Constraint::Length(10),
                    Constraint::Fill(1),
                ],
            )
            .header(
                Row::new(vec!["Name", "Kind", "Path"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::bordered()
                    .title("Targets")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded)
                    .border_style(pane_border_style(focused)),
            )
            .fg(if focused {
                ACTIVE_COLOR
            } else {
                INACTIVE_COLOR
            }),
            area,
        );
    }

    if !app.filtered_targets.is_empty() {
        let mut scrollbar_state =
            ScrollbarState::new(app.filtered_targets.len()).position(target_offset);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn matched_lines(
    fields: &[String],
    pattern: &MultiPattern,
    base_style: Style,
) -> Vec<Line<'static>> {
    let joined = fields.join(" ");
    let haystack = Utf32String::from(joined.as_str());
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut matched = Vec::new();
    let _ = pattern
        .column_pattern(0)
        .indices(haystack.slice(..), &mut matcher, &mut matched);

    let mut offset = 0;
    let mut field_matches = Vec::new();
    for (column, field) in fields.iter().enumerate() {
        field_matches.clear();
        let haystack = Utf32String::from(field.as_str());
        let _ = pattern.column_pattern(column + 1).indices(
            haystack.slice(..),
            &mut matcher,
            &mut field_matches,
        );
        matched.extend(field_matches.iter().map(|index| *index + offset));
        offset += field.chars().count() as u32 + 1;
    }
    matched.sort_unstable();
    matched.dedup();

    let mut offset = 0;
    fields
        .iter()
        .map(|field| {
            let end = offset + field.chars().count();
            let line = Line::from(
                field
                    .chars()
                    .enumerate()
                    .map(|(index, character)| {
                        let style = if matched.binary_search(&((offset + index) as u32)).is_ok() {
                            base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            base_style
                        };
                        Span::styled(character.to_string(), style)
                    })
                    .collect::<Vec<_>>(),
            );
            offset = end + 1;

            line
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use nucleo::pattern::{CaseMatching, Normalization};

    #[test]
    fn highlights_fuzzy_matches_across_metadata_fields() {
        let mut pattern = MultiPattern::new(3);
        pattern.reparse(
            0,
            "main src",
            CaseMatching::Smart,
            Normalization::Smart,
            false,
        );

        let lines = matched_lines(
            &["main".to_owned(), "src/main.rs".to_owned()],
            &pattern,
            Style::default(),
        );

        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Yellow));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn highlights_scoped_column_matches() {
        let mut pattern = MultiPattern::new(3);
        pattern.reparse(2, "src", CaseMatching::Smart, Normalization::Smart, false);

        let lines = matched_lines(
            &["main".to_owned(), "src/main.rs".to_owned()],
            &pattern,
            Style::default(),
        );

        assert_eq!(lines[0].spans[0].style.fg, None);
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Yellow));
    }
}
