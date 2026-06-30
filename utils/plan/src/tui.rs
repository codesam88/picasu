use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Type,
    Priority,
    Area,
    Slug,
}

impl SortField {
    pub fn all() -> [SortField; 4] {
        [
            SortField::Type,
            SortField::Priority,
            SortField::Area,
            SortField::Slug,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            SortField::Type => "Type",
            SortField::Priority => "Priority",
            SortField::Area => "Area",
            SortField::Slug => "Slug",
        }
    }

    pub fn as_sort_key(&self) -> &'static str {
        match self {
            SortField::Type => "type",
            SortField::Priority => "priority",
            SortField::Area => "area",
            SortField::Slug => "slug",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn symbol(&self) -> &'static str {
        match self {
            SortDirection::Ascending => "▲",
            SortDirection::Descending => "▼",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SortState {
    pub primary: Option<(SortField, SortDirection)>,
    pub secondary: Option<(SortField, SortDirection)>,
}

impl SortState {
    pub fn new() -> Self {
        SortState {
            primary: None,
            secondary: None,
        }
    }

    /// Toggle the given sort field according to the state machine:
    ///
    /// | Current | Press | Result |
    /// |---|---|---|
    /// | Inactive | Space | Becomes primary ▲, old primary → secondary |
    /// | Primary ▲ | Space | Primary ▼ |
    /// | Primary ▼ | Space | Remove. Secondary (if any) → primary |
    /// | Secondary | Space | Swap with primary |
    pub fn toggle(&mut self, field: SortField) {
        if let Some((pf, pd)) = self.primary
            && field == pf
        {
            match pd {
                SortDirection::Ascending => {
                    self.primary = Some((field, SortDirection::Descending));
                }
                SortDirection::Descending => {
                    self.primary = self.secondary.take();
                }
            }
            return;
        }

        if let Some((sf, _sd)) = self.secondary
            && field == sf
        {
            let old_primary = self.primary.take();
            self.primary = self.secondary.take();
            self.secondary = old_primary;
            return;
        }

        let old_primary = self.primary.take();
        self.primary = Some((field, SortDirection::Ascending));
        self.secondary = old_primary;
    }

    /// Returns sort keys with direction for use with plan::cmp_by_key.
    /// The first entry is primary sort, second (if any) is secondary.
    pub fn sort_keys(&self) -> Vec<(&'static str, SortDirection)> {
        let mut keys = Vec::new();
        if let Some((f, d)) = self.primary {
            keys.push((f.as_sort_key(), d));
        }
        if let Some((f, d)) = self.secondary {
            keys.push((f.as_sort_key(), d));
        }
        keys
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Quit,
    OpenPreview,
    OpenEditor,
}

struct App<'a> {
    #[allow(dead_code)]
    root: &'a Path,
    tasks: Vec<crate::plan::LoadedTask>,
    task_paths: HashMap<String, PathBuf>,
    columns: Vec<Column>,
    selected_column: usize,
    selected_task: usize,
    selected_field: usize,
    sort_state: SortState,
    quit: bool,
}

struct Column {
    status: String,
    task_indices: Vec<usize>,
}

impl<'a> App<'a> {
    fn new(root: &'a Path, tasks: Vec<crate::plan::LoadedTask>, entries: &[PathBuf]) -> Self {
        let path_map: HashMap<String, PathBuf> = entries
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|slug| (slug.to_string(), p.clone()))
            })
            .collect();

        let task_count = tasks.len();
        let mut app = Self {
            root,
            tasks,
            task_paths: path_map,
            columns: Vec::new(),
            selected_column: 0,
            selected_task: 0,
            selected_field: 0,
            sort_state: SortState::new(),
            quit: false,
        };

        app.apply_sort();
        debug_assert!(
            task_count == app.tasks.len(),
            "task count changed during sort"
        );
        app
    }

