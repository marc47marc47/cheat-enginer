use std::io;
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use ce_engine::VERSION_LINE;
use ce_engine::ui::app::App;

fn main() -> anyhow::Result<()> {
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("{VERSION_LINE}");
        return Ok(());
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()> {
    let mut app = App::new();
    app.refresh_processes();

    let tick_rate = Duration::from_millis(50);

    loop {
        terminal.draw(|f| app.draw(f))?;

        app.handle_events(tick_rate)?;

        // Re-apply freezes and refresh address table values
        app.tick();

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
