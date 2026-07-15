# sl-lsl

Pure **Linden Scripting Language** (LSL) tooling for Second Life / OpenSim
clients. Three pieces so far: a [`logos`](https://crates.io/crates/logos)
**lexer** that turns LSL source into a token stream, an **error-tolerant
recursive-descent parser** that turns that stream into a syntax tree, and a
**semantic pass** that checks the tree against the grid's library.

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

## The parser

`parse(source)` turns the token stream into an owned, fully-spanned syntax tree
(`ast::Script`). It is a hand-written **recursive-descent** parser with a Pratt
loop for expression precedence, and — like the lexer — it is **error-tolerant**:
it never aborts, dropping `Expr::Error` / `Stmt::Error` placeholders where the
input does not parse and returning the recovered errors alongside the tree, so a
half-typed statement does not discard the rest of the file.

```rust
use sl_lsl::parse;

fn main() {
    let result = parse("default { state_entry() { llSay(0, \"hi\"); } }");
    assert!(!result.has_errors());
    assert_eq!(result.script.states.len(), 1);
}
```

Like the lexer, the parser holds the small set of LSL **keywords** but *not* the
LSL library — a called name, an event name or a constant stays a plain
identifier for the semantic pass to resolve against the grid's symbol table
(`protocol-lsl-syntax`, the `LSLSyntax` capability). LSL's operator precedence
is transcribed from Linden Lab's own grammar, including the quirk that `&&` and
`||` share one left-associative level, and the `<`/`>` ambiguity between the
angle brackets of a vector/rotation constructor and the relational/shift
operators. The reference for real LSL grammar is Linden Lab's MIT-licensed
`secondlife/tailslide` (and the community `lslint` it descends from) — read, not
bound to.

**Luau/SLua is a separate language** — this crate is scoped to LSL.

## The semantic pass

`analyze(&script, &syntax)` walks the parse tree against the grid library
(`LslSyntax`, decoded from the `LSLSyntax` capability) and returns a list of
`Diagnostic`s: **undefined symbols** (calls, variables, states, labels,
events), **call arity and types** at each call site, **`return` correctness**
(a value where none is wanted, a missing value, a function that can fall off its
end), **duplicate definitions**, and **state reachability**.

This earns its keep because **SL has no compile-without-save**: compilation
happens as part of the upload, so every "did I typo that function name?" is a
network round-trip that *mutates the world* (the in-world script is replaced and
its state resets). Local checking is the only way to type-check without touching
the grid.

The bar is deliberately high — **a false error on code the grid would compile is
worse than no error at all** — so the pass is conservative: symbol checks are
gated on a non-empty library table (an unfetched table stays silent rather than
flagging every `ll*` call), type inference returns "unknown" and *skips* the
check whenever it cannot pin a type down, resolution is order-insensitive (so
LSL's single-pass rule can only cause a missed error, never a false one), and
fall-off-the-end is a warning, not an error. Meeting that bar is *proven* by the
differential-testing oracle (`viewer-lsl-differential-testing`) that diffs this
pass against `tailslide`.

```rust
use sl_lsl::syntax::LslSyntax;
use sl_lsl::{analyze, parse};

fn main() {
    let script = parse("default { state_entry() { llSay(0, \"hi\"); } }").script;
    // An empty table (grid data not yet fetched) suppresses symbol checks.
    let diagnostics = analyze(&script, &LslSyntax::default());
    assert!(diagnostics.is_empty());
}
```

## Rendering diagnostics

`render_diagnostic(source, &diag, &syntax)` turns a `Diagnostic` into
`rustc`-grade output — the source line with a caret underlining the offending
bytes, a `--> line:col` locator, and, where the library table allows it, a
**"did you mean…?"** suggestion (edit distance over the grid's real symbol
names) or the grid's own signature quoted back on a type error:

```text
error: call to undefined function `llSy`
 --> 5:9
  |
5 |         llSy(0, "hi");
  |         ^^^^ did you mean `llSay`?
```

The *same* renderer serves grid-side errors: `render_grid_error` (and the
`ScriptCompileError::render` convenience in `sl-proto`) resolves the simulator's
bare `(line, col)` to a byte span and renders it through the identical caret
plumbing, so a compile error only the grid can produce arrives with a caret and
its source line — indistinguishable from a locally-found one. Colour is opt-in
(`RenderStyle::color`) and off by default, so the output is safe to log and
diff.

## Downstream

This one token stream is shared by both the highlighter
(`viewer-lsl-editor-highlight`, which colours words by a lookup against the
grid keyword table) and the parser (`viewer-lsl-parser-tree`). Do not grow a
second lexer. The parse tree feeds the semantic pass
(`viewer-lsl-semantic-pass`), whose structured `Diagnostic`s in turn feed the
reader-facing rendering (`viewer-lsl-diagnostics`) and the language server
(`viewer-lsl-lsp-server`, `viewer-lsl-lsp-diagnostics-nav`).
