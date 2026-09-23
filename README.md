# Bulletrix

A keyboard-driven, zoomable outliner for the terminal — infinite nesting and
zoom-based navigation (the core WorkFlowy mental model), built with
[Textual](https://textual.textualize.io/).

Bulletrix is an independent project, not affiliated with or endorsed by
WorkFlowy.

## Features

- Infinite nesting with Tab / Shift+Tab indent and outdent
- Zoom navigation — drill into any bullet as its own page, with a breadcrumb trail back up
- Notes on any bullet
- `#tag` / `@tag` highlighting
- Full-text search across the whole outline
- Mark items complete, and optionally hide completed items
- Undo/redo for edits (typing coalesces into single steps; navigation like zoom and search isn't part of the history)
- Autosaves to a local JSON file after every edit
- Import from OPML (e.g. a WorkFlowy export), merged in as new top-level items

## Installation

Requires Python 3.10+.

```bash
git clone git@github.com:abbo2t/bulletrix.git
cd bulletrix
python3 -m venv .venv
.venv/bin/pip install -e ".[dev]"
```

The `[dev]` extra pulls in `pytest`/`pytest-asyncio` for running the test
suite; drop it (`.venv/bin/pip install -e .`) if you only want to run the app.

## Usage

Launch it:

```bash
.venv/bin/bulletrix
```

Your outline is saved to `~/.bulletrix/outline.json` by default. Point it at
a different file with `--file`:

```bash
.venv/bin/bulletrix --file ./my-outline.json
```

### Importing from WorkFlowy (or any other outliner)

Export your outline as **OPML** from WorkFlowy (or another outliner — OPML is
a standard interchange format), then:

```bash
.venv/bin/bulletrix --import export.opml
```

This merges the OPML file's top-level items into `--file` (or the default
outline) as new top-level bullets, alongside whatever's already there — it
never overwrites or deletes existing content. It runs once and exits without
launching the TUI.

### Keybindings

Editing is modal, vim-style. You start in **NORMAL** mode, where keys are
commands; `i`/`a`/`I`/`A`/`o`/`O`/`cc` drop you into **INSERT** mode, where
keys type into the selected bullet's text; `Escape` returns to NORMAL.

**NORMAL mode**

| Key | Action |
|---|---|
| `i` / `a` | insert before / after the cursor |
| `I` / `A` | insert at the start / end of the line |
| `o` / `O` | open a new bullet below / above and start inserting |
| `cc` | clear the line's text and start inserting |
| `dd` | delete the bullet |
| `x` | delete the character under the cursor |
| `h` / `l` | move the cursor left/right within the line |
| `j` / `k` | move the selection down/up |
| `0` / `$` | jump to the start/end of the line |
| `gg` / `G` | jump to the first/last visible bullet |
| `s` | flash.nvim-style jump: press a character, then a label, to jump straight to it |
| `Tab` / `Shift+Tab` | indent / outdent |
| `Enter` / `L` | zoom into the selected bullet |
| `H` | zoom back out |
| `Space` / `za` | toggle fold; `zo` / `zc` open/close explicitly |
| `yy` | copy the selected bullet's text to the clipboard |
| `u` | undo |
| `Ctrl+R` | redo |
| `Ctrl+↑` / `Ctrl+↓` | move the bullet up/down among its siblings |
| `Ctrl+D` | toggle complete |
| `Ctrl+O` | toggle editing the bullet's note |
| `Ctrl+N` | add a child under the selected bullet, in INSERT mode |
| `/` | search |
| `Ctrl+H` | hide/show completed items |
| `Ctrl+S` | save now (autosave already covers you, but this is instant) |
| `q` | quit |

**INSERT mode**

| Key | Action |
|---|---|
| _any printable key_ | insert into the selected bullet's text |
| `Enter` | split the bullet at the cursor into a new one |
| `Backspace` at the start of a line | merge into the previous bullet |
| `Delete` | delete the character to the right of the cursor |
| `Escape` | return to NORMAL mode |
| `Tab` / `Shift+Tab`, `Ctrl+D`, `Ctrl+O`, `Ctrl+N`, `Ctrl+↑`/`Ctrl+↓`, `Ctrl+→`/`Ctrl+←`, `Ctrl+R` | same as NORMAL mode |

`u` and `Ctrl+R` only trigger undo/redo in NORMAL mode — in INSERT mode `u`
types a literal "u", matching vim.

> `yy` copies via the terminal's OSC 52 clipboard sequence rather than
> shelling out to `pbcopy`/`xclip`/`clip.exe`, so it works over SSH and in
> WSL2 with Windows Terminal without any extra setup.

## Development

Run the test suite:

```bash
.venv/bin/pytest
```

Layout:

| File | Responsibility |
|---|---|
| `src/bulletrix/models.py` | The outline data structure and all tree operations (indent, zoom, search, ...) — no UI code |
| `src/bulletrix/outline_view.py` | The Textual widget: rendering and all keyboard handling |
| `src/bulletrix/undo.py` | The bounded undo/redo snapshot stack |
| `src/bulletrix/app.py` | App layout, breadcrumb, search bar, autosave wiring |
| `src/bulletrix/storage.py` | JSON load/save |
| `src/bulletrix/opml_import.py` | OPML parsing and merge-import |

The test suite drives the real app headlessly via Textual's `run_test()`
pilot (simulated keypresses against the actual app), rather than mocking
pieces of it, since a full-screen TUI can't meaningfully be screenshotted in
most automated environments.

## FAQ

**Why does a brand-new outline start with one empty bullet instead of a blank screen?**
The top level always has at least one item so there's always a line to type
into — the same reason a blank document always starts with a cursor on line
one instead of nothing at all.

**Where is my data actually stored?**
`~/.bulletrix/outline.json` by default, written atomically (via a temp file
+ rename) after every edit. Pass `--file` to use a different path — handy for
keeping multiple separate outlines.

**Can I sync my outline across machines?**
Not built in, but since it's a single plain JSON file, putting it under a
synced folder (Dropbox, iCloud Drive, a private git repo, Syncthing, etc.)
works fine as long as you're not editing it from two machines at once.

**How do tags work?**
Type `#tag` or `@tag` anywhere in a bullet's text or note — it's highlighted
automatically. There's no dedicated tag browser yet; `Ctrl+F` search matches
tag text like any other text.

**Does it support vim-style keybindings?**
Yes — editing is modal (NORMAL/INSERT), with `hjkl` movement, `dd`/`cc`
commands, and a flash.nvim-style `s` jump-to-character feature. See
Keybindings above.

**Can I export back out to OPML/Markdown/etc.?**
Not yet — only OPML *import* exists so far.

**Why "Bulletrix" and not something WorkFlowy-adjacent?**
To avoid trading on WorkFlowy's name/trademark while still being upfront
about the inspiration — it's an independent project with no affiliation.

**My terminal shows garbled characters instead of bullets/lines.**
Bulletrix uses Unicode box-drawing and bullet characters (`▾ ▸ • » ›`). Any
modern terminal emulator with a reasonably complete Unicode font should
render these fine; if yours doesn't, try a different terminal or font.
