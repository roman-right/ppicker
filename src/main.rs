mod editors;
mod exec;
mod menu;
mod store;
mod tui;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use editors::Kind;
use store::{Config, Project, Store};

#[derive(Parser)]
#[command(
    name = "pp",
    version,
    about = "Fast project picker: open a project in your editor"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add a project: a folder (default: current directory) or a workspace file
    /// (`.code-workspace`, `.sublime-project`)
    Add {
        path: Option<PathBuf>,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        tag: Vec<String>,
        /// Editor command to remember for this project
        #[arg(long)]
        ide: Option<String>,
    },
    /// Show, set or forget the editor remembered for a project
    /// (it is also remembered automatically whenever you open the project)
    Editor {
        project: String,
        /// Editor command, e.g. `pycharm`; omit to just show the current one
        ide: Option<String>,
        /// Forget it and go back to the global default editor
        #[arg(long, conflicts_with = "ide")]
        reset: bool,
    },
    /// Remove a project by name or path
    Rm { project: String },
    /// List projects
    List {
        /// Print tab-separated name, path, tags, editor, kind
        #[arg(long)]
        plain: bool,
    },
    /// Add every git repository found directly inside the given directories
    Scan {
        dirs: Vec<PathBuf>,
        #[arg(short, long)]
        tag: Vec<String>,
        /// Also add workspace files (`*.code-workspace`, `*.sublime-project`) found in
        /// those directories and one level below
        #[arg(short, long)]
        workspaces: bool,
    },
    /// Open a project without the picker, in its editor or the one given
    Open {
        project: String,
        ide: Option<String>,
    },
    /// Pick through an external dmenu-compatible launcher (rofi, wofi, fuzzel, fzf)
    Menu {
        /// Menu command; `{prompt}` is replaced with the step name
        #[arg(long, default_value = menu::DEFAULT_CMD)]
        cmd: String,
        /// Also ask which editor to use, instead of opening in the remembered one
        #[arg(long)]
        ask: bool,
    },
    /// Import projects from a TOML file (`-` for stdin), e.g. from `code-compass migrate`
    Import { file: String },
    /// Print the config and data file locations
    Paths,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("pp: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    let mut store = Store::load()?;

    match cli.command {
        None => {
            if store.projects.is_empty() {
                println!("No projects yet. Try `pp add .` or `pp scan ~/code`.");
                return Ok(());
            }
            store.sort_recent();
            let tags = store.all_tags();
            let projects = store.projects.clone();
            // Editor changes made in the picker are saved the moment they happen.
            let on_ide_change = Box::new(|path: &Path, ide: Option<&str>| {
                store.set_ide(path, ide.map(String::from));
                let _ = store.save();
            });
            let picked = tui::pick(projects, tags, &cfg, on_ide_change)?;
            if let Some(p) = picked {
                launch(&mut store, &cfg, &p.project, &p.ide)?;
            }
        }
        Some(Cmd::Add {
            path,
            name,
            tag,
            ide,
        }) => {
            let path = path.unwrap_or_else(|| PathBuf::from("."));
            if let Some(ide) = &ide {
                // Fail before anything is changed.
                check_editor(&cfg, ide, Kind::of(&store::canonical(&path)?))?;
            }
            let created = store.add(&path, name, tag)?;
            if ide.is_some() {
                store.set_ide(&store::canonical(&path)?, ide);
            }
            store.save()?;
            println!("{}", if created { "added" } else { "updated" });
        }
        Some(Cmd::Editor {
            project,
            ide,
            reset,
        }) => {
            let Some(p) = store.find(&project).cloned() else {
                bail!("no such project: {project}");
            };
            if reset {
                store.set_ide(&p.path, None);
            } else if let Some(ide) = ide {
                check_editor(&cfg, &ide, p.kind())?;
                store.set_ide(&p.path, Some(ide));
            }
            store.save()?;
            let now = store.find(&project).cloned().unwrap_or(p);
            match cfg.effective_ide(&now) {
                Some((ide, true)) => println!("{}: {ide} (remembered)", now.name),
                Some((ide, false)) => println!("{}: {ide} (default)", now.name),
                None => println!(
                    "{}: no editor can open this ({})",
                    now.name,
                    now.kind().label()
                ),
            }
        }
        Some(Cmd::Rm { project }) => {
            if !store.remove(&project) {
                bail!("no such project: {project}");
            }
            store.save()?;
        }
        Some(Cmd::List { plain }) => {
            store.sort_recent();
            for p in &store.projects {
                let ide = cfg.effective_ide(p).map_or("-", |(i, _)| i);
                if plain {
                    let kind = if p.kind().is_workspace() {
                        "workspace"
                    } else {
                        "folder"
                    };
                    println!(
                        "{}\t{}\t{}\t{ide}\t{kind}",
                        p.name,
                        p.path.display(),
                        p.tags.join(",")
                    );
                } else {
                    let mark = if p.kind().is_workspace() { " ◇" } else { "" };
                    println!("{:<28} {:<10} {}{mark}", p.name, ide, p.path.display());
                }
            }
        }
        Some(Cmd::Scan {
            dirs,
            tag,
            workspaces,
        }) => {
            let dirs = if dirs.is_empty() {
                cfg.scan_dirs.clone()
            } else {
                dirs
            };
            if dirs.is_empty() {
                bail!("give a directory, or set `scan_dirs` in the config");
            }
            let mut added = 0;
            for dir in dirs {
                for entry in std::fs::read_dir(&dir)? {
                    let path = entry?.path();
                    if path.join(".git").exists() && store.add(&path, None, tag.clone())? {
                        added += 1;
                    }
                    if workspaces {
                        for ws in workspace_files(&path) {
                            if store.add(&ws, None, tag.clone())? {
                                added += 1;
                            }
                        }
                    }
                }
                if workspaces {
                    for ws in workspace_files(&dir) {
                        if store.add(&ws, None, tag.clone())? {
                            added += 1;
                        }
                    }
                }
            }
            store.save()?;
            println!("added {added} project(s)");
        }
        Some(Cmd::Open { project, ide }) => {
            let Some(p) = store.find(&project).cloned() else {
                bail!("no such project: {project}");
            };
            let ide = match ide {
                Some(ide) => {
                    check_editor(&cfg, &ide, p.kind())?;
                    ide
                }
                None => match cfg.effective_ide(&p) {
                    Some((ide, _)) => ide.to_string(),
                    None => bail!(
                        "no configured editor can open this {}; add {} to `ides`",
                        p.kind().label(),
                        p.kind().needs()
                    ),
                },
            };
            launch(&mut store, &cfg, &p, &ide)?;
        }
        Some(Cmd::Menu { cmd, ask }) => {
            store.sort_recent();
            let lines: Vec<String> = store
                .projects
                .iter()
                .map(|p| format!("{}  ·  {}", p.name, p.path.display()))
                .collect();
            let Some(i) = menu::choose(&cmd, "project", &lines)? else {
                return Ok(());
            };
            let project = store.projects[i].clone();

            let choices = cfg.choices(project.ide.as_deref(), project.kind());
            if choices.is_empty() {
                bail!(
                    "no configured editor can open this {}; add {} to `ides`",
                    project.kind().label(),
                    project.kind().needs()
                );
            }
            let ide = if ask {
                let Some(j) = menu::choose(&cmd, "editor", &choices)? else {
                    return Ok(());
                };
                choices[j].clone()
            } else {
                choices[0].clone()
            };
            launch(&mut store, &cfg, &project, &ide)?;
        }
        Some(Cmd::Import { file }) => {
            let text = if file == "-" {
                std::io::read_to_string(std::io::stdin())?
            } else {
                std::fs::read_to_string(&file)?
            };
            let incoming: Store = toml::from_str(&text)?;
            let total = incoming.projects.len();
            let added = store.merge(incoming);
            store.save()?;
            println!("imported {added} new project(s), {total} in file");
        }
        Some(Cmd::Paths) => {
            println!("config: {}", Config::path()?.display());
            println!("data:   {}", Store::path()?.display());
        }
    }
    Ok(())
}

/// Start the editor first, and only remember it (and the open time) if that
/// worked, so a typo'd or missing command is never saved as the project's editor.
fn launch(store: &mut Store, cfg: &Config, project: &Project, ide: &str) -> Result<()> {
    exec::open(&project.path, project.kind(), ide, cfg)?;
    store.record_launch(&project.path, Some(ide));
    store.save()
}

/// Reject an editor that cannot open this kind of project.
fn check_editor(cfg: &Config, ide: &str, kind: Kind) -> Result<()> {
    if !cfg.supports(ide, kind) {
        bail!(
            "`{ide}` cannot open a {}; use {}",
            kind.label(),
            kind.needs()
        );
    }
    Ok(())
}

/// Workspace files directly inside `dir`.
fn workspace_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && Kind::of(p).is_workspace())
        .collect()
}
