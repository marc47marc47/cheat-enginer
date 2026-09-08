use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::InputMode;
use crate::hex::HexModel;

/// Renders a [`HexModel`]. Holds only view state: whether the goto prompt is
/// open and what has been typed into it.
///
/// `draw` performs no I/O. It reports the row count it can show back to the
/// model and renders the bytes the model already has; `App::tick` does the
/// reading, on its own interval.
pub struct HexViewer {
    pub editing_address: bool,
    pub address_input: String,
    pub model: HexModel,
}

impl HexViewer {
    pub fn new() -> Self {
        Self {
            editing_address: false,
            address_input: String::new(),
            model: HexModel::new(),
        }
    }

    pub fn address(&self) -> usize {
        self.model.address()
    }

    pub fn set_address(&mut self, address: usize) {
        self.model.set_address(address);
    }

    pub fn scroll_up(&mut self) {
        self.model.scroll_up();
    }

    pub fn scroll_down(&mut self) {
        self.model.scroll_down();
    }

    pub fn page_up(&mut self) {
        self.model.page_up();
    }

    pub fn page_down(&mut self) {
        self.model.page_down();
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, input_mode: InputMode) {
        let prompt_open = self.editing_address && input_mode == InputMode::Editing;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if prompt_open { 3 } else { 0 }),
                Constraint::Min(3),
            ])
            .split(area);

        if prompt_open {
            let input = Paragraph::new(format!(" 0x{}", self.address_input))
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" [g]oto address (hex): "),
                );
            frame.render_widget(input, chunks[0]);
        }

        let hex_area = chunks[1];
        let visible_rows = (hex_area.height as usize).saturating_sub(2);
        // Layout still decides how much fits - it just records that instead of
        // reading memory here.
        self.model.set_rows(visible_rows);

        let bytes_per_row = self.model.bytes_per_row();
        let base = self.model.address();
        let mut lines = Vec::new();

        for row in 0..visible_rows {
            let offset = row * bytes_per_row;
            let addr = base + offset;
            let mut spans = vec![Span::styled(
                format!("0x{addr:016X}  "),
                Style::default().fg(Color::Cyan),
            )];

            for col in 0..bytes_per_row {
                let idx = offset + col;
                match self.model.byte_at(idx) {
                    Some(byte) => {
                        let style = if self.model.changed_at(idx) {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        spans.push(Span::styled(format!("{byte:02X} "), style));
                    }
                    None => spans.push(Span::styled("   ", Style::default())),
                }
                if col == 7 {
                    spans.push(Span::raw(" "));
                }
            }

            spans.push(Span::raw(" |"));

            for col in 0..bytes_per_row {
                let idx = offset + col;
                if let Some(byte) = self.model.byte_at(idx) {
                    let ch = if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    };
                    let style = if self.model.changed_at(idx) {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Green)
                    };
                    spans.push(Span::styled(format!("{ch}"), style));
                }
            }

            spans.push(Span::raw("|"));
            lines.push(Line::from(spans));
        }

        let hex_widget = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(
            format!(" Hex Viewer - 0x{base:016X} | [g]oto Up/Down:Scroll PgUp/PgDn "),
        ));
        frame.render_widget(hex_widget, hex_area);
    }
}

impl Default for HexViewer {
    fn default() -> Self {
        Self::new()
    }
}
