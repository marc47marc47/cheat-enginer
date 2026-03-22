use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::address::{AddressEntry, AddressTable};
use crate::error::Result;
use crate::platform::{self, ProcessHandle, ProcessInfo};
use crate::process;
use crate::scan::scanner::{ScanProgress, Scanner};
use crate::scan::value_type::{ScanType, ScanValue, ValueType};

use super::address_list_view::AddressListView;
use super::hex_viewer::HexViewer;
use super::process_list::ProcessListView;
use super::scanner_view::ScannerView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    ProcessList,
    Main,
    HexViewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainPanel {
    Scanner,
    AddressList,
}

/// Background scan result sent back via channel
struct ScanDone {
    scanner: Scanner,
    handle: Box<dyn ProcessHandle>,
    result: std::result::Result<usize, String>,
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct App {
    pub screen: Screen,
    pub input_mode: InputMode,
    pub should_quit: bool,
    pub confirm_quit: bool,
    pub error_message: Option<(String, Instant)>,

    // Platform
    pub platform: Box<dyn platform::Platform>,
    pub process_handle: Option<Box<dyn ProcessHandle>>,
    pub attached_process: Option<ProcessInfo>,

    // Process list
    pub process_list: Vec<ProcessInfo>,
    pub process_view: ProcessListView,

    // Scanner
    pub scanner: Scanner,
    pub scanner_view: ScannerView,
    pub scan_progress: Option<Arc<ScanProgress>>,

    // Background scan
    scanning: bool,
    scan_start: Option<Instant>,
    scan_receiver: Option<mpsc::Receiver<ScanDone>>,

    // Address list
    pub address_table: AddressTable,
    pub address_view: AddressListView,
    last_value_update: Instant,
    values_dirty: bool,

    // Hex viewer
    pub hex_viewer: HexViewer,

    // Main panel focus
    pub main_panel: MainPanel,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::ProcessList,
            input_mode: InputMode::Normal,
            should_quit: false,
            confirm_quit: false,
            error_message: None,
            platform: platform::create_platform(),
            process_handle: None,
            attached_process: None,
            process_list: Vec::new(),
            process_view: ProcessListView::new(),
            scanner: Scanner::new(),
            scanner_view: ScannerView::new(),
            scan_progress: None,
            scanning: false,
            scan_start: None,
            scan_receiver: None,
            address_table: AddressTable::new(),
            address_view: AddressListView::new(),
            last_value_update: Instant::now(),
            values_dirty: false,
            hex_viewer: HexViewer::new(),
            main_panel: MainPanel::Scanner,
        }
    }

    pub fn set_error(&mut self, msg: String) {
        self.error_message = Some((msg, Instant::now()));
    }

    pub fn refresh_processes(&mut self) {
        match self.platform.enumerate_processes() {
            Ok(procs) => self.process_list = procs,
            Err(e) => self.set_error(format!("Failed to enumerate processes: {e}")),
        }
    }

    pub fn attach_process(&mut self, pid: u32) {
        match self.platform.attach(pid) {
            Ok(handle) => {
                self.attached_process = self.process_list.iter().find(|p| p.pid == pid).cloned();
                self.process_handle = Some(handle);
                self.screen = Screen::Main;
                self.scanner.reset();
            }
            Err(e) => self.set_error(format!("Failed to attach: {e}")),
        }
    }

    pub fn do_scan(&mut self) {
        if self.scanning {
            return;
        }

        let Some(handle) = self.process_handle.take() else {
            self.set_error("No process attached".into());
            return;
        };

        let scan_type = self.scanner_view.scan_type();
        let value_type = self.scanner.value_type();

        let target = if scan_type == ScanType::ExactValue
            || scan_type == ScanType::GreaterThan
            || scan_type == ScanType::LessThan
        {
            match ScanValue::parse(&self.scanner_view.value_input, value_type) {
                Some(v) => Some(v),
                None => {
                    self.process_handle = Some(handle);
                    self.set_error("Invalid value".into());
                    return;
                }
            }
        } else {
            None
        };

        let progress = Arc::new(ScanProgress::new(0));
        self.scan_progress = Some(Arc::clone(&progress));
        self.scanning = true;
        self.scan_start = Some(Instant::now());

        let mut scanner = std::mem::replace(&mut self.scanner, Scanner::new());
        self.scanner.set_value_type(value_type);

        let (tx, rx) = mpsc::channel();
        self.scan_receiver = Some(rx);

        std::thread::spawn(move || {
            let result = if scanner.has_scanned() {
                scanner.next_scan(handle.as_ref(), scan_type, target.as_ref(), Some(progress))
            } else {
                scanner.first_scan(handle.as_ref(), scan_type, target.as_ref(), Some(progress))
            };

            let _ = tx.send(ScanDone {
                scanner,
                handle,
                result: result.map_err(|e| e.to_string()),
            });
        });
    }

    fn poll_scan(&mut self) {
        if !self.scanning {
            return;
        }

        let Some(ref rx) = self.scan_receiver else {
            return;
        };

        match rx.try_recv() {
            Ok(done) => {
                self.scanner = done.scanner;
                self.process_handle = Some(done.handle);
                self.scanning = false;
                self.scan_start = None;
                self.scan_progress = None;
                self.scan_receiver = None;

                match done.result {
                    Ok(count) => {
                        self.scanner_view.result_scroll = 0;
                        self.scanner_view.result_selected = 0;
                        if count == 0 && self.scanner.has_scanned() {
                            self.set_error("No results found".into());
                        }
                    }
                    Err(e) => self.set_error(format!("Scan error: {e}")),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.scanning = false;
                self.scan_start = None;
                self.scan_progress = None;
                self.scan_receiver = None;
                self.set_error("Scan thread crashed".into());
            }
        }
    }

    pub fn add_selected_to_address_table(&mut self) {
        let results = self.scanner.results();
        let idx = self.scanner_view.result_selected;
        if idx < results.len() {
            let result = &results[idx];
            let entry = AddressEntry::new(
                result.address,
                self.scanner.value_type(),
                String::new(),
            );
            self.address_table.add(entry);
        }
    }

    /// Get the address currently selected in the active panel (scanner results or address table)
    fn selected_address(&self) -> Option<usize> {
        match self.main_panel {
            MainPanel::Scanner => {
                let results = self.scanner.results();
                results.get(self.scanner_view.result_selected).map(|r| r.address)
            }
            MainPanel::AddressList => {
                self.address_table.entries.get(self.address_view.selected).map(|e| e.address)
            }
        }
    }

    pub fn update_values(&mut self) {
        let should_update = self.values_dirty
            || self.last_value_update.elapsed() >= Duration::from_secs(5);

        if !should_update {
            return;
        }

        if let Some(ref handle) = self.process_handle {
            self.address_table.update_values(handle.as_ref());
            self.address_table.write_frozen_values(handle.as_ref());
        }

        self.last_value_update = Instant::now();
        self.values_dirty = false;
    }

    pub fn handle_events(&mut self, timeout: Duration) -> Result<()> {
        self.poll_scan();

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key);
                }
            }
        }

        if let Some((_, instant)) = &self.error_message {
            if instant.elapsed() > Duration::from_secs(5) {
                self.error_message = None;
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl+C always quits immediately
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Quit confirmation dialog
        if self.confirm_quit {
            match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                _ => self.confirm_quit = false, // any other key cancels
            }
            return;
        }

        // Block most input while scanning
        if self.scanning {
            if key.code == KeyCode::Esc {
                self.confirm_quit = true;
            }
            return;
        }

        if self.input_mode == InputMode::Editing {
            self.handle_editing_key(key);
            return;
        }

        // Esc triggers quit confirmation on all screens
        if key.code == KeyCode::Esc {
            self.confirm_quit = true;
            return;
        }

        // Global F-key shortcuts
        match key.code {
            KeyCode::F(1) => {
                self.screen = Screen::ProcessList;
                self.refresh_processes();
                return;
            }
            KeyCode::F(2) => {
                if self.process_handle.is_some() {
                    self.screen = Screen::Main;
                }
                return;
            }
            KeyCode::F(3) => {
                if self.process_handle.is_some() {
                    // If on Main screen, jump hex viewer to selected address
                    if self.screen == Screen::Main {
                        if let Some(addr) = self.selected_address() {
                            self.hex_viewer.address = addr;
                        }
                    }
                    self.screen = Screen::HexViewer;
                }
                return;
            }
            _ => {}
        }

        match self.screen {
            Screen::ProcessList => self.handle_process_list_key(key),
            Screen::Main => self.handle_main_key(key),
            Screen::HexViewer => self.handle_hex_viewer_key(key),
        }
    }

    fn handle_process_list_key(&mut self, key: KeyEvent) {
        let filtered_len = process::filter_processes(&self.process_list, &self.process_view.filter_input).len();
        match key.code {
            KeyCode::Up => {
                if self.process_view.selected > 0 {
                    self.process_view.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.process_view.selected + 1 < filtered_len {
                    self.process_view.selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.process_view.selected = self.process_view.selected.saturating_sub(20);
            }
            KeyCode::PageDown => {
                self.process_view.selected = (self.process_view.selected + 20).min(filtered_len.saturating_sub(1));
            }
            KeyCode::Enter => {
                let filtered = process::filter_processes(&self.process_list, &self.process_view.filter_input);
                if let Some(proc) = filtered.get(self.process_view.selected) {
                    let pid = proc.pid;
                    self.attach_process(pid);
                }
            }
            KeyCode::F(5) => self.refresh_processes(),
            KeyCode::Backspace => {
                self.process_view.filter_input.pop();
                self.process_view.selected = 0;
                self.process_view.scroll_offset = 0;
            }
            KeyCode::Char(c) => {
                self.process_view.filter_input.push(c);
                self.process_view.selected = 0;
                self.process_view.scroll_offset = 0;
            }
            _ => {}
        }
        // Keep selected in bounds after filter change
        let new_filtered_len = process::filter_processes(&self.process_list, &self.process_view.filter_input).len();
        if self.process_view.selected >= new_filtered_len && new_filtered_len > 0 {
            self.process_view.selected = new_filtered_len - 1;
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => {
                self.main_panel = match self.main_panel {
                    MainPanel::Scanner => MainPanel::AddressList,
                    MainPanel::AddressList => MainPanel::Scanner,
                };
            }
            _ => match self.main_panel {
                MainPanel::Scanner => self.handle_scanner_key(key),
                MainPanel::AddressList => self.handle_address_key(key),
            },
        }
    }

    fn handle_scanner_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.scanner_view.result_selected > 0 {
                    self.scanner_view.result_selected -= 1;
                }
            }
            KeyCode::Down => {
                let count = self.scanner.result_count();
                if self.scanner_view.result_selected + 1 < count {
                    self.scanner_view.result_selected += 1;
                }
            }
            KeyCode::Char('v') | KeyCode::Char('e') => {
                self.input_mode = InputMode::Editing;
                self.scanner_view.editing_value = true;
            }
            KeyCode::Char('t') => {
                self.scanner_view.cycle_value_type();
                self.scanner.set_value_type(self.scanner_view.value_type());
            }
            KeyCode::Char('s') => {
                self.scanner_view.cycle_scan_type();
            }
            KeyCode::Enter => {
                self.do_scan();
            }
            KeyCode::Char('r') => {
                self.scanner.reset();
                self.scanner_view.result_selected = 0;
                self.scanner_view.result_scroll = 0;
            }
            KeyCode::Char('a') => {
                self.add_selected_to_address_table();
            }
            _ => {}
        }
    }

    fn handle_address_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.address_view.selected > 0 {
                    self.address_view.selected -= 1;
                    self.values_dirty = true;
                }
            }
            KeyCode::Down => {
                if self.address_view.selected + 1 < self.address_table.entries.len() {
                    self.address_view.selected += 1;
                    self.values_dirty = true;
                }
            }
            KeyCode::Char('f') => {
                self.address_table.toggle_freeze(self.address_view.selected);
            }
            KeyCode::Delete => {
                if !self.address_table.entries.is_empty() {
                    self.address_table.remove(self.address_view.selected);
                    if self.address_view.selected >= self.address_table.entries.len()
                        && self.address_view.selected > 0
                    {
                        self.address_view.selected -= 1;
                    }
                }
            }
            KeyCode::Char('e') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_value = true;
                self.address_view.edit_buffer.clear();
            }
            KeyCode::Char('d') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_description = true;
                if let Some(entry) = self.address_table.entries.get(self.address_view.selected) {
                    self.address_view.edit_buffer = entry.description.clone();
                }
            }
            KeyCode::Char('S') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_save_path = true;
                self.address_view.edit_buffer = "cheat_table.json".to_string();
            }
            KeyCode::Char('L') => {
                self.input_mode = InputMode::Editing;
                self.address_view.editing_load_path = true;
                self.address_view.edit_buffer = "cheat_table.json".to_string();
            }
            _ => {}
        }
    }

    fn handle_hex_viewer_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.hex_viewer.scroll_up(),
            KeyCode::Down => self.hex_viewer.scroll_down(),
            KeyCode::PageUp => {
                for _ in 0..16 {
                    self.hex_viewer.scroll_up();
                }
            }
            KeyCode::PageDown => {
                for _ in 0..16 {
                    self.hex_viewer.scroll_down();
                }
            }
            KeyCode::Char('g') => {
                self.input_mode = InputMode::Editing;
                self.hex_viewer.editing_address = true;
                self.hex_viewer.address_input.clear();
            }
            _ => {}
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.clear_editing_state();
            }
            KeyCode::Enter => {
                self.commit_edit();
                self.input_mode = InputMode::Normal;
                self.clear_editing_state();
            }
            KeyCode::Backspace => {
                self.active_edit_buffer_mut().map(|b| { b.pop(); });
            }
            KeyCode::Char(c) => {
                self.active_edit_buffer_mut().map(|b| b.push(c));
            }
            _ => {}
        }
    }

    fn active_edit_buffer_mut(&mut self) -> Option<&mut String> {
        if self.process_view.editing_filter {
            return Some(&mut self.process_view.filter_input);
        }
        if self.scanner_view.editing_value {
            return Some(&mut self.scanner_view.value_input);
        }
        if self.address_view.editing_value
            || self.address_view.editing_description
            || self.address_view.editing_save_path
            || self.address_view.editing_load_path
        {
            return Some(&mut self.address_view.edit_buffer);
        }
        if self.hex_viewer.editing_address {
            return Some(&mut self.hex_viewer.address_input);
        }
        None
    }

    fn commit_edit(&mut self) {
        if self.process_view.editing_filter {
            self.process_view.selected = 0;
            return;
        }
        if self.hex_viewer.editing_address {
            if let Ok(addr) = usize::from_str_radix(&self.hex_viewer.address_input, 16) {
                self.hex_viewer.address = addr;
            } else {
                self.set_error("Invalid hex address".into());
            }
            return;
        }
        if self.address_view.editing_value {
            let idx = self.address_view.selected;
            if let Some(entry) = self.address_table.entries.get(idx) {
                let vt = entry.value_type;
                if let Some(val) = ScanValue::parse(&self.address_view.edit_buffer, vt) {
                    if let Some(ref handle) = self.process_handle {
                        if let Err(e) = self.address_table.write_value(handle.as_ref(), idx, val) {
                            self.set_error(format!("Write error: {e}"));
                        }
                    }
                } else {
                    self.set_error("Invalid value".into());
                }
            }
            return;
        }
        if self.address_view.editing_description {
            let idx = self.address_view.selected;
            if let Some(entry) = self.address_table.entries.get_mut(idx) {
                entry.description = self.address_view.edit_buffer.clone();
            }
            return;
        }
        if self.address_view.editing_save_path {
            let path = self.address_view.edit_buffer.clone();
            if let Err(e) = self.address_table.save_to_file(&path) {
                self.set_error(format!("Save error: {e}"));
            }
            return;
        }
        if self.address_view.editing_load_path {
            let path = self.address_view.edit_buffer.clone();
            match AddressTable::load_from_file(&path) {
                Ok(table) => self.address_table = table,
                Err(e) => self.set_error(format!("Load error: {e}")),
            }
            return;
        }
    }

    fn clear_editing_state(&mut self) {
        self.process_view.editing_filter = false;
        self.scanner_view.editing_value = false;
        self.address_view.editing_value = false;
        self.address_view.editing_description = false;
        self.address_view.editing_save_path = false;
        self.address_view.editing_load_path = false;
        self.hex_viewer.editing_address = false;
    }

    fn spinner_frame(&self) -> &'static str {
        let elapsed = self.scan_start.map(|s| s.elapsed()).unwrap_or_default();
        let idx = (elapsed.as_millis() / 80) as usize % SPINNER_FRAMES.len();
        SPINNER_FRAMES[idx]
    }

    fn scan_elapsed_str(&self) -> String {
        let elapsed = self.scan_start.map(|s| s.elapsed()).unwrap_or_default();
        let secs = elapsed.as_secs_f64();
        if secs < 1.0 {
            format!("{:.0}ms", elapsed.as_millis())
        } else {
            format!("{secs:.1}s")
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let size = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(3)])
            .split(size);

        // Status bar at top
        self.draw_status_bar(frame, chunks[0]);

        match self.screen {
            Screen::ProcessList => {
                let filtered = process::filter_processes(&self.process_list, &self.process_view.filter_input);
                // list area height = chunks[1].height - search bar(3) - borders(2)
                let visible_rows = chunks[1].height.saturating_sub(5) as usize;
                self.process_view.ensure_visible(visible_rows, filtered.len());
                self.process_view.draw(frame, chunks[1], &filtered, self.input_mode);
            }
            Screen::Main => {
                let main_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(chunks[1]);

                self.scanner_view.draw(
                    frame,
                    main_chunks[0],
                    &self.scanner,
                    self.input_mode,
                    self.main_panel == MainPanel::Scanner,
                    self.scanning,
                    self.spinner_frame(),
                    &self.scan_elapsed_str(),
                    self.scan_progress.as_ref(),
                );
                self.address_view.draw(
                    frame,
                    main_chunks[1],
                    &self.address_table,
                    self.input_mode,
                    self.main_panel == MainPanel::AddressList,
                );
            }
            Screen::HexViewer => {
                self.hex_viewer.draw(frame, chunks[1], self.process_handle.as_deref(), self.input_mode);
            }
        }

        // Quit confirmation overlay
        if self.confirm_quit {
            self.draw_quit_dialog(frame, size);
        }
    }

    fn draw_quit_dialog(&self, frame: &mut Frame, area: Rect) {
        let dialog_width = 40u16;
        let dialog_height = 5u16;
        let x = area.width.saturating_sub(dialog_width) / 2;
        let y = area.height.saturating_sub(dialog_height) / 2;
        let dialog_area = Rect::new(x, y, dialog_width.min(area.width), dialog_height.min(area.height));

        frame.render_widget(Clear, dialog_area);

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Quit? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("[q] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("Quit  "),
                Span::styled("[c] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Cancel"),
            ]),
        ];

        let dialog = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Confirm Exit "),
        );
        frame.render_widget(dialog, dialog_area);
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![];

        if let Some(ref proc) = self.attached_process {
            spans.push(Span::styled(
                format!(" [{}:{}] ", proc.name, proc.pid),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                " [No process] ",
                Style::default().fg(Color::DarkGray),
            ));
        }

        if self.scanning {
            let spinner = self.spinner_frame();
            let elapsed = self.scan_elapsed_str();
            spans.push(Span::styled(
                format!(" {spinner} Scanning... ({elapsed}) "),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        } else {
            let help = match self.screen {
                Screen::ProcessList => "F5:Refresh Enter:Attach Esc:Quit | Type to filter",
                Screen::Main => "F1:Proc F2:Main F3:Hex Tab:Panel t:Type s:Mode v:Value Enter:Scan r:Reset a:Add",
                Screen::HexViewer => "F1:Proc F2:Main F3:Hex g:GoTo Up/Down:Scroll",
            };
            spans.push(Span::styled(
                format!(" {help}"),
                Style::default().fg(Color::DarkGray),
            ));
        }

        if let Some((ref msg, _)) = self.error_message {
            spans.push(Span::styled(
                format!("  ERR: {msg}"),
                Style::default().fg(Color::Red),
            ));
        }

        let bar = Paragraph::new(Line::from(spans));
        frame.render_widget(bar, area);
    }
}
