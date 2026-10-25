# dashboard

![demo](docs/demo.gif)

Terminal dashboard for browsing local Rust projects, opening them in Zellij, and launching Cargo targets.

## What it does

- Scans `PROJECTS_DIR` (default: `~/code`) for git repos and Cargo projects
- Sorts projects by **recently opened**, with **bookmarks** persisted across runs
- Shows project metadata, git branch/status, size, language stats, and recent GitHub Actions runs
- Discovers Cargo **bin**, **example**, **test**, and **bench** targets
- Lets you **open** a project, **run** a target in a Zellij pane, or **explore/edit** the target source


## Installation

### Prerequisites

Install Rust/Cargo, [Fish](https://fishshell.com/), [Zellij](https://zellij.dev/), [Helix](https://helix-editor.com/), and the [GitHub CLI](https://cli.github.com/). Install [`tokei`](https://github.com/XAMPPRocky/tokei) to show language statistics.

### Install dashboard

```fish
git clone https://github.com/dmyyy/cargo-dashboard.git
cd cargo-dashboard
cargo install --path . --locked
```

### Authenticate GitHub CLI

GitHub Actions runs require an authenticated `gh` session:

```fish
gh auth login
gh auth status
```

Follow the interactive prompts to sign in to GitHub.com. Then start dashboard with `dashboard`; set `PROJECTS_DIR` first to scan a directory other than `~/code`.

## Keymap

### Global

- `q` / `Ctrl-c` — quit
- `Esc` — clear filters / cancel pending `g`
- `gg` — jump to top
- `ge` — jump to bottom
- `j` / `k` or `↓` / `↑` — move selection
- `h` / `l` or `←` / `→` — move focus between panes

### Projects

- `Enter` — open project in a new Zellij tab with OMP in a floating pane
- `O` — open project without switching to it; OMP starts without taking focus
- `a` — add project (`cargo new`) or clone from git URL
- `b` — toggle bookmark
- `d` — delete project (with confirmation)
- `/` — search projects
- `Backspace` — delete from active project filter without entering search mode

### Targets

- `Enter` — launch selected target in a new Zellij pane
- `e` — explore/edit target source in Helix
- `/` — search targets
- `Backspace` — delete from active target filter without entering search mode

### CI runs

- `Enter` — open selected GitHub Actions run in browser

## Notes

- Bookmarks and recents are stored under `~/.local/state/dashboard` (or `XDG_STATE_HOME`).
- CI requires `gh`; language stats require `tokei`.
- Opening/running targets assumes a Zellij-based workflow and `hx` for target editing.
