# plan tui — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an interactive TUI subcommand to the `plan` tool that renders the kanban view, lets users browse tasks with arrow keys, toggle sort keys with Space, and open task files in `$EDITOR` with Enter.

**Architecture:** New `src/tui.rs` module with a ratatui event loop. Sort state machine tested separately. Reuses existing `plan.rs` task loading/sorting.

**Tech Stack:** Rust + ratatui 0.30 + crossterm, edition 2024.

**Worktree:** `.worktrees/xcargo/snabfab` on branch `refactor/xtask`. All paths relative to worktree root.

## Global Constraints

- Edition 2024, `-D warnings -A clippy::unwrap_used` for clippy
- ratatui 0.30.x, crossterm 0.28.x
- Module `plan.rs` must not change — all TUI code goes in `src/tui.rs`
- `just utils-check` must pass after each task
- `just utils-test` must pass after each task

---

### Task 1: Dependencies and subcommand routing

**Files:**

- Modify: `utils/plan/Cargo.toml`
- Modify: `utils/plan/src/main.rs`

**Interfaces:**

- Consumes: existing `plan::*` functions
- Produces: `plan tui` is a recognized subcommand that routes to `tui::run_tui()`

- [ ] **Step 1: Add ratatui and crossterm to Cargo.toml**

```toml
[dependencies]
# ... existing deps remain ...
ratatui = "0.30"
crossterm = "0.28"
```

- [ ] **Step 2: Add `tui` module declaration and subcommand routing to `src/main.rs`**

`main.rs` needs to check if the first positional arg is `"tui"`. If so, parse remaining args as filter flags and call `tui::run_tui()`.

Insert after `mod plan;`:

```rust
mod tui;
```

At the start of `main()`, before the flag loop:

```rust
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Check for 'tui' subcommand as first positional argument
    let is_tui = args.first().map(|s| s == "tui").unwrap_or(false);
    let start = if is_tui { 1 } else { 0 };

    // Only parse flags from 'start' onward
    let args = &args[start..];
    // ... existing argument parsing loop using `args` (no longer `args`) ...
```

Change the remaining code to use the `args` slice. The `let args: Vec<String> = ...` stays, then:

```rust
fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let is_tui = raw_args.first().map(|s| s == "tui").unwrap_or(false);
    let args: Vec<String> = if is_tui { raw_args.into_iter().skip(1).collect() } else { raw_args };
    // ... rest of the existing argument parsing ...
```

After the existing flag parsing and `plan::list_tasks(...)` call, add:

```rust
    if is_tui {
        tui::run_tui(
            &root,
            status_filter.as_deref(),
            type_filter.as_deref(),
            priority_filter.as_deref(),
            area_filter.as_deref(),
            search_query.as_deref(),
        );
        return;
    }

    plan::list_tasks(
        &root,
        // ... unchanged ...
    );
```

Also update the help text to mention `plan tui`:

```rust
fn print_help() {
    println!("plan [FLAGS]");
    println!("plan tui [FLAGS]  interactive kanban browser\n");
```

Add a `tui` entry to the help text body.

- [ ] **Step 3: Make plan.rs types and functions public**

`tui.rs` needs access to `LoadedTask`, `Task`, `KANBAN_ORDER`, `load_and_filter_tasks`, `read_task_files`, `cmp_tasks`, and `cmp_by_key` from `plan.rs`. Change visibility:

In `plan.rs`:

```rust
pub const KANBAN_ORDER: &[&str] = &["idea", "backlog", "open", "in-progress", "blocked", "done"];
```

Add `pub` to structs:

```rust
pub struct LoadedTask {
    pub slug: String,
    pub task: Task,
}

pub struct Task {
    pub status: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub priority: String,
    #[serde(default)]
    pub area: String,
}
```

Add `pub` to functions:

