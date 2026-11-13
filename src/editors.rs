//! Editors: what they can open, and how to find their command on this machine.
//!
//! A project is either a folder or a workspace file (`.code-workspace`,
//! `.sublime-project`). Workspace files only make sense in one editor family,
//! so an editor can only be offered for the projects it understands.
//!
//! Editor commands are named differently on every system (`code`, `code.cmd`,
//! `codium`, a flatpak...). A config entry is therefore either a plain
//! command, or a *logical name* such as `vscode` that stands for a list of
//! candidate commands; the first one found on `PATH` is used.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// What a project path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    VsCodeWorkspace,
    SublimeProject,
}

impl Kind {
    /// Decided by the file extension; anything else is treated as a folder.
    pub fn of(path: &Path) -> Kind {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("code-workspace") => Kind::VsCodeWorkspace,
            Some("sublime-project") => Kind::SublimeProject,
            _ => Kind::Folder,
        }
    }

    pub fn is_workspace(self) -> bool {
        self != Kind::Folder
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Folder => "folder",
            Kind::VsCodeWorkspace => "VS Code workspace",
            Kind::SublimeProject => "Sublime project",
        }
    }

    /// Which config entry would make this kind openable, for error messages.
    pub fn needs(self) -> &'static str {
        match self {
            Kind::Folder => "any editor",
            Kind::VsCodeWorkspace => "a VS Code editor (e.g. `vscode`)",
            Kind::SublimeProject => "Sublime Text (e.g. `sublime`)",
        }
    }
}

/// A group of editors that share what they can open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    VsCode,
    JetBrains,
    Sublime,
    Zed,
    Other,
}

impl Family {
    pub fn can_open(self, kind: Kind) -> bool {
        match kind {
            Kind::Folder => true,
            Kind::VsCodeWorkspace => self == Family::VsCode,
            Kind::SublimeProject => self == Family::Sublime,
        }
    }
}

use Family::*;

/// Logical editor names: family and the commands to try, in order.
const ALIASES: &[(&str, Family, &[&str])] = &[
    (
        "vscode",
        VsCode,
        &[
            "code",
            "code-insiders",
            "codium",
            "vscodium",
            "flatpak run com.visualstudio.code",
        ],
    ),
    ("cursor", VsCode, &["cursor"]),
    ("windsurf", VsCode, &["windsurf"]),
    (
        "pycharm",
        JetBrains,
        &[
            "pycharm",
            "pycharm-professional",
            "pycharm-community",
            "charm",
            "pycharm64",
            "flatpak run com.jetbrains.PyCharm-Community",
            "flatpak run com.jetbrains.PyCharm-Professional",
        ],
    ),
    (
        "idea",
        JetBrains,
        &[
            "idea",
            "intellij-idea-ultimate",
            "intellij-idea-community",
            "idea64",
        ],
    ),
    ("webstorm", JetBrains, &["webstorm", "webstorm64"]),
    ("goland", JetBrains, &["goland", "goland64"]),
    ("rustrover", JetBrains, &["rustrover", "rustrover64"]),
    ("clion", JetBrains, &["clion", "clion64"]),
    ("phpstorm", JetBrains, &["phpstorm", "phpstorm64"]),
    ("rider", JetBrains, &["rider", "rider64"]),
    ("rubymine", JetBrains, &["rubymine", "rubymine64"]),
    (
        "sublime",
        Sublime,
        &["subl", "sublime_text", "sublime-text"],
    ),
    ("zed", Zed, &["zed", "zeditor", "zedit"]),
];

