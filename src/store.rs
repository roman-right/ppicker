//! Persistent project list and user configuration.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::editors::{Editor, Kind};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "ppicker").context("cannot determine config directory")
}

/// `(config dir, data dir)`. Setting `PPICKER_HOME` keeps everything under that
/// one folder (`config/` and `data/`): handy for tests and portable installs.
fn base_dirs() -> Result<(PathBuf, PathBuf)> {
    if let Some(home) = std::env::var_os("PPICKER_HOME") {
        let home = PathBuf::from(home);
        return Ok((home.join("config"), home.join("data")));
    }
    let d = dirs()?;
    Ok((d.config_dir().to_path_buf(), d.data_dir().to_path_buf()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub last_opened: Option<i64>,
    /// Editor remembered for this project: the one it was last opened with, or
    /// one chosen by hand. `None` means the global default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide: Option<String>,
}

impl Project {
    /// A folder, or a workspace file such as `shop.code-workspace`.
    pub fn kind(&self) -> Kind {
        Kind::of(&self.path)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default, rename = "project")]
    pub projects: Vec<Project>,
}

impl Store {
    pub fn path() -> Result<PathBuf> {
        Ok(base_dirs()?.1.join("projects.toml"))
    }

    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        // Write atomically so a crash never leaves a truncated list.
        let tmp = p.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    /// Add a project, or update name/tags if the path is already known.
    /// The path is a folder or a workspace file (`.code-workspace`,
    /// `.sublime-project`); other files are rejected.
    pub fn add(&mut self, path: &Path, name: Option<String>, tags: Vec<String>) -> Result<bool> {
        let path = canonical(path)?;
        let kind = Kind::of(&path);
        if path.is_file() && !kind.is_workspace() {
            bail!(
                "{} is a file, not a folder or a known workspace file (.code-workspace, .sublime-project)",
                path.display()
            );
        }
        if path.is_dir() && kind.is_workspace() {
            bail!(
                "{} is a folder, but its name looks like a workspace file",
                path.display()
            );
        }
        let name = name.unwrap_or_else(|| {
            // `shop.code-workspace` is called "shop"; folders keep their name.
            let part = if kind.is_workspace() {
                path.file_stem()
            } else {
                path.file_name()
            };
            part.map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        });
        if let Some(existing) = self.projects.iter_mut().find(|p| p.path == path) {
            existing.name = name;
            if !tags.is_empty() {
                existing.tags = tags;
            }
            return Ok(false);
        }
        self.projects.push(Project {
            name,
            path,
            tags,
            last_opened: None,
            ide: None,
        });
        Ok(true)
    }

    /// Merge projects from another list. Existing paths keep their data but
    /// gain tags and the more recent `last_opened`. Returns how many are new.
    pub fn merge(&mut self, other: Store) -> usize {
        let mut added = 0;
        for p in other.projects {
            match self.projects.iter_mut().find(|e| e.path == p.path) {
                Some(e) => {
                    for t in p.tags {
                        if !e.tags.contains(&t) {
                            e.tags.push(t);
                        }
                    }
                    if e.ide.is_none() {
                        e.ide = p.ide;
                    }
                    e.last_opened = e.last_opened.max(p.last_opened);
                }
                None => {
                    self.projects.push(p);
                    added += 1;
                }
            }
        }
        added
    }

    /// Pin (or with `None`, unpin) an editor for one project.
    pub fn set_ide(&mut self, path: &Path, ide: Option<String>) -> bool {
        match self.projects.iter_mut().find(|p| p.path == path) {
            Some(p) => {
                p.ide = ide;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.projects.len();
        self.projects.retain(|p| !matches_key(p, key));
        self.projects.len() != before
    }

    pub fn find(&self, key: &str) -> Option<&Project> {
        self.projects.iter().find(|p| matches_key(p, key))
    }

    /// Note that a project was just launched. If it was opened with an editor,
    /// that editor is remembered as the project's editor for next time.
    pub fn record_launch(&mut self, path: &Path, ide: Option<&str>) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.path == path) {
            p.last_opened = Some(now());
            if let Some(ide) = ide {
                p.ide = Some(ide.to_string());
            }
        }
    }

    /// Most recently used first; never-opened projects go last, by name.
    pub fn sort_recent(&mut self) {
        self.projects.sort_by(|a, b| {
            b.last_opened
                .cmp(&a.last_opened)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
    }

    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.projects.iter().flat_map(|p| p.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        tags
    }
}

fn matches_key(p: &Project, key: &str) -> bool {
    p.name == key || p.path.to_string_lossy() == key
}

/// Absolute, symlink-resolved form used as a project's identity.
pub fn canonical(path: &Path) -> Result<PathBuf> {
    let abs = fs::canonicalize(path)
        .with_context(|| format!("path does not exist: {}", path.display()))?;
    Ok(strip_unc(abs))
}

/// `canonicalize` on Windows yields `\\?\C:\...`; keep paths readable.
fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Editors, by command or logical name (`vscode`, `pycharm`, ...).
    /// The first one that can open a project is its default.
    pub ides: Vec<String>,
    /// Directories scanned by `pp scan` when none is given.
    pub scan_dirs: Vec<PathBuf>,
    /// Overrides for what a name runs: `vscode = ["code-insiders", "code"]`
    /// (the first command found on PATH is used). Can also define new names.
    pub commands: HashMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ides: vec!["code".into()],
            scan_dirs: vec![],
            commands: HashMap::new(),
        }
    }
}

