//! Ratatui port of bulletrix.
//!
//! `bulletrix [--file PATH] [--style modal|traditional] [--import OPML_FILE]`.
//! The outline defaults to `~/.bulletrix/outline.json` (same file and format
//! as the Python version) and the style to `editing_style` in
//! `~/.bulletrix/config.toml`. `--import` merges an OPML file into the
//! outline and exits. `mouse = false` in the config file leaves the mouse
//! to the terminal.

mod action;
mod autosave;
mod clipboard;
mod config;
mod editor;
mod keymap;
mod mouse;
mod ui;
mod undo;

use action::Action;
use autosave::Autosave;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use editor::Editor;
use keymap::Keymap;
use model::{opml, storage, Outline};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

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
    if let Some(opml_path) = &opts.import {
        return run_import(outline, opml_path, &opts.file);
    }

    let mut editor = Editor::new(outline, opts.style);
    let mut keymap = keymap::for_style(opts.style);
    let mut autosave = Autosave::new(opts.file, &mut editor);

    install_panic_hook();
    let result = init_terminal(opts.mouse).and_then(|mut terminal| {
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

/// `--import`: merge an OPML file into the outline file and exit.
fn run_import(mut outline: Outline, opml_path: &Path, file: &Path) -> ExitCode {
    let result = opml::import_opml(&mut outline, opml_path)
        .and_then(|added| storage::save(file, &outline.to_json_string()).map(|()| added));
    match result {
        Ok(added) => {
            println!(
                "Imported {added} top-level item(s) from {} into {}",
                opml_path.display(),
                file.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Import failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    editor: &mut Editor,
    keymap: &mut dyn Keymap,
    autosave: &mut Autosave,
) -> io::Result<()> {
    let mut note: Option<String> = None;
    let mut mouse = mouse::Mouse::default();
    let mut screen = ui::ScreenMap::default();
    let mut redraw = true;
    while !editor.should_quit {
        if redraw {
            terminal.draw(|frame| screen = ui::draw(frame, editor, keymap, note.as_deref()))?;
        }
        // Only redraw when an event did something: with the mouse captured,
        // every movement is an event.
        redraw = true;
        let force_save = match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                keymap::dispatch(editor, keymap, key) == Some(Action::Save)
            }
            Event::Mouse(ev) => {
                let actions = mouse.resolve(ev, &screen, editor, Instant::now());
                if actions.is_empty() {
                    redraw = false;
                    continue;
                }
                for action in actions {
                    editor.apply(action);
                }
                false
            }
            Event::Resize(..) => continue,
            _ => {
                redraw = false;
                continue;
            }
        };
        note = settle(terminal, editor, autosave, force_save);
    }
    Ok(())
}

/// Does the I/O the last actions asked for (clipboard, saving) and returns
/// the status-bar message, if any.
fn settle(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    editor: &mut Editor,
    autosave: &mut Autosave,
    force_save: bool,
) -> Option<String> {
    let mut notice = editor.notice.take().map(String::from);
    if let Some(text) = editor.pending_copy.take() {
        notice = Some(match clipboard::copy(terminal.backend_mut(), &text) {
            Ok(()) => "copied".into(),
            Err(e) => format!("copy failed: {e}"),
        });
    }
    match autosave.sync(editor, force_save) {
        Err(e) => Some(format!("save failed: {e}")),
        Ok(_) if force_save => Some("saved".into()),
        Ok(_) => notice,
    }
}

// -- terminal lifecycle --------------------------------------------------

fn init_terminal(mouse: bool) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if mouse {
        execute!(stdout, EnableMouseCapture)?;
    }
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
