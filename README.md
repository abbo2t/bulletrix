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

Bulletrix's text is always directly editable — there's no mode to enter
first, just click/select a bullet and type.

| Key | Action |
|---|---|
| _any printable key_ | insert into the selected bullet's text |
| `Enter` | split the bullet at the cursor into a new one |
| `Tab` / `Shift+Tab` | indent / outdent |
| `↑` / `↓` | move the selection up/down |
| `←` / `→` | move the cursor within the line (or to the adjacent bullet at the start/end of a line) |
| `Home` / `End` | jump to the start/end of the line |
| `Backspace` at the start of a line | merge into the previous bullet |
| `Ctrl+↑` / `Ctrl+↓` | move the bullet up/down among its siblings |
| `Ctrl+→` / `Ctrl+←` | zoom into the selected bullet / zoom back out |
| `Ctrl+D` | toggle complete |
| `Ctrl+K` | collapse/expand children |
| `Ctrl+O` | toggle editing the bullet's note |
| `Ctrl+N` | add a child under the selected bullet |
| `Ctrl+F` | search |
| `Ctrl+H` | hide/show completed items |
| `Ctrl+S` | save now (autosave already covers you, but this is instant) |
| `Ctrl+Q` | quit |

> A separate vim-style modal editing mode (Normal/Insert, `hjkl`, `dd`, `gg`,
> and a [flash.nvim](https://github.com/folke/flash.nvim)-style jump-to-character
> feature) exists on the `vim-modal-keybindings` and `flash-jump` branches but
> hasn't been merged to `main` yet.

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
Not on `main` yet — see the note under Keybindings above about the
`vim-modal-keybindings` and `flash-jump` branches.

**Can I export back out to OPML/Markdown/etc.?**
Not yet — only OPML *import* exists so far.

**Why "Bulletrix" and not something WorkFlowy-adjacent?**
To avoid trading on WorkFlowy's name/trademark while still being upfront
about the inspiration — it's an independent project with no affiliation.

**My terminal shows garbled characters instead of bullets/lines.**
Bulletrix uses Unicode box-drawing and bullet characters (`▾ ▸ • » ›`). Any
modern terminal emulator with a reasonably complete Unicode font should
render these fine; if yours doesn't, try a different terminal or font.
