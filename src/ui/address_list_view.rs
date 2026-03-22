use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::layout::{Constraint, Direction, Layout};

use crate::address::AddressTable;
use super::app::InputMode;

pub struct AddressListView {
    pub selected: usize,
    pub editing_value: bool,
    pub editing_description: bool,
    pub editing_save_path: bool,
    pub editing_load_path: bool,
    pub edit_buffer: String,
}

impl AddressListView {
    pub fn new() -> Self {
        Self {
            selected: 0,
            editing_value: false,
            editing_description: false,
            editing_save_path: false,
            editing_load_path: false,
            edit_buffer: String::new(),
        }
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        table: &AddressTable,
        input_mode: InputMode,
        focused: bool,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // If editing, show input at bottom
        let (list_area, input_area) = if self.is_editing() && input_mode == InputMode::Editing {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };

        let items: Vec<ListItem> = table
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == self.selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let freeze_icon = if entry.frozen { "[F]" } else { "[ ]" };
                let val = entry
                    .current_value
                    .as_ref()
                    .map(|v| v.display_value())
                    .unwrap_or_else(|| "???".into());
                let desc = if entry.description.is_empty() {
                    "<no description>"
                } else {
                    &entry.description
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{freeze_icon} "),
                        Style::default().fg(if entry.frozen { Color::Red } else { Color::DarkGray }),
                    ),
                    Span::styled(
                        format!("0x{:016X} ", entry.address),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        format!("{:>12} ", entry.value_type.label()),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(format!("{:>12} ", val), Style::default().fg(Color::Green)),
                    Span::styled(desc.to_string(), Style::default().fg(Color::White)),
                ]))
                .style(style)
            })
            .collect();

        let title = format!(
            " Address Table ({}) | [f]reeze [e]dit [d]esc [S]ave [L]oad Del:Remove ",
            table.entries.len()
        );
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );
        frame.render_widget(list, list_area);

        if let Some(input_area) = input_area {
            let label = if self.editing_value {
                "Enter new value"
            } else if self.editing_description {
                "Enter description"
            } else if self.editing_save_path {
                "Save path"
            } else {
                "Load path"
            };
            let input = Paragraph::new(format!(" {}", self.edit_buffer))
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {label}: ")),
                );
            frame.render_widget(input, input_area);
        }
    }

    fn is_editing(&self) -> bool {
        self.editing_value || self.editing_description || self.editing_save_path || self.editing_load_path
    }
}
