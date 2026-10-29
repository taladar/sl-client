---
id: viewer-lsl-editor-completion
title: LSL editor autocomplete and signature help from the grid's table
topic: viewer
status: done
origin: viewer-lsl-editor-highlight (2026-10-29)
refs: [viewer-lsl-editor-highlight, protocol-lsl-syntax, viewer-lsl-lsp-server]
---

Context: [context/viewer.md](../context/viewer.md).

The grid's `LSLSyntax` document (`GridLslSyntax` in
`sl-viewer-asset-editors`) carries every library function's return type,
typed arguments, tooltip, energy and sleep cost, every constant's type and
value and every event's signature. Firestorm offers no completion at all.

- **Completion**: while a word is typed in the script editor, a popup of the
  library symbols (and the script's own globals, functions and states from the
  outline) that start with it, filtered as typing goes on, `Enter` / `Tab` to
  accept, `Esc` to dismiss. The `:shortcode:` popup in `sl_viewer_chat` is the
  nearest existing shape.
- **Signature help**: inside a call's parentheses, the function's signature
  with the current argument emphasised, and its tooltip and sleep cost.
- **Hover**: a library symbol's tooltip under the pointer.

`Tab` is the code editor's indent key; accepting a completion with it must take
precedence only while the popup is open.

## Done (2026-10-29)

- `sl-highlight`'s `help` module: the language-neutral shapes (the Language
  Server Protocol's `CompletionItem`, `SignatureInformation` and `Hover`, cut
  down to `Completion`, `Signature`, `Hover` and their `Note`s), a
  `CodeAssist` trait a language implements, and the ranking every language
  shares — prefix, prefix ignoring case, then (from two typed characters) a
  word that only contains the typed text, so `say` finds `llRegionSay`.
- `sl-lsl`'s `help::LslAssist` over the grid's table, from the token stream:
  what is offered depends on where the caret is (types, `default` and `state`
  between globals; constants and globals in a global's initializer; types in a
  parameter list; events — completed to their whole header — between a
  state's handlers; the library, keywords, the script's functions and globals
  and the parameters and locals above the caret in a body; states after
  `state`, labels after `jump`, members after `.`), nothing in a comment, a
  string, after a number or where a name is declared. Signature help counts
  the commas at the call's own level past nested calls, lists and vector
  literals, for library calls, the script's own functions and handler
  headers, with the argument's tooltip and the sleep cost. Hover describes
  library functions, constants, events, documented keywords and the script's
  own functions and globals.
- `sl-viewer-ui-widgets`' `ui_code_assist`: a `CodeAssistant` component on any
  code editor, the list as a popover at the caret (`Up` / `Down`, `Enter` /
  `Tab` / click to accept, `Escape` to close, `Ctrl+Space` to open with
  nothing typed) with the selected entry's signature and documentation under
  it, the signature tip over the caret with the active argument in the accent
  (`Ctrl+Shift+Space` brings it back after `Escape`), and the hover tip under
  a word the pointer rests on. `Tab` accepts only while the list is open.
  Skinned through `.sk-code-assist-list` / `-detail` / `-active`; the notes
  (sleep, deprecated, god mode) are translated.
- The script editor hands every body an `LslAssist` over `GridLslSyntax`
  beside its highlighter, so the help follows the grid's table too.

Covered by unit tests (the ranking, the LSL provider's places, signatures and
hovers, and the widget's typing, `Enter` / `Tab` / `Escape` and signature
following in a headless editor); the popups' placement and skinning have not
yet been looked at in a running viewer.
