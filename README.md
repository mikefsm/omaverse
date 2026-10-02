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

The document on the right is the whole book, continuously. The outline on the
left is navigation: selecting a section scrolls to it, and collapsing one folds
it away in the document too. The cursor and the outline follow each other.

In the outline pane:

| Key | Action |
|---|---|
| `Enter` | New section below, cursor lands in its heading |
| `Tab` / `Shift+Tab` | Indent / outdent |
| `Alt+↑` / `Alt+↓` | Move up / down |
| `←` | Fold the section, or step out to its parent |
| `→` | Unfold, or step in to the first child |
| `Delete` | Delete section (confirms if it has text or children) |
| `Ctrl+Enter` / `F2` | Jump into the document |

In the document:

| Key | Action |
|---|---|
| `Ctrl+Enter` / `Escape` | Back to the outline pane |

Anywhere:

| Key | Action |
|---|---|
| `Ctrl+N` | New outline (opens a file chooser) |
| `Ctrl+O` | Open an outline |
| `Ctrl+S` | Save now (autosave already runs every 800 ms) |
| `Ctrl+\\` | Toggle the library sidebar |
| `Ctrl+W` / `Ctrl+Q` | Close (saves first) |

## Build

Needs Rust, GTK 4.12+, and libadwaita 1.5+.

```sh
cargo build --release
./target/release/omaverse path/to/romans.md
```

## Status

Working: library sidebar with folder groups and recents, new/open via file
chooser, the whole book as one continuous document with headings styled by
depth, an outline pane that navigates and folds it, keyboard restructuring,
delete with confirmation, autosave, atomic writes.

Not yet: inline fold triangles in the document itself (folding is driven from
the outline pane for now), live-rendered bold/italic inside body text,
installer / `.desktop` entry / external-change detection / spellcheck.

### Design notes

The text buffer holds the file verbatim and is the single source of truth. The
outline tree is derived from it by parsing, so there are no two copies to keep
in sync, and what is saved is exactly what was typed — appearance comes entirely
from tags that hide the `- ` markers and the leading indentation.

Structural editing lives in `src/edit.rs` as pure functions of
(document, selection) → (document, new selection), and line classification for
rendering lives in `src/docview.rs`, also pure. Both are tested without a
display; the widget handlers only translate a keystroke into a command.
