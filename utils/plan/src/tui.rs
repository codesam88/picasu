use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FIELD_COUNT: usize = 4; // type, priority, area, slug

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Quit,
    OpenEditor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Browse,
    Preview,
}

struct App<'a> {
    #[allow(dead_code)]
    root: &'a Path,
    entries: Vec<PathBuf>,
    tasks: Vec<crate::plan::LoadedTask>,
    task_paths: HashMap<String, PathBuf>,
    columns: Vec<Column>,
    selected_column: usize,
    selected_task: usize,
    selected_field: usize,
    quit: bool,
    filter_status: Option<String>,
    filter_type: Option<String>,
    filter_priority: Option<String>,
    filter_area: Option<String>,
    filter_search: Option<String>,
    mode: Mode,
    preview: Vec<Line<'static>>,
    preview_scroll: usize,
}

struct Column {
    status: String,
    task_indices: Vec<usize>,
}

impl<'a> App<'a> {
    fn new(root: &'a Path, tasks: Vec<crate::plan::LoadedTask>, entries: Vec<PathBuf>) -> Self {
        let path_map: HashMap<String, PathBuf> = entries
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|slug| (slug.to_string(), p.clone()))
            })
            .collect();

        let mut app = Self {
            root,
            entries,
            tasks,
            task_paths: path_map,
            columns: Vec::new(),
            selected_column: 0,
            selected_task: 0,
            selected_field: 0,
            quit: false,
            filter_status: None,
            filter_type: None,
            filter_priority: None,
            filter_area: None,
            filter_search: None,
            mode: Mode::Browse,
            preview: Vec::new(),
            preview_scroll: 0,
        };

        app.sort_tasks();
        app.rebuild_columns();
        app
    }

    fn sort_tasks(&mut self) {
        self.tasks.sort_by(|a, b| crate::plan::cmp_tasks(a, b, &[]));
    }

    fn reload_tasks(&mut self) {
        self.tasks = crate::plan::load_and_filter_tasks(
            &self.entries,
            self.filter_status.as_deref(),
            self.filter_type.as_deref(),
            self.filter_priority.as_deref(),
            self.filter_area.as_deref(),
            self.filter_search.as_deref(),
        );
        let path_map: HashMap<String, PathBuf> = self
            .entries
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|slug| (slug.to_string(), p.clone()))
            })
            .collect();
        self.task_paths = path_map;
        self.selected_column = 0;
        self.selected_task = 0;
        self.sort_tasks();
        self.rebuild_columns();
    }

    fn has_active_filters(&self) -> bool {
        self.filter_status.is_some()
            || self.filter_type.is_some()
            || self.filter_priority.is_some()
            || self.filter_area.is_some()
            || self.filter_search.is_some()
    }

    fn rebuild_columns(&mut self) {
        let kanban_order = crate::plan::KANBAN_ORDER;
        let mut columns = Vec::new();
        for &status in kanban_order {
            let indices: Vec<usize> = self
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.task.status == status)
                .map(|(i, _)| i)
                .collect();
            columns.push(Column {
                status: status.to_string(),
                task_indices: indices,
            });
        }
        self.columns = columns;

        if self.selected_column >= self.columns.len() {
            self.selected_column = self.columns.len().saturating_sub(1);
        }
        if let Some(col) = self.columns.get(self.selected_column)
            && self.selected_task >= col.task_indices.len()
        {
            self.selected_task = col.task_indices.len().saturating_sub(1);
        }
    }
}

pub fn run_tui(
    root: &Path,
    status_filter: Option<&str>,
    type_filter: Option<&str>,
    priority_filter: Option<&str>,
    area_filter: Option<&str>,
    search_query: Option<&str>,
) {
    let tasks_dir = root.join(".plan").join("tasks");
    let entries = crate::plan::read_task_files(&tasks_dir).unwrap_or_default();
    let tasks = crate::plan::load_and_filter_tasks(
        &entries,
        status_filter,
        type_filter,
        priority_filter,
        area_filter,
        search_query,
    );

    if tasks.is_empty() {
        eprintln!("(no tasks match filters)");
        return;
    }

    let mut app = App::new(root, tasks, entries);
    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("failed to init terminal: {e}");
            return;
        }
    };

    while !app.quit {
        if let Err(e) = terminal.draw(|frame| app.render(frame)) {
            eprintln!("render error: {e}");
            break;
        }
        match app.handle_events() {
            Ok(action) => match action {
                Action::None => {}
                Action::Quit => break,
                Action::OpenEditor => {
                    if let Some((_slug, path)) = app.current_task_path() {
                        ratatui::restore();
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                        let status = std::process::Command::new(&editor).arg(path).status();
                        if let Ok(s) = status
                            && !s.success()
                        {
                            eprintln!("{editor} exited with code: {:?}", s.code());
                        }
                        if let Ok(t) = ratatui::try_init() {
                            terminal = t;
                        } else {
                            eprintln!("failed to re-init terminal after editor");
                            break;
                        }
                    }
                }
            },
            Err(e) => {
                eprintln!("event error: {e}");
                break;
            }
        }
    }

    ratatui::restore();
}

