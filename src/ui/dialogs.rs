use super::{App, widgets::centered_rect};
use ratatui::{
    Frame,
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, BorderType, Clear, Paragraph},
};
pub(super) fn render_delete_dialog(frame: &mut Frame, app: &App) {
    let Some(project) = app.current_project() else {
        return;
    };

    let area = centered_rect(frame.area(), 56, 3);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("Delete {}? [y/n]", project.name))
            .alignment(Alignment::Center)
            .block(
                Block::bordered()
                    .title("Confirm Deletion")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );
}

pub(super) fn render_create_project_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(frame.area(), 72, 3);
    let width = area.width.saturating_sub(2) as usize;
    let scroll = app.create_project_input.visual_scroll(width);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(app.create_project_input.value())
            .alignment(Alignment::Left)
            .scroll((0, scroll as u16))
            .block(
                Block::bordered()
                    .title("Add Project: name OR git url")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );

    let cursor = app.create_project_input.visual_cursor().max(scroll) - scroll + 1;
    frame.set_cursor_position((area.x + cursor as u16, area.y + 1));
}

pub(super) fn render_rename_project_dialog(frame: &mut Frame, app: &App) {
    let area = centered_rect(frame.area(), 56, 3);
    let width = area.width.saturating_sub(2) as usize;
    let scroll = app.rename_project_input.visual_scroll(width);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(app.rename_project_input.value())
            .alignment(Alignment::Left)
            .scroll((0, scroll as u16))
            .block(
                Block::bordered()
                    .title("Rename Cargo Project")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );

    let cursor = app.rename_project_input.visual_cursor().max(scroll) - scroll + 1;
    frame.set_cursor_position((area.x + cursor as u16, area.y + 1));
}

pub(super) fn render_create_project_pending_dialog(frame: &mut Frame) {
    let area = centered_rect(frame.area(), 40, 3);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new("Creating project…")
            .alignment(Alignment::Center)
            .block(
                Block::bordered()
                    .title("Please wait")
                    .title_alignment(Alignment::Center)
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );
}

pub(super) fn render_help_dialog(frame: &mut Frame) {
    let area = centered_rect(frame.area(), 62, 19);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(
            "Navigation\n\
  h / ←, l / →    Move between panels\n\
  j / ↓, k / ↑    Move selection\n\
  g g / g e       First / last item\n\
\n\
Actions\n\
  /                Search projects or targets\n\
  %n/%p query      Search name/path; targets also support %k kind\n\
  Enter            Open project / run target / open CI run\n\
  O                Open project without switching to its tab\n\
  +                Open project in a Vibe tab\n\
  Space            Expand / minimize project worktrees\n\
  a, b, c, d, r    Add, bookmark, clean, delete, rename project\n\
  R                Refresh project discovery\n\
  Esc              Clear filters\n\
\n\
? / Esc            Close this help",
        )
        .block(
            Block::bordered()
                .title("Keyboard shortcuts")
                .title_alignment(Alignment::Center)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::White).bg(Color::Black)),
        area,
    );
}