```rust
pub fn cmp_tasks(a: &LoadedTask, b: &LoadedTask, sort_keys: &[String]) -> std::cmp::Ordering { ... }
pub fn cmp_by_key(a: &LoadedTask, b: &LoadedTask, key: &str) -> std::cmp::Ordering { ... }
pub fn load_and_filter_tasks(...) -> Vec<LoadedTask> { ... }
pub fn read_task_files(tasks_dir: &std::path::Path) -> Result<Vec<PathBuf>, String> { ... }
```

- [ ] **Step 4: Create `src/tui.rs` skeleton**

```rust
use std::path::Path;

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

    eprintln!("plan tui: {} tasks loaded (TUI not yet implemented)", tasks.len());
}
```

- [ ] **Step 5: Run verification**

```bash
cargo check -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 2: Sort state machine

**Files:**

- Create: `utils/plan/src/tui.rs` (add types and test module)

**Interfaces:**

- Produces: `SortField`, `SortDirection`, `SortState` — pure-logic types with unit-tested transitions
- `SortState::sort_keys() -> Vec<(&str, crossterm::style::Color)>` — maps to `plan.rs` sort key strings

- [ ] **Step 1: Add sort types to `tui.rs`**

Add before the `run_tui` function:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Type,
    Priority,
    Area,
    Slug,
}

impl SortField {
    fn all() -> [SortField; 4] {
        [SortField::Type, SortField::Priority, SortField::Area, SortField::Slug]
    }

    fn label(&self) -> &'static str {
        match self {
            SortField::Type => "Type",
            SortField::Priority => "Priority",
            SortField::Area => "Area",
            SortField::Slug => "Slug",
        }
    }

    fn as_sort_key(&self) -> &'static str {
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
    fn symbol(&self) -> &'static str {
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
        SortState { primary: None, secondary: None }
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
        if let Some((pf, pd)) = self.primary {
            if field == pf {
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
        }

        if let Some((sf, _sd)) = self.secondary {
            if field == sf {
                let old_primary = self.primary.take();
                self.primary = self.secondary.take();
                self.secondary = old_primary;
                return;
            }
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
```

- [ ] **Step 2: Write failing tests for sort state machine in tui.rs**

```rust
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
        assert_eq!(s.primary, Some((SortField::Priority, SortDirection::Ascending)));
        assert!(s.secondary.is_none());
    }

    #[test]
    fn toggle_inactive_pushes_old_primary_to_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Type);
        assert_eq!(s.primary, Some((SortField::Type, SortDirection::Ascending)));
        assert_eq!(s.secondary, Some((SortField::Priority, SortDirection::Ascending)));
    }

    #[test]
    fn toggle_primary_ascending_reverses_to_descending() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Priority);
        assert_eq!(s.primary, Some((SortField::Priority, SortDirection::Descending)));
    }

    #[test]
    fn toggle_primary_descending_removes_and_promotes_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Type);
        // Priority is primary ▲, Type is secondary ▲
        // Toggle Priority: ▲ → ▼
        s.toggle(SortField::Priority);
        assert_eq!(s.primary, Some((SortField::Priority, SortDirection::Descending)));
        assert_eq!(s.secondary, Some((SortField::Type, SortDirection::Ascending)));
        // Toggle Priority: ▼ → remove, promote Type to primary
        s.toggle(SortField::Priority);
        assert_eq!(s.primary, Some((SortField::Type, SortDirection::Ascending)));
        assert!(s.secondary.is_none());
    }

    #[test]
    fn toggle_secondary_swaps_with_primary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority); // primary ▲
        s.toggle(SortField::Type);     // Type becomes primary ▲, Priority → secondary ▲
        // s.primary = Type, s.secondary = Priority
        s.toggle(SortField::Priority); // secondary → swap: primary = Priority, secondary = Type
        assert_eq!(s.primary, Some((SortField::Priority, SortDirection::Ascending)));
        assert_eq!(s.secondary, Some((SortField::Type, SortDirection::Ascending)));
    }

    #[test]
    fn sort_keys_empty_when_no_keys_active() {
        let s = SortState::new();
        assert!(s.sort_keys().is_empty());
    }

    #[test]
    fn sort_keys_returns_primary_then_secondary() {
        let mut s = SortState::new();
        s.toggle(SortField::Priority);
        s.toggle(SortField::Type);
        let keys = s.sort_keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], ("priority", SortDirection::Ascending));
        assert_eq!(keys[1], ("type", SortDirection::Ascending));
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
        assert_eq!(s.secondary, Some((SortField::Priority, SortDirection::Ascending)));
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
```

