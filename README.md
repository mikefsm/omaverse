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
| `Ctrl+Shift+A` | Annotate the selected words, or reopen the note under the cursor |
| `Escape` | Back to the outline pane |

Anywhere:

| Key | Action |
|---|---|
| `Ctrl+N` | New outline (opens a file chooser) |
| `Ctrl+O` | Open an outline |
| `Ctrl+S` | Save now (autosave already runs every 800 ms) |
| `Ctrl+\\` | Toggle the library sidebar |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo (also `Ctrl+Y`) |
| `Ctrl+W` / `Ctrl+Q` | Close (saves first) |

## Install

Needs Rust, GTK 4.12+, and libadwaita 1.5+.

```sh
./install.sh
```

That builds, then puts the binary in `~/.local/bin`, a desktop entry in
`~/.local/share/applications` and an icon in `~/.local/share/icons`, so omaverse
appears in the omarchy launcher and opens `.md` files on double-click. The
desktop entry is named for the application id so the window picks up its icon
under Wayland.

To run without installing:

```sh
cargo build --release
./target/release/omaverse path/to/romans.md
```

## Status

Working: library sidebar with folder groups and recents, new/open via file
chooser, the whole book as one continuous document with headings styled by
depth, an outline pane that navigates and folds it, keyboard restructuring,
delete with confirmation, autosave, atomic writes.

Every section with anything underneath it carries a disclosure triangle in the
margin beside its heading, and folding one hides its contents in place. Nothing
is lost: the lifted text is parked with a mark and spliced back whenever the file
is read or saved. The outline pane folds the same sections with the same state.

Bold, italic and inline code render as you write: `**bold**`, `*italic*`,
`_italic_` and `` `code` ``. The markers stay visible on the line the cursor is
on, so what you are editing is never hidden from you, and a `*` opening a prose
list is left alone rather than read as emphasis.

Not yet: spellcheck.

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

### Files changing underneath you

These outlines live in Dropbox, so another machine can rewrite one while it is
open here. Saving therefore refuses to write whenever the file on disk is not
the one omaverse last left — that check lives in the save itself, not in a
timer, because autosave runs more often than any watch interval and a timer
always loses the race.

When that happens you are asked which version wins, and the one you set aside is
written to `~/.local/state/omaverse/conflicts/` rather than discarded. If the
file changes while you have no unsaved edits, it just reloads.

### Theming

omaverse follows the current omarchy theme. Themes publish a palette at
`~/.local/state/omarchy/current/theme/colors.toml`, and that is mapped onto
libadwaita's named colours, so the window, sidebar, header bar, selection and
accent all match whatever theme is set. Light and dark come from the palette's
own `mode` rather than the desktop preference, which can disagree with the
palette actually loaded. A theme switch is picked up within a few seconds, with
no restart.

Typography is deliberately left alone: the serif body and the outline keep
their faces and sizes and simply inherit the themed foreground.

### Notes

Select some words and press `Ctrl+Shift+A`, or click any underlined phrase. The
note opens with the cursor already in it; `Ctrl+Enter` files it, `Escape`
abandons it.

A note is either your own comment or a quotation. Filling in the source marks it
as a quotation, so the file records whose words they are rather than leaving you
to remember months later. Sources already used in the same outline are offered
back with one click, which keeps the wording consistent without a bibliography
to maintain.

In the file an annotated phrase is `==the words==[^n1]`, with the note itself
written as an ordinary Markdown footnote at the foot of the file:

```markdown
[^n1]: The word is stronger than it looks in English.

[^n2]: > An invented sentence standing in for a quoted paragraph.
    >
    > — A. Author, Some Commentary, p. 52
```

Both render correctly elsewhere: Obsidian shows `==` as a highlight and the
quotation as a blockquote with its attribution. Deleting a note unwraps the
phrase, leaving the text as it was.

### Folding

The triangles are drawn in the text view's own left gutter rather than written
into the text, because a marker character in the buffer would end up in the file.

GtkTextView has no real folding either. An `invisible` tag hides the glyphs but
keeps the line boxes, so a folded section leaves a blank hole exactly as tall as what
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
