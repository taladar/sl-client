# sl-lsl

Pure **Linden Scripting Language** (LSL) tooling for Second Life / OpenSim
clients. The first piece is a [`logos`](https://crates.io/crates/logos)
**lexer** that turns LSL source into a token stream.

Like its siblings `sl-prim` (prim tessellation), `sl-anim` (keyframe motion) and
`sl-avatar` (skeleton / base body) the crate is deliberately **Bevy-free and
I/O-free**: it turns a borrowed `&str` into owned tokens and never opens a file
or fetches from the grid. A lexer has no business knowing about circuits or
capabilities, and keeping it clean means it can be tested, fuzzed and reused
(a linter, a CI check, an external tool) without a live grid.

## What it does

The scanner classifies exactly what a hand-written editor scanner classifies
(comments, strings, numbers, operators — mirroring the reference
`llkeywords.cpp`) and emits every *word* as a single `Token::Identifier`. It
deliberately does **not** bake in the LSL library: distinguishing a keyword, a
built-in function, a constant or a user symbol is a lookup one layer up against
the keyword table the grid serves at runtime (the `LSLSyntax` capability),
rather than a set of grammar literals frozen at build time.

That layering is why a hand-written scanner over one shared token stream was
chosen over a tree-sitter grammar: every existing LSL grammar — tree-sitter and
TextMate/Sublime alike — enumerates the ~500 library functions as grammar
literals (one compiles to a 9.4 MB `parser.c`), which is exactly backwards for a
language whose symbol table the grid serves at runtime, and their licences are
unusable. A hand-written scanner is cheaper and carries no C toolchain.

## Error tolerance

An editor lexer re-lexes broken, half-typed code on every keystroke, so the
scanner never aborts:

- an unterminated `/* … */` block comment or `"…"` string runs to
  end-of-input (matching the reference viewer's two-sided delimiter behaviour);
- `"…\"…"` escapes are honoured, so an escaped quote does not end a string;
- any byte that begins no valid token becomes a `Token::Error` the caller can
  highlight.

At the 65,536-character LSL script size limit a full re-lex is microseconds, so
there is no incremental-lexing machinery — re-lexing the whole buffer is free.

## Usage

```rust
use sl_lsl::{lex, Token};

fn main() {
    let toks = lex("integer n = 0x2A; // hi");
    assert_eq!(toks.first().map(|t| t.token), Some(Token::Identifier));

    // comments are included in the stream; filter them with `is_trivia()`.
    let code: Vec<_> = lex("x // c")
        .into_iter()
        .filter(|t| !t.is_trivia())
        .collect();
    assert_eq!(code.len(), 1);
}
```

Each `SpannedToken` carries the token kind and its byte range in the source
(`token.text(source)` slices out the matched text). Whitespace is dropped;
comments are kept.

## Downstream

This one token stream is shared by both the highlighter
(`viewer-lsl-editor-highlight`, which colours words by a lookup against the
grid keyword table) and the parser (`viewer-lsl-parser-tree`). Do not grow a
second lexer.
