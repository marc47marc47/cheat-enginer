use std::sync::Arc;
use std::sync::atomic::Ordering;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph};

use crate::scan::scanner::{ScanProgress, Scanner};
use crate::scan::value_type::{ScanType, ValueType};
use super::app::{AUTO_SCAN_INTERVAL, InputMode};

pub struct ScannerView {
    pub value_input: String,
    pub editing_value: bool,
    pub result_selected: usize,
    pub result_scroll: usize,
    /// Re-run the current scan every `AUTO_SCAN_INTERVAL`. Pairs with the
    /// Unchanged scan mode to repeatedly drop anything that moved.
    pub auto_scan: bool,
    value_type_index: usize,
    scan_type_index: usize,
}

impl ScannerView {
    pub fn new() -> Self {
        Self {
            value_input: String::new(),
            editing_value: false,
            result_selected: 0,
            result_scroll: 0,
            auto_scan: false,
            value_type_index: 2, // default U32
            scan_type_index: 0,
        }
    }

    pub fn value_type(&self) -> ValueType {
        ValueType::ALL[self.value_type_index]
    }

    pub fn scan_type(&self) -> ScanType {
        ScanType::ALL[self.scan_type_index]
    }

    pub fn cycle_value_type(&mut self) {
        self.value_type_index = (self.value_type_index + 1) % ValueType::ALL.len();
    }

    pub fn cycle_scan_type(&mut self) {
        self.scan_type_index = (self.scan_type_index + 1) % ScanType::ALL.len();
    }