/// Family of a plain command, from its file name (`C:\...\Code.exe` -> VsCode).
pub fn family_of_command(cmd: &str) -> Family {
    // Quote-aware, like the command is when it is run: a path with spaces
    // must be written "C:\Program Files\...\Code.exe".
    let tokens = split_args(cmd);
    let first = tokens.first().map(String::as_str).unwrap_or("");
    let file = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let name = file.to_ascii_lowercase();
    let name = name
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .trim_end_matches(".bat")
        .trim_end_matches(".sh");
    match name {
        "code" | "code-insiders" | "codium" | "vscodium" | "cursor" | "windsurf" | "positron" => {
            VsCode
        }
        "subl" | "sublime_text" | "sublime-text" => Sublime,
        "zed" | "zeditor" => Zed,
        n if ["pycharm", "charm", "idea", "intellij-idea"]
            .iter()
            .any(|p| n.starts_with(p))
            || [
                "webstorm",
                "goland",
                "rustrover",
                "clion",
                "phpstorm",
                "rider",
                "rubymine",
                "datagrip",
                "dataspell",
                "studio",
            ]
            .contains(&n) =>
        {
            JetBrains
        }
        _ => Other,
    }
}

/// An editor as named in the config, ready to be started.
#[derive(Clone, Debug)]
pub struct Editor {
    /// The name as written in the config; this is what is shown and remembered.
    pub label: String,
    pub family: Family,
    /// Command lines to try in order; the first whose program exists wins.
    candidates: Vec<String>,
}

impl Editor {
    /// `overrides` is the config's `[commands]` table: it replaces the built-in
    /// candidates of a logical name, or defines a new name.
    pub fn parse(label: &str, overrides: &HashMap<String, Vec<String>>) -> Editor {
        let builtin = ALIASES.iter().find(|(n, _, _)| *n == label);
        let custom = overrides.get(label);
        let candidates: Vec<String> = match (custom, builtin) {
            (Some(c), _) => c.clone(),
            (None, Some((_, _, c))) => c.iter().map(|s| s.to_string()).collect(),
            (None, None) => vec![label.to_string()],
        };
        let family = match builtin {
            Some((_, f, _)) => *f,
            None => family_of_command(candidates.first().map(String::as_str).unwrap_or(label)),
        };
        Editor {
            label: label.to_string(),
            family,
            candidates,
        }
    }

    pub fn supports(&self, kind: Kind) -> bool {
        self.family.can_open(kind)
    }

    /// The program and its leading arguments, found through `find`.
    pub fn resolve_with(
        &self,
        find: impl Fn(&str) -> Option<PathBuf>,
    ) -> Result<(PathBuf, Vec<String>)> {
        for cand in &self.candidates {
            let mut parts = split_args(cand);
            if parts.is_empty() {
                continue;
            }
            let program = parts.remove(0);
            if let Some(found) = find(&program) {
                return Ok((found, parts));
            }
        }
        if self.candidates.len() == 1 && self.candidates[0] == self.label {
            bail!("command not found: {}", self.label);
        }
        let tried: Vec<&str> = self
            .candidates
            .iter()
            .map(|c| c.split_whitespace().next().unwrap_or(c))
            .collect();
        bail!(
            "no command for `{}` found on PATH (tried: {})",
            self.label,
            tried.join(", ")
        )
    }

    /// Resolve against the real `PATH` (on Windows this honours `.cmd`/`.exe`).
    pub fn resolve(&self) -> Result<(PathBuf, Vec<String>)> {
        self.resolve_with(|name| which::which(name).ok())
    }
}

