# plan tui — interactive kanban TUI for task browsing

## Problem

The plan tool lists `.plan/tasks` in table or kanban view but requires re-running with different flags to change sort order. There is no way to browse tasks interactively or open a task file in an editor from the terminal.

## Solution

Add a `plan tui` subcommand that renders the same kanban layout as `plan -k` in an interactive terminal UI built with `ratatui` + `crossterm`. Users navigate tasks with arrow keys, toggle sort keys with Space, and open a task file in `$EDITOR` with Enter.

## Dependencies

Add `ratatui` and `crossterm` to `utils/plan/Cargo.toml`.

## Layout

Three zones, terminal height permitting:

```
┌─ Sort: [Type] [Priority▲] [Area] [Slug] ────────────────  <- sort bar (focusable)
│
│  OPEN (2) ──────────────  │  IN-PROGRESS (1) ──────────  <- kanban columns
│  ⬤ bugs-a  bug  high be  │  feat-x  feat  high back
│    feat-b  feat med  fr   │
│                           │
│  DONE (1) ──────────────  │  BACKLOG (0) ──────────────
│  chore  chore low frnt    │  (none)
│                           │
├─────────────────────────────────────────────────────────
│  Space:toggle sort  Enter:edit  Tab:focus  q:quit         <- footer
```

Two focus zones: **sort bar** (top) and **task list** (middle). The user tabs between them.

Empty kanban columns are skipped during Left/Right navigation — the cursor never lands on a "(none)" column.

When the sort bar is focused, Up/Down/Enter are no-ops (key does nothing).

## Cursor & navigation

| Key            | Sort bar zone               | Task list zone                           |
| -------------- | --------------------------- | ---------------------------------------- |
| `Left`/`Right` | Move between sort key slots | Move between kanban columns              |
| `Up`/`Down`    | —                           | Move between tasks in the current column |
| `Tab`          | Switch to task list         | Switch to sort bar                       |
| `Space`        | Toggle/swap sort key        | —                                        |
| `Enter`        | —                           | Open task file in `$EDITOR`              |
| `q` / `Esc`    | Quit                        | Quit                                     |

## Sort interaction

Sort keys shown in the bar are: Type, Priority, Area, Slug (status is implicit as the kanban grouping dimension).

- **Inactive key**: name with no suffix.
- **Primary sort key**: name + `▲` (or `▼` if descending).
- **Secondary sort key**: name + dim `▲`/`▼`.

Space cycles through three states: inactive → ascending → descending → inactive (removed).

| Current state            | Space pressed                                                                                 | Result |
| ------------------------ | --------------------------------------------------------------------------------------------- | ------ |
| Inactive                 | Becomes primary sort. Previous primary (if any) → secondary.                                  |
| Primary ascending (▲)    | Toggle to descending (▼).                                                                     |
| Primary descending (▼)   | **Remove** from sort chain. Secondary (if any) promoted to primary.                           |
| Secondary ascending (▼▲) | **Swap** with primary: secondary → primary, primary → secondary. Direction preserved per key. |

When a key is removed and no sort keys remain, the default sort (priority ascending, slug ascending) applies.

Tasks are sorted within each kanban column by the active sort keys. Default sort (when none activated) = priority ascending, then slug ascending — matching the existing default.

## Editor integration

`std::process::Command::new(editor).arg(path).status()`, where `editor` = `$EDITOR` env var with fallback to `"vi"`. Blocks until the editor process exits, then re-renders the TUI.

Task file path = `<root>/.plan/tasks/<slug>.md` (already known from task loading).

## Reused code

- `plan::load_and_filter_tasks()` — loads, parses, and filters tasks
- `plan::read_task_files()` — enumerates `.plan/tasks/*.md`
- `plan::validate_all()` — frontmatter validation

## Module structure

- `src/tui.rs` — TUI event loop, ratatui UI building, editor spawning
- `src/main.rs` — recognize `tui` subcommand, route to `tui::run_tui()`

No changes to `src/plan.rs`.

## CLI integration

`plan tui [filter-flags]` — same `-s`, `-t`, `-p`, `-a`, `-q`, `-r` flags as the table/kanban mode. Filters are applied before entering the TUI.

## Non-goals

- Inline editing of task files within the TUI. The editor is the tool for that.
- Drag-and-drop of tasks between columns.
- Task creation or deletion from the TUI.
- Scrolling within a column (all columns fit on screen; if a column overflows, it scrolls vertically with Up/Down).

## Open questions

None.
