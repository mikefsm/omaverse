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
| `Ctrl+M` | Merge into the section above — drops this heading, its text joins |
| `Ctrl+X` / `Ctrl+V` | Cut a section, paste it in below the selection |
| drag a row | Move a section anywhere: drop on an edge for a sibling, in the middle for a child |
| `Ctrl+Enter` / `F2` | Jump into the document |

In the document:

| Key | Action |
|---|---|
| `Ctrl+Enter` | Split the section in two at the cursor |
| `Tab` / `Shift+Tab` | Indent / outdent the section the cursor is in |
| `Alt+↑` / `Alt+↓` | Move that section up / down |
| `Escape` | Back to the outline pane |

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

Folding a section in the outline pane folds it away in the document too, and
nothing is lost: the lifted text is parked with a mark and spliced back whenever
the file is read or saved.

Not yet: fold triangles in the document itself (folding is driven from the
outline pane), live-rendered bold/italic inside body text, installer /
`.desktop` entry / external-change detection / spellcheck.

### Reorganising

Restructuring works from either side — the same keys apply to the selected row
in the outline and to the section the cursor is in while writing, because
needing to leave the text to reorganise it is exactly when you don't want to.

The outline pane is where sections get moved: `Tab`/`Shift+Tab` to change depth,
`Alt+↑`/`Alt+↓` between siblings, and dragging for anything further. Cut and paste
carry a whole section — heading, text and everything nested under it — in an
internal clipboard, so cutting a section never clobbers what you copied.

`Ctrl+M` is the common case of "this actually belongs with the one above": it
drops the heading and its text joins the section above, adopting its children.
`Ctrl+Enter` in the document is the inverse: it splits at the cursor, which is
also how you impose structure on a book you have just pasted in whole. Moving *text* rather than sections needs nothing
special — it is one continuous buffer, so ordinary select, cut and paste work
across section boundaries.

### Folding

GtkTextView has no real folding. An `invisible` tag hides the glyphs but keeps
the line boxes, so a folded section leaves a blank hole exactly as tall as what
it hid. Folding therefore removes the lines from the buffer and parks them with
a `GtkTextMark`, which moves with surrounding edits. Everything that reads the
document -- saving, re-parsing -- goes through `full_text()`, which splices the
parked text back, so a fold can never cost you content.

### Design notes

The text buffer holds the file verbatim and is the single source of truth. The
outline tree is derived from it by parsing, so there are no two copies to keep
in sync, and what is saved is exactly what was typed — appearance comes entirely
from tags that hide the `- ` markers and the leading indentation.

Structural editing lives in `src/edit.rs` as pure functions of
(document, selection) → (document, new selection), and line classification for
rendering lives in `src/docview.rs`, also pure. Both are tested without a
display; the widget handlers only translate a keystroke into a command.
