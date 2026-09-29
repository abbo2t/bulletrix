//! Load/save an outline as JSON with atomic writes (port of storage.py).

use crate::Outline;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

/// A missing file is a fresh outline; an unreadable or malformed one is an
/// error, so the caller never silently replaces real data with an empty outline.
pub fn load(path: &Path) -> io::Result<Outline> {
    match std::fs::read_to_string(path) {
        Ok(json) => Outline::from_json_str(&json).map_err(|e| io::Error::new(ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Outline::new()),
        Err(e) => Err(e),
    }
}

/// Writes to a sibling `.tmp` file and renames it over `path`, so a crash
/// mid-write never leaves a truncated outline behind.
pub fn save(path: &Path, json: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulletrix-storage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn save_creates_parent_dirs_and_round_trips() {
        let path = temp_dir("roundtrip").join("nested").join("outline.json");
        let mut outline = Outline::new();
        let first = outline.get(outline.root()).children[0];
        outline.get_mut(first).text = "hello".into();

        save(&path, &outline.to_json_string()).unwrap();
        let loaded = load(&path).unwrap();
        let first = loaded.get(loaded.root()).children[0];
        assert_eq!(loaded.get(first).text, "hello");
        assert!(!tmp_path(&path).exists());
    }

    #[test]
    fn missing_file_loads_a_fresh_outline() {
        let loaded = load(&temp_dir("missing").join("outline.json")).unwrap();
        assert_eq!(loaded.get(loaded.root()).children.len(), 1);
    }

    #[test]
    fn malformed_file_is_an_error_not_an_empty_outline() {
        let dir = temp_dir("malformed");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("outline.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = load(&path).err().expect("malformed JSON must not load");
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }
}