    fn apply_sort(&mut self) {
        let keys = self.sort_state.sort_keys();
        if keys.is_empty() {
            self.tasks.sort_by(|a, b| crate::plan::cmp_tasks(a, b, &[]));
        } else {
            self.tasks.sort_by(|a, b| {
                for (key, direction) in &keys {
                    let ord = crate::plan::cmp_by_key(a, b, key);
                    if ord != std::cmp::Ordering::Equal {
                        return match direction {
                            SortDirection::Ascending => ord,
                            SortDirection::Descending => ord.reverse(),
                        };
                    }
                }
                std::cmp::Ordering::Equal
            });
        }
        self.rebuild_columns();
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
            if !indices.is_empty() {
                columns.push(Column {
                    status: status.to_string(),
                    task_indices: indices,
                });
            }
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

    let mut app = App::new(root, tasks, &entries);
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
                Action::OpenPreview => {
                    if let Some((_slug, path)) = app.current_task_path() {
                        ratatui::restore();
                        let previewers = ["glow", "view", "cat"];
                        let cmd = previewers.iter().find(|cmd| {
                            std::process::Command::new(cmd)
                                .arg("--version")
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status()
                                .is_ok()
                        });
                        if let Some(cmd) = cmd {
                            std::process::Command::new(cmd).arg(path).status().ok();
                        } else {
                            if let Ok(s) = std::fs::read_to_string(path) {
                                println!("{}", s);
                            }
                        }
                        if let Ok(t) = ratatui::try_init() {
                            terminal = t;
                        } else {
                            eprintln!("failed to re-init terminal after preview");
                            break;
                        }
                    }
                }
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
        let [sort_bar_area, task_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        self.render_sort_bar(frame, sort_bar_area);
        self.render_task_area(frame, task_area);
        self.render_footer(frame, footer_area);
    }

    fn render_sort_bar(&self, frame: &mut Frame, area: Rect) {
        let fields = SortField::all();
        let mut spans = Vec::new();
        spans.push(Span::raw(" Sort: "));

        for (i, field) in fields.iter().enumerate() {
            let is_active_field = self.selected_field == i;

            let is_primary = self
                .sort_state
                .primary
                .map(|(f, _)| f == *field)
                .unwrap_or(false);
            let is_secondary = self
                .sort_state
                .secondary
                .map(|(f, _)| f == *field)
                .unwrap_or(false);

            let dir = if is_primary {
                self.sort_state.primary.unwrap().1.symbol()
            } else if is_secondary {
                self.sort_state.secondary.unwrap().1.symbol()
            } else {
                ""
            };

            let label = format!("{}{}", field.label(), dir);
            let style = if is_active_field {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if is_primary {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if is_secondary {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };

            spans.push(Span::styled(format!(" {} ", label), style));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_task_area(&mut self, frame: &mut Frame, area: Rect) {
        if self.columns.is_empty() || area.width < 10 {
            return;
        }

        let slug_w = self
            .tasks
            .iter()
            .map(|t| t.slug.len())
            .max()
            .unwrap_or(4)
            .clamp(4, 28);
        let type_w = 8;
        let prio_w = 8;

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
            let dash_count = area.width.saturating_sub(header.len() as u16 + 2);
            let sep = "─".repeat(dash_count.max(1) as usize);

            lines.push(Line::from(Span::styled(
                format!("{} {}", header, sep),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));

            if column.task_indices.is_empty() {
                lines.push(Line::from(Span::raw(" (none)")));
            } else {
                for (row_idx, &task_idx) in column.task_indices.iter().enumerate() {
                    let task = &self.tasks[task_idx];
                    let is_selected = is_active && row_idx == self.selected_task;

                    if is_selected {
                        selected_line = lines.len();
                    }

                    // fields: 0=type, 1=priority, 2=area, 3=slug
                    let indicator = if is_selected { "›" } else { " " };
                    let slug_hl = is_selected && self.selected_field == 3;
                    let type_hl = is_selected && self.selected_field == 0;
                    let prio_hl = is_selected && self.selected_field == 1;
                    let area_hl = is_selected && self.selected_field == 2;

                    let mut spans = Vec::new();
                    spans.push(Span::styled(
                        format!(
                            " {}{:<slug_w$}",
                            indicator,
                            task.slug,
                            slug_w = slug_w.saturating_sub(1)
                        ),
                        if slug_hl {
                            Style::default().add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        },
                    ));
                    spans.push(Span::styled(
                        format!(" {:<type_w$}", task.task.task_type, type_w = type_w),
                        if type_hl {
                            Style::default().add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        },
                    ));
                    let pc = priority_color(&task.task.priority);
                    spans.push(Span::styled(
                        format!(" {:<prio_w$}", task.task.priority, prio_w = prio_w),
                        if prio_hl {
                            Style::default().fg(pc).add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default().fg(pc)
                        },
                    ));
                    spans.push(Span::styled(
                        format!(" {}", task.task.area),
                        if area_hl {
                            Style::default().add_modifier(Modifier::REVERSED)
                        } else if is_selected {
                            Style::default().add_modifier(Modifier::DIM)
                        } else {
                            Style::default()
                        },
                    ));
                    lines.push(Line::from(spans));
                }
            }

            lines.push(Line::from(""));
        }

        // Trim trailing blank line
        lines.pop();

        // Vertical scroll to keep selected line visible
        let visible_lines = area.height as usize;
        let vert_scroll = selected_line.saturating_sub(visible_lines.saturating_sub(2));

        frame.render_widget(Paragraph::new(lines).scroll((vert_scroll as u16, 0)), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let text = Line::from(Span::styled(
            " Space:sort  Enter:preview  e:edit  ↑↓←→:navigate  q/Ctrl-c:quit ",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(text), area);
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
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(Action::Quit),
                KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                    return Ok(Action::Quit);
                }
                KeyCode::Enter => {
                    if !self.columns.is_empty() {
                        return Ok(Action::OpenPreview);
                    }
                }
                KeyCode::Char('e') => {
                    if !self.columns.is_empty() {
                        return Ok(Action::OpenEditor);
                    }
                }
                _ => self.handle_task_key(key.code),
            }
        }
        Ok(Action::None)
    }

    fn handle_task_key(&mut self, code: KeyCode) {
        if self.columns.is_empty() {
            return;
        }
        let field_count = SortField::all().len();
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
                if self.selected_field > 0 {
                    self.selected_field -= 1;
                } else if self.selected_column > 0 {
                    self.selected_column -= 1;
                    self.selected_task = self.selected_task.min(
                        self.columns[self.selected_column]
                            .task_indices
                            .len()
                            .saturating_sub(1),
                    );
                    self.selected_field = field_count - 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_field + 1 < field_count {
                    self.selected_field += 1;
                } else if self.selected_column + 1 < self.columns.len() {
                    self.selected_column += 1;
                    self.selected_task = self.selected_task.min(
                        self.columns[self.selected_column]
                            .task_indices
                            .len()
                            .saturating_sub(1),
                    );
                    self.selected_field = 0;
                }
            }
            KeyCode::Char(' ') => {
                let field = SortField::all()[self.selected_field];
                self.sort_state.toggle(field);
                self.apply_sort();
            }
            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_state_starts_empty() {
        let s = SortState::new();
        assert!(s.primary.is_none());
        assert!(s.secondary.is_none());
    }

    #[test]
    fn toggle_inactive_becomes_primary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        assert_eq!(
            s.primary,
            Some((SortField::Priority, SortDirection::Ascending))
        );
        assert!(s.secondary.is_none());
    }

    #[test]
    fn toggle_inactive_pushes_old_primary_to_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Type);
        assert_eq!(s.primary, Some((SortField::Type, SortDirection::Ascending)));
        assert_eq!(
            s.secondary,
            Some((SortField::Priority, SortDirection::Ascending))
        );
    }

    #[test]
    fn toggle_primary_ascending_reverses_to_descending() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Priority);
        assert_eq!(
            s.primary,
            Some((SortField::Priority, SortDirection::Descending))
        );
    }

