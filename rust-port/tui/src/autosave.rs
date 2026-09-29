use crate::editor::Editor;
use model::storage;
use std::io;
use std::path::{Path, PathBuf};

/// Writes the outline whenever its persisted form changes, like the Python
/// app's save-on-every-change, but skipping writes when nothing changed.
pub struct Autosave {
    path: PathBuf,
    last_saved: String,
}

impl Autosave {
    /// Starts from the loaded state, so merely opening a file never rewrites it.
    pub fn new(path: PathBuf, editor: &mut Editor) -> Self {
        let last_saved = editor.persisted_json();
        Autosave { path, last_saved }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes if the state changed since the last write, or always if
    /// `force`. Returns whether it wrote.
    pub fn sync(&mut self, editor: &mut Editor, force: bool) -> io::Result<bool> {
        let json = editor.persisted_json();
        if !force && json == self.last_saved {
            return Ok(false);
        }
        storage::save(&self.path, &json)?;
        self.last_saved = json;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::config::EditingStyle;
    use model::Outline;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulletrix-autosave-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("outline.json")
    }

    fn reopen(path: &Path) -> Editor {
        Editor::new(storage::load(path).unwrap(), EditingStyle::Modal)
    }

    #[test]
    fn opening_without_changes_never_writes() {
        let path = temp_file("noop");
        let mut ed = Editor::new(Outline::new(), EditingStyle::Modal);
        let mut autosave = Autosave::new(path.clone(), &mut ed);
        assert!(!autosave.sync(&mut ed, false).unwrap());
        assert!(!path.exists());
        assert!(autosave.sync(&mut ed, true).unwrap(), "force always writes");
        assert!(path.exists());
    }

    #[test]
    fn edits_persist_and_reopening_restores_zoom_and_cursor() {
        let path = temp_file("reopen");
        let mut ed = Editor::new(Outline::new(), EditingStyle::Traditional);
        let mut autosave = Autosave::new(path.clone(), &mut ed);
        for c in "parent".chars() {
            ed.apply(Action::InsertChar(c));
        }
        ed.apply(Action::NewChild);
        for c in "child".chars() {
            ed.apply(Action::InsertChar(c));
        }
        ed.apply(Action::MoveUp);
        ed.apply(Action::ZoomIn);
        ed.apply(Action::MoveDown);
        assert!(autosave.sync(&mut ed, false).unwrap());

        let reopened = reopen(&path);
        assert_eq!(reopened.outline.get(reopened.outline.zoom_root()).text, "parent");
        assert_eq!(reopened.outline.get(reopened.selected).text, "child");
        assert_eq!(reopened.cursor, ed.cursor);
    }

    #[test]
    fn reopening_falls_back_to_first_row_when_selected_node_is_gone() {
        let path = temp_file("gone");
        let mut ed = Editor::new(Outline::new(), EditingStyle::Modal);
        let json = ed.persisted_json().replace(
            &format!("\"selected_id\": \"{}\"", ed.outline.get(ed.selected).uuid),
            "\"selected_id\": \"no-such-node\"",
        );
        storage::save(&path, &json).unwrap();

        let reopened = reopen(&path);
        let first = reopened.outline.flatten()[0].node;
        assert_eq!(reopened.selected, first);
    }
}
