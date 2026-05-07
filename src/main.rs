mod app;
mod editor;
mod header;
mod ui;

use std::io;
use std::path::PathBuf;

use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, EditMode, InputMode, View};

#[derive(Parser)]
#[command(name = "hex-forge", about = "Terminal hex editor with file format awareness")]
struct Cli {
    /// File to open
    file: PathBuf,

    /// Read-only mode
    #[arg(short, long)]
    readonly: bool,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    if !cli.file.exists() {
        eprintln!("Error: file not found: {}", cli.file.display());
        std::process::exit(1);
    }

    let mut app = App::open(&cli.file, cli.readonly)?;

    // Install panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            // Handle input modes first (goto, search, quit-confirm)
            if app.input_mode != InputMode::Normal {
                match app.input_mode {
                    InputMode::GotoOffset => match key.code {
                        KeyCode::Enter => {
                            app.execute_goto();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Esc => {
                            app.input_buf.clear();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Char(c) => {
                            app.input_buf.push(c);
                        }
                        KeyCode::Backspace => {
                            app.input_buf.pop();
                        }
                        _ => {}
                    },
                    InputMode::Search => match key.code {
                        KeyCode::Enter => {
                            app.execute_search();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Esc => {
                            app.input_buf.clear();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Char(c) => {
                            app.input_buf.push(c);
                        }
                        KeyCode::Backspace => {
                            app.input_buf.pop();
                        }
                        _ => {}
                    },
                    InputMode::QuitConfirm => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(()),
                        _ => {
                            app.input_mode = InputMode::Normal;
                            app.status_msg = Some("Quit cancelled".to_string());
                        }
                    },
                    InputMode::Normal => {}
                }
                continue;
            }

            // Normal mode keybindings
            match key.code {
                // Ctrl combos
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if app.modified {
                        app.input_mode = InputMode::QuitConfirm;
                        app.status_msg =
                            Some("Unsaved changes! Press Y to quit, any other key to cancel".to_string());
                    } else {
                        return Ok(());
                    }
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.save()?;
                }
                KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.input_mode = InputMode::GotoOffset;
                    app.input_buf.clear();
                    app.status_msg = Some("Goto offset (hex with 0x prefix, or decimal):".to_string());
                }
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.input_mode = InputMode::Search;
                    app.input_buf.clear();
                    app.status_msg =
                        Some("Search (hex: \"FF 00\", ascii: \"text\"):".to_string());
                }

                // View toggle
                KeyCode::F(1) => {
                    app.view = match app.view {
                        View::Hex => View::Info,
                        View::Info => View::Hex,
                    };
                }

                // Search shortcuts
                KeyCode::Char('/') => {
                    app.input_mode = InputMode::Search;
                    app.input_buf.clear();
                    app.status_msg =
                        Some("Search (hex: \"FF 00\", ascii: \"text\"):".to_string());
                }
                KeyCode::Char('n') if app.edit_mode == EditMode::Hex && app.view == View::Hex => {
                    app.find_next();
                }

                // Tab: toggle edit mode
                KeyCode::Tab => {
                    app.edit_mode = match app.edit_mode {
                        EditMode::Hex => EditMode::Ascii,
                        EditMode::Ascii => EditMode::Hex,
                    };
                    app.status_msg = Some(format!("Edit mode: {:?}", app.edit_mode));
                }

                // Navigation
                KeyCode::Up => app.move_cursor_up(),
                KeyCode::Down => app.move_cursor_down(),
                KeyCode::Left => app.move_cursor_left(),
                KeyCode::Right => app.move_cursor_right(),
                KeyCode::PageUp => app.page_up(),
                KeyCode::PageDown => app.page_down(),
                KeyCode::Home => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.cursor = 0;
                        app.ensure_cursor_visible();
                    } else {
                        // Start of line
                        app.cursor -= app.cursor % 16;
                        app.ensure_cursor_visible();
                    }
                }
                KeyCode::End => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        app.cursor = app.file_size().saturating_sub(1);
                        app.ensure_cursor_visible();
                    } else {
                        // End of line
                        let line_start = app.cursor - (app.cursor % 16);
                        app.cursor = (line_start + 15).min(app.file_size().saturating_sub(1));
                        app.ensure_cursor_visible();
                    }
                }

                // Selection
                KeyCode::Char('v') if app.edit_mode == EditMode::Hex => {
                    if app.selection_start.is_some() {
                        app.selection_start = None;
                        app.status_msg = Some("Selection cleared".to_string());
                    } else {
                        app.selection_start = Some(app.cursor);
                        app.status_msg = Some("Selection started (move cursor, 'y' to copy)".to_string());
                    }
                }
                KeyCode::Char('y') if app.edit_mode == EditMode::Hex && app.selection_start.is_some() => {
                    app.copy_selection();
                }

                // Editing
                KeyCode::Char(c) if app.view == View::Hex => {
                    match app.edit_mode {
                        EditMode::Hex => {
                            if c.is_ascii_hexdigit() {
                                app.input_hex_digit(c);
                            }
                        }
                        EditMode::Ascii => {
                            if c.is_ascii() && !c.is_ascii_control() {
                                app.input_ascii_char(c);
                            }
                        }
                    }
                }

                KeyCode::Esc => {
                    app.selection_start = None;
                    app.status_msg = None;
                }

                _ => {}
            }
        }
    }
}
