use serde::Deserialize;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditingStyle {
    #[default]
    Modal,
    Traditional,
}

impl FromStr for EditingStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "modal" => Ok(EditingStyle::Modal),
            "traditional" => Ok(EditingStyle::Traditional),
            other => Err(format!(
                "unknown editing style {other:?} (expected \"modal\" or \"traditional\")"
            )),
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    editing_style: EditingStyle,
    mouse: bool,
}

impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            editing_style: EditingStyle::default(),
            mouse: true,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub style: EditingStyle,
    pub file: PathBuf,
    /// Merge this OPML file into `file` and exit instead of starting the TUI.
    pub import: Option<PathBuf>,
    /// Capture the mouse. Off leaves clicks and drags to the terminal (e.g.
    /// for its own text selection).
    pub mouse: bool,
}

/// `~/.bulletrix`, where both the outline and the config file live.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".bulletrix"))
}

/// Parses `--style modal|traditional`, `--file PATH` and `--import OPML_FILE`
/// (each as `--flag value` or `--flag=value`). `--style` overrides the
/// config file's `editing_style`; `--file` defaults to `outline.json` in
/// `data_dir`; `mouse` comes only from the config file.
pub fn resolve(args: impl IntoIterator<Item = String>, data_dir: Option<&Path>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut style_flag = None;
    let mut file_flag = None;
    let mut import_flag = None;
    while let Some(arg) = args.next() {
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        let slot = match name.as_str() {
            "--style" => &mut style_flag,
            "--file" => &mut file_flag,
            "--import" => &mut import_flag,
            _ => return Err(format!("unrecognized argument {arg:?}")),
        };
        let value = match inline_value {
            Some(v) => v,
            None => args.next().ok_or_else(|| format!("{name} needs a value"))?,
        };
        *slot = Some(value);
    }

    let config = read_config(data_dir.map(|d| d.join("config.toml")).as_deref())?;
    let style = match style_flag {
        Some(value) => value.parse()?,
        None => config.editing_style,
    };
    let file = match (file_flag, data_dir) {
        (Some(f), _) => PathBuf::from(f),
        (None, Some(dir)) => dir.join("outline.json"),
        (None, None) => return Err("$HOME is not set; pass --file PATH".into()),
    };
    Ok(Options {
        style,
        file,
        import: import_flag.map(PathBuf::from),
        mouse: config.mouse,
    })
}

fn read_config(config_path: Option<&Path>) -> Result<ConfigFile, String> {
    let Some(path) = config_path else {
        return Ok(ConfigFile::default());
    };
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh data dir, optionally containing a config.toml.
    fn data_dir_with(name: &str, config: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bulletrix-config-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(contents) = config {
            std::fs::write(dir.join("config.toml"), contents).unwrap();
        }
        dir
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_to_modal_and_outline_json_in_the_data_dir() {
        let dir = data_dir_with("defaults", None);
        let opts = resolve(args(&[]), Some(&dir)).unwrap();
        assert_eq!(opts.style, EditingStyle::Modal);
        assert_eq!(opts.file, dir.join("outline.json"));
        assert!(opts.mouse);
    }

    #[test]
    fn reads_style_and_mouse_from_config_file() {
        let dir = data_dir_with("file", Some("editing_style = \"traditional\"\nmouse = false\n"));
        let opts = resolve(args(&[]), Some(&dir)).unwrap();
        assert_eq!(opts.style, EditingStyle::Traditional);
        assert!(!opts.mouse);
    }

    #[test]
    fn flags_override_defaults_in_both_spellings() {
        let dir = data_dir_with("override", Some("editing_style = \"traditional\"\n"));
        let opts = resolve(args(&["--style", "modal", "--file=/tmp/x.json"]), Some(&dir)).unwrap();
        assert_eq!(
            opts,
            Options { style: EditingStyle::Modal, file: PathBuf::from("/tmp/x.json"), import: None, mouse: true }
        );
        let opts = resolve(args(&["--style=modal", "--file", "y.json", "--import", "in.opml"]), Some(&dir)).unwrap();
        assert_eq!(
            opts,
            Options {
                style: EditingStyle::Modal,
                file: PathBuf::from("y.json"),
                import: Some(PathBuf::from("in.opml")),
                mouse: true,
            }
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(resolve(args(&["--style", "emacs"]), None).is_err());
        assert!(resolve(args(&["--file"]), None).is_err());
        assert!(resolve(args(&["--bogus"]), None).is_err());
        assert!(resolve(args(&[]), None).is_err(), "no HOME and no --file");
        let dir = data_dir_with("typo", Some("editing-style = \"traditional\"\n"));
        assert!(resolve(args(&[]), Some(&dir)).is_err());
    }
}
