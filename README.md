# ppicker

A tiny, instant project and runner picker for tiling window managers.
Press a hotkey, type a few letters, hit Enter: your IDE opens, or a script runs.
No typing paths, no dozens of hotkeys.

- ~1 MB single binary, starts in milliseconds
- Fuzzy search, tag filters, git branch and last-opened preview
- Actions auto-detected: IDEs, `cargo`, `npm`/`pnpm`/`yarn`/`bun` scripts, `uv`, `just`, `make`, `docker compose`, `git`
- Works as a built-in TUI or through rofi / wofi / fuzzel / fzf

## Install

```shell
pipx install ppicker     # or: uv tool install ppicker
cargo install ppicker
```

The command is `pp`.

## Use

```shell
pp add .                  # register the current directory
pp scan ~/code -t work    # register every git repo inside ~/code
pp                        # open the picker
pp menu                   # pick through rofi instead
pp run myproj "git pull"  # no UI
```

Keys: type to search, `Enter` for actions, `1`-`9` to run an action, `Ctrl-o` to run the
default (first) action immediately, `Tab` to cycle tags, `Esc` to go back / quit.

## i3 / sway

```
bindsym $mod+p exec --no-startup-id alacritty --class pp-popup -e pp
for_window [class="pp-popup"] floating enable, resize set 960 520, move position center
```

Or with rofi: `bindsym $mod+p exec --no-startup-id pp menu`.
Other launchers: `pp menu --cmd "fuzzel --dmenu"` (`{prompt}` is replaced by the step name).

## Config

`pp paths` prints the locations. `config.toml`:

```toml
ides = ["code", "pycharm"]   # first one is the default action
scan_dirs = ["/home/me/code"]
```

Per-project actions go in `<project>/.ppicker.toml`:

```toml
[[action]]
label = "Dev server"
cmd = "npm run dev"
```

## Migrating from `code-compass` 0.1.x

This is the successor to the Qt-based `code-compass` package. To carry your projects over:

```shell
pipx install ppicker
pipx run code-compass==0.2.0 migrate   # reads the old database, pipes it into `pp import -`
```

Or just re-add them with `pp scan ~/code`.
