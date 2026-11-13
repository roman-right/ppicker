//! Interactive picker: fuzzy project search; Enter opens the project in its editor.

use std::fs;
use std::path::Path;

use anyhow::Result;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config as MatcherConfig, Matcher, Utf32Str};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::store::{Config, Project, now};

const ACCENT: Color = Color::Rgb(137, 180, 250);
const MATCH: Color = Color::Rgb(250, 179, 135);
const DIM: Color = Color::Rgb(127, 132, 156);
const SELECT_BG: Color = Color::Rgb(49, 50, 68);

#[derive(PartialEq)]
enum Stage {
    Projects,
    /// "Open with..." list for the selected project.
    Editors,
}

/// What the picker asks of the program around it: starting editors and
/// saving changes. The picker calls these while it is running, so it can stay
/// open after launching something.
pub trait Host {
    /// A project's remembered editor changed (`None` = the global default).
    fn ide_changed(&mut self, path: &Path, ide: Option<&str>);
    /// Start `ide` on `project`. An error (say, the command is not installed)
    /// is shown in the picker instead of closing it.
    fn open(&mut self, project: &Project, ide: &str) -> Result<()>;
}

struct App<'a> {
    cfg: &'a Config,
    projects: Vec<Project>,
    tags: Vec<String>,
    tag: Option<usize>,
    query: String,
    /// Indices into `projects` after tag filter and fuzzy match.
    visible: Vec<usize>,
    list: ListState,
    stage: Stage,
    /// Editors offered for the selected project, remembered one first.
    editors: Vec<String>,
    editor_list: ListState,
    matcher: Matcher,
    /// Set when the picker should close.
    done: bool,
    host: Box<dyn Host + 'a>,
    /// One-line feedback shown in place of the key hints until the next key.
    status: Option<String>,
}

impl<'a> App<'a> {
    fn new(
        projects: Vec<Project>,
        tags: Vec<String>,
        cfg: &'a Config,
        host: Box<dyn Host + 'a>,
    ) -> Self {
        let mut app = Self {
            cfg,
            projects,
            tags,
            tag: None,
            query: String::new(),
            visible: vec![],
            list: ListState::default(),
            stage: Stage::Projects,
            editors: vec![],
            editor_list: ListState::default(),
            matcher: Matcher::new(MatcherConfig::DEFAULT.match_paths()),
            done: false,
            host,
            status: None,
        };
        app.refilter();
        app
    }

    fn refilter(&mut self) {
        let tag = self.tag.map(|i| self.tags[i].clone());
        let candidates: Vec<(usize, String)> = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| tag.as_ref().is_none_or(|t| p.tags.contains(t)))
            .map(|(i, p)| (i, format!("{} {}", p.name, p.path.display())))
            .collect();

        self.visible = if self.query.is_empty() {
            candidates.into_iter().map(|(i, _)| i).collect()
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let mut scored: Vec<(usize, u32)> = candidates
                .iter()
                .filter_map(|(i, hay)| {
                    let mut buf = Vec::new();
                    pattern
                        .score(Utf32Str::new(hay, &mut buf), &mut self.matcher)
                        .map(|s| (*i, s))
                })
                .collect();
            scored.sort_by_key(|s| std::cmp::Reverse(s.1));
            scored.into_iter().map(|(i, _)| i).collect()
        };
        self.list.select(if self.visible.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    fn current(&self) -> Option<&Project> {
        self.list
            .selected()
            .and_then(|i| self.visible.get(i))
            .map(|&i| &self.projects[i])
    }

    fn move_sel(&mut self, delta: isize) {
        let (state, len) = match self.stage {
            Stage::Projects => (&mut self.list, self.visible.len()),
            Stage::Editors => (&mut self.editor_list, self.editors.len()),
        };
        if len == 0 {
            return;
        }
        let cur = state.selected().unwrap_or(0) as isize;
        state.select(Some((cur + delta).rem_euclid(len as isize) as usize));
    }

    fn cycle_tag(&mut self, forward: bool) {
        if self.tags.is_empty() {
            return;
        }
        let n = self.tags.len() as isize + 1; // +1 for "all"
        let cur = self.tag.map_or(0, |i| i as isize + 1);
        let next = (cur + if forward { 1 } else { -1 }).rem_euclid(n);
        self.tag = if next == 0 {
            None
        } else {
            Some(next as usize - 1)
        };
        self.refilter();
    }

    /// Editor that opens for a project, and whether one is remembered for it.
    /// `None` if no configured editor can open this kind of project.
    fn effective_ide<'p>(&'p self, p: &'p Project) -> Option<(&'p str, bool)> {
        self.cfg.effective_ide(p)
    }

