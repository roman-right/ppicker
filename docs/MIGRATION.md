# Moving from `code-compass` to `ppicker`

`code-compass` (the Qt desktop app) is retired and replaced by **ppicker**, a much smaller and
faster project picker built for tiling window managers. This page explains what is changing,
what you need to do, and how to bring your projects with you.

## What is happening

1. **`ppicker` is released** on PyPI and crates.io. Its command is `pp`.
2. **`code-compass` 0.2.0 is released** as a final version. It still works, but prints a notice
   when you start it and adds a `migrate` command that copies your projects to ppicker.
3. **A short migration period of about two weeks** follows the 0.2.0 release (until late
   November 2026).
4. **After that, the `code-compass` name on PyPI is handed over to a different, unrelated
   project.** Old `code-compass` releases may be removed from PyPI at that point.

> **Heads up.** Once the name has been handed over, `pip install code-compass` will install
> *something else*. If you rely on the old app, migrate before then, or keep using the code from
> the [`legacy/code-compass`](https://github.com/roman-right/ppicker/tree/legacy/code-compass)
> branch.

Your data is safe throughout: the migration only *reads* the old database and never changes it.

## How to move

**1. Install ppicker.**

```shell
pipx install ppicker          # or: uv tool install ppicker
```

**2. Copy your projects.**

```shell
pipx run code-compass==0.2.0 migrate
```

If the name on PyPI has already been handed over, install the final release straight from
GitHub instead; it is the same code:

```shell
pipx run --spec git+https://github.com/roman-right/ppicker@code-compass-v0.2.0 code-compass migrate
```

This reads `~/.config/code_compass/data.db` and adds every project to ppicker. If `pp` is on your
PATH it imports directly; otherwise it writes `ppicker-import.toml` and tells you to run
`pp import ppicker-import.toml`. Running it twice is harmless: projects are matched by path, so
nothing is duplicated.

**3. Carry over your settings.** The old config lived in `~/.config/code_compass/config.yaml`;
the new one is `config.toml` (run `pp paths` for its location).

| Old (`config.yaml`) | New (`config.toml`) |
|---|---|
| `ide_commands: [pycharm, code]` | `ides = ["pycharm", "code"]` |
| `projects_path: /home/me/Projects` | `scan_dirs = ["/home/me/Projects"]` |
| `cookiecutter: https://...` | not supported (see below) |

**4. Try it.** Run `pp`, type a few letters, press Enter. See the
[README](../README.md#how-it-works) for a walkthrough.

## What carries over

| Old app | ppicker |
|---|---|
| Projects: name, path | same |
| Last opened date | same, shown as "3d ago" and used to sort the list |
| Categories (tabs) | become **tags**: filter with `Tab` in the picker |
| Choose an IDE for the "Run" button | the **first editor in `ides`** is the default; every project then remembers the editor you last opened it with |
| Qt window with a table | a fast terminal picker, or rofi/wofi/fuzzel with `pp menu` |

## What is different or gone

ppicker does one thing: open a project in an editor, fast. Some things the old app did are not
part of it:

- **Creating a new project** from a cookiecutter template, and creating a virtualenv for it.
  Use `cookiecutter` or `uv init` directly, then `pp add .`.
- **Editing and deleting entries in a window.** Use `pp add --name` / `-t` to change a name or
  tags, and `pp rm <project>` to forget one (it never deletes files).
- **A window with buttons.** There is no graphical window; the picker runs in a terminal. If you
  want a graphical list, use `pp menu` with rofi, wofi or fuzzel.

## Questions

**Will I lose my projects?** No. The old database is only read. You can keep it as a backup.

**Can I keep using the old app?** Yes, version 0.2.0 keeps working. It will only receive this
notice and the `migrate` command. For the source, use the `legacy/code-compass` branch.

**Why the new name?** The old name is being handed to another project, so the successor needed
its own. It also no longer matches what the tool does.

**Something did not migrate.** Please open an issue at
<https://github.com/roman-right/ppicker/issues> and include the output of
`code-compass migrate`.
