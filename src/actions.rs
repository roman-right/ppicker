//! Per-project actions: IDE launchers plus auto-detected runners.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::store::Config;

#[derive(Debug, Clone)]
pub struct Action {
    pub label: String,
    pub cmd: String,
    /// Detached actions (IDEs) are spawned in the background; the rest run
    /// in the foreground of the terminal that launched `pp`.
    pub detached: bool,
    /// Set for "Open in <editor>" actions; holds the editor command.
    pub ide: Option<String>,
}

#[derive(Deserialize, Default)]
struct LocalFile {
    #[serde(default, rename = "action")]
    actions: Vec<LocalAction>,
}

#[derive(Deserialize)]
struct LocalAction {
    label: String,
    cmd: String,
    #[serde(default)]
    detached: bool,
}

fn has(dir: &Path, name: &str) -> bool {
    dir.join(name).exists()
}

fn run(label: &str, cmd: &str) -> Action {
    Action {
        label: label.into(),
        cmd: cmd.into(),
        detached: false,
        ide: None,
    }
}

/// Collect actions for a project. Order: editors (the project's pinned one
/// first, so action 1 and `Ctrl-o` always open the right editor), then
/// user-defined actions, then detected runners.
pub fn detect(dir: &Path, cfg: &Config, pinned_ide: Option<&str>) -> Vec<Action> {
    let mut out = Vec::new();

    // 1. Editors.
    for ide in cfg.ide_choices(pinned_ide) {
        out.push(Action {
            label: format!("Open in {ide}"),
            cmd: format!("{ide} ."),
            detached: true,
            ide: Some(ide),
        });
    }

    // 2. Project-local actions: <project>/.ppicker.toml
    if let Ok(text) = fs::read_to_string(dir.join(".ppicker.toml"))
        && let Ok(local) = toml::from_str::<LocalFile>(&text)
    {
        out.extend(local.actions.into_iter().map(|a| Action {
            label: a.label,
            cmd: a.cmd,
            detached: a.detached,
            ide: None,
        }));
    }

    // 3. Detected runners.
    if has(dir, "Cargo.toml") {
        out.push(run("cargo run", "cargo run"));
        out.push(run("cargo test", "cargo test"));
    }
    if let Ok(text) = fs::read_to_string(dir.join("package.json")) {
        let pm = if has(dir, "pnpm-lock.yaml") {
            "pnpm"
        } else if has(dir, "yarn.lock") {
            "yarn"
        } else if has(dir, "bun.lockb") || has(dir, "bun.lock") {
            "bun"
        } else {
            "npm"
        };
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(scripts) = json.get("scripts").and_then(|s| s.as_object())
        {
            for name in scripts.keys() {
                out.push(run(
                    &format!("{pm} run {name}"),
                    &format!("{pm} run {name}"),
                ));
            }
        }
    }
    if has(dir, "pyproject.toml") {
        if has(dir, "uv.lock") {
            out.push(run("uv sync", "uv sync"));
            out.push(run("uv run pytest", "uv run pytest"));
        } else {
            out.push(run("pytest", "pytest"));
        }
    }
    if has(dir, "justfile") || has(dir, "Justfile") {
        out.push(run("just", "just"));
    }
    if has(dir, "Makefile") {
        out.push(run("make", "make"));
    }
    if has(dir, "docker-compose.yml") || has(dir, "compose.yaml") {
        out.push(run("docker compose up", "docker compose up"));
    }
    if dir.join(".git").exists() {
        out.push(run("git status", "git status"));
        out.push(run("git pull", "git pull"));
    }
    out.push(run(
        "Shell here",
        if cfg!(windows) { "cmd" } else { "$SHELL" },
    ));
    out
}