- [ ] **Step 3: Run tests — they should pass**

```bash
cargo test -p plan -- tests::sort_state_starts_empty --nocapture
cargo test -p plan
```

- [ ] **Step 4: Run verification**

```bash
cargo check -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 3: TUI shell — terminal, event loop, layout skeleton

**Files:**

- Modify: `utils/plan/src/tui.rs`

**Interfaces:**

- Consumes: `SortField`, `SortDirection`, `SortState` from Task 2
- Produces: Working TUI shell with quit, focus switching, and layout rendering

- [ ] **Step 1: Add App struct and imports to `tui.rs`**

Replace the skeleton `run_tui` with the full implementation.

Imports:

```rust
use std::{collections::HashMap, path::Path, process::Command};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
```

App struct:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
enum FocusZone {
    SortBar,
    TaskList,
}

struct App<'a> {
    root: &'a Path,
    tasks: Vec<crate::plan::LoadedTask>,
    task_paths: HashMap<String, &'a Path>,
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
```

- [ ] **Step 2: Implement App::new and task loading**

```rust
impl<'a> App<'a> {
    fn new(
        root: &'a Path,
        tasks: Vec<crate::plan::LoadedTask>,
    ) -> Self {
        let entries = crate::plan::read_task_files(&root.join(".plan").join("tasks"))
            .unwrap_or_default();
        let path_map: HashMap<String, &Path> = entries
            .iter()
            .filter_map(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|slug| (slug.to_string(), p.as_path()))
            })
            .collect();

        // Group tasks by status into columns
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
```

I need to make `KANBAN_ORDER` public in `plan.rs`:

```rust
// plan.rs
pub const KANBAN_ORDER: &[&str] = &["idea", "backlog", "open", "in-progress", "blocked", "done"];
```

- [ ] **Step 3: Implement terminal setup and run_tui**

```rust
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

    let _ = ratatui::restore();
}
```

- [ ] **Step 4: Implement render skeleton (sort bar + empty task area + footer)**

```rust
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
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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
```

- [ ] **Step 5: Implement event handling (quit, Tab focus switch)**

```rust
impl App<'_> {
    fn handle_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
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
                    self.selected_task = self.columns[self.selected_column].task_indices.len().saturating_sub(1);
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
                    self.selected_task =
                        self.selected_task.min(self.columns[self.selected_column].task_indices.len().saturating_sub(1));
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.selected_column + 1 < self.columns.len() {
                    self.selected_column += 1;
                    self.selected_task =
                        self.selected_task.min(self.columns[self.selected_column].task_indices.len().saturating_sub(1));
                }
            }
            KeyCode::Char(' ') => {
                // Space in task list is a no-op (sorting from sort bar only)
            }
            KeyCode::Enter => {
                // TODO: open in editor (Task 7)
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 6: Verify compilation**

```bash
cargo check -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 4: Kanban rendering with task navigation

**Files:**

- Modify: `utils/plan/src/tui.rs`

**Interfaces:**

- Consumes: `App.columns`, `App.tasks`, `App.selected_column`, `App.selected_task`
- Produces: Visual kanban board with colored column headers and task rows

- [ ] **Step 1: Implement task area rendering with kanban columns**

Replace the `render_task_area` placeholder:

