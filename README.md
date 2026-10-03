# Omaverse

A small scripture outlining app for omarchy. Collapsible outline tree on the
left, free-text body for the selected node on the right, plain Markdown on disk.

Built because Notion's nested toggle lists are the right shape for outlining a
book of scripture, and the rest of Notion is not needed for it.

## File format

Outlines are ordinary Markdown. Headings carry the structure: the number of
hashes is the depth. Everything else is the body of the section above it.

```markdown
---
title: Jude
---

# Greeting and occasion

Placeholder opening sentence.

## Those who crept in

Placeholder sentence belonging to the nested section.

- an ordinary list bullet
- another one

# The charge
```

Nothing here is a private convention. `-` and `*` are list bullets and mean
nothing to omaverse; the title lives in frontmatter so all six heading levels
stay yours; body text is flush left, so pasted text needs no re-indenting.

Reading is forgiving — a missing blank line after a heading, or a jump from `#`
straight to `###`, are both accepted. Writing is canonical: one blank line after
every heading, and a blank line around each body.

Anything before the first heading is kept as a preamble rather than silently
dropped, and `[^id]:` definitions at the foot of the file are notes.

## Testing against a real compositor

`tools/vpointer` is a small development tool, not part of the app. It drives a
pointer through the wlr-virtual-pointer protocol, and unlike `wlrctl` it can
hold a button down -- press, move, release -- which is the only way to exercise
a drag. It speaks to whatever `WAYLAND_DISPLAY` names, so pointing it at a
nested test compositor keeps it away from the real desktop.

```sh
cargo build --release --manifest-path tools/vpointer/Cargo.toml
WAYLAND_DISPLAY=wayland-0 tools/vpointer/target/release/vpointer \
    --size 941x1030 move 350 141 down left move 350 57 up left
```

## Install

### On Arch and omarchy

```sh
cd packaging && makepkg -si
```

`packaging/PKGBUILD` builds the tagged release and runs the test suite before
installing. `packaging/PKGBUILD-git` tracks the latest commit instead. Both
install the binary, desktop entry, icon and AppStream metadata system-wide.

### From the source tree

Needs Rust, GTK 4.12+, libadwaita 1.5+ and enchant.

```sh
./install.sh
```

That builds, then puts the binary in `~/.local/bin`, a desktop entry in
`~/.local/share/applications`, an icon in `~/.local/share/icons` and metadata in
`~/.local/share/metainfo`, so Omaverse appears in the omarchy launcher and opens
`.md` files on double-click. The desktop entry is named for the application id so
the window picks up its icon under Wayland.

To run without installing:

```sh
cargo build --release
./target/release/omaverse path/to/romans.md
```

### Where your documents live

On first run Omaverse creates `~/Documents/Omaverse` and scans it, recursively,
with each subfolder becoming a group in the sidebar. To keep them somewhere else
— a Dropbox folder, say — use **Library folder…** in the document menu, or edit
`~/.config/omaverse/config.toml`.

### Fonts

Greek and Hebrew are set in SBL BibLit, falling back to SBL Greek and SBL Hebrew.
These are published by the Society of Biblical Literature and are not packaged
for Arch, so install them yourself into `~/.local/share/fonts`. Without them
Omaverse falls back to the system serif, which will render pointed Hebrew and
polytonic Greek less well but will not fail.

Spell checking needs a dictionary: `pacman -S hunspell-en_us`. Without one,
nothing is marked and nothing breaks.

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

Misspelled words are underlined in the prose. `F7`, or a right-click, offers
corrections and an **Add to dictionary** button — which matters more than it
sounds, because a general dictionary does not know most biblical names and
without it the marking is just noise.

Spelling is checked through enchant rather than libspelling, deliberately:
libspelling requires GtkSourceView, which would mean swapping the document's
widget and buffer for GtkSourceView's own, and those bring their own undo
implementation and their own gutter. A misspelling is just another tag, which
this already knows how to do.

### Reorganising

Sections can also be dragged in the outline: dropped on a row's edge a section
becomes a sibling, dropped in its middle it becomes a child, and it carries its
text and everything nested under it.

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

### Interlinears

`Ctrl+I`, or **New interlinear** in the + menu. Name the passage, choose Hebrew
or Greek, and paste the text straight into the dialog: it is split into words
there and then, because the words have to exist before there is anything to
annotate.

A sheet opens into its own view — it is not prose and has no sections to fold.
Each word is a column with its annotations beneath it, and Hebrew lays out right
to left. Sheets are `.toml` beside the outlines, and the file extension is what
says which is which.

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

## Licence

MIT. See `LICENSE`.
