//! Opening a project in an editor.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::editors::Kind;
use crate::store::Config;

/// Start the editor named `ide` on the project at `path`, fully detached so it
/// outlives the picker (and the terminal it ran in).
///
/// A folder is opened as `<editor> .` from inside it; a workspace file as
/// `<editor> <file>`. The editor command is found on `PATH`, so logical names
/// like `vscode` work whatever the local command is called.
pub fn open(path: &Path, kind: Kind, ide: &str, cfg: &Config) -> Result<()> {
    let editor = cfg.editor(ide);
    if !editor.supports(kind) {
        bail!(
            "`{ide}` cannot open a {}; use {}",
            kind.label(),
            kind.needs()
        );
    }
    let (program, mut args) = editor.resolve()?;

    let cwd = if kind.is_workspace() {
        args.push(path.to_string_lossy().into_owned());
        path.parent().unwrap_or(path)
    } else {
        args.push(".".into());
        path
    };

    let mut c = Command::new(&program);
    c.args(&args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New session: the editor survives the picker's terminal closing.
        unsafe {
            c.pre_exec(|| {
                if libc_setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    c.spawn()
        .with_context(|| format!("failed to start {}", program.display()))?;
    Ok(())
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
}
