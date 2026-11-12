//! Interactive picker: fuzzy project search, then an action list.

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

use crate::actions::{self, Action};
use crate::store::{Config, Project, now};

const ACCENT: Color = Color::Rgb(137, 180, 250);
const MATCH: Color = Color::Rgb(250, 179, 135);
const DIM: Color = Color::Rgb(127, 132, 156);
const SELECT_BG: Color = Color::Rgb(49, 50, 68);

#[derive(PartialEq)]
enum Stage {
    Projects,
    Actions,
}

pub struct Picked {
    pub project: Project,
    pub action: Action,
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
    actions: Vec<Action>,
    action_list: ListState,
    matcher: Matcher,
    done: Option<Option<Picked>>,
}

impl<'a> App<'a> {
    fn new(projects: Vec<Project>, tags: Vec<String>, cfg: &'a Config) -> Self {
        let mut app = Self {
            cfg,
            projects,
            tags,
            tag: None,
            query: String::new(),
            visible: vec![],
            list: ListState::default(),
            stage: Stage::Projects,
            actions: vec![],
            action_list: ListState::default(),
            matcher: Matcher::new(MatcherConfig::DEFAULT.match_paths()),
            done: None,
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
            Stage::Actions => (&mut self.action_list, self.actions.len()),
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

    fn enter_actions(&mut self) {
        if let Some(p) = self.current() {
            self.actions = actions::detect(&p.path, self.cfg);
            self.action_list.select(Some(0));
            self.stage = Stage::Actions;
        }
    }

    fn finish(&mut self, index: usize) {
        if let (Some(p), Some(a)) = (self.current(), self.actions.get(index)) {
            self.done = Some(Some(Picked {
                project: p.clone(),
                action: a.clone(),
            }));
        }
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        match (code, ctrl) {
            (KeyCode::Char('c'), true) => self.done = Some(None),
            (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), true) => self.move_sel(1),
            (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), true) => self.move_sel(-1),
            _ => match self.stage {
                Stage::Projects => self.on_key_projects(code, ctrl),
                Stage::Actions => self.on_key_actions(code),
            },
        }
    }

    fn on_key_projects(&mut self, code: KeyCode, ctrl: bool) {
        match (code, ctrl) {
            (KeyCode::Esc, _) => self.done = Some(None),
            (KeyCode::Enter, _) | (KeyCode::Right, _) => self.enter_actions(),
            // Ctrl-O: skip the action list and run the default action.
            (KeyCode::Char('o'), true) => {
                self.enter_actions();
                self.finish(0);
            }
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
            (KeyCode::Char(c), false) => {
                self.query.push(c);
                self.refilter();
            }
            _ => {}
        }
    }

    fn on_key_actions(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Backspace => self.stage = Stage::Projects,
            KeyCode::Enter => {
                if let Some(i) = self.action_list.selected() {
                    self.finish(i);
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.actions.len() {
                    self.finish(i);
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
            Stage::Actions => self.draw_actions(f, right),
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
                let spans: Vec<Span> = p
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

    fn draw_actions(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(" Run ");
        let items: Vec<ListItem> = self
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let num = if i < 9 {
                    format!("{} ", i + 1)
                } else {
                    "  ".into()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(num, Style::new().fg(DIM)),
                    Span::raw(a.label.clone()),
                ]))
            })
            .collect();
        let list = List::new(items)
            .block(block)
            .highlight_symbol("▌ ")
            .highlight_style(Style::new().bg(SELECT_BG).add_modifier(Modifier::BOLD));
        f.render_stateful_widget(list, area, &mut self.action_list);
    }

