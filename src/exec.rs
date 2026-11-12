//! Executing an action inside a project directory.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::actions::Action;

fn shell(cmd: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

pub fn execute(dir: &Path, action: &Action) -> Result<()> {
    let mut c = shell(&action.cmd);
    c.current_dir(dir);

    if action.detached {
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // New session: the IDE survives the picker's terminal closing.
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
            .with_context(|| format!("failed to start `{}`", action.cmd))?;
        return Ok(());
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace the picker process so the command owns the terminal.
        let err = c.exec();
        Err(err).with_context(|| format!("failed to run `{}`", action.cmd))
    }
    #[cfg(not(unix))]
    {
        let status = c
            .status()
            .with_context(|| format!("failed to run `{}`", action.cmd))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
}