    /// Whether the picker stays open after opening. `Enter` follows the
    /// `keep_open` setting; the alternate key (`Ctrl-o`) does the opposite.
    fn stays_open(&self, alternate: bool) -> bool {
        self.cfg.keep_open != alternate
    }

    /// Open the selected project in its own editor.
    fn open_current(&mut self, alternate: bool) {
        let Some(p) = self.current() else { return };
        match self.effective_ide(p) {
            Some((ide, _)) => {
                let ide = ide.to_string();
                self.open_selected(ide, self.stays_open(alternate));
            }
            None => self.status = Some(no_editor_message(p)),
        }
    }

    /// Start `ide` on the selected project. On success the picker closes, or
    /// stays open (ready for the next project) if `stay`. On failure it always
    /// stays open and says why.
    fn open_selected(&mut self, ide: String, stay: bool) {
        let Some(&idx) = self.list.selected().and_then(|i| self.visible.get(i)) else {
            return;
        };
        let project = self.projects[idx].clone();
        match self.host.open(&project, &ide) {
            Ok(()) => {
                // Mirror what the host saved, so the list and details are current.
                self.projects[idx].ide = Some(ide.clone());
                self.projects[idx].last_opened = Some(now());
                if stay {
                    self.stage = Stage::Projects;
                    self.status = Some(format!("opened {} in {ide}", project.name));
                } else {
                    self.done = true;
                }
            }
            Err(e) => self.status = Some(format!("{e:#}")),
        }
    }

    /// Show the "Open with..." list (Right arrow). Only editors that can open
    /// this kind of project (folder or workspace file) are listed.
    fn enter_editors(&mut self) {
        let Some(p) = self.current() else { return };
        let choices = self.cfg.choices(p.ide.as_deref(), p.kind());
        if choices.is_empty() {
            self.status = Some(no_editor_message(p));
            return;
        }
        self.editors = choices;
        self.editor_list.select(Some(0));
        self.stage = Stage::Editors;
    }

    fn open_with(&mut self, index: usize, alternate: bool) {
        if let Some(ide) = self.editors.get(index).cloned() {
            self.open_selected(ide, self.stays_open(alternate));
        }
    }

    /// Remember an editor for the selected project (`None` = global default)
    /// and save it immediately.
    fn set_project_ide(&mut self, ide: Option<String>) {
        let Some(&idx) = self.list.selected().and_then(|i| self.visible.get(i)) else {
            return;
        };
        self.projects[idx].ide = ide.clone();
        let p = &self.projects[idx];
        self.host.ide_changed(&p.path, ide.as_deref());
        let (name, remembered) = self.cfg.effective_ide(p).unwrap_or(("none", false));
        self.status = Some(format!(
            "{}: editor is now {name}{}",
            p.name,
            if remembered { "" } else { " (global default)" }
        ));
    }

