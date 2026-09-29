//! Ratatui port of bulletrix. Editing style is chosen by `--style
//! modal|traditional` or `editing_style` in `~/.bulletrix/config.toml`.
//! Not yet ported: persistence, search, OPML import, clipboard copy.

mod action;
mod config;
mod editor;
mod keymap;
mod ui;
mod undo;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use editor::Editor;
use keymap::Keymap;
use model::Outline;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};

fn main() -> io::Result<()> {
    let style = match config::resolve_style(std::env::args().skip(1), config::config_path().as_deref()) {
        Ok(style) => style,
        Err(e) => {
            eprintln!("bulletrix: {e}");
            std::process::exit(2);
        }
    };

    install_panic_hook();
    let mut terminal = init_terminal()?;
    let mut outline = Outline::new();
    seed_demo_content(&mut outline);
    let mut editor = Editor::new(outline, style);
    let mut keymap = keymap::for_style(style);
    let result = run(&mut terminal, &mut editor, keymap.as_mut());
    restore_terminal(&mut terminal)?;
    result
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    editor: &mut Editor,
    keymap: &mut dyn Keymap,
) -> io::Result<()> {
    while !editor.should_quit {
        terminal.draw(|frame| ui::draw(frame, editor, keymap))?;
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                keymap::dispatch(editor, keymap, key);
            }
        }
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

fn seed_demo_content(outline: &mut Outline) {
    let root = outline.root();
    // Outline::new() (like Python's Outline.__init__) seeds root with one
    // empty child - reuse it rather than leave a blank row in the demo.
    let groceries = outline.get(root).children[0];
    outline.get_mut(groceries).text = "Groceries #errand".into();

    let milk = outline.create_node("Buy milk");
    outline.get_mut(milk).completed = true;
    outline.add_first_child(groceries, milk);
    let eggs = outline.create_node("Buy eggs");
    outline.insert_sibling_after(milk, eggs);

    let work = outline.create_node("Work @acme");
    outline.insert_sibling_after(groceries, work);
    let review = outline.create_node("Review PR #4");
    outline.get_mut(review).note = "check the WSL2 focus bug writeup first".into();
    outline.add_first_child(work, review);

    let idea = outline.create_node("Port bulletrix to ratatui");
    outline.insert_sibling_after(work, idea);
}