    /// Vertical split of the scanner pane: two fixed control rows, then
    /// results. Shared by `draw` and `ensure_visible` so the two cannot drift
    /// apart. The controls are stacked rather than side by side because the
    /// pane is only half the terminal wide.
    fn layout(area: Rect) -> std::rc::Rc<[Rect]> {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(3)])
            .split(area)
    }

    /// Number of result rows that fit in the pane (inside the borders).
    fn visible_rows(area: Rect) -> usize {
        (Self::layout(area)[2].height as usize).saturating_sub(2)
    }

    /// Scroll the results window so the selected row stays on screen. Called
    /// from the draw path, which is the only place the pane height is known.
    /// Only the scroll offset is touched - the selection belongs to the key
    /// handlers.
    pub fn ensure_visible(&mut self, area: Rect, total: usize) {
        let rows = Self::visible_rows(area);
        if total == 0 || rows == 0 {
            self.result_scroll = 0;
            return;
        }
        // Keep the last page full when the result set shrinks, otherwise the
        // pane renders half empty with rows still available above.
        self.result_scroll = self.result_scroll.min(total.saturating_sub(rows));
        if self.result_selected >= total {
            // Stale index - leave it to the key handlers rather than scrolling
            // to a row that is not there.
            return;
        }
        if self.result_selected < self.result_scroll {
            self.result_scroll = self.result_selected;
        }
        if self.result_selected >= self.result_scroll + rows {
            self.result_scroll = self.result_selected - rows + 1;
        }
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        scanner: &Scanner,
        input_mode: InputMode,
        focused: bool,
        scanning: bool,
        spinner: &str,
        elapsed: &str,
        progress: Option<&Arc<ScanProgress>>,
    ) {
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let chunks = Self::layout(area);

        // Row 1: value type + scan mode. "2 Bytes (u16)" needs 13 columns and
        // "Unknown Initial" needs 15, plus borders.
        let controls_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(16), Constraint::Min(18)])
            .split(chunks[0]);

        let type_label = self.value_type().label();
        let type_widget = Paragraph::new(format!(" {type_label}"))
            .block(Block::default().borders(Borders::ALL).title(" [t]ype ").border_style(border_style));
        frame.render_widget(type_widget, controls_chunks[0]);

        let scan_label = self.scan_type().label();
        let scan_widget = Paragraph::new(format!(" {scan_label}"))
            .block(Block::default().borders(Borders::ALL).title(" [s]can mode ").border_style(border_style));
        frame.render_widget(scan_widget, controls_chunks[1]);

        let is_editing_value = self.editing_value && input_mode == InputMode::Editing;
        let status = if scanner.has_scanned() { "Next Scan" } else { "First Scan" };

        let value_content = if is_editing_value {
            Line::from(vec![
                Span::styled(format!(" {}", self.value_input), Style::default().fg(Color::Yellow)),
                Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
            ])
        } else {
            Line::from(Span::styled(format!(" {}", self.value_input), Style::default()))
        };

        // Row 2: the search value gets the full pane width.
        let value_widget = Paragraph::new(value_content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" [v]alue → Enter:{status} "))
                    .border_style(if is_editing_value {
                        Style::default().fg(Color::Yellow)
                    } else {
                        border_style
                    }),
            );
        frame.render_widget(value_widget, chunks[1]);

        // Results area - show scanning animation or results
        if scanning {
            self.draw_scanning_animation(frame, chunks[2], border_style, spinner, elapsed, progress);
        } else {
            self.draw_results(frame, chunks[2], scanner, border_style);
        }
    }

    fn draw_scanning_animation(
        &self,
        frame: &mut Frame,
        area: Rect,
        border_style: Style,
        spinner: &str,
        elapsed: &str,
        progress: Option<&Arc<ScanProgress>>,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Scanning... ")
            .border_style(border_style);
        frame.render_widget(block, area);

        // Progress bar
        let (pct, found) = if let Some(p) = progress {
            let percentage = p.percentage();
            let found = p.found_count.load(Ordering::Relaxed);
            (percentage, found)
        } else {
            (0.0, 0)
        };

        // Recalculate inner area (inside the border)
        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        if inner.height < 3 {
            return;
        }

        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .split(inner);

        // Spinner line
        let spin_text = Line::from(vec![
            Span::styled(
                format!("  {spinner} "),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("Scanning memory... ({elapsed})"),
                Style::default().fg(Color::White),
            ),
        ]);
        frame.render_widget(Paragraph::new(spin_text), content_chunks[1]);

        // Progress gauge
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .ratio((pct / 100.0).clamp(0.0, 1.0))
            .label(format!("{pct:.0}%"));
        frame.render_widget(gauge, content_chunks[2]);

        // Found count
        let found_text = Line::from(Span::styled(
            format!("  Found: {found} matches"),
            Style::default().fg(Color::Green),
        ));
        frame.render_widget(Paragraph::new(found_text), content_chunks[3]);
    }

    fn draw_results(&self, frame: &mut Frame, area: Rect, scanner: &Scanner, border_style: Style) {
        let results = scanner.results();
        let max_display = (area.height as usize).saturating_sub(2);
        let start = self.result_scroll;
        let end = (start + max_display).min(results.len());
        let visible = if start < results.len() {
            &results[start..end]
        } else {
            &[]
        };

        let items: Vec<ListItem> = visible
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let global_idx = start + i;
                let style = if global_idx == self.result_selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("0x{:X}  ", r.address), Style::default().fg(Color::Yellow)),
                    Span::styled(r.value.display_value(), Style::default().fg(Color::White)),
                ]))
                .style(style)
            })
            .collect();

        let position = if results.len() > max_display && !results.is_empty() {
            format!(" ↕{}/{}", self.result_selected + 1, results.len())
        } else {
            String::new()
        };
        // Kept short - the pane is only half the terminal wide and an
        // over-long block title is clipped, not wrapped. Key hints live in the
        // status bar instead.
        let mut title = vec![Span::raw(format!(
            " Results: {}{} ",
            scanner.result_count(),
            position
        ))];
        if self.auto_scan {
            title.push(Span::styled(
                format!("[AUTO {}ms] ", AUTO_SCAN_INTERVAL.as_millis()),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        }

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(title))
                .border_style(border_style),
        );
        frame.render_widget(list, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 rows total: 6 for the two control rows, leaving 6 for the results
    /// block, minus 2 borders = 4 visible result rows.
    fn pane() -> Rect {
        Rect::new(0, 0, 80, 12)
    }

    #[test]
    fn test_visible_rows() {
        assert_eq!(ScannerView::visible_rows(pane()), 4);
    }

    #[test]
    fn test_scroll_follows_cursor_down() {
        let mut view = ScannerView::new();

        // Still on the first page - no scrolling yet.
        view.result_selected = 3;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 0);

        // One past the last visible row - the window advances by one.
        view.result_selected = 4;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 1);

        view.result_selected = 50;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 47);
    }

    #[test]
    fn test_scroll_follows_cursor_up() {
        let mut view = ScannerView::new();
        view.result_selected = 50;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 47);

        view.result_selected = 46;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 46);

        view.result_selected = 0;
        view.ensure_visible(pane(), 100);
        assert_eq!(view.result_scroll, 0);
    }

    #[test]
    fn test_scroll_reset_when_empty() {
        let mut view = ScannerView::new();
        view.result_scroll = 30;
        view.ensure_visible(pane(), 0);
        assert_eq!(view.result_scroll, 0);
    }

    #[test]
    fn test_ensure_visible_does_not_move_selection() {
        let mut view = ScannerView::new();
        view.result_selected = 90;
        // Stale selection after the result set shrank - drawing must not
        // silently reposition the cursor.
        view.ensure_visible(pane(), 5);
        assert_eq!(view.result_selected, 90);
    }
}
