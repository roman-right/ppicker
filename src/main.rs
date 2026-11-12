mod actions;
mod exec;
mod menu;
mod store;
mod tui;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use store::{Config, Store};

#[derive(Parser)]
#[command(name = "pp", version, about = "Fast project and runner picker")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add a project (default: current directory)
    Add {
        path: Option<PathBuf>,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        tag: Vec<String>,
        /// Editor command to pin for this project (default: the global one)
        #[arg(long)]
        ide: Option<String>,
    },
    /// Show, set or reset the editor pinned for a project
    Editor {
        project: String,
        /// Editor command to pin, e.g. `pycharm`; omit to just show the current one
        ide: Option<String>,
        /// Go back to the global default editor
        #[arg(long, conflicts_with = "ide")]
        reset: bool,
    },
    /// Remove a project by name or path
    Rm { project: String },
    /// List projects
    List {
        /// Print tab-separated name, path, tags
        #[arg(long)]
        plain: bool,
    },
    /// Add every git repository found directly inside the given directories
    Scan {
        dirs: Vec<PathBuf>,
        #[arg(short, long)]
        tag: Vec<String>,
    },
    /// Run an action for a project without the picker (default: first action)
    Run {
        project: String,
        /// Action label (or its 1-based number)
        action: Option<String>,
    },
    /// Pick through an external dmenu-compatible launcher (rofi, wofi, fuzzel, fzf)
    Menu {
        /// Menu command; `{prompt}` is replaced with the step name
        #[arg(long, default_value = menu::DEFAULT_CMD)]
        cmd: String,
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
            let on_ide_change = Box::new(|path: &std::path::Path, ide: Option<&str>| {
                store.set_ide(path, ide.map(String::from));
                let _ = store.save();
            });
            let picked = tui::pick(projects, tags, &cfg, on_ide_change)?;
            if let Some(p) = picked {
                launch(&mut store, &p.project.path, &p.action)?;
            }
        }
        Some(Cmd::Add {
            path,
            name,
            tag,
            ide,
        }) => {
            let path = path.unwrap_or_else(|| PathBuf::from("."));
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
            } else if ide.is_some() {
                store.set_ide(&p.path, ide);
            }
            store.save()?;
            let now = store.find(&project).and_then(|p| p.ide.clone());
            match now {
                Some(ide) => println!("{}: {ide} (pinned)", p.name),
                None => println!("{}: {} (default)", p.name, cfg.default_ide()),
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
                if plain {
                    println!("{}\t{}\t{}", p.name, p.path.display(), p.tags.join(","));
                } else {
                    println!("{:<28} {}", p.name, p.path.display());
                }
            }
        }
        Some(Cmd::Scan { dirs, tag }) => {
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
                }
            }
            store.save()?;
            println!("added {added} project(s)");
        }
        Some(Cmd::Run { project, action }) => {
            let Some(p) = store.find(&project).cloned() else {
                bail!("no such project: {project}");
            };
            let list = actions::detect(&p.path, &cfg, p.ide.as_deref());
            let chosen = match action {
                None => list.first(),
                Some(a) => match a.parse::<usize>() {
                    Ok(n) => list.get(n.wrapping_sub(1)),
                    Err(_) => list.iter().find(|x| x.label.eq_ignore_ascii_case(&a)),
                },
            };
            let Some(chosen) = chosen else {
                bail!("no such action")
            };
            launch(&mut store, &p.path, chosen)?;
        }
        Some(Cmd::Menu { cmd }) => {
            store.sort_recent();
            let lines: Vec<String> = store
                .projects
                .iter()
                .map(|p| format!("{}  ·  {}", p.name, p.path.display()))
                .collect();
            let Some(i) = menu::choose(&cmd, "project", &lines)? else {
                return Ok(());
            };
            let path = store.projects[i].path.clone();
            let pinned = store.projects[i].ide.clone();

            let list = actions::detect(&path, &cfg, pinned.as_deref());
            let labels: Vec<String> = list.iter().map(|a| a.label.clone()).collect();
            let Some(j) = menu::choose(&cmd, "run", &labels)? else {
                return Ok(());
            };
            launch(&mut store, &path, &list[j])?;
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

/// Record the launch, then run. Saving first matters: on Unix a foreground
/// action replaces this process via exec and never returns.
fn launch(store: &mut Store, path: &std::path::Path, action: &actions::Action) -> Result<()> {
    store.touch(path);
    store.save()?;
    exec::execute(path, action)
}