impl Config {
    pub fn editor(&self, label: &str) -> Editor {
        Editor::parse(label, &self.commands)
    }

    /// Can the editor named `label` open this kind of project?
    pub fn supports(&self, label: &str, kind: Kind) -> bool {
        self.editor(label).supports(kind)
    }

    /// Configured editors that can open `kind`, in config order.
    pub fn compatible(&self, kind: Kind) -> Vec<String> {
        self.ides
            .iter()
            .filter(|i| self.supports(i, kind))
            .cloned()
            .collect()
    }

    /// The editor used for `kind` when a project has none remembered.
    pub fn default_ide(&self, kind: Kind) -> Option<&str> {
        self.ides
            .iter()
            .find(|i| self.supports(i, kind))
            .map(String::as_str)
    }

    /// Editors to offer for a project, remembered one first, then the rest.
    /// Only editors that can open `kind` appear.
    pub fn choices(&self, remembered: Option<&str>, kind: Kind) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(r) = remembered
            && self.supports(r, kind)
        {
            out.push(r.to_string());
        }
        for ide in self.compatible(kind) {
            if !out.contains(&ide) {
                out.push(ide);
            }
        }
        out
    }

    /// The editor that opens `p`, and whether it is remembered for it. `None`
    /// when no configured editor can open this kind of project.
    pub fn effective_ide<'a>(&'a self, p: &'a Project) -> Option<(&'a str, bool)> {
        let kind = p.kind();
        if let Some(ide) = p.ide.as_deref()
            && self.supports(ide, kind)
        {
            return Some((ide, true));
        }
        self.default_ide(kind).map(|d| (d, false))
    }

    pub fn path() -> Result<PathBuf> {
        Ok(base_dirs()?.0.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedups_by_path_and_updates() {
        let dir = std::env::temp_dir();
        let mut s = Store::default();
        assert!(s.add(&dir, Some("tmp".into()), vec![]).unwrap());
        assert!(
            !s.add(&dir, Some("renamed".into()), vec!["x".into()])
                .unwrap()
        );
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].name, "renamed");
        assert_eq!(s.projects[0].tags, vec!["x"]);
    }

    #[test]
    fn missing_path_is_an_error() {
        let mut s = Store::default();
        assert!(
            s.add(Path::new("/definitely/not/here"), None, vec![])
                .is_err()
        );
    }

    #[test]
    fn recent_first_then_name() {
        let mk = |n: &str, t| Project {
            name: n.into(),
            path: n.into(),
            tags: vec![],
            last_opened: t,
            ide: None,
        };
        let mut s = Store {
            projects: vec![mk("b", None), mk("a", None), mk("c", Some(5))],
        };
        s.sort_recent();
        let names: Vec<_> = s.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }

    #[test]
    fn merge_adds_new_and_enriches_existing() {
        let mk = |tags: &[&str], t| Project {
            name: "x".into(),
            path: "/x".into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            last_opened: t,
            ide: None,
        };
        let mut s = Store {
            projects: vec![mk(&["a"], Some(5))],
        };
        let other = Store {
            projects: vec![
                mk(&["a", "b"], Some(9)),
                Project {
                    name: "y".into(),
                    path: "/y".into(),
                    tags: vec![],
                    last_opened: None,
                    ide: None,
                },
            ],
        };
        assert_eq!(s.merge(other), 1);
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.projects[0].tags, vec!["a", "b"]);
        assert_eq!(s.projects[0].last_opened, Some(9));
    }

    #[test]
    fn launching_with_an_editor_remembers_it_other_actions_do_not() {
        let mk = |ide: Option<&str>| Project {
            name: "x".into(),
            path: "/x".into(),
            tags: vec![],
            last_opened: None,
            ide: ide.map(String::from),
        };
        let mut s = Store {
            projects: vec![mk(None)],
        };
        let p = Path::new("/x");

        s.record_launch(p, Some("vim"));
        assert_eq!(s.projects[0].ide.as_deref(), Some("vim"));
        assert!(s.projects[0].last_opened.is_some());

        // Running `git pull` (no editor) must not forget it.
        s.record_launch(p, None);
        assert_eq!(s.projects[0].ide.as_deref(), Some("vim"));

        // Opening with a different editor replaces it.
        s.record_launch(p, Some("code"));
        assert_eq!(s.projects[0].ide.as_deref(), Some("code"));
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pp_store_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn workspace_files_can_be_added_and_are_named_after_the_file() {
        let d = scratch("ws");
        let ws = d.join("shop.code-workspace");
        fs::write(&ws, "{}").unwrap();
        let mut s = Store::default();
        assert!(s.add(&ws, None, vec![]).unwrap());
        assert_eq!(s.projects[0].name, "shop");
        assert_eq!(s.projects[0].kind(), Kind::VsCodeWorkspace);
        // The folder next to it is a separate, ordinary project.
        assert!(s.add(&d, None, vec![]).unwrap());
        assert_eq!(s.projects[1].kind(), Kind::Folder);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn plain_files_are_rejected() {
        let d = scratch("file");
        let f = d.join("notes.txt");
        fs::write(&f, "x").unwrap();
        let err = Store::default().add(&f, None, vec![]).unwrap_err();
        assert!(err.to_string().contains("not a folder"), "{err}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn editors_are_offered_only_for_what_they_can_open() {
        let cfg = Config {
            ides: vec!["code".into(), "pycharm".into(), "vim".into()],
            ..Config::default()
        };
        assert_eq!(cfg.compatible(Kind::Folder), vec!["code", "pycharm", "vim"]);
        assert_eq!(cfg.compatible(Kind::VsCodeWorkspace), vec!["code"]);
        assert_eq!(cfg.default_ide(Kind::VsCodeWorkspace), Some("code"));
        // A remembered editor that cannot open this kind is ignored.
        assert_eq!(
            cfg.choices(Some("pycharm"), Kind::VsCodeWorkspace),
            vec!["code"]
        );
        assert_eq!(
            cfg.choices(Some("pycharm"), Kind::Folder),
            vec!["pycharm", "code", "vim"]
        );

        let only_pycharm = Config {
            ides: vec!["pycharm".into()],
            ..Config::default()
        };
        assert_eq!(only_pycharm.default_ide(Kind::VsCodeWorkspace), None);
        assert!(only_pycharm.compatible(Kind::SublimeProject).is_empty());
    }

    #[test]
    fn effective_editor_falls_back_when_the_remembered_one_does_not_fit() {
        let cfg = Config {
            ides: vec!["code".into(), "pycharm".into()],
            ..Config::default()
        };
        let mut p = Project {
            name: "shop".into(),
            path: "/w/shop.code-workspace".into(),
            tags: vec![],
            last_opened: None,
            ide: Some("pycharm".into()),
        };
        assert_eq!(cfg.effective_ide(&p), Some(("code", false)));
        p.ide = Some("code".into());
        assert_eq!(cfg.effective_ide(&p), Some(("code", true)));
    }

    #[test]
    fn toml_roundtrip() {
        let mut s = Store::default();
        s.projects.push(Project {
            name: "x".into(),
            path: "/x".into(),
            tags: vec!["t".into()],
            last_opened: Some(1),
            ide: None,
        });
        let back: Store = toml::from_str(&toml::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back.projects[0].tags, vec!["t"]);
    }
}
