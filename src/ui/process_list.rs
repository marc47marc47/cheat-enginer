use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::layout::{Constraint, Direction, Layout};

use crate::platform::ProcessInfo;
use super::app::InputMode;

pub struct ProcessListView {
    pub selected: usize,
    pub filter_input: String,
    pub editing_filter: bool,
    pub scroll_offset: usize,
}

impl ProcessListView {
    pub fn new() -> Self {
        Self {
            selected: 0,
            filter_input: String::new(),
            editing_filter: false,
            scroll_offset: 0,
        }
    }

    /// Ensure selected item is visible, adjust scroll_offset
    pub fn ensure_visible(&mut self, visible_rows: usize, total: usize) {
        if total == 0 || visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected - visible_rows + 1;
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect, processes: &[ProcessInfo], _input_mode: InputMode) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(area);

        // Search bar
        let filter_display = if self.filter_input.is_empty() {
            Span::styled("Type to search...", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(&self.filter_input, Style::default().fg(Color::Yellow))
        };
        let cursor = Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK));

        let filter = Paragraph::new(Line::from(vec![
            Span::raw(" "),
            filter_display,
            cursor,
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Search ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(filter, chunks[0]);

        // Process list - two columns: [PID  Name] [Window Title]
        let list_area = chunks[1];
        let inner_height = list_area.height.saturating_sub(2) as usize; // minus borders
        let total = processes.len();

        // Build lines with scroll
        let start = self.scroll_offset;
        let end = (start + inner_height).min(total);

        let col_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(list_area);

        // Left column: PID + Name
        let mut left_lines: Vec<Line> = Vec::new();
        // Right column: Window Title
        let mut right_lines: Vec<Line> = Vec::new();

        for i in start..end {
            let p = &processes[i];
            let is_selected = i == self.selected;
            let base_style = if is_selected {
                Style::default().bg(Color::Rgb(50, 50, 60)).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            // Left: PID + name
            let left = Line::from(vec![
                Span::styled(format!(" {:>7} ", p.pid), base_style.fg(Color::Cyan)),
                Span::styled(
                    truncate_str(&p.name, col_chunks[0].width.saturating_sub(12) as usize),
                    base_style.fg(Color::White),
                ),
            ]);
            left_lines.push(left);

            // Right: window title
            let title_text = p.window_title.as_deref().unwrap_or("");
            let max_title_width = col_chunks[1].width.saturating_sub(3) as usize;
            let right = Line::from(vec![
                Span::styled(
                    format!(" {}", truncate_str(title_text, max_title_width)),
                    if title_text.is_empty() {
                        base_style.fg(Color::DarkGray)
                    } else {
                        base_style.fg(Color::Green)
                    },
                ),
            ]);
            right_lines.push(right);
        }

        // Scrollbar indicator in title
        let scroll_info = if total > inner_height {
            let pct = if total <= 1 { 100 } else { (self.scroll_offset * 100) / (total - 1).max(1) };
            format!(" {}/{} ({pct}%) ", end, total)
        } else {
            format!(" {} ", total)
        };

        let left_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" PID  |  Process{scroll_info}"));
        let left_widget = Paragraph::new(left_lines).block(left_block);
        frame.render_widget(left_widget, col_chunks[0]);

        let right_block = Block::default()
            .borders(Borders::ALL)
            .title(" Window Title ");
        let right_widget = Paragraph::new(right_lines).block(right_block);
        frame.render_widget(right_widget, col_chunks[1]);
    }
}

fn truncate_str(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if max_width == 0 {
        return String::new();
    }
    let mut width = 0;
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    let ellipsis_width = 3; // "..."

    // First check if the full string fits
    let total_width: usize = s.chars().map(|c| c.width().unwrap_or(1)).sum();
    if total_width <= max_width {
        return s.to_string();
    }

    if max_width <= ellipsis_width {
        // Too narrow for ellipsis, just take what fits
        for c in s.chars() {
            let cw = c.width().unwrap_or(1);
            if width + cw > max_width {
                break;
            }
            result.push(c);
            width += cw;
        }
        return result;
    }

    let target = max_width - ellipsis_width;
    for c in s.chars() {
        let cw = c.width().unwrap_or(1);
        if width + cw > target {
            break;
        }
        result.push(c);
        width += cw;
    }
    result.push_str("...");
    result
}