    fn draw_hint(&self, f: &mut Frame, area: Rect) {
        let text = match self.stage {
            Stage::Projects => " ⏎ actions   ^o open default   ⇥ tag   ^u clear   esc quit",
            Stage::Actions => " ⏎ run   1-9 quick run   esc back",
        };
        f.render_widget(Paragraph::new(text).style(Style::new().fg(DIM)), area);
    }
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

/// Run the picker. Returns `None` if the user cancelled.
pub fn pick(projects: Vec<Project>, tags: Vec<String>, cfg: &Config) -> Result<Option<Picked>> {
    let mut terminal: DefaultTerminal = ratatui::init();
    let result = run_loop(&mut terminal, App::new(projects, tags, cfg));
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, mut app: App) -> Result<Option<Picked>> {
    loop {
        terminal.draw(|f| app.draw(f))?;
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key.code, key.modifiers);
        }
        if let Some(done) = app.done.take() {
            return Ok(done);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn proj(name: &str, tags: &[&str]) -> Project {
        Project {
            name: name.into(),
            path: format!("/tmp/{name}").into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            last_opened: None,
        }
    }

    fn app(cfg: &Config) -> App<'_> {
        let ps = vec![
            proj("alpha", &["work"]),
            proj("beta", &["oss"]),
            proj("gamma", &["work"]),
        ];
        App::new(ps, vec!["oss".into(), "work".into()], cfg)
    }

    fn type_str(app: &mut App, s: &str) {
        for c in s.chars() {
            app.on_key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    #[test]
    fn fuzzy_filter_narrows_and_ranks() {
        let cfg = Config::default();
        let mut a = app(&cfg);
        assert_eq!(a.visible.len(), 3);
        type_str(&mut a, "gm");
        assert_eq!(a.current().unwrap().name, "gamma");
        a.on_key(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(a.visible.len(), 3);
    }

    #[test]
    fn tag_cycle_filters() {
        let cfg = Config::default();
        let mut a = app(&cfg);
        a.on_key(KeyCode::Tab, KeyModifiers::NONE); // oss
        assert_eq!(a.visible.len(), 1);
        a.on_key(KeyCode::Tab, KeyModifiers::NONE); // work
        assert_eq!(a.visible.len(), 2);
        a.on_key(KeyCode::Tab, KeyModifiers::NONE); // back to all
        assert_eq!(a.visible.len(), 3);
    }

    #[test]
    fn enter_then_number_picks_action() {
        let cfg = Config::default();
        let mut a = app(&cfg);
        type_str(&mut a, "beta");
        a.on_key(KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.stage == Stage::Actions);
        a.on_key(KeyCode::Char('1'), KeyModifiers::NONE);
        let picked = a.done.take().unwrap().unwrap();
        assert_eq!(picked.project.name, "beta");
        assert!(picked.action.detached); // first action is the IDE
    }

    #[test]
    fn esc_goes_back_then_quits() {
        let cfg = Config::default();
        let mut a = app(&cfg);
        a.on_key(KeyCode::Enter, KeyModifiers::NONE);
        a.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.stage == Stage::Projects && a.done.is_none());
        a.on_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.done, Some(None)));
    }

    #[test]
    fn renders_without_panicking_at_small_and_large_sizes() {
        let cfg = Config::default();
        for (w, h) in [(20, 5), (80, 24), (200, 60)] {
            let mut a = app(&cfg);
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| a.draw(f)).unwrap();
            a.on_key(KeyCode::Enter, KeyModifiers::NONE);
            t.draw(|f| a.draw(f)).unwrap();
        }
    }

    /// Prints both screens: `cargo test dump -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn dump_screens() {
        let cfg = Config::default();
        let mut a = app(&cfg);
        let mut t = Terminal::new(TestBackend::new(96, 24)).unwrap();
        type_str(&mut a, "a");
        for step in 0..2 {
            t.draw(|f| a.draw(f)).unwrap();
            let buf = t.backend().buffer().clone();
            println!("--- screen {step} ---");
            for y in 0..buf.area.height {
                let row: String = (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                println!("{}", row.trim_end());
            }
            a.on_key(KeyCode::Enter, KeyModifiers::NONE);
        }
    }
}
