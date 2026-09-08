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
    pub scroll_offset: usize,
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
            scroll_offset: 0,
            editing_value: false,
            editing_description: false,
            editing_save_path: false,
            editing_load_path: false,
            edit_buffer: String::new(),
        }
    }

    /// Split of the pane into (list, optional edit input). Shared by `draw` and
    /// `ensure_visible` so the two cannot drift apart.
    fn areas(&self, area: Rect, input_mode: InputMode) -> (Rect, Option<Rect>) {
        if self.is_editing() && input_mode == InputMode::Editing {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        }
    }

    /// Scroll the list so the selected row stays on screen. Called from the
    /// draw path, which is the only place the pane height is known. Only the
    /// scroll offset is touched - the selection belongs to the key handlers.
    pub fn ensure_visible(&mut self, area: Rect, input_mode: InputMode, total: usize) {
        let rows = (self.areas(area, input_mode).0.height as usize).saturating_sub(2);
        if total == 0 || rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        // Keep the last page full after entries are deleted, otherwise the
        // pane renders half empty with rows still available above.
        self.scroll_offset = self.scroll_offset.min(total.saturating_sub(rows));
        if self.selected >= total {
            // Stale index - leave it to the key handlers rather than scrolling
            // to a row that is not there.
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if self.selected >= self.scroll_offset + rows {
            self.scroll_offset = self.selected - rows + 1;
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
        let (list_area, input_area) = self.areas(area, input_mode);

        // Only the visible window is rendered - without this the list is
        // silently truncated at the bottom and the cursor walks off screen.
        let total = table.entries.len();
        let max_display = (list_area.height as usize).saturating_sub(2);
        let start = self.scroll_offset.min(total);
        let end = (start + max_display).min(total);

        let items: Vec<ListItem> = table.entries[start..end]
            .iter()
            .enumerate()
            .map(|(offset, entry)| {
                let i = start + offset;
                let style = if i == self.selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let (freeze_icon, freeze_color) = match (entry.frozen, entry.freeze_error) {
                    (true, true) => ("[!]", Color::Yellow),
                    (true, false) => ("[F]", Color::Red),
                    _ => ("[ ]", Color::DarkGray),
                };
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
                        Style::default().fg(freeze_color),
                    ),
                    // Compact columns - the pane is only half the terminal wide.
                    Span::styled(
                        format!("0x{:X} ", entry.address),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        format!("{:<4} ", entry.value_type.short_label()),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(format!("{:>10} ", val), Style::default().fg(Color::Green)),
                    Span::styled(desc.to_string(), Style::default().fg(Color::White)),
                ]))
                .style(style)
            })
            .collect();

        let freeze_errors = table.freeze_error_count();
        let error_note = if freeze_errors > 0 {
            format!(" [!] {freeze_errors} freeze fail")
        } else {
            String::new()
        };
        let position = if total > max_display && total > 0 {
            format!(" ↕{}/{}", self.selected + 1, total)
        } else {
            String::new()
        };
        // Kept short - the pane is only half the terminal wide and an
        // over-long block title is clipped, not wrapped. Key hints live in the
        // status bar instead.
        let title = format!(" Addresses ({total}){position}{error_note} ");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::AddressEntry;
    use crate::scan::value_type::ValueType;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// 12 rows minus 2 borders = 10 visible rows when not editing.
    fn pane() -> Rect {
        Rect::new(0, 0, 80, 12)
    }

    fn table_with(n: usize) -> AddressTable {
        let mut table = AddressTable::new();
        for i in 0..n {
            table.add(AddressEntry::new(
                0x1000 + i * 0x10,
                ValueType::U32,
                format!("entry{i}"),
            ));
        }
        table
    }

    #[test]
    fn test_scroll_follows_cursor_down() {
        let mut view = AddressListView::new();

        view.selected = 9;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 0);

        view.selected = 10;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 1);

        view.selected = 29;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 20);
    }

    #[test]
    fn test_scroll_follows_cursor_up() {
        let mut view = AddressListView::new();
        view.selected = 29;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 20);

        view.selected = 5;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 5);
    }

    #[test]
    fn test_edit_input_shrinks_the_window() {
        let mut view = AddressListView::new();
        view.editing_value = true;
        view.selected = 9;
        // The edit box takes 3 rows, leaving 12 - 3 - 2 = 7 visible.
        view.ensure_visible(pane(), InputMode::Editing, 30);
        assert_eq!(view.scroll_offset, 3);
    }

    #[test]
    fn test_ensure_visible_does_not_move_selection() {
        let mut view = AddressListView::new();
        view.selected = 40;
        view.ensure_visible(pane(), InputMode::Normal, 3);
        assert_eq!(view.selected, 40);
    }

    #[test]
    fn test_last_page_stays_full_after_deletions() {
        let mut view = AddressListView::new();
        view.selected = 29;
        view.ensure_visible(pane(), InputMode::Normal, 30);
        assert_eq!(view.scroll_offset, 20);

        // Ten entries deleted while parked at the bottom - the window must
        // slide back so all 10 rows stay filled instead of showing one row.
        view.selected = 19;
        view.ensure_visible(pane(), InputMode::Normal, 20);
        assert_eq!(view.scroll_offset, 10);
    }

    /// End-to-end: render past the bottom of the pane and confirm the window
    /// actually moved - the row under the cursor must be on screen and the
    /// first rows must have scrolled off.
    #[test]
    fn test_selected_row_is_rendered_after_scrolling() {
        let table = table_with(30);
        let mut view = AddressListView::new();
        view.selected = 25;
        view.ensure_visible(pane(), InputMode::Normal, table.entries.len());

        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|f| view.draw(f, pane(), &table, InputMode::Normal, true))
            .unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(rendered.contains("entry25"), "cursor row must be visible");
        assert!(!rendered.contains("entry0 "), "top rows must scroll off");
        assert!(rendered.contains("26/30"), "title shows cursor position");
    }
}
