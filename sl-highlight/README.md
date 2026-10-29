# sl-highlight

The **language-neutral** half of a script editor's structure: what a
highlighter for one language hands the editor, in a vocabulary no single
language owns.

- **Captures** — each highlighted range is named by a
  [tree-sitter capture name](https://tree-sitter.github.io/tree-sitter/syntax-highlighting#highlights)
  (`keyword`, `function.builtin`, `string.escape`, …). Those names are the
  closest thing editors share to a standard, and they are hierarchical: a scheme
  that colours only `function` still colours `function.builtin`. Second Life's
  LSL highlighter (`sl-lsl`) fills them in today; a Lua one, when the grid adds
  Lua, fills in the same names and every skin's syntax scheme colours it with no
  change.
- **Brackets** — every bracket with its partner, for brace matching.
- **Folds** — the ranges an editor can collapse.
- **Outline** — the named things a file is made of, for a jump list.
- **Assist** — what an editor offers while a file is typed: completions, a
  call's signature and a symbol's description, in the Language Server
  Protocol's shapes, through a `CodeAssist` trait a language implements.

Like `sl-lsl`, `sl-notecard` and the other pure crates it is **Bevy-free and
I/O-free**, and it has no dependencies at all: a language crate depends on it,
and the viewer's editor widget reads it, without either knowing about the
other.
