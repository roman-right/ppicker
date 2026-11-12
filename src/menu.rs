//! dmenu-compatible mode: delegate the picking UI to rofi, wofi, fuzzel, fzf...

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

pub const DEFAULT_CMD: &str = "rofi -dmenu -i -p {prompt}";

/// Show `lines` in the external menu; returns the index of the chosen line.
pub fn choose(menu_cmd: &str, prompt: &str, lines: &[String]) -> Result<Option<usize>> {
    let cmd = menu_cmd.replace("{prompt}", prompt);
    let mut sh = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", &cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &cmd]);
        c
    };
    let mut child = sh
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start menu command `{cmd}`"))?;

    // Write from a thread so a large list cannot deadlock against the pipe.
    let mut stdin = child.stdin.take().context("no stdin")?;
    let payload = lines.join("\n");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });
    let out = child.wait_with_output()?;
    let _ = writer.join();

    if !out.status.success() {
        return Ok(None); // user cancelled (Esc)
    }
    let chosen = String::from_utf8_lossy(&out.stdout);
    let chosen = chosen.trim_end_matches(['\n', '\r']);
    Ok(lines.iter().position(|l| l == chosen))
}
