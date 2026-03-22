use std::sync::Arc;
use std::sync::atomic::Ordering;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph};

use crate::scan::scanner::{ScanProgress, Scanner};
use crate::scan::value_type::{ScanType, ValueType};
use super::app::InputMode;

pub struct ScannerView {
    pub value_input: String,
    pub editing_value: bool,
    pub result_selected: usize,
    pub result_scroll: usize,
    value_type_index: usize,
    scan_type_index: usize,
}

const SCAN_TYPES: &[ScanType] = &[
    ScanType::ExactValue,
    ScanType::UnknownInitial,
    ScanType::Increased,
    ScanType::Decreased,
    ScanType::Changed,
    ScanType::Unchanged,
    ScanType::GreaterThan,
    ScanType::LessThan,
];

fn scan_type_label(st: ScanType) -> &'static str {
    match st {
        ScanType::ExactValue => "Exact Value",
        ScanType::UnknownInitial => "Unknown Initial",
        ScanType::Increased => "Increased",
        ScanType::Decreased => "Decreased",
        ScanType::Changed => "Changed",
        ScanType::Unchanged => "Unchanged",
        ScanType::GreaterThan => "Greater Than",
        ScanType::LessThan => "Less Than",
    }
}

impl ScannerView {
    pub fn new() -> Self {
        Self {
            value_input: String::new(),
            editing_value: false,
            result_selected: 0,
            result_scroll: 0,
            value_type_index: 2, // default U32
            scan_type_index: 0,
        }
    }

    pub fn value_type(&self) -> ValueType {
        ValueType::ALL[self.value_type_index]
    }

    pub fn scan_type(&self) -> ScanType {
        SCAN_TYPES[self.scan_type_index]
    }

    pub fn cycle_value_type(&mut self) {
        self.value_type_index = (self.value_type_index + 1) % ValueType::ALL.len();
    }

    pub fn cycle_scan_type(&mut self) {
        self.scan_type_index = (self.scan_type_index + 1) % SCAN_TYPES.len();
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

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(3)])
            .split(area);

        // Controls
        let controls_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20),
                Constraint::Length(20),
                Constraint::Min(20),
            ])
            .split(chunks[0]);

        let type_label = self.value_type().label();
        let type_widget = Paragraph::new(format!(" {type_label}"))
            .block(Block::default().borders(Borders::ALL).title(" [t]ype ").border_style(border_style));
        frame.render_widget(type_widget, controls_chunks[0]);

        let scan_label = scan_type_label(self.scan_type());
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
        frame.render_widget(value_widget, controls_chunks[2]);

        // Results area - show scanning animation or results
        if scanning {
            self.draw_scanning_animation(frame, chunks[1], border_style, spinner, elapsed, progress);
        } else {
            self.draw_results(frame, chunks[1], scanner, border_style);
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
        let inner_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .split(area);

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
                    Span::styled(format!("0x{:016X}  ", r.address), Style::default().fg(Color::Yellow)),
                    Span::styled(r.value.display_value(), Style::default().fg(Color::White)),
                ]))
                .style(style)
            })
            .collect();

        let title = format!(
            " Results: {} | [a]dd to table | [r]eset ",
            scanner.result_count()
        );
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        );
        frame.render_widget(list, area);
    }
}
