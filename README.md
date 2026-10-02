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

The directory is scanned recursively. Immediate subfolders become sidebar
groups, so `Outlines/New Testament/romans.md` appears under "New Testament".

## Keys

| Key | Action |
|---|---|
| `Ctrl+S` | Save now (autosave already runs every 800 ms) |
| `Ctrl+\` | Toggle the library sidebar |

More arrive with node editing in phase 2.

## Build

Needs Rust, GTK 4.12+, and libadwaita 1.5+.

```sh
cargo build --release
./target/release/omaverse path/to/romans.md
```

## Status

Phase 1 of 4. Working: library sidebar, outline tree with persistent fold state,
body editing, autosave, atomic writes.

Not yet: creating and restructuring nodes from the keyboard (phase 2),
live-rendered Markdown in the body pane (phase 3), installer / `.desktop` entry /
external-change detection / spellcheck (phase 4).