```rust
impl App<'_> {
    fn render_task_area(&mut self, frame: &mut Frame, area: Rect) {
        let total_cols = self.columns.len();
        if total_cols == 0 || area.width < 10 {
            return;
        }

        // Determine visible columns based on terminal width
        let min_col_width = 20u16;
        let max_visible = (area.width / min_col_width).max(1).min(total_cols) as usize;

        // Keep scroll_offset in bounds
        if self.selected_column < self.scroll_offset {
            self.scroll_offset = self.selected_column;
        } else if self.selected_column >= self.scroll_offset + max_visible {
            self.scroll_offset = self.selected_column + 1 - max_visible;
        }

        let visible_columns = &self.columns[self.scroll_offset..]
            .iter()
            .take(max_visible)
            .enumerate();
        let col_width = area.width / max_visible as u16;

        for (vis_idx, column) in visible_columns {
            let col_start = self.scroll_offset + vis_idx;
            let x = area.x + vis_idx as u16 * col_width;
            let col_area = Rect::new(x, area.y, col_width, area.height);

            self.render_column(frame, col_area, column, col_start);
        }
    }

    fn render_column(&self, frame: &mut Frame, area: Rect, column: &Column, col_idx: usize) {
        let is_active = col_idx == self.selected_column;

        // Column header
        let status_color = status_color(&column.status);
        let header_label = format!(" {} ({}) ", column.status.to_uppercase(), column.task_indices.len());
        let header_style = Style::default()
            .fg(status_color)
            .add_modifier(Modifier::BOLD);
        let header_line = Line::from(Span::styled(header_label.clone(), header_style));

        // Separator
        let dash_count = area.width.saturating_sub(header_label.len().max(5) as u16);
        let sep = "─".repeat(dash_count as usize);

        let mut lines = vec![header_line, Line::from(Span::raw(sep))];

        // Task rows
        for (row_idx, &task_idx) in column.task_indices.iter().enumerate() {
            let is_selected = is_active && row_idx == self.selected_task;
            let task = &self.tasks[task_idx];

            let slug = &task.slug;
            let task_type = &task.task_type;
            let priority = &task.priority;
            let area = &task.area;

            let task_line = if is_selected {
                let indicator = "› ";
                Line::from(vec![
                    Span::styled(
                        format!("{}{:<width$}", indicator, slug, width = 14usize.saturating_sub(2)),
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::styled(
                        format!(" {:<8}", task_type),
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::styled(
                        format!(" {:<6}", priority),
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                    Span::styled(
                        format!(" {}", area),
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                ])
            } else {
                let pc = priority_color(priority);
                Line::from(vec![
                    Span::raw(format!("  {:<14}", slug)),
                    Span::raw(format!(" {:<8}", task_type)),
                    Span::styled(format!(" {:<6}", priority), Style::default().fg(pc)),
                    Span::raw(format!(" {}", area)),
                ])
            };

            lines.push(task_line);
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::default().borders(Borders::NONE)),
            area,
        );
    }
}
```

Add the helper functions (they exist in `plan.rs` but are private):

Need to make `status_color` and `priority_color` public in `plan.rs`:

```rust
// plan.rs
pub fn status_color(status: &str) -> Color { ... }
pub fn priority_color(p: &str) -> Color { ... }
```

Then in `tui.rs` use `crate::plan::status_color` and `crate::plan::priority_color`.

- [ ] **Step 2: Update Cargo.toml imports — ensure `tui.rs` imports `crate::plan`**

