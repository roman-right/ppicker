# ppicker

![ppicker in an i3 popup](assets/screenshot.png)

A tiny, instant project picker for tiling window managers.
Press a hotkey, type a few letters, hit Enter: the project opens in your editor.
No typing paths, no dozens of hotkeys.

- ~1 MB single binary, starts in milliseconds
- Fuzzy search, tag filters, git branch and last-opened preview
- Remembers which editor each project was last opened with
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
pp open myproj            # no UI
```

In the picker: type to search, `Enter` opens the project in its editor, `→` lets you open it
with a different one, `Tab` cycles tags, `Esc` quits.

## Editors

Set the editors you use in the config; the first is the default:

```toml
ides = ["code", "pycharm", "vim"]
```

A project you have never opened uses the default. **Whichever editor you open a project with
becomes that project's editor**, so the next `Enter` (or `pp open`, or `pp menu`) opens it there.
The list shows each project's editor next to its name.

To open with another editor: press `→` and pick one (or `1`-`9`); it is remembered from then on.

To change it without opening the project:

- `Ctrl-e` steps the selected project through your editors and wraps back to the default.
- From the command line:

```shell
pp editor myproj              # show the current editor
pp editor myproj pycharm      # set one (any command works, it need not be in `ides`)
pp editor myproj --reset      # forget it, back to the default
pp add . --ide pycharm        # register and set in one go
pp open myproj vim            # open once with a specific editor (and remember it)
```

The editor is started as `<command> .` inside the project directory, detached from the picker.

## i3 / sway

```
bindsym $mod+p exec --no-startup-id alacritty --class pp-popup,pp-popup -e pp
for_window [class="pp-popup"] floating enable, resize set 960 520, move position center
```

Or with rofi: `bindsym $mod+p exec --no-startup-id pp menu`. Add `--ask` to choose the
editor in a second step. Other launchers: `pp menu --cmd "fuzzel --dmenu"`
(`{prompt}` is replaced by the step name).

## Config

`pp paths` prints the locations. `config.toml`:

```toml
ides = ["code", "pycharm"]   # editors offered for projects; the first is the default
scan_dirs = ["/home/me/code"]
```

## Migrating from `code-compass` 0.1.x

This is the successor to the Qt-based `code-compass` package. To carry your projects over:

```shell
pipx install ppicker
pipx run code-compass==0.2.0 migrate   # reads the old database, pipes it into `pp import -`
```

Or just re-add them with `pp scan ~/code`.