/// Split on whitespace, keeping `"quoted groups"` together. No backslash
/// escapes, so Windows paths survive intact.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashMap<String, Vec<String>> {
        HashMap::new()
    }

    #[test]
    fn kind_comes_from_the_extension() {
        assert_eq!(
            Kind::of(Path::new("/w/shop.code-workspace")),
            Kind::VsCodeWorkspace
        );
        assert_eq!(
            Kind::of(Path::new("C:\\w\\Shop.CODE-WORKSPACE")),
            Kind::VsCodeWorkspace
        );
        assert_eq!(
            Kind::of(Path::new("/w/a.sublime-project")),
            Kind::SublimeProject
        );
        assert_eq!(Kind::of(Path::new("/w/folder")), Kind::Folder);
        assert_eq!(Kind::of(Path::new("/w/folder.d")), Kind::Folder);
    }

    #[test]
    fn family_is_inferred_from_the_command_name() {
        assert_eq!(family_of_command("code"), VsCode);
        assert_eq!(family_of_command("code -n"), VsCode);
        assert_eq!(
            family_of_command(
                r#""C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe" -n"#
            ),
            VsCode
        );
        assert_eq!(family_of_command("/usr/bin/codium"), VsCode);
        assert_eq!(family_of_command("pycharm64.exe"), JetBrains);
        assert_eq!(family_of_command("pycharm-community"), JetBrains);
        assert_eq!(family_of_command("idea"), JetBrains);
        assert_eq!(family_of_command("subl"), Sublime);
        assert_eq!(family_of_command("nvim"), Other);
        assert_eq!(family_of_command("alacritty -e nvim"), Other);
    }

    #[test]
    fn only_the_right_family_opens_a_workspace() {
        let vs = Editor::parse("vscode", &none());
        let py = Editor::parse("pycharm", &none());
        let raw = Editor::parse("code", &none());
        assert!(vs.supports(Kind::VsCodeWorkspace) && vs.supports(Kind::Folder));
        assert!(raw.supports(Kind::VsCodeWorkspace));
        assert!(!py.supports(Kind::VsCodeWorkspace) && py.supports(Kind::Folder));
        assert!(!vs.supports(Kind::SublimeProject));
    }

    #[test]
    fn logical_name_tries_candidates_in_order() {
        let vs = Editor::parse("vscode", &none());
        // Only codium is installed on this "system".
        let (p, args) = vs
            .resolve_with(|n| (n == "codium").then(|| PathBuf::from("/usr/bin/codium")))
            .unwrap();
        assert_eq!(p, PathBuf::from("/usr/bin/codium"));
        assert!(args.is_empty());
        // Both installed: the first candidate wins.
        let (p, _) = vs
            .resolve_with(|n| Some(PathBuf::from(format!("/bin/{n}"))))
            .unwrap();
        assert_eq!(p, PathBuf::from("/bin/code"));
    }

    #[test]
    fn flatpak_candidate_keeps_its_arguments() {
        let vs = Editor::parse("vscode", &none());
        let (p, args) = vs
            .resolve_with(|n| (n == "flatpak").then(|| PathBuf::from("/usr/bin/flatpak")))
            .unwrap();
        assert_eq!(p, PathBuf::from("/usr/bin/flatpak"));
        assert_eq!(args, vec!["run", "com.visualstudio.code"]);
    }

    #[test]
    fn not_found_errors_say_what_was_tried() {
        let e = Editor::parse("vscode", &none())
            .resolve_with(|_| None)
            .unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("vscode") && msg.contains("codium"), "{msg}");
        let e = Editor::parse("nvim-qt", &none())
            .resolve_with(|_| None)
            .unwrap_err();
        assert_eq!(e.to_string(), "command not found: nvim-qt");
    }

    #[test]
    fn config_overrides_candidates_and_can_define_new_names() {
        let mut o = none();
        o.insert("vscode".into(), vec!["C:\\Tools\\Code.exe".into()]);
        o.insert("work".into(), vec!["codium".into(), "code".into()]);
        let vs = Editor::parse("vscode", &o);
        let (p, _) = vs.resolve_with(|n| Some(PathBuf::from(n))).unwrap();
        assert_eq!(p, PathBuf::from("C:\\Tools\\Code.exe"));
        assert_eq!(vs.family, VsCode); // still a VS Code editor
        let work = Editor::parse("work", &o);
        assert_eq!(work.family, VsCode); // inferred from `codium`
        assert!(work.supports(Kind::VsCodeWorkspace));
    }

    #[test]
    fn split_args_handles_quotes_and_windows_paths() {
        assert_eq!(split_args("code -n"), vec!["code", "-n"]);
        assert_eq!(
            split_args(r#""C:\Program Files\Code\Code.exe" -n"#),
            vec![r"C:\Program Files\Code\Code.exe", "-n"]
        );
        assert_eq!(split_args("  a   b  "), vec!["a", "b"]);
        assert_eq!(split_args(""), Vec::<String>::new());
    }
}