Add to `utils/plan/Cargo.toml` if missing (it has `termcolor` already — the color types in `plan.rs` use `termcolor::Color`, but ratatui uses `ratatui::style::Color`. We need to reconcile this.

The `plan.rs` uses `termcolor::Color` for its terminal output. For `tui.rs`, we need `ratatui::style::Color`. These are different types! We can't directly reuse the `status_color` and `priority_color` functions from `plan.rs` because they return `termcolor::Color`, not `ratatui::style::Color`.

I need to either:
a. Create parallel color functions in tui.rs
b. Convert between the two color types

Let me create parallel functions in tui.rs. This is the cleanest approach since plan.rs must not change.

Add to `tui.rs`:

```rust
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
```

- [ ] **Step 3: Verify compilation and test sort state machine still passes**

```bash
cargo test -p plan
cargo check -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 5: Sort integration into task ordering

**Files:**

- Modify: `utils/plan/src/tui.rs`

**Interfaces:**

- Consumes: `App.sort_state`, `App.tasks`, `App.columns`
- Produces: Tasks are re-sorted within columns whenever sort state changes

- [ ] **Step 1: Add sort application to App**

When sort state changes, we need to re-sort the tasks within each column. Add a method:

```rust
impl App<'_> {
    fn apply_sort(&mut self) {
        let keys = self.sort_state.sort_keys();
        if keys.is_empty() {
            // Default sort: priority ascending, then slug ascending
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

        // Rebuild columns with new task order
        self.rebuild_columns();
    }

    fn rebuild_columns(&mut self) {
        let kanban_order = crate::plan::KANBAN_ORDER;
        let mut columns = Vec::new();
        for &status in kanban_order {
            let indices: Vec<usize> = self.tasks
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

        // Clamp selection
        if self.selected_column >= self.columns.len() {
            self.selected_column = self.columns.len().saturating_sub(1);
        }
        if let Some(col) = self.columns.get(self.selected_column) {
            if self.selected_task >= col.task_indices.len() {
                self.selected_task = col.task_indices.len().saturating_sub(1);
            }
        }
    }
}
```

Make `cmp_by_key` public in `plan.rs`:

```rust
pub fn cmp_by_key(a: &LoadedTask, b: &LoadedTask, key: &str) -> std::cmp::Ordering { ... }
```

- [ ] **Step 2: Wire apply_sort into sort state changes**

In `handle_sort_bar_key`, after calling `self.sort_state.toggle(field)`:

```rust
KeyCode::Char(' ') => {
    let field = SortField::all()[self.sort_cursor];
    self.sort_state.toggle(field);
    self.apply_sort();
}
```

Also call `apply_sort()` at the end of `App::new()` after building columns initially:

```rust
// In App::new(), after initial column setup
let mut app = Self { ... };
app.apply_sort();
app
```

Wait, actually initially there's no sort state (all None), so `apply_sort` will sort by default (priority then slug). The tasks are already loaded in the order from `plan::load_and_filter_tasks`. But we should still call `apply_sort` to ensure consistent ordering.

Actually, `load_and_filter_tasks` loads them in filesystem order (readdir order), which is not sorted. So we need `apply_sort` to sort initially.

Let me revise: create the App with `tasks` in the order from `load_and_filter_tasks`, then in `App::new` after building the initial `Self`, call `app.apply_sort()` before returning. But this is a self-referential issue — I need to call it after construction.

The simplest approach: call `apply_sort` right before returning `app` in `App::new` (or right after constructing Self):

```rust
fn new(root: &'a Path, tasks: Vec<crate::plan::LoadedTask>) -> Self {
    // ... build path_map, columns ...

    let mut app = Self {
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
    };

    // Apply default sort (priority ascending, slug ascending)
    app.apply_sort();

    app
}
```

Wait, but `new` takes ownership and returns Self. I can't call methods on Self before it's returned... actually I can, since `app` is a local variable.

Let me adjust:

```rust
pub fn new(root: &'a Path, tasks: Vec<crate::plan::LoadedTask>) -> Self {
    let entries = crate::plan::read_task_files(&root.join(".plan").join("tasks"))
        .unwrap_or_default();
    let path_map: HashMap<String, &Path> = entries
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .map(|slug| (slug.to_string(), p.as_path()))
        })
        .collect();

    let task_count = tasks.len();
    let mut app = Self {
        root,
        tasks,
        task_paths: path_map,
        focus: FocusZone::TaskList,
        columns: Vec::new(),
        selected_column: 0,
        selected_task: 0,
        sort_state: SortState::new(),
        sort_cursor: 0,
        quit: false,
        scroll_offset: 0,
    };

    app.apply_sort();
    debug_assert!(task_count == app.tasks.len(), "task count changed during sort");
    app
}
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p plan
cargo test -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 6: Editor spawning

**Files:**

- Modify: `utils/plan/src/tui.rs`

**Interfaces:**

