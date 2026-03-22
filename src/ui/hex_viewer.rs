use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::platform::ProcessHandle;
use super::app::InputMode;

pub struct HexViewer {
    pub address: usize,
    pub editing_address: bool,
    pub address_input: String,
    previous_data: Vec<u8>,
    current_data: Vec<u8>,
    bytes_per_row: usize,
}

impl HexViewer {
    pub fn new() -> Self {
        Self {
            address: 0,
            editing_address: false,
            address_input: String::new(),
            previous_data: Vec::new(),
            current_data: Vec::new(),
            bytes_per_row: 16,
        }
    }

    pub fn scroll_up(&mut self) {
        self.address = self.address.saturating_sub(self.bytes_per_row);
    }

    pub fn scroll_down(&mut self) {
        self.address = self.address.saturating_add(self.bytes_per_row);
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        handle: Option<&dyn ProcessHandle>,
        input_mode: InputMode,
    ) {
        let chunks = if self.editing_address && input_mode == InputMode::Editing {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(3)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(0), Constraint::Min(3)])
                .split(area)
        };

        if self.editing_address && input_mode == InputMode::Editing {
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
        let total_bytes = visible_rows * self.bytes_per_row;

        // Read memory
        if let Some(handle) = handle {
            self.previous_data = std::mem::take(&mut self.current_data);
            self.current_data = handle
                .read_memory(self.address, total_bytes)
                .unwrap_or_else(|_| vec![0; total_bytes]);
            if self.current_data.len() < total_bytes {
                self.current_data.resize(total_bytes, 0);
            }
        }

        let mut lines = Vec::new();
        for row in 0..visible_rows {
            let offset = row * self.bytes_per_row;
            let addr = self.address + offset;
            let mut spans = vec![Span::styled(
                format!("0x{addr:016X}  "),
                Style::default().fg(Color::Cyan),
            )];

            // Hex bytes
            for col in 0..self.bytes_per_row {
                let idx = offset + col;
                if idx < self.current_data.len() {
                    let byte = self.current_data[idx];
                    let changed = self.previous_data.len() > idx && self.previous_data[idx] != byte;
                    let style = if changed {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    spans.push(Span::styled(format!("{byte:02X} "), style));
                } else {
                    spans.push(Span::styled("   ", Style::default()));
                }
                if col == 7 {
                    spans.push(Span::raw(" "));
                }
            }

            spans.push(Span::raw(" |"));

            // ASCII
            for col in 0..self.bytes_per_row {
                let idx = offset + col;
                if idx < self.current_data.len() {
                    let byte = self.current_data[idx];
                    let ch = if byte.is_ascii_graphic() || byte == b' ' {
                        byte as char
                    } else {
                        '.'
                    };
                    let changed = self.previous_data.len() > idx && self.previous_data[idx] != byte;
                    let style = if changed {
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

        let hex_widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " Hex Viewer - 0x{:016X} | [g]oto Up/Down:Scroll PgUp/PgDn ",
                    self.address
                )),
        );
        frame.render_widget(hex_widget, hex_area);
    }
}
