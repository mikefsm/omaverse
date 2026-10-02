# omaverse

A small scripture outlining app for omarchy. Collapsible outline tree on the
left, free-text body for the selected node on the right, plain Markdown on disk.

Built because Notion's nested toggle lists are the right shape for outlining a
book of scripture, and the rest of Notion is not needed for it.

## File format

Outlines are ordinary Markdown. Each node is a `-` bullet — its **title**, shown
in the outline pane — plus optional indented body text, shown in the body pane.

```markdown
# Romans

- Introduction (1:1-17)

  Paul stacks three self-descriptions here, each one
  pointing away from himself.

  * servant — δοῦλος, not διάκονος
  * apostle — sent with authority

  - The greeting

    Note the inversion: "called to be an apostle".
  - Thanksgiving
- God's wrath revealed (1:18-32)
```

Rules:

1. A leading `# ` heading is the document title; otherwise the filename is used.
2. `- ` at indent *N* is an outline node at depth *N* / 2.
3. Any other line indented under a node is that node's body.
4. **`*` and `+` bullets are prose inside a body, never structure.** One file has
   to carry both the outline tree and prose that may itself contain bullets, so
   `-` is structural and `*` is not. Both render identically everywhere else; the
   distinction only means something to omaverse.

Reading is forgiving — tabs, over-indentation, and missing blank lines are all
accepted. Writing is strict: 2 spaces per level, a blank line around each body.
So the first save of a hand-written file produces a one-time whitespace diff.

Nothing app-specific is ever written into the Markdown. Fold state and the
current selection live in `~/.local/state/omaverse/`, which is deliberately
outside Dropbox: it is per-machine and disposable.

## Configuration

`~/.config/omaverse/config.toml`, written with defaults on first run:

```toml
outline_dir = "/home/you/Dropbox/Documents/Biblical Studies/Outlines"
```

This is only where the sidebar **looks** and where the file chooser **starts** —
not a restriction. `Ctrl+N` opens a normal save dialog, so an outline can go
anywhere on disk. The directory is scanned recursively and immediate subfolders
become sidebar groups, so `Outlines/New Testament/romans.md` appears under
"New Testament".

Outlines saved outside that directory would otherwise be invisible to the
sidebar, so the last ten files opened from elsewhere appear under a **Recent**
group. `Ctrl+O` opens any file directly.

Creating a new outline never overwrites an existing file: if the name is already
taken, that outline is opened instead.

## Keys

In the outline pane:

| Key | Action |
|---|---|
| `Enter` | New node below |
| `Tab` / `Shift+Tab` | Indent / outdent |
| `Alt+↑` / `Alt+↓` | Move up / down |
| `←` / `→` | Collapse / expand |
| `Delete` | Delete node (confirms if it has notes or children) |
| `F2` | Rename — jumps to the title field |

Anywhere:

| Key | Action |
|---|---|
| `Ctrl+Enter` | Jump between title and body |
| `Ctrl+N` | New outline (opens a file chooser) |
| `Ctrl+O` | Open an outline |
| `Ctrl+S` | Save now (autosave already runs every 800 ms) |
| `Ctrl+\\` | Toggle the library sidebar |

A node's title is edited in the field above the body, not in the tree. `Enter`
there drops into the body, which is the order you write in. Pressing `Enter` in
the tree makes a new node and puts the cursor in its title, so a fresh outline is
type-Enter-type-Enter.

## Build

Needs Rust, GTK 4.12+, and libadwaita 1.5+.

```sh
cargo build --release
./target/release/omaverse path/to/romans.md
```

## Status

Phases 1 and 2 of 4. Working: library sidebar with folder groups and recents,
new/open via file chooser, outline tree with persistent fold state, full keyboard
restructuring, rename, delete with confirmation, body editing, autosave, atomic
writes.

Not yet: live-rendered Markdown in the body pane (phase 3), installer /
`.desktop` entry / external-change detection / spellcheck (phase 4).

### Design notes

Structural editing lives in `src/edit.rs` as pure functions of
(document, selection) → (document, new selection), with no GTK involved, so the
behaviour is tested without needing a display. The widget handlers only translate
a keystroke into a command and apply the result.