- Consumes: `App.selected_column`, `App.selected_task`, `App.task_paths`
- Produces: Enter opens selected task file in $EDITOR

- [ ] **Step 1: Implement editor spawn**

Add a method to App:

```rust
impl App<'_> {
    fn open_in_editor(&self) {
        let col = &self.columns[self.selected_column];
        let task_idx = col.task_indices[self.selected_task];
        let task = &self.tasks[task_idx];
        let slug = &task.slug;

        let file_path = match self.task_paths.get(slug) {
            Some(p) => p,
            None => {
                eprintln!("task file not found for slug: {slug}");
                return;
            }
        };

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

        // Restore terminal before spawning editor
        let _ = ratatui::restore();

        let status = Command::new(&editor)
            .arg(file_path)
            .status();

        match status {
            Ok(s) if !s.success() => {
                eprintln!("{editor} exited with error: {s}");
            }
            Err(e) => {
                eprintln!("failed to launch {editor}: {e}");
            }
            _ => {}
        }

        // Re-initialize terminal after editor exits
        // We don't need to manually enter alternate screen again —
        // ratatui::try_init handles it.
        // But we need to re-enter raw mode and alternate screen.
        // A simpler approach: just re-init the terminal.
        let _ = crossterm::terminal::enable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
        // The existing terminal variable in run_tui should still be valid
        // if we use ratatui::try_init() each time... but we destroyed the old one.
        // Better approach: suspend the TUI, spawn editor, resume.
    }
}
```

Actually, the terminal restore/re-init is tricky. The proper ratatui pattern is:

```rust
fn suspend_and_edit(file_path: &Path) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    drop(terminal);  // Drop the terminal binding
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen).ok();
    crossterm::terminal::disable_raw_mode().ok();

    Command::new(&editor).arg(file_path).status().ok();

    crossterm::terminal::enable_raw_mode().ok();
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen).ok();
    // Re-create terminal in run_tui
}
```

But since the `terminal` is owned by `run_tui()` (the local variable in the event loop), I need to restructure. Let me make the `run_tui` function own the terminal and pass it around, or better, have a method that takes a closure.

The simplest approach: have `run_tui` own the terminal, and the event loop passes a reference to it. When Enter is pressed, `run_tui` drops the terminal reference, spawns the editor, then recreates the terminal.

But ownership makes this tricky. Let me use a different approach: pass a "suspend" callback to the App, or have the event loop return a special "action" value.

Actually, the cleanest way is to make `handle_events` return an action enum:

```rust
enum Action {
    None,
    Quit,
    OpenEditor,
}
```

Then in `run_tui`:

```rust
while !app.quit {
    terminal.draw(|frame| app.render(frame))?;
    match app.handle_events()? {
        Action::None => {}
        Action::Quit => app.quit = true,
        Action::OpenEditor => {
            let (slug, path) = app.current_task_path();
            drop(terminal);
            restore_terminal();
            open_editor(&slug, &path);
            terminal = init_terminal()?;
        }
    }
}
```

Let me restructure this way. Update run_tui:

```rust
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

    'outer: while !app.quit {
        if let Err(e) = terminal.draw(|frame| app.render(frame)) {
            eprintln!("render error: {e}");
            break;
        }
        match app.handle_events() {
            Ok(action) => match action {
                Action::None => {}
                Action::Quit => break 'outer,
                Action::OpenEditor => {
                    if let Some((_slug, path)) = app.current_task_path() {
                        // Suspend TUI
                        let _ = ratatui::restore();
                        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                        let status = std::process::Command::new(&editor).arg(path).status();
                        if let Ok(s) = status {
                            if !s.success() {
                                eprintln!("{editor} exited with code: {:?}", s.code());
                            }
                        }
                        // Re-init TUI (restore will have already left alt screen)
                        if let Ok(t) = ratatui::try_init() {
                            terminal = t;
                        } else {
                            eprintln!("failed to re-init terminal after editor");
                            break 'outer;
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

    let _ = ratatui::restore();
}
```

Update `handle_events` to return `Action`:

