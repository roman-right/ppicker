//! Persistent project list and user configuration.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "ppicker").context("cannot determine config directory")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub last_opened: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default, rename = "project")]
    pub projects: Vec<Project>,
}

impl Store {
    pub fn path() -> Result<PathBuf> {
        Ok(dirs()?.data_dir().join("projects.toml"))
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
    pub fn add(&mut self, path: &Path, name: Option<String>, tags: Vec<String>) -> Result<bool> {
        let path = fs::canonicalize(path)
            .with_context(|| format!("path does not exist: {}", path.display()))?;
        let path = strip_unc(path);
        let name = name.unwrap_or_else(|| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
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

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.projects.len();
        self.projects.retain(|p| !matches_key(p, key));
        self.projects.len() != before
    }

    pub fn find(&self, key: &str) -> Option<&Project> {
        self.projects.iter().find(|p| matches_key(p, key))
    }

    pub fn touch(&mut self, path: &Path) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.path == path) {
            p.last_opened = Some(now());
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
    /// Commands used for the "open in ..." actions, first is the default.
    pub ides: Vec<String>,
    /// Directories scanned by `pp scan` when none is given.
    pub scan_dirs: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ides: vec!["code".into()],
            scan_dirs: vec![],
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        Ok(dirs()?.config_dir().join("config.toml"))
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
                },
            ],
        };
        assert_eq!(s.merge(other), 1);
        assert_eq!(s.projects.len(), 2);
        assert_eq!(s.projects[0].tags, vec!["a", "b"]);
        assert_eq!(s.projects[0].last_opened, Some(9));
    }

    #[test]
    fn toml_roundtrip() {
        let mut s = Store::default();
        s.projects.push(Project {
            name: "x".into(),
            path: "/x".into(),
            tags: vec!["t".into()],
            last_opened: Some(1),
        });
        let back: Store = toml::from_str(&toml::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back.projects[0].tags, vec!["t"]);
    }
}
