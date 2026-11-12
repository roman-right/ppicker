//! Opening a project in an editor.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

/// Start `<ide> .` in the project directory, fully detached so the editor
/// outlives the picker (and the terminal it ran in).
pub fn open(dir: &Path, ide: &str) -> Result<()> {
    let cmd = format!("{ide} .");
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", &cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &cmd]);
        c
    };
    c.current_dir(dir)
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
        .with_context(|| format!("failed to start `{cmd}`"))?;
    Ok(())
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "setsid"]
    fn libc_setsid() -> i32;
}