```rust
enum Action {
    None,
    Quit,
    OpenEditor,
}

impl App<'_> {
    fn handle_events(&mut self) -> Result<Action, Box<dyn std::error::Error>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    return Ok(Action::None);
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(Action::Quit),
                    KeyCode::Tab => {
                        self.focus = match self.focus {
                            FocusZone::SortBar => FocusZone::TaskList,
                            FocusZone::TaskList => FocusZone::SortBar,
                        };
                    }
                    KeyCode::Enter => {
                        if self.focus == FocusZone::TaskList && !self.columns.is_empty() {
                            return Ok(Action::OpenEditor);
                        }
                    }
                    _ => match self.focus {
                        FocusZone::SortBar => self.handle_sort_bar_key(key.code),
                        FocusZone::TaskList => self.handle_task_key(key.code),
                    },
                }
            }
        }
        Ok(Action::None)
    }
}
```

And add `current_task_path` to App:

```rust
impl App<'_> {
    fn current_task_path(&self) -> Option<(&str, &Path)> {
        let col = self.columns.get(self.selected_column)?;
        let task_idx = col.task_indices.get(self.selected_task)?;
        let task = self.tasks.get(*task_idx)?;
        let path = self.task_paths.get(&task.slug)?;
        Some((&task.slug, path))
    }
}
```

- [ ] **Step 2: Verify compilation**

```bash
cargo check -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

---

### Task 7: Polish and edge cases

**Files:**

- Modify: `utils/plan/src/tui.rs`

- [ ] **Step 1: Handle empty columns edge case**

In `handle_task_key`, when all tasks are in a single column and the user presses Up on the first task or Down on the last task, we should wrap (not silently do nothing). Also handle the case where `self.columns` is empty (shouldn't happen if tasks > 0, but guard):

```rust
fn handle_task_key(&mut self, code: KeyCode) {
    if self.columns.is_empty() {
        return;
    }
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            if self.selected_task > 0 {
                self.selected_task -= 1;
            } else if self.selected_column > 0 {
                // Move to previous column, select last task
                self.selected_column -= 1;
                self.selected_task = self.columns[self.selected_column]
                    .task_indices
                    .len()
                    .saturating_sub(1);
            }
            // Already at first task of first column — no-op
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let col = &self.columns[self.selected_column];
            if self.selected_task + 1 < col.task_indices.len() {
                self.selected_task += 1;
            } else if self.selected_column + 1 < self.columns.len() {
                self.selected_column += 1;
                self.selected_task = 0;
            }
            // Already at last task of last column — no-op
        }
        // ... rest unchanged
    }
}
```

- [ ] **Step 2: Run full verification suite**

```bash
cargo check -p plan
cargo test -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
```

- [ ] **Step 3: Smoke test the TUI manually**

```bash
# Should enter TUI
cargo run -p plan -- --root . tui

# With status filter
cargo run -p plan -- --root . tui -s open

# Should still show table
cargo run -p plan -- --root . -k
```

---

### Task 8: Update justfile and help text

**Files:**

- Modify: `utils/plan/src/main.rs`
- Modify: `justfile`

- [ ] **Step 1: Update main.rs help text**

```rust
fn print_help() {
    println!("plan [FLAGS]");
    println!("plan tui [FLAGS]  interactive kanban browser\n");
    println!("Search tasks in .plan directory. Default is table view sorted by priority then slug.\n");
    println!("Subcommands:");
    println!("  tui               interactive kanban browser (arrow keys, Space to sort, Enter to edit)\n");
    // ... rest of help unchanged
}
```

- [ ] **Step 2: Add `plan tui` to justfile**

```just
# Run plan tui <args>
[group('utils')]
plan-tui *args:
    cargo run -p plan -- --root {{justfile_directory()}} tui {{args}}
```

This way `just plan-tui -s open` works.

- [ ] **Step 3: Run full verification**

```bash
cargo check -p plan
cargo test -p plan
cargo clippy -p plan -- -D warnings -A clippy::unwrap_used
just utils-check
just utils-test
```