    /// Ctrl-e: step through the editors that can open this project; wraps to
    /// the global default.
    fn cycle_ide(&mut self) {
        let Some(p) = self.current() else { return };
        let list = self.cfg.compatible(p.kind());
        if list.len() < 2 {
            self.status = Some(if list.is_empty() {
                no_editor_message(p)
            } else {
                "only one editor can open this; add more to `ides` in config.toml".into()
            });
            return;
        }
        let at = p
            .ide
            .as_ref()
            .and_then(|x| list.iter().position(|y| y == x))
            .unwrap_or(0);
        let next = (at + 1) % list.len();
        let new = (next != 0).then(|| list[next].clone());
        self.set_project_ide(new);
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let mut ctrl = mods.contains(KeyModifiers::CONTROL);
        // Some terminals (and Windows consoles) deliver Ctrl+letter as the raw
        // control character (^O = 0x0F), with or without the Ctrl modifier. Treat
        // it as Ctrl+letter instead of typing garbage into the search box. Tab,
        // Enter and Backspace arrive as their own keys, so their code points
        // are left alone.
        let code = match code {
            KeyCode::Char(c) if ('\x01'..='\x1a').contains(&c) && !"\x08\t\n\r".contains(c) => {
                ctrl = true;
                KeyCode::Char((c as u8 - 1 + b'a') as char)
            }
            // With a non-Latin keyboard layout active, Ctrl+<key> reports that
            // layout's letter (Russian: Ctrl+O arrives as Ctrl+щ). Shortcuts
            // should work in any layout, so map by key position.
            KeyCode::Char(c) if ctrl => KeyCode::Char(latin_key(c)),
            other => other,
        };
        self.status = None;
        match (code, ctrl) {
            (KeyCode::Char('c'), true) => self.done = true,
            (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), true) => self.move_sel(1),
            (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), true) => self.move_sel(-1),
            _ => match self.stage {
                Stage::Projects => self.on_key_projects(code, ctrl),
                Stage::Editors => self.on_key_editors(code, ctrl),
            },
        }
    }

    fn on_key_projects(&mut self, code: KeyCode, ctrl: bool) {
        match (code, ctrl) {
            (KeyCode::Esc, _) => self.done = true,
            (KeyCode::Enter, _) => self.open_current(false),
            // Ctrl-O: open, but do the opposite of Enter about closing the picker.
            (KeyCode::Char('o'), true) => self.open_current(true),
            (KeyCode::Right, _) => self.enter_editors(),
            (KeyCode::Char('e'), true) => self.cycle_ide(),
            (KeyCode::Tab, _) => self.cycle_tag(true),
            (KeyCode::BackTab, _) => self.cycle_tag(false),
            (KeyCode::Char('u'), true) => {
                self.query.clear();
                self.refilter();
            }
            (KeyCode::Backspace, _) => {
                self.query.pop();
                self.refilter();
            }
            (KeyCode::Char(c), false) if !c.is_control() => {
                self.query.push(c);
                self.refilter();
            }
            _ => {}
        }
    }

    fn on_key_editors(&mut self, code: KeyCode, ctrl: bool) {
        match (code, ctrl) {
            (KeyCode::Esc | KeyCode::Left | KeyCode::Backspace, _) => self.stage = Stage::Projects,
            (KeyCode::Enter, _) => {
                if let Some(i) = self.editor_list.selected() {
                    self.open_with(i, false);
                }
            }
            (KeyCode::Char('o'), true) => {
                if let Some(i) = self.editor_list.selected() {
                    self.open_with(i, true);
                }
            }
            (KeyCode::Char(c @ '1'..='9'), false) => {
                let i = c as usize - '1' as usize;
                if i < self.editors.len() {
                    self.open_with(i, false);
                }
            }
            _ => {}
        }
    }

    // ---- rendering ----

    fn draw(&mut self, f: &mut Frame) {
        let area = centered(f.area(), 92, 22);
        let [search, tags, body, hint] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .areas(area);

        self.draw_search(f, search);
        self.draw_tags(f, tags);

        let [left, right] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(body);
        self.draw_projects(f, left);
        match self.stage {
            Stage::Projects => self.draw_preview(f, right),
            Stage::Editors => self.draw_editors(f, right),
        }
        self.draw_hint(f, hint);
    }

    fn draw_search(&self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(
                " ppicker ",
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
        let line = Line::from(vec![
            Span::styled("❯ ", Style::new().fg(ACCENT)),
            Span::raw(self.query.as_str()),
        ]);
        f.render_widget(Paragraph::new(line).block(block), area);
        if self.stage == Stage::Projects {
            f.set_cursor_position((area.x + 3 + self.query.chars().count() as u16, area.y + 1));
        }
    }

    fn draw_tags(&self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];
        let chip = |label: &str, on: bool| {
            if on {
                Span::styled(
                    format!(" {label} "),
                    Style::new().fg(Color::Black).bg(ACCENT),
                )
            } else {
                Span::styled(format!(" {label} "), Style::new().fg(DIM))
            }
        };
        spans.push(chip("all", self.tag.is_none()));
        for (i, t) in self.tags.iter().enumerate() {
            spans.push(chip(t, self.tag == Some(i)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_projects(&mut self, f: &mut Frame, area: Rect) {
        let focused = self.stage == Stage::Projects;
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if focused { ACCENT } else { DIM }))
            .title(format!(" Projects ({}) ", self.visible.len()));

        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let cfg = self.cfg;
        let items: Vec<ListItem> = self
            .visible
            .iter()
            .map(|&i| {
                let p = &self.projects[i];
                let mut idx = Vec::new();
                if !self.query.is_empty() {
                    let mut buf = Vec::new();
                    pattern.indices(
                        Utf32Str::new(&p.name, &mut buf),
                        &mut self.matcher,
                        &mut idx,
                    );
                }
                let mut spans: Vec<Span> = p
                    .name
                    .chars()
                    .enumerate()
                    .map(|(n, ch)| {
                        if idx.contains(&(n as u32)) {
                            Span::styled(
                                ch.to_string(),
                                Style::new().fg(MATCH).add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw(ch.to_string())
                        }
                    })
                    .collect();
                // Workspace files get a marker, since they look like folders otherwise.
                if p.kind().is_workspace() {
                    spans.push(Span::styled(" ◇", Style::new().fg(ACCENT)));
                }
                // Show which editor Enter will use, right in the list.
                let ide = cfg.effective_ide(p).map_or("-", |(i, _)| i);
                spans.push(Span::styled(format!("  {ide}"), Style::new().fg(DIM)));
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_symbol("▌ ")
            .highlight_style(Style::new().bg(SELECT_BG).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, area, &mut self.list);
    }

    fn draw_preview(&self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(DIM))
            .title(" Details ");
        let mut lines = Vec::new();
        if let Some(p) = self.current() {
            let label = |s: &str| Span::styled(format!("{s:<8}"), Style::new().fg(DIM));
            lines.push(Line::from(Span::styled(
                p.name.clone(),
                Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                label("path"),
                Span::raw(p.path.display().to_string()),
            ]));
            if let Some(b) = git_branch(&p.path) {
                lines.push(Line::from(vec![label("branch"), Span::raw(b)]));
            }
            if p.kind().is_workspace() {
                lines.push(Line::from(vec![
                    label("type"),
                    Span::styled(p.kind().label(), Style::new().fg(ACCENT)),
                ]));
            }
            match self.effective_ide(p) {
                Some((ide, remembered)) => lines.push(Line::from(vec![
                    label("editor"),
                    Span::styled(ide.to_string(), Style::new().fg(MATCH)),
                    Span::styled(
                        if remembered {
                            "  last used"
                        } else {
                            "  default"
                        },
                        Style::new().fg(DIM),
                    ),
                ])),
                None => lines.push(Line::from(vec![
                    label("editor"),
                    Span::styled("none can open this", Style::new().fg(Color::Red)),
                ])),
            }
            lines.push(Line::from(vec![
                label("opened"),
                Span::raw(ago(p.last_opened)),
            ]));
            if !p.tags.is_empty() {
                lines.push(Line::from(vec![
                    label("tags"),
                    Span::raw(p.tags.join(", ")),
                ]));
            }
            if !p.path.exists() {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "path no longer exists",
                    Style::new().fg(Color::Red),
                ));
            }
        } else {
            lines.push(Line::styled(
                "No projects. Run `pp add .` or `pp scan ~/code`.",
                Style::new().fg(DIM),
            ));
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }

    fn draw_editors(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(" Open with ");
        let remembered = self.current().is_some_and(|p| p.ide.is_some());
        let items: Vec<ListItem> = self
            .editors
            .iter()
            .enumerate()
            .map(|(i, ide)| {
                let num = if i < 9 {
                    format!("{} ", i + 1)
                } else {
                    "  ".into()
                };
                let mut spans = vec![
                    Span::styled(num, Style::new().fg(DIM)),
                    Span::raw(ide.clone()),
                ];
                if i == 0 {
                    let note = if remembered {
                        "  ★ last used"
                    } else {
                        "  ★ default"
                    };
                    spans.push(Span::styled(note, Style::new().fg(MATCH)));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_symbol("▌ ")
            .highlight_style(Style::new().bg(SELECT_BG).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, area, &mut self.editor_list);
    }

    fn draw_hint(&self, f: &mut Frame, area: Rect) {
        if let Some(msg) = &self.status {
            f.render_widget(
                Paragraph::new(format!(" {msg}")).style(Style::new().fg(MATCH)),
                area,
            );
            return;
        }
        let (enter, ctrl_o) = if self.cfg.keep_open {
            ("open, stay", "open, quit")
        } else {
            ("open", "open, stay")
        };
        let text = match self.stage {
            Stage::Projects => format!(
                " ⏎ {enter}   ^o {ctrl_o}   → open with…   ^e switch editor   ⇥ tag   esc quit"
            ),
            Stage::Editors => format!(" ⏎ {enter}   1-9 quick open   ^o {ctrl_o}   esc back"),
        };
        f.render_widget(Paragraph::new(text).style(Style::new().fg(DIM)), area);
    }
}

/// The Latin letter on the same physical key as a Russian (ЙЦУКЕН) letter, so
/// `Ctrl+щ` works as `Ctrl+o`. Anything else is returned unchanged.
fn latin_key(c: char) -> char {
    const RU: &str = "йцукенгшщзфывапролдячсмить";
    const EN: &str = "qwertyuiopasdfghjklzxcvbnm";
    let lower = c.to_lowercase().next().unwrap_or(c);
    RU.chars()
        .position(|r| r == lower)
        .and_then(|i| EN.chars().nth(i))
        .unwrap_or(c)
}

fn no_editor_message(p: &Project) -> String {
    format!(
        "no configured editor can open this {}; add {} to `ides`",
        p.kind().label(),
        p.kind().needs()
    )
}

fn centered(area: Rect, max_w: u16, max_h: u16) -> Rect {
    let w = area.width.min(max_w);
    let h = area.height.min(max_h);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn git_branch(dir: &Path) -> Option<String> {
    let head = fs::read_to_string(dir.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(b) => Some(b.to_string()),
        None => Some(format!("detached @ {}", head.get(..7).unwrap_or(head))),
    }
}

fn ago(ts: Option<i64>) -> String {
    let Some(ts) = ts else { return "never".into() };
    let secs = (now() - ts).max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

/// Run the picker until the user quits. Opening and saving go through `host`.
pub fn pick<'a>(
    projects: Vec<Project>,
    tags: Vec<String>,
    cfg: &'a Config,
    host: Box<dyn Host + 'a>,
) -> Result<()> {
    let mut terminal: DefaultTerminal = ratatui::init();
    let result = run_loop(&mut terminal, App::new(projects, tags, cfg, host));
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, mut app: App) -> Result<()> {
    while !app.done {
        terminal.draw(|f| app.draw(f))?;
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key.code, key.modifiers);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    type IdeLog = Rc<RefCell<Vec<(PathBuf, Option<String>)>>>;

    /// What the fake host saw, shared with the test.
    #[derive(Clone, Default)]
    struct Rec {
        ide_changes: IdeLog,
        /// (project name, editor) for every successful open.
        opened: Rc<RefCell<Vec<(String, String)>>>,
        fail_open: Rc<Cell<bool>>,
    }

    impl Rec {
        fn ide_log(&self) -> Vec<Option<String>> {
            self.ide_changes
                .borrow()
                .iter()
                .map(|(_, i)| i.clone())
                .collect()
        }

        fn opened(&self) -> Vec<(String, String)> {
            self.opened.borrow().clone()
        }
    }

    struct FakeHost(Rec);

    impl Host for FakeHost {
        fn ide_changed(&mut self, path: &Path, ide: Option<&str>) {
            self.0
                .ide_changes
                .borrow_mut()
                .push((path.to_path_buf(), ide.map(String::from)));
        }

        fn open(&mut self, p: &Project, ide: &str) -> Result<()> {
            if self.0.fail_open.get() {
                bail!("command not found: {ide}");
            }
            self.0
                .opened
                .borrow_mut()
                .push((p.name.clone(), ide.to_string()));
            Ok(())
        }
    }

    fn proj(name: &str, tags: &[&str], ide: Option<&str>) -> Project {
        Project {
            name: name.into(),
            path: format!("/tmp/{name}").into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            last_opened: None,
            ide: ide.map(String::from),
        }
    }

    fn three_ides() -> Config {
        Config {
            ides: vec!["code".into(), "pycharm".into(), "vim".into()],
            ..Config::default()
        }
    }

    /// Three projects (beta already remembers vim) with a recording host.
    fn app(cfg: &Config) -> (App<'_>, Rec) {
        let rec = Rec::default();
        let ps = vec![
            proj("alpha", &["work"], None),
            proj("beta", &["oss"], Some("vim")),
            proj("gamma", &["work"], None),
        ];
        let app = App::new(
            ps,
            vec!["oss".into(), "work".into()],
            cfg,
            Box::new(FakeHost(rec.clone())),
        );
        (app, rec)
    }

    /// A folder and a VS Code workspace file side by side.
    fn app_with_workspace(cfg: &Config) -> (App<'_>, Rec) {
        let rec = Rec::default();
        let ps = vec![
            proj("alpha", &[], None),
            Project {
                name: "shop".into(),
                path: "/tmp/shop.code-workspace".into(),
                tags: vec![],
                last_opened: None,
                ide: None,
            },
        ];
        let app = App::new(ps, vec![], cfg, Box::new(FakeHost(rec.clone())));
        (app, rec)
    }

    fn key(a: &mut App, code: KeyCode) {
        a.on_key(code, KeyModifiers::NONE);
    }

    fn ctrl(a: &mut App, c: char) {
        a.on_key(KeyCode::Char(c), KeyModifiers::CONTROL);
    }

    fn type_str(a: &mut App, s: &str) {
        for c in s.chars() {
            key(a, KeyCode::Char(c));
        }
    }

    /// Everything on screen as one string.
    fn screen(a: &mut App) -> String {
        let mut t = Terminal::new(TestBackend::new(100, 24)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let buf = t.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn fuzzy_filter_narrows_and_ranks() {
        let cfg = three_ides();
        let (mut a, _) = app(&cfg);
        assert_eq!(a.visible.len(), 3);
        type_str(&mut a, "gm");
        assert_eq!(a.current().unwrap().name, "gamma");
        ctrl(&mut a, 'u');
        assert_eq!(a.visible.len(), 3);
    }

    #[test]
    fn tag_cycle_filters() {
        let cfg = three_ides();
        let (mut a, _) = app(&cfg);
        key(&mut a, KeyCode::Tab); // oss
        assert_eq!(a.visible.len(), 1);
        key(&mut a, KeyCode::Tab); // work
        assert_eq!(a.visible.len(), 2);
        key(&mut a, KeyCode::Tab); // back to all
        assert_eq!(a.visible.len(), 3);
    }

    #[test]
    fn enter_opens_in_the_global_default_and_closes() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        key(&mut a, KeyCode::Enter);
        assert!(a.done);
        assert_eq!(rec.opened(), vec![("alpha".into(), "code".into())]);
    }

    #[test]
    fn enter_uses_the_projects_own_editor() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "beta"); // remembered vim
        key(&mut a, KeyCode::Enter);
        assert_eq!(rec.opened(), vec![("beta".into(), "vim".into())]);
    }

    #[test]
    fn right_arrow_lists_editors_remembered_first_and_number_opens() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "beta");
        key(&mut a, KeyCode::Right);
        assert!(a.stage == Stage::Editors);
        assert_eq!(a.editors, vec!["vim", "code", "pycharm"]);
        key(&mut a, KeyCode::Char('3'));
        assert_eq!(rec.opened(), vec![("beta".into(), "pycharm".into())]);
        assert!(a.done);
    }

    #[test]
    fn esc_goes_back_then_quits() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        key(&mut a, KeyCode::Right);
        key(&mut a, KeyCode::Esc);
        assert!(a.stage == Stage::Projects && !a.done);
        key(&mut a, KeyCode::Esc);
        assert!(a.done);
        assert!(rec.opened().is_empty());
    }

    #[test]
    fn ctrl_o_opens_and_keeps_the_picker_open_for_the_next_project() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        ctrl(&mut a, 'o');
        assert!(!a.done, "picker must stay open");
        assert_eq!(a.status.as_deref(), Some("opened alpha in code"));

        // Straight on to a second project, in the same session.
        ctrl(&mut a, 'u');
        type_str(&mut a, "gamma");
        ctrl(&mut a, 'o');
        assert!(!a.done);
        assert_eq!(
            rec.opened(),
            vec![
                ("alpha".into(), "code".into()),
                ("gamma".into(), "code".into())
            ]
        );

        // The list reflects what was just done.
        let alpha = a.projects.iter().find(|p| p.name == "alpha").unwrap();
        assert_eq!(alpha.ide.as_deref(), Some("code"));
        assert!(alpha.last_opened.is_some());

        key(&mut a, KeyCode::Esc);
        assert!(a.done);
    }

    #[test]
    fn ctrl_o_in_the_editor_list_opens_with_that_editor_and_stays() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "beta");
        key(&mut a, KeyCode::Right);
        key(&mut a, KeyCode::Down); // "code"
        ctrl(&mut a, 'o');
        assert_eq!(rec.opened(), vec![("beta".into(), "code".into())]);
        assert!(!a.done);
        assert!(a.stage == Stage::Projects, "back to the project list");
    }

    #[test]
    fn keep_open_setting_swaps_enter_and_ctrl_o() {
        let cfg = Config {
            keep_open: true,
            ..three_ides()
        };
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        key(&mut a, KeyCode::Enter);
        assert!(!a.done, "Enter stays open with keep_open");
        ctrl(&mut a, 'o');
        assert!(a.done, "Ctrl-o quits with keep_open");
        assert_eq!(rec.opened().len(), 2);
    }

    #[test]
    fn a_failed_open_keeps_the_picker_open_and_says_why() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        rec.fail_open.set(true);
        type_str(&mut a, "alpha");
        key(&mut a, KeyCode::Enter);
        assert!(!a.done);
        assert_eq!(a.status.as_deref(), Some("command not found: code"));
        assert!(rec.opened().is_empty());
        // Nothing is remembered for a launch that did not happen.
        assert_eq!(a.current().unwrap().ide, None);
        assert_eq!(a.current().unwrap().last_opened, None);
    }

    #[test]
    fn raw_control_characters_act_as_ctrl_keys_and_never_enter_the_search() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        // ^O as a bare 0x0F, the way some terminals deliver it.
        a.on_key(KeyCode::Char('\x0f'), KeyModifiers::NONE);
        assert!(!a.done);
        assert_eq!(rec.opened(), vec![("alpha".into(), "code".into())]);
        // ^U (0x15) clears the search; nothing invisible is left behind.
        a.on_key(KeyCode::Char('\x15'), KeyModifiers::NONE);
        assert_eq!(a.query, "");
        type_str(&mut a, "gamma");
        assert_eq!(a.query, "gamma");
        assert_eq!(a.current().unwrap().name, "gamma");
        // A control character that means nothing is simply ignored.
        a.on_key(KeyCode::Char('\x07'), KeyModifiers::NONE);
        assert_eq!(a.query, "gamma");
    }

    #[test]
    fn shortcuts_work_with_a_russian_keyboard_layout() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        // Ctrl+O with the Russian layout active arrives as Ctrl+щ.
        a.on_key(KeyCode::Char('щ'), KeyModifiers::CONTROL);
        assert_eq!(rec.opened(), vec![("alpha".into(), "code".into())]);
        assert!(!a.done);
        // Ctrl+у is Ctrl+e: switch editor.
        a.on_key(KeyCode::Char('у'), KeyModifiers::CONTROL);
        assert_eq!(a.current().unwrap().ide.as_deref(), Some("pycharm"));
        // Ctrl+г is Ctrl+u: clear the search.
        a.on_key(KeyCode::Char('г'), KeyModifiers::CONTROL);
        assert_eq!(a.query, "");
        // Plain Cyrillic typing still searches (project names may be Russian).
        type_str(&mut a, "щ");
        assert_eq!(a.query, "щ");
        assert_eq!(latin_key('Щ'), 'o');
        assert_eq!(latin_key('x'), 'x');
    }

    #[test]
    fn the_hint_line_describes_the_keys_for_the_current_setting() {
        let cfg = three_ides();
        let (mut a, _) = app(&cfg);
        let s = screen(&mut a);
        assert!(s.contains("⏎ open ") && s.contains("^o open, stay"), "{s}");

        let cfg = Config {
            keep_open: true,
            ..three_ides()
        };
        let (mut a, _) = app(&cfg);
        let s = screen(&mut a);
        assert!(
            s.contains("⏎ open, stay") && s.contains("^o open, quit"),
            "{s}"
        );
    }

    #[test]
    fn ctrl_e_cycles_editors_saves_each_and_wraps_to_default() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");

        ctrl(&mut a, 'e');
        assert_eq!(a.current().unwrap().ide.as_deref(), Some("pycharm"));
        ctrl(&mut a, 'e');
        assert_eq!(a.current().unwrap().ide.as_deref(), Some("vim"));
        ctrl(&mut a, 'e'); // back to the global default, stored as "not set"
        assert_eq!(a.current().unwrap().ide, None);

        assert_eq!(
            rec.ide_log(),
            vec![Some("pycharm".into()), Some("vim".into()), None]
        );
    }

    #[test]
    fn enter_after_ctrl_e_opens_the_chosen_editor() {
        let cfg = three_ides();
        let (mut a, rec) = app(&cfg);
        type_str(&mut a, "alpha");
        ctrl(&mut a, 'e');
        key(&mut a, KeyCode::Enter);
        assert_eq!(rec.opened(), vec![("alpha".into(), "pycharm".into())]);
    }

    #[test]
    fn ctrl_e_with_one_editor_explains_instead_of_changing() {
        let cfg = Config::default();
        let (mut a, rec) = app(&cfg);
        ctrl(&mut a, 'e');
        assert!(a.status.as_deref().unwrap().contains("only one editor"));
        assert!(rec.ide_log().is_empty());
    }

    #[test]
    fn a_workspace_is_offered_only_the_editors_that_can_open_it() {
        let cfg = Config {
            ides: vec!["pycharm".into(), "code".into(), "vim".into()],
            ..Config::default()
        };
        let (mut a, rec) = app_with_workspace(&cfg);
        type_str(&mut a, "shop");
        // Enter picks the first editor that can open it: "code", not "pycharm".
        key(&mut a, KeyCode::Right);
        assert_eq!(a.editors, vec!["code"]);
        key(&mut a, KeyCode::Esc);
        key(&mut a, KeyCode::Enter);
        assert_eq!(rec.opened(), vec![("shop".into(), "code".into())]);

        // The folder next to it still gets everything.
        let (mut a, _) = app_with_workspace(&cfg);
        type_str(&mut a, "alpha");
        key(&mut a, KeyCode::Right);
        assert_eq!(a.editors, vec!["pycharm", "code", "vim"]);
    }

    #[test]
    fn a_workspace_with_no_capable_editor_explains_instead_of_opening() {
        let cfg = Config {
            ides: vec!["pycharm".into(), "vim".into()],
            ..Config::default()
        };
        let (mut a, rec) = app_with_workspace(&cfg);
        type_str(&mut a, "shop");
        key(&mut a, KeyCode::Enter);
        assert!(!a.done && rec.opened().is_empty());
        let msg = a.status.clone().unwrap();
        assert!(
            msg.contains("VS Code workspace") && msg.contains("vscode"),
            "{msg}"
        );
        key(&mut a, KeyCode::Right);
        assert!(a.stage == Stage::Projects); // no empty editor list
    }

    #[test]
    fn ctrl_e_cycles_only_through_editors_that_fit() {
        let cfg = Config {
            ides: vec!["code".into(), "pycharm".into(), "codium".into()],
            ..Config::default()
        };
        let (mut a, _) = app_with_workspace(&cfg);
        type_str(&mut a, "shop");
        ctrl(&mut a, 'e');
        assert_eq!(a.current().unwrap().ide.as_deref(), Some("codium")); // skips pycharm
        ctrl(&mut a, 'e');
        assert_eq!(a.current().unwrap().ide, None); // wrapped to the default
    }

    #[test]
    fn workspaces_are_marked_in_the_list_and_described_in_details() {
        let cfg = three_ides();
        let (mut a, _) = app_with_workspace(&cfg);
        type_str(&mut a, "shop");
        let s = screen(&mut a);
        assert!(s.contains('◇'), "list marker missing");
        assert!(s.contains("VS Code workspace"), "type row missing");
    }

    #[test]
    fn renders_without_panicking_at_small_and_large_sizes() {
        let cfg = three_ides();
        for (w, h) in [(20, 5), (80, 24), (200, 60)] {
            let (mut a, _) = app(&cfg);
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| a.draw(f)).unwrap();
            key(&mut a, KeyCode::Right);
            t.draw(|f| a.draw(f)).unwrap();
        }
    }

    /// Prints both screens: `cargo test dump -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn dump_screens() {
        let cfg = three_ides();
        let (mut a, _) = app(&cfg);
        for step in 0..2 {
            println!("--- screen {step} ---");
            for line in screen(&mut a).lines() {
                println!("{}", line.trim_end());
            }
            key(&mut a, KeyCode::Right);
        }
    }
}