    #[test]
    fn toggle_primary_descending_removes_and_promotes_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority); // primary = (P, ▲)
        s.toggle(SortField::Type); // primary = (T, ▲), secondary = (P, ▲)
        s.toggle(SortField::Priority); // P is secondary → swap: primary = (P, ▲), secondary = (T, ▲)
        s.toggle(SortField::Priority); // P is primary ▲ → ▼: primary = (P, ▼), secondary = (T, ▲)
        assert_eq!(
            s.primary,
            Some((SortField::Priority, SortDirection::Descending))
        );
        assert_eq!(
            s.secondary,
            Some((SortField::Type, SortDirection::Ascending))
        );
        // Toggle Priority: ▼ → remove, promote Type to primary
        s.toggle(SortField::Priority);
        assert_eq!(s.primary, Some((SortField::Type, SortDirection::Ascending)));
        assert!(s.secondary.is_none());
    }

    #[test]
    fn toggle_secondary_swaps_with_primary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority); // primary ▲
        s.toggle(SortField::Type); // Type becomes primary ▲, Priority → secondary ▲
        // s.primary = Type, s.secondary = Priority
        s.toggle(SortField::Priority); // secondary → swap: primary = Priority, secondary = Type
        assert_eq!(
            s.primary,
            Some((SortField::Priority, SortDirection::Ascending))
        );
        assert_eq!(
            s.secondary,
            Some((SortField::Type, SortDirection::Ascending))
        );
    }

    #[test]
    fn sort_keys_empty_when_no_keys_active() {
        let s = SortState::new();
        assert!(s.sort_keys().is_empty());
    }

    #[test]
    fn sort_keys_returns_primary_then_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority); // primary = (P, ▲)
        s.toggle(SortField::Type); // primary = (T, ▲), secondary = (P, ▲)
        let keys = s.sort_keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], ("type", SortDirection::Ascending));
        assert_eq!(keys[1], ("priority", SortDirection::Ascending));
    }

    #[test]
    fn toggle_different_fields_accumulate() {
        let mut s = SortState::new();
        s.toggle(SortField::Type);
        s.toggle(SortField::Priority);
        s.toggle(SortField::Area);
        assert_eq!(s.primary, Some((SortField::Area, SortDirection::Ascending)));
        assert_eq!(
            s.secondary,
            Some((SortField::Priority, SortDirection::Ascending))
        );
    }

    #[test]
    fn sort_field_as_sort_key_maps_correctly() {
        assert_eq!(SortField::Type.as_sort_key(), "type");
        assert_eq!(SortField::Priority.as_sort_key(), "priority");
        assert_eq!(SortField::Area.as_sort_key(), "area");
        assert_eq!(SortField::Slug.as_sort_key(), "slug");
    }

    #[test]
    fn sort_field_label_is_readable() {
        assert_eq!(SortField::Type.label(), "Type");
        assert_eq!(SortField::Priority.label(), "Priority");
        assert_eq!(SortField::Area.label(), "Area");
        assert_eq!(SortField::Slug.label(), "Slug");
    }

    #[test]
    fn sort_direction_symbol_roundtrips() {
        assert_eq!(SortDirection::Ascending.symbol(), "▲");
        assert_eq!(SortDirection::Descending.symbol(), "▼");
    }
}