impl App<'_> {
    fn render(&mut self, frame: &mut Frame) {
        match self.mode {
            Mode::Browse => self.render_browse(frame),
            Mode::Preview => self.render_preview(frame),
        }
    }

    fn render_browse(&mut self, frame: &mut Frame) {
        let slug_w = self
            .tasks
            .iter()
            .map(|t| t.slug.len())
            .max()
            .unwrap_or(4)
            .clamp(4, 28);
        let type_w: usize = 8;
        let prio_w: usize = 8;
        let slot = |w: usize| w + 1;

        let [head_area, task_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.render_header(frame, head_area, slug_w, type_w, prio_w, slot);
        self.render_task_area(frame, task_area, slug_w, type_w, prio_w, slot);
        self.render_footer(frame, footer_area);
    }

    fn render_preview(&self, frame: &mut Frame) {
        let [title_area, body_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(frame.area());
        let title = Line::from(Span::styled(
            " Preview \u{2014} q/Esc to close, \u{2191}\u{2193} to scroll ",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(title, title_area);
        frame.render_widget(
            Paragraph::new(self.preview.clone()).scroll((self.preview_scroll as u16, 0)),
            body_area,
        );
    }

    fn render_header(
        &self,
        frame: &mut Frame,
        area: Rect,
        slug_w: usize,
        type_w: usize,
        prio_w: usize,
        slot: impl Fn(usize) -> usize,
    ) {
        // Header labels in same order as task rows: slug, type, priority, area
        // Each padded with same slot widths so columns align
        let mut spans = Vec::new();
        spans.push(Span::styled(
            format!("{:<w$}", format!(" {}", "Issue"), w = slot(slug_w)),
            if self.selected_field == 3 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));
        spans.push(Span::styled(
            format!("{:<w$}", format!(" {}", "Type"), w = slot(type_w)),
            if self.selected_field == 0 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));
        spans.push(Span::styled(
            format!("{:<w$}", format!(" {}", "Priority"), w = slot(prio_w)),
            if self.selected_field == 1 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));
        spans.push(Span::styled(
            format!(" {}", "Area"),
            if self.selected_field == 2 {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_task_area(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        slug_w: usize,
        type_w: usize,
        prio_w: usize,
        slot: impl Fn(usize) -> usize,
    ) {
        if self.columns.is_empty() || area.width < 10 {
            return;
        }
        // Color scheme — BOLD only on active field, non-active selected row has plain bg
        let sel_base = Style::default().bg(Color::White).fg(Color::Black);
        let sel_act = Style::default()
            .bg(Color::White)
            .fg(Color::Rgb(0, 0, 0))
            .add_modifier(Modifier::BOLD);
        let sel_prio_base = |pc: Color| Style::default().bg(pc).fg(Color::Black);
        let sel_prio_act = |pc: Color| {
            Style::default()
                .bg(pc)
                .fg(Color::Rgb(0, 0, 0))
                .add_modifier(Modifier::BOLD)
        };
        let is_vivid = |c: Color| {
            matches!(
                c,
                Color::Red
                    | Color::Yellow
                    | Color::Blue
                    | Color::Magenta
                    | Color::Cyan
                    | Color::Green
            )
        };

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut selected_line = 0usize;

        for (col_idx, column) in self.columns.iter().enumerate() {
            let is_active = col_idx == self.selected_column;
            let color = status_color(&column.status);

            let header = format!(
                "{} ({})",
                column.status.to_uppercase(),
                column.task_indices.len()
            );
            let dash = area.width.saturating_sub(header.len() as u16 + 2);
            lines.push(Line::from(Span::styled(
                format!("{} {}", header, "─".repeat(dash.max(1) as usize)),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));

            if column.task_indices.is_empty() {
                lines.push(Line::from(Span::raw(" (none)")));
            } else {
                for (row_idx, &task_idx) in column.task_indices.iter().enumerate() {
                    let task = &self.tasks[task_idx];
                    let is_sel = is_active && row_idx == self.selected_task;
                    if is_sel {
                        selected_line = lines.len();
                    }
                    let is_act = |f: usize| is_sel && self.selected_field == f;

                    let mut spans = Vec::new();

                    // Slug (field 3)
                    let slen = slot(slug_w);
                    let slug = if is_act(3) {
                        let n = slen.saturating_sub(2);
                        let s = if task.slug.len() > n {
                            format!("{}…", &task.slug[..n.saturating_sub(1)])
                        } else {
                            task.slug.clone()
                        };
                        format!("{:<w$}", format!("[{}]", s), w = slen)
                    } else {
                        let n = slen.saturating_sub(1);
                        let s = if task.slug.len() > n {
                            format!("{}…", &task.slug[..n.saturating_sub(1)])
                        } else {
                            task.slug.clone()
                        };
                        format!("{:<w$}", format!(" {}", s), w = slen)
                    };
                    let slug_st = if is_sel && is_act(3) {
                        sel_act
                    } else if is_sel {
                        sel_base
                    } else {
                        Style::default()
                    };
                    spans.push(Span::styled(slug, slug_st));

                    // Type (field 0)
                    let content = if is_act(0) {
                        format!(
                            "{:<w$}",
                            format!("[{}]", task.task.task_type),
                            w = slot(type_w)
                        )
                    } else {
                        format!(
                            "{:<w$}",
                            format!(" {}", task.task.task_type),
                            w = slot(type_w)
                        )
                    };
                    let type_st = if is_sel && is_act(0) {
                        sel_act
                    } else if is_sel {
                        sel_base
                    } else {
                        Style::default()
                    };
                    spans.push(Span::styled(content, type_st));

                    // Priority (field 1) — vivid colors get colored bg, low uses plain row highlight
                    let pc = priority_color(&task.task.priority);
                    let vivid = is_vivid(pc);
                    let content = if is_act(1) {
                        format!(
                            "{:<w$}",
                            format!("[{}]", task.task.priority),
                            w = slot(prio_w)
                        )
                    } else {
                        format!(
                            "{:<w$}",
                            format!(" {}", task.task.priority),
                            w = slot(prio_w)
                        )
                    };
                    let prio_st = if is_sel && vivid && is_act(1) {
                        sel_prio_act(pc)
                    } else if is_sel && vivid {
                        sel_prio_base(pc)
                    } else if is_sel && is_act(1) {
                        sel_act
                    } else if is_sel {
                        sel_base
                    } else {
                        Style::default().fg(pc)
                    };
                    spans.push(Span::styled(content, prio_st));

                    // Area (field 2)
                    let content = if is_act(2) {
                        format!("[{}]", task.task.area)
                    } else {
                        format!(" {}", task.task.area)
                    };
                    let area_st = if is_sel && is_act(2) {
                        sel_act
                    } else if is_sel {
                        sel_base
                    } else {
                        Style::default()
                    };
                    spans.push(Span::styled(content, area_st));

                    lines.push(Line::from(spans));
                }
            }

            lines.push(Line::from(""));
        }

        lines.pop();
        let visible_lines = area.height as usize;
        let vert_scroll = selected_line.saturating_sub(visible_lines.saturating_sub(2));
        frame.render_widget(Paragraph::new(lines).scroll((vert_scroll as u16, 0)), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let text = format!(
            " Enter:preview  f:filter  e:edit  {}  Ctrl-c:quit ",
            if self.has_active_filters() {
                "q:clear-filters"
            } else {
                "q:quit"
            }
        );

        let filters: Vec<(&str, &str)> = [
            ("status", self.filter_status.as_deref()),
            ("type", self.filter_type.as_deref()),
            ("priority", self.filter_priority.as_deref()),
            ("area", self.filter_area.as_deref()),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect();

        let mut spans = vec![Span::styled(text, Style::default().fg(Color::DarkGray))];
        if !filters.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("Filter:", Style::default().fg(Color::Yellow)));
            for (k, v) in &filters {
                spans.push(Span::styled(
                    format!(" {}={}", k, v),
                    Style::default().fg(Color::Yellow),
                ));
            }
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

impl App<'_> {
    fn handle_events(&mut self) -> Result<Action, Box<dyn std::error::Error>> {
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                return Ok(Action::None);
            }
            match self.mode {
                Mode::Preview => match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.mode = Mode::Browse,
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.preview_scroll = self.preview_scroll.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.preview_scroll = self.preview_scroll.saturating_add(1);
                    }
                    KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                        return Ok(Action::Quit);
                    }
                    _ => {}
                },
                Mode::Browse => {
                    return Ok(match key.code {
                        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                            Action::Quit
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if self.has_active_filters() {
                                self.filter_status = None;
                                self.filter_type = None;
                                self.filter_priority = None;
                                self.filter_area = None;
                                self.filter_search = None;
                                self.reload_tasks();
                                Action::None
                            } else {
                                Action::Quit
                            }
                        }
                        KeyCode::Enter if !self.columns.is_empty() => {
                            self.open_preview();
                            Action::None
                        }
                        KeyCode::Char('f') if !self.columns.is_empty() => {
                            let col = &self.columns[self.selected_column];
                            if let Some(&task_idx) = col.task_indices.get(self.selected_task) {
                                let task = &self.tasks[task_idx];
                                match self.selected_field {
                                    0 => self.filter_type = Some(task.task.task_type.clone()),
                                    1 => self.filter_priority = Some(task.task.priority.clone()),
                                    2 => self.filter_area = Some(task.task.area.clone()),
                                    3 => self.filter_search = Some(task.slug.clone()),
                                    _ => {}
                                }
                                self.reload_tasks();
                            }
                            Action::None
                        }
                        KeyCode::Char('e') if !self.columns.is_empty() => Action::OpenEditor,
                        _ => {
                            self.handle_task_key(key.code);
                            Action::None
                        }
                    });
                }
            }
        }
        Ok(Action::None)
    }

    fn handle_task_key(&mut self, code: KeyCode) {
        if self.columns.is_empty() {
            return;
        }
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_task > 0 {
                    self.selected_task -= 1;
                } else if self.selected_column > 0 {
                    self.selected_column -= 1;
                    self.selected_task = self.columns[self.selected_column]
                        .task_indices
                        .len()
                        .saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let col = &self.columns[self.selected_column];
                if self.selected_task + 1 < col.task_indices.len() {
                    self.selected_task += 1;
                } else if self.selected_column + 1 < self.columns.len() {
                    self.selected_column += 1;
                    self.selected_task = 0;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected_field = if self.selected_field > 0 {
                    self.selected_field - 1
                } else {
                    FIELD_COUNT - 1
                };
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected_field = if self.selected_field + 1 < FIELD_COUNT {
                    self.selected_field + 1
                } else {
                    0
                };
            }
            _ => {}
        }
    }

    fn open_preview(&mut self) {
        if let Some((_slug, path)) = self.current_task_path()
            && let Ok(content) = std::fs::read_to_string(path)
        {
            let width = 80u16;
            let skin = ratskin::RatSkin::default();
            let text = ratskin::RatSkin::parse_text(&content);
            self.preview = skin.parse(text, width);
            self.preview_scroll = 0;
            self.mode = Mode::Preview;
        }
    }

    fn current_task_path(&self) -> Option<(&str, &Path)> {
        let col = self.columns.get(self.selected_column)?;
        let task_idx = col.task_indices.get(self.selected_task)?;
        let task = self.tasks.get(*task_idx)?;
        let path = self.task_paths.get(&task.slug)?;
        Some((&task.slug, path))
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "in-progress" => Color::Magenta,
        "open" => Color::Yellow,
        "blocked" => Color::Red,
        "backlog" => Color::Blue,
        "idea" => Color::Cyan,
        "done" => Color::Green,
        _ => Color::White,
    }
}

fn priority_color(p: &str) -> Color {
    match p {
        "high" => Color::Red,
        "medium" => Color::Yellow,
        "low" => Color::DarkGray,
        _ => Color::White,
    }
}
