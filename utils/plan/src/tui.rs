#![allow(dead_code)]

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
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
enum FocusZone {
    SortBar,
    TaskList,
}

struct App<'a> {
    root: &'a Path,
    tasks: Vec<crate::plan::LoadedTask>,
    task_paths: HashMap<String, PathBuf>,
    focus: FocusZone,
    columns: Vec<Column>,
    selected_column: usize,
    selected_task: usize,
    sort_state: SortState,
    sort_cursor: usize,
    quit: bool,
    scroll_offset: usize,
}

struct Column {
    status: String,
    task_indices: Vec<usize>,
}

impl<'a> App<'a> {
    fn new(root: &'a Path, tasks: Vec<crate::plan::LoadedTask>) -> Self {
        let entries =
            crate::plan::read_task_files(&root.join(".plan").join("tasks")).unwrap_or_default();
        let path_map: HashMap<String, PathBuf> = entries
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|slug| (slug.to_string(), p.clone()))
            })
            .collect();

        let kanban_order = crate::plan::KANBAN_ORDER;
        let mut columns = Vec::new();
        for &status in kanban_order {
            let indices: Vec<usize> = tasks
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

        let selected_column = 0;
        let selected_task = 0;

        Self {
            root,
            tasks,
            task_paths: path_map,
            focus: FocusZone::TaskList,
            columns,
            selected_column,
            selected_task,
            sort_state: SortState::new(),
            sort_cursor: 0,
            quit: false,
            scroll_offset: 0,
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

    let mut app = App::new(root, tasks);
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
        if let Err(e) = app.handle_events() {
            eprintln!("event error: {e}");
            break;
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
            let is_focused = self.focus == FocusZone::SortBar && self.sort_cursor == i;

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
            let style = if is_focused {
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
        // TODO: render kanban columns (Task 4)
        // Placeholder until then
        let text = format!("{} tasks loaded", self.tasks.len());
        frame.render_widget(Paragraph::new(text), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let text = Line::from(Span::styled(
            " Space:toggle sort  Enter:edit  Tab:focus  q:quit ",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(text), area);
    }
}

impl App<'_> {
    fn handle_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                return Ok(());
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                KeyCode::Tab => {
                    self.focus = match self.focus {
                        FocusZone::SortBar => FocusZone::TaskList,
                        FocusZone::TaskList => FocusZone::SortBar,
                    };
                }
                _ => match self.focus {
                    FocusZone::SortBar => self.handle_sort_bar_key(key.code),
                    FocusZone::TaskList => self.handle_task_key(key.code),
                },
            }
        }
        Ok(())
    }

    fn handle_sort_bar_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left | KeyCode::Char('h') => {
                if self.sort_cursor > 0 {
                    self.sort_cursor -= 1;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.sort_cursor < SortField::all().len() - 1 {
                    self.sort_cursor += 1;
                }
            }
            KeyCode::Char(' ') => {
                let field = SortField::all()[self.sort_cursor];
                self.sort_state.toggle(field);
            }
            _ => {}
        }
    }

    fn handle_task_key(&mut self, code: KeyCode) {
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
                if self.selected_column > 0 {
                    self.selected_column -= 1;
                    self.selected_task = self.selected_task.min(
                        self.columns[self.selected_column]
                            .task_indices
                            .len()
                            .saturating_sub(1),
                    );
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_column + 1 < self.columns.len() {
                    self.selected_column += 1;
                    self.selected_task = self.selected_task.min(
                        self.columns[self.selected_column]
                            .task_indices
                            .len()
                            .saturating_sub(1),
                    );
                }
            }
            KeyCode::Char(' ') => {}
            KeyCode::Enter => {}
            _ => {}
        }
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
        // Area → primary, Type → secondary (Priority was pushed to nowhere)
        // Wait: let me trace correctly:
        // 1. toggle(Type): primary = Type, secondary = None
        // 2. toggle(Priority): primary = Priority, secondary = Type
        // 3. toggle(Area): primary = Area, secondary = Priority
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
