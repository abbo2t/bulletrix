//! Ratatui port of bulletrix.
//!
//! `bulletrix [--file PATH] [--style modal|traditional]`. The outline
//! defaults to `~/.bulletrix/outline.json` (same file and format as the
//! Python version) and the style to `editing_style` in
//! `~/.bulletrix/config.toml`. Not yet ported: search, OPML import,
//! clipboard copy.

mod action;
mod autosave;
mod config;
mod editor;
mod keymap;
mod ui;
mod undo;

use action::Action;
use autosave::Autosave;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use editor::Editor;
use keymap::Keymap;
use model::storage;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::process::ExitCode;

fn main() -> ExitCode {
    let opts = match config::resolve(std::env::args().skip(1), config::data_dir().as_deref()) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("bulletrix: {e}");
            return ExitCode::from(2);
        }
    };
    // Load before touching the terminal, so a bad file is a plain error
    // message rather than a flash of alternate screen.
    let outline = match storage::load(&opts.file) {
        Ok(outline) => outline,
        Err(e) => {
            eprintln!("bulletrix: can't load {}: {e}", opts.file.display());
            return ExitCode::FAILURE;
        }
    };

    let mut editor = Editor::new(outline, opts.style);
    let mut keymap = keymap::for_style(opts.style);
    let mut autosave = Autosave::new(opts.file, &mut editor);

    install_panic_hook();
    let result = init_terminal().and_then(|mut terminal| {
        let result = run(&mut terminal, &mut editor, keymap.as_mut(), &mut autosave);
        restore_terminal(&mut terminal)?;
        result
    });
    if let Err(e) = result {
        eprintln!("bulletrix: {e}");
        return ExitCode::FAILURE;
    }
    // Autosave already ran after the last key; this only matters if that failed.
    if let Err(e) = autosave.sync(&mut editor, false) {
        eprintln!("bulletrix: couldn't save {}: {e}", autosave.path().display());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    editor: &mut Editor,
    keymap: &mut dyn Keymap,
    autosave: &mut Autosave,
) -> io::Result<()> {
    let mut note: Option<String> = None;
    while !editor.should_quit {
        terminal.draw(|frame| ui::draw(frame, editor, keymap, note.as_deref()))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let action = keymap::dispatch(editor, keymap, key);
        let force = action == Some(Action::Save);
        note = match autosave.sync(editor, force) {
            Ok(_) if force => Some("saved".into()),
            Ok(_) => None,
            Err(e) => Some(format!("save failed: {e}")),
        };
    }
    Ok(())
}

// -- terminal lifecycle --------------------------------------------------

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()
}

/// Without this, a panic mid-draw leaves the terminal in raw mode / the
/// alternate screen, and the shell looks frozen until the user runs `reset`.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original(info);
    }));
}
