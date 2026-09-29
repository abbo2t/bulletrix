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

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigFile {
    editing_style: EditingStyle,
}

pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".bulletrix").join("config.toml"))
}

/// `--style` on the command line overrides `editing_style` in the config file;
/// a missing config file means the default.
pub fn resolve_style(
    args: impl IntoIterator<Item = String>,
    config_path: Option<&Path>,
) -> Result<EditingStyle, String> {
    let mut args = args.into_iter();
    let mut flag = None;
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--style=") {
            flag = Some(value.to_string());
        } else if arg == "--style" {
            flag = Some(args.next().ok_or("--style needs a value")?);
        } else {
            return Err(format!("unrecognized argument {arg:?}"));
        }
    }
    if let Some(value) = flag {
        return value.parse();
    }

    let Some(path) = config_path else {
        return Ok(EditingStyle::default());
    };
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str::<ConfigFile>(&contents)
            .map(|c| c.editing_style)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(EditingStyle::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("bulletrix-config-test-{}-{name}.toml", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_to_modal_when_no_flag_and_no_file() {
        let missing = std::env::temp_dir().join("bulletrix-definitely-missing.toml");
        assert_eq!(resolve_style(args(&[]), Some(&missing)), Ok(EditingStyle::Modal));
    }

    #[test]
    fn reads_style_from_config_file() {
        let path = write_config("file", "editing_style = \"traditional\"\n");
        assert_eq!(resolve_style(args(&[]), Some(&path)), Ok(EditingStyle::Traditional));
    }

    #[test]
    fn flag_overrides_config_file_in_both_spellings() {
        let path = write_config("override", "editing_style = \"traditional\"\n");
        assert_eq!(resolve_style(args(&["--style", "modal"]), Some(&path)), Ok(EditingStyle::Modal));
        assert_eq!(resolve_style(args(&["--style=modal"]), Some(&path)), Ok(EditingStyle::Modal));
    }

    #[test]
    fn rejects_unknown_values_and_misspelled_keys() {
        assert!(resolve_style(args(&["--style", "emacs"]), None).is_err());
        let path = write_config("typo", "editing-style = \"traditional\"\n");
        assert!(resolve_style(args(&[]), Some(&path)).is_err());
    }
}
