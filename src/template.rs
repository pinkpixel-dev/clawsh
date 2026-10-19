//! Workspace templates: a folder plus named tabs, each with optional
//! `claude` flags and an opening prompt.
//!
//! One TOML file per template in `$XDG_CONFIG_HOME/clawsh/templates/` (or
//! `~/.config/clawsh/templates/`). `ctrl+b t` opens one and `ctrl+b T` saves
//! the current workspace as one.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::picker::expand_home;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    /// The workspace folder. `~` is expanded.
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<Tab>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    /// Shown as if set with `ctrl+b ,`. Left out, the tab names itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Extra `claude` flags, like `["--model", "opus"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Sent as the first prompt when the tab starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

impl Tab {
    /// What follows `claude`: the flags, then the prompt as the positional
    /// argument, which Claude sends as the session's first message.
    pub fn spawn_args(&self) -> Vec<String> {
        let mut args = self.args.clone();
        args.extend(self.prompt.iter().filter(|p| !p.trim().is_empty()).cloned());
        args
    }
}

impl Template {
    pub fn folder(&self) -> PathBuf {
        expand_home(self.path.trim())
    }

    /// The tabs to open. A template with none still gets one plain tab.
    pub fn tabs_or_default(&self) -> Vec<Tab> {
        if self.tabs.is_empty() {
            vec![Tab::default()]
        } else {
            self.tabs.clone()
        }
    }
}

pub fn default_dir() -> Option<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(config.join("clawsh/templates"))
}

pub fn load(path: &Path) -> Result<Template> {
    let raw = fs::read_to_string(path).with_context(|| format!("can't read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("{} isn't a valid template", path.display()))
}

/// Every `.toml` file in `dir`, by name. Broken files are listed with their
/// error so they don't just vanish from the picker.
pub fn list(dir: &Path) -> Vec<(PathBuf, Result<Template, String>)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .map(|p| {
            let t = load(&p).map_err(|e| format!("{:#}", e.root_cause()));
            (p, t)
        })
        .collect();
    out.sort_by_key(|(p, t)| match t {
        Ok(t) => t.name.to_lowercase(),
        Err(_) => p.file_stem().unwrap_or_default().to_string_lossy().to_lowercase(),
    });
    out
}

/// Writes `template` to `<dir>/<slug of its name>.toml`, replacing a template
/// saved under the same name. Returns the file it wrote.
pub fn save(dir: &Path, template: &Template) -> Result<PathBuf> {
    let slug = slug(&template.name);
    anyhow::ensure!(!slug.is_empty(), "the name needs a letter or digit");
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{slug}.toml"));
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, toml::to_string_pretty(template)?)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Lowercase letters, digits, and single dashes, for a file name.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// `path` with the home folder written as `~`, so templates read nicely.
pub fn display_path(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) && path != home => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-template-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn parses_a_hand_written_template() {
        let t: Template = toml::from_str(
            r#"
            name = "slate"
            path = "~/code/slate"

            [[tabs]]
            name = "main"

            [[tabs]]
            args = ["--model", "opus"]
            prompt = "Read DOCS/ROADMAP.md"
            "#,
        )
        .unwrap();
        assert_eq!(t.tabs.len(), 2);
        assert_eq!(t.tabs[0].name.as_deref(), Some("main"));
        assert!(t.tabs[0].spawn_args().is_empty());
        assert_eq!(t.tabs[1].spawn_args(), ["--model", "opus", "Read DOCS/ROADMAP.md"]);
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(t.folder(), PathBuf::from(home).join("code/slate"));
        }
    }

    #[test]
    fn no_tabs_still_opens_one() {
        let t: Template = toml::from_str("name = \"x\"\npath = \"/tmp\"").unwrap();
        assert_eq!(t.tabs_or_default(), [Tab::default()]);
    }

    #[test]
    fn blank_prompts_are_not_sent() {
        let tab = Tab { prompt: Some("  ".into()), ..Tab::default() };
        assert!(tab.spawn_args().is_empty());
    }

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(slug("Slate: Review!"), "slate-review");
        assert_eq!(slug("  ../../etc  "), "etc");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn saves_lists_and_replaces_by_name() {
        let dir = temp("save");
        let mut t = Template {
            name: "Quota".into(),
            path: "/p/quota".into(),
            tabs: vec![Tab { name: Some("main".into()), ..Tab::default() }],
        };
        let path = save(&dir, &t).unwrap();
        assert_eq!(path, dir.join("quota.toml"));
        assert_eq!(load(&path).unwrap(), t);

        t.tabs.push(Tab::default());
        save(&dir, &t).unwrap();
        fs::write(dir.join("broken.toml"), "name = ").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let listed = list(&dir);
        assert_eq!(listed.len(), 2, "same name replaced, .txt skipped");
        assert!(listed[0].1.is_err(), "broken sorts by file name");
        assert_eq!(listed[1].1.as_ref().unwrap().tabs.len(), 2);
        assert!(save(&dir, &Template { name: "??".into(), ..t }).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
