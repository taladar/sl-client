//! What a script upload is answered with: the grid's compiler run over the
//! source, and what the result does to the script.
//!
//! There is no dry run on any grid: a viewer's Save POSTs the source to
//! `UpdateScriptTask` (or `UpdateScriptAgent`), and the answer to that POST
//! carries `compiled` and an `errors` array of strings. This module is that
//! answer. The session has already stored the source by the time it runs
//! (the crate's `uploads` module), because **every grid stores a source that does not
//! compile** — a failed compile leaves the item holding the broken text, which
//! the next open shows, and stops the script.
//!
//! Everything below was measured by the `script-compile` census on aditi and
//! the local OpenSim (`book/src/gridspec/lsl.md` § Compiling), and the two
//! flavours split on all of it:
//!
//! | | Second Life | OpenSim (YEngine) |
//! | --- | --- | --- |
//! | errors | one, the first: names before types | every one |
//! | its form | `(L, C) : ERROR : <msg>\n`, from zero | `(L,C) Error: <msg>`, from zero |
//! | where | the token after the fault, where Linden's parser stood | the fault |
//! | a column counts | bytes, a tab two | characters |
//! | a failed script | stopped | no instance: `GetScriptRunning` unanswered |
//! | `new_asset` of a task upload | the stored asset | the item's own id |
//! | an agent upload | compiled like a task's | not compiled: `compiled` true |
//! | the `luau` target | the Luau compiler, the item marked Luau | ignored |
//!
//! The messages are this workspace's compiler's own, which say more than
//! Linden's terse ones (`sl_lsl_runtime::CompileError`): what has to match a
//! grid is *when* a source is refused and where, not the wording. Where a
//! position cannot follow the grid exactly it is said at the test that holds
//! the rest (`sl-conformance`'s `script-compile` tables).
//!
//! The Luau target is the exception: Second Life answers with the reference
//! Luau compiler's own text (`:2: Expected identifier when parsing
//! expression, got '='`), and so does this module, by running that compiler
//! (through `mlua`). It compiles and does not run: a Luau script has no
//! instance here.

use std::collections::BTreeSet;

use sl_lsl::Token;
use sl_lsl_runtime::{CompileError, CompileErrorKind, Dialect};
use sl_proto::{
    AssetKey, InventoryKey, ScriptLanguage, ScriptUploadAnswer, UploadedScript, UploadedScriptItem,
};

use crate::world::SceneFixtures;

/// Whose compiler the grid runs, and so which sources it refuses and how it
/// words a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCompiler {
    /// Second Life's: one error, at the token after the fault, with the
    /// column in bytes.
    SecondLife,
    /// OpenSim's YEngine, its default engine: every error, at the fault, the
    /// column in characters. A source that picks XEngine (`//XEngine:`) is
    /// compiled as YEngine compiles it: the fake grid runs one engine.
    YEngine,
}

impl ScriptCompiler {
    /// The compiler dialect, for what compiles.
    const fn dialect(self) -> Dialect {
        match self {
            Self::SecondLife => Dialect::SecondLife,
            Self::YEngine => Dialect::OpenSim,
        }
    }
}

/// What becomes of a script in a prim whose upload did not compile
/// ([`CompilePolicy::failed_script`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedScript {
    /// It is stopped, and says so when asked: Second Life.
    Stopped,
    /// It has no instance, so `GetScriptRunning` is not answered: OpenSim.
    NoInstance,
}

/// What a task script upload's answer names as `new_asset`
/// ([`CompilePolicy::task_new_asset`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNewAsset {
    /// The asset the source was stored as: Second Life.
    StoredAsset,
    /// The script item's own id, and no `new_inventory_item`: OpenSim
    /// (`UpdateItemAsset.cs`'s `TaskInventoryScriptUpdater` writes
    /// `m_inventoryItemID`, into a reply that has no item field).
    ItemId,
}

/// How a grid answers a script upload, where the live grids disagree
/// ([`ImitatedGrid::script_compile_policy`](crate::ImitatedGrid::script_compile_policy)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilePolicy {
    /// Whose compiler runs.
    pub compiler: ScriptCompiler,
    /// Whether an upload to a script in the agent's inventory is compiled:
    /// Second Life compiles it like a task's; OpenSim compiles none and
    /// answers `compiled` whatever the source.
    pub compiles_agent_uploads: bool,
    /// What a failed compile leaves of a script in a prim.
    pub failed_script: FailedScript,
    /// What a task upload's answer names as `new_asset`.
    pub task_new_asset: TaskNewAsset,
}

impl Default for CompilePolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.script_compile_policy()
    }
}

/// What one compile said: whether the source compiled, the errors as the grid
/// words them, and the language the item is marked with afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOutcome {
    /// Whether it compiled.
    pub compiled: bool,
    /// The errors, each as the grid words it.
    pub errors: Vec<String>,
    /// The language the item is marked with afterwards.
    pub language: ScriptLanguage,
}

/// Compiles `source` as `compiler` does for an upload naming `target`. The
/// `asset` is the stored source's id, which YEngine names when the source is
/// empty.
#[must_use]
pub fn compile_source(
    compiler: ScriptCompiler,
    target: &str,
    source: &[u8],
    asset: AssetKey,
) -> CompileOutcome {
    let text = String::from_utf8_lossy(source);
    if compiler == ScriptCompiler::SecondLife && target == sl_proto::ScriptTarget::Luau.to_wire() {
        // Second Life marks the item Luau, compiled or not, and answers with
        // its Luau compiler's own words.
        let errors = luau_errors(source);
        return CompileOutcome {
            compiled: errors.is_empty(),
            errors,
            language: ScriptLanguage::Luau,
        };
    }
    if compiler == ScriptCompiler::YEngine && without_engine_line(&text).is_empty() {
        return CompileOutcome {
            compiled: false,
            errors: vec![format!("fetched source empty asset://{asset}")],
            language: ScriptLanguage::Lsl,
        };
    }
    let errors = match sl_lsl_runtime::compile_for(&text, compiler.dialect()) {
        Ok(_program) => Vec::new(),
        Err(errors) => match compiler {
            ScriptCompiler::SecondLife => second_life_errors(&text, &errors),
            ScriptCompiler::YEngine => yengine_errors(&text, &errors),
        },
    };
    CompileOutcome {
        compiled: errors.is_empty(),
        errors,
        language: ScriptLanguage::Lsl,
    }
}

/// `source` without the first line an OpenSim source may open with to pick
/// its engine (`//YEngine:`, `//XEngine:`, in any case).
fn without_engine_line(source: &str) -> &str {
    let (first, rest) = source.split_once('\n').unwrap_or((source, ""));
    let picked = first
        .trim_end_matches('\r')
        .strip_prefix("//")
        .is_some_and(|name| {
            let name = name.trim().to_ascii_lowercase();
            name == "yengine:" || name == "xengine:"
        });
    if picked { rest } else { source }
}

/// What Second Life's Luau target answers `source` with: nothing when it
/// compiles, else the one error the reference Luau compiler reports.
///
/// That error is the compiler's own text, and Second Life passes it on as it
/// comes: `luau_compile` answers a failure with bytecode that is a zero byte
/// and `:<line>: <message>`, the line one-based and no column, for the first
/// parse error and for a compile error alike (an exhausted register file, a
/// nesting too deep). `mlua` hands back what follows the `:`.
fn luau_errors(source: &[u8]) -> Vec<String> {
    match mlua::Compiler::new().compile(source) {
        Ok(_bytecode) => Vec::new(),
        Err(mlua::Error::SyntaxError { message, .. }) => vec![format!(":{message}")],
        Err(other) => vec![other.to_string()],
    }
}

/// The one error Second Life answers a source with, in its words: a syntax
/// error if there is one (the parser stops at the first), else the first
/// name that resolves to nothing or is declared twice (names are resolved
/// before types), else the first error.
fn second_life_errors(source: &str, errors: &[CompileError]) -> Vec<String> {
    let first = errors
        .iter()
        .find(|error| error.kind == CompileErrorKind::Syntax)
        .or_else(|| {
            errors.iter().find(|error| {
                matches!(
                    error.kind,
                    CompileErrorKind::Undefined | CompileErrorKind::AlreadyDefined
                )
            })
        })
        .or_else(|| errors.first());
    first
        .map(|error| {
            let offset = if error.kind == CompileErrorKind::Syntax {
                error.span.start
            } else {
                linden_offset(source, &error.span)
            };
            let (line, column) = linden_line_column(source, offset);
            format!("({line}, {column}) : ERROR : {}\n", error.message)
        })
        .into_iter()
        .collect()
}

/// Where Linden's parser stood when it found the fault spanning `span`: the
/// last token it had read when it reduced the construct.
///
/// A construct that ends in a closing token (`)`, `]`, `}`, `;`) is reduced
/// on that token, so the fault is placed there — a call to an undefined
/// function at its `)`, a missing `return` at the body's `}`. Any other is
/// reduced only once the parser has seen the token after it, so the fault is
/// placed on that one — an undeclared `y` in `y = 1` at the `=`, a string
/// assigned to an integer at the `;` after it. A name followed by `(` is a
/// call, which ends at the matching `)`. Measured by the census's legs on
/// aditi; at the end of the source, the end of the span.
fn linden_offset(source: &str, span: &core::ops::Range<usize>) -> usize {
    let tokens: Vec<sl_lsl::SpannedToken> = sl_lsl::tokens(source)
        .filter(|token| !token.is_trivia())
        .collect();
    let closing = |token: &Token| {
        matches!(
            token,
            Token::RParen | Token::RBracket | Token::RBrace | Token::Semicolon
        )
    };
    if let Some(last) = tokens
        .iter()
        .rfind(|token| token.span.start >= span.start && token.span.end <= span.end)
        && closing(&last.token)
    {
        return last.span.start;
    }
    let Some(next) = tokens.iter().position(|token| token.span.start >= span.end) else {
        return span.end;
    };
    let after = tokens.get(next..).unwrap_or_default();
    match after.first() {
        Some(open) if open.token == Token::LParen => {
            let mut depth = 0_usize;
            for token in after {
                match token.token {
                    Token::LParen => depth = depth.saturating_add(1),
                    Token::RParen => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return token.span.start;
                        }
                    }
                    _other => {}
                }
            }
            open.span.start
        }
        Some(token) => token.span.start,
        None => span.end,
    }
}

/// The zero-based line and column of byte `offset` as Second Life counts
/// them: lines by `\n` (so CR LF reads as LF), the column in **bytes**, with
/// a tab counted as two (two tabs before a fault moved it by four).
fn linden_line_column(source: &str, offset: usize) -> (usize, usize) {
    let before = source.get(..offset).unwrap_or(source);
    let line = before.matches('\n').count();
    let start = before
        .rfind('\n')
        .map_or(0, |newline| newline.saturating_add(1));
    let column = before
        .get(start..)
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character == '\t' {
                2
            } else {
                character.len_utf8()
            }
        })
        .sum();
    (line, column)
}

/// Every error, as YEngine words it: the position from zero, in characters.
///
/// YEngine places an unknown label or state at the statement — the `jump`
/// or `state` — rather than at the name, so an undefined name right after
/// one of those words is placed on the word.
fn yengine_errors(source: &str, errors: &[CompileError]) -> Vec<String> {
    let index = sl_lsl_runtime::bytecode::Source::new(source);
    let unknown = yengine_unknown_characters(source, &index);
    if !unknown.is_empty() {
        return unknown;
    }
    let tokens: Vec<sl_lsl::SpannedToken> = sl_lsl::tokens(source)
        .filter(|token| !token.is_trivia())
        .collect();
    errors
        .iter()
        .map(|error| {
            let before = tokens
                .iter()
                .enumerate()
                .rfind(|(_at, token)| token.span.end <= error.span.start);
            let statement = match error.kind {
                CompileErrorKind::Undefined => before
                    .filter(|(_at, token)| matches!(token.text(source), Some("jump" | "state"))),
                CompileErrorKind::Syntax => before.filter(|(at, token)| {
                    is_yengine_statement_keyword(token.text(source))
                        && brace_depth(source, tokens.get(..*at).unwrap_or_default()) == 0
                }),
                _ => None,
            };
            let position = statement.map_or(error.position, |(_at, token)| {
                index.position(token.span.start)
            });
            format!(
                "({},{}) Error: {}",
                position.line.saturating_sub(1),
                position.column.saturating_sub(1),
                error.message
            )
        })
        .collect()
}

/// Whether `text` is one of the words YEngine's tokenizer makes a statement
/// keyword of (`MMRScriptTokenize.cs`'s `delims`), which no declaration
/// starts with: outside every body YEngine faults on the word itself, where
/// this workspace's lexer, which knows no keywords, reads it as a type name
/// and faults on the token after.
fn is_yengine_statement_keyword(text: Option<&str>) -> bool {
    matches!(
        text,
        Some("do" | "else" | "for" | "if" | "jump" | "return" | "while")
    )
}

/// How many braces `tokens` (of `source`) leave open.
fn brace_depth(source: &str, tokens: &[sl_lsl::SpannedToken]) -> usize {
    tokens
        .iter()
        .fold(0_usize, |depth, token| match token.text(source) {
            Some("{") => depth.saturating_add(1),
            Some("}") => depth.saturating_sub(1),
            _ => depth,
        })
}

/// The punctuation YEngine's tokenizer knows (`MMRScriptTokenize.cs`'s
/// `delims`, one character each of its operators).
const YENGINE_DELIMITERS: &str = "~!@%^&*()-+={}[];:<>,.?/|#";

/// YEngine's tokenizer errors for `source`: one `unknown character` for each
/// character outside a string or comment that is no letter, digit, `_`,
/// quote, white space or [`YENGINE_DELIMITERS`].
///
/// YEngine tokenizes the whole source before it parses
/// (`MMRScriptCompile.cs`: a tokenizer error returns before `Reduce`), so
/// when there is one it answers these and no parse error at all: a backtick
/// on line 3 is the first error even when line 1 does not parse.
fn yengine_unknown_characters(
    source: &str,
    index: &sl_lsl_runtime::bytecode::Source,
) -> Vec<String> {
    sl_lsl::tokens(source)
        .filter(|token| token.token == Token::Error)
        .flat_map(|token| {
            let text = token.text(source).unwrap_or_default();
            text.char_indices().map(move |(offset, character)| {
                (token.span.start.saturating_add(offset), character)
            })
        })
        .filter(|(_offset, character)| {
            !(character.is_alphanumeric()
                || character.is_whitespace()
                || matches!(character, '_' | '"' | '\'')
                || YENGINE_DELIMITERS.contains(*character))
        })
        .map(|(offset, character)| {
            let position = index.position(offset);
            format!(
                "({},{}) Error: unknown character '{character}'",
                position.line.saturating_sub(1),
                position.column.saturating_sub(1)
            )
        })
        .collect()
}

/// Compiles a stored script upload as the region's grid does, applies what
/// the compile does to the script, and returns the answer.
pub(crate) fn answer_upload(
    world: &mut SceneFixtures,
    upload: &UploadedScript,
) -> ScriptUploadAnswer {
    let policy = world.scripts;
    let mut answer = upload.compiled_unchecked();
    let compiles = match &upload.item {
        UploadedScriptItem::Task(_request) => true,
        UploadedScriptItem::Agent(_request) => policy.compiles_agent_uploads,
    };
    if !compiles {
        return answer;
    }
    let outcome = compile_source(
        policy.compiler,
        upload.item.target(),
        &upload.source,
        upload.new_asset,
    );
    answer.compiled = outcome.compiled;
    answer.errors.clone_from(&outcome.errors);
    let UploadedScriptItem::Task(request) = &upload.item else {
        return answer;
    };
    if policy.task_new_asset == TaskNewAsset::ItemId {
        answer.new_asset = request.item_id.uuid();
        answer.new_inventory_item = None;
    }
    let Some(local_id) = world.local_id_of(request.task_id) else {
        tracing::debug!(
            "a script upload named object {}, which this region does not have",
            request.task_id
        );
        return answer;
    };
    let Some(contents) = world.objects.contents_mut(local_id) else {
        return answer;
    };
    let Some(mut item) = contents
        .items
        .iter()
        .find(|held| held.item_id == request.item_id)
        .cloned()
    else {
        tracing::debug!(
            "a script upload named item {}, which {} does not hold",
            request.item_id,
            request.task_id
        );
        return answer;
    };
    // The run state the compile leaves. A clean compile runs the script as
    // the upload asked (a save over a running script is its restart, which
    // the engine will make once the grid runs scripts:
    // `server-fake-grid-script-engine-wiring`).
    set_run_state(
        &mut contents.running,
        &mut contents.uncompiled,
        request.item_id,
        outcome.compiled,
        request.is_script_running,
        policy.failed_script,
    );
    let flags =
        (item.flags & !ScriptLanguage::SUBTYPE_MASK) | u32::from(outcome.language.subtype());
    if flags != item.flags {
        item.flags = flags;
        answer.inventory_flags = Some(flags);
        contents.write(item);
    }
    answer
}

/// Records what a compile of the script `item` leaves it doing: running as
/// the upload asked if it compiled, stopped or without an instance if not.
fn set_run_state(
    running: &mut BTreeSet<InventoryKey>,
    uncompiled: &mut BTreeSet<InventoryKey>,
    item: InventoryKey,
    compiled: bool,
    run: bool,
    failed: FailedScript,
) {
    if compiled {
        let _was = uncompiled.remove(&item);
        if run {
            let _was = running.insert(item);
        } else {
            let _was = running.remove(&item);
        }
        return;
    }
    let _was = running.remove(&item);
    if failed == FailedScript::NoInstance {
        let _was = uncompiled.insert(item);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// The asset id the tests store sources under.
    fn asset() -> AssetKey {
        AssetKey::from(uuid::Uuid::from_u128(0xA55E7))
    }

    /// What Second Life answers `source`.
    fn second_life(source: &str) -> Vec<String> {
        compile_source(
            ScriptCompiler::SecondLife,
            "mono",
            source.as_bytes(),
            asset(),
        )
        .errors
    }

    /// The `(line, column)` prefix of each error of Second Life's answer.
    fn second_life_at(source: &str) -> Vec<String> {
        second_life(source)
            .iter()
            .map(|error| error.split(" : ").next().unwrap_or_default().to_owned())
            .collect()
    }

    /// The census's broken sources, each with the position Second Life
    /// answered it at (`book/src/gridspec/lsl.md` § Compiling).
    #[test]
    fn second_life_places_an_error_where_its_parser_stood() {
        let state_entry =
            |body: &str| format!("default\n{{\n    state_entry()\n    {{\n{body}\n    }}\n}}\n");
        for (body, at) in [
            ("        integer x = ;", "(4, 20)"),
            ("        integer x = \"a\";", "(4, 23)"),
            ("        y = 1;", "(4, 10)"),
            ("        llNope();", "(4, 15)"),
            ("        jump nowhere;", "(4, 20)"),
            ("        state other;", "(4, 19)"),
            ("        string s = \"äöü\"; integer x = ;", "(4, 41)"),
            ("\t\tinteger x = ;", "(4, 16)"),
            (
                "        integer x = \"a\";\n        y = 1;\n        llNope();",
                "(5, 10)",
            ),
        ] {
            assert_eq!(second_life_at(&state_entry(body)), vec![at], "{body}");
        }
        for (source, at) in [
            (
                "integer f()\n{\n    if (TRUE) return 1;\n}\ndefault\n{\n    state_entry()\n    {\n    }\n}\n",
                "(3, 0)",
            ),
            (
                "integer x;\ninteger x;\ndefault\n{\n    state_entry()\n    {\n    }\n}\n",
                "(1, 9)",
            ),
            ("", "(0, 0)"),
            (
                "state other\n{\n    state_entry()\n    {\n    }\n}\n",
                "(0, 0)",
            ),
            (
                "default\r\n{\r\n    state_entry()\r\n    {\r\n        integer x = ;\r\n    }\r\n}\r\n",
                "(4, 20)",
            ),
        ] {
            assert_eq!(second_life_at(source), vec![at], "{source:?}");
        }
    }

    /// Second Life's error has its form exactly — the space before the colon
    /// and the newline included — and a source that compiles has none.
    #[test]
    fn second_life_words_an_error_in_its_form() {
        let errors =
            second_life("default\n{\n    state_entry()\n    {\n        y = 1;\n    }\n}\n");
        assert_eq!(errors.len(), 1);
        let error = errors.first().map(String::as_str).unwrap_or_default();
        assert!(error.starts_with("(4, 10) : ERROR : "), "{error:?}");
        assert!(error.ends_with('\n'), "{error:?}");
        assert_eq!(
            second_life("default\n{\n    state_entry()\n    {\n    }\n}\n"),
            Vec::<String>::new()
        );
    }

    /// YEngine answers every error at the fault, counting the engine line,
    /// and compiles what it is lenient about.
    #[test]
    fn yengine_answers_every_error_at_the_fault() {
        let yengine = |source: &str| {
            compile_source(ScriptCompiler::YEngine, "mono", source.as_bytes(), asset())
        };
        let three = yengine(
            "//YEngine:\ndefault\n{\n    state_entry()\n    {\n        integer x = \"a\";\n        y = 1;\n        llNope();\n    }\n}\n",
        );
        let at: Vec<&str> = three
            .errors
            .iter()
            .map(|error| error.split(' ').next().unwrap_or_default())
            .collect();
        assert_eq!(at, vec!["(5,20)", "(6,8)", "(7,8)"]);
        for (body, at) in [("jump nowhere;", "(5,8)"), ("state other;", "(5,8)")] {
            let source = format!(
                "//YEngine:\ndefault\n{{\n    state_entry()\n    {{\n        {body}\n    }}\n}}\n"
            );
            let errors = yengine(&source).errors;
            assert!(
                errors.first().is_some_and(|error| error.starts_with(at)),
                "{body}: {errors:?}"
            );
        }
        assert!(three.errors.iter().all(|error| error.contains(") Error: ")));
        let missing_return = yengine(
            "//YEngine:\ninteger f()\n{\n    if (TRUE) return 1;\n}\ndefault\n{\n    state_entry()\n    {\n    }\n}\n",
        );
        assert!(missing_return.compiled, "{:?}", missing_return.errors);
        assert_eq!(
            yengine("//YEngine:\n").errors,
            vec![format!("fetched source empty asset://{}", asset())]
        );
        let experience = yengine(
            "//YEngine:\ndefault\n{\n    touch_start(integer n)\n    {\n        llClearExperiencePermissions(NULL_KEY);\n    }\n}\n",
        );
        assert!(!experience.compiled, "OpenSim has no experience functions");
    }

    /// Second Life compiles the Luau target with the Luau compiler and marks
    /// the item Luau either way; LSL source happens to parse as Lua. OpenSim
    /// ignores the target and refuses Luau source as LSL.
    #[test]
    fn the_luau_target_is_compiled_as_luau_on_second_life_and_ignored_on_opensim() {
        let lsl = b"default\n{\n    state_entry()\n    {\n    }\n}\n";
        let second_life = compile_source(ScriptCompiler::SecondLife, "luau", lsl, asset());
        assert!(second_life.compiled, "{:?}", second_life.errors);
        assert_eq!(second_life.language, ScriptLanguage::Luau);
        let broken = b"local a = 1\nlocal b = = 2\nlocal c = 3\n";
        let refused = compile_source(ScriptCompiler::SecondLife, "luau", broken, asset());
        assert!(!refused.compiled);
        assert_eq!(
            refused.errors,
            vec![":2: Expected identifier when parsing expression, got '='".to_owned()]
        );
        assert_eq!(refused.language, ScriptLanguage::Luau);
        let opensim = compile_source(ScriptCompiler::YEngine, "luau", lsl, asset());
        assert!(opensim.compiled, "{:?}", opensim.errors);
        assert_eq!(opensim.language, ScriptLanguage::Lsl);
        let as_lsl = compile_source(ScriptCompiler::YEngine, "luau", broken, asset());
        assert!(!as_lsl.compiled);
        assert_eq!(as_lsl.language, ScriptLanguage::Lsl);
    }

    /// YEngine tokenizes before it parses: a character it does not know is
    /// answered alone, every one of them, even after a line that does not
    /// parse; inside a string or a comment it is nothing.
    #[test]
    fn yengine_answers_unknown_characters_before_any_parse_error() {
        let yengine = |source: &str| {
            compile_source(ScriptCompiler::YEngine, "mono", source.as_bytes(), asset()).errors
        };
        assert_eq!(
            yengine("//YEngine:\nlocal n = 1\nlocal s = `n is {n}`\n"),
            vec![
                "(2,10) Error: unknown character '`'".to_owned(),
                "(2,19) Error: unknown character '`'".to_owned(),
            ]
        );
        assert_eq!(
            yengine(
                "default\n{\n    state_entry()\n    {\n        integer x = 1 \\ 2;\n    }\n}\n"
            ),
            vec!["(4,22) Error: unknown character '\\'".to_owned()]
        );
        let quoted = yengine(
            "default\n{\n    state_entry()\n    {\n        llOwnerSay(\"`\"); // `\n        integer x = ;\n    }\n}\n",
        );
        assert!(
            quoted
                .iter()
                .all(|error| !error.contains("unknown character")),
            "{quoted:?}"
        );
    }

    /// A statement keyword outside every body is YEngine's fault at the word
    /// itself (`luau_unclosed_block`, measured `(1,0)`); inside a body the
    /// error stays where the parse put it.
    #[test]
    fn yengine_faults_a_statement_keyword_outside_a_body_on_the_word() {
        let yengine = |source: &str| {
            compile_source(ScriptCompiler::YEngine, "mono", source.as_bytes(), asset()).errors
        };
        let top = yengine("//YEngine:\nif true then\n    local x = 1\n");
        assert!(
            top.first().is_some_and(|error| error.starts_with("(1,0) ")),
            "{top:?}"
        );
        let inside = yengine(
            "default\n{\n    state_entry()\n    {\n        if (TRUE) integer x = ;\n    }\n}\n",
        );
        assert!(
            inside
                .first()
                .is_some_and(|error| !error.starts_with("(4,8) ")),
            "{inside:?}"
        );
    }

    /// A compile error past the parse — the register file exhausted — is
    /// answered in the same `:<line>: ` form as a syntax error.
    #[test]
    fn a_luau_compile_error_is_worded_as_a_syntax_error_is() {
        let mut locals = String::new();
        for index in 0..201 {
            let index = index.to_string();
            for piece in ["    local v", &index, " = tostring(", &index, ")\n"] {
                locals.push_str(piece);
            }
        }
        let source = format!("local function f()\n{locals}end\n");
        assert_eq!(
            luau_errors(source.as_bytes()),
            vec![
                ":202: Out of local registers when trying to allocate v200: exceeded limit 200"
                    .to_owned()
            ]
        );
        assert_eq!(luau_errors(b""), Vec::<String>::new());
    }

    /// A clean compile runs the script as the upload asked; a failed one
    /// stops it, and on OpenSim leaves it without an instance until a clean
    /// compile gives it one back.
    #[test]
    fn a_compile_sets_the_run_state() {
        let item = InventoryKey::from(uuid::Uuid::from_u128(0x17E));
        let mut running = BTreeSet::new();
        let mut uncompiled = BTreeSet::new();
        set_run_state(
            &mut running,
            &mut uncompiled,
            item,
            true,
            true,
            FailedScript::Stopped,
        );
        assert!(running.contains(&item));
        set_run_state(
            &mut running,
            &mut uncompiled,
            item,
            false,
            true,
            FailedScript::Stopped,
        );
        assert!(!running.contains(&item));
        assert!(!uncompiled.contains(&item));
        set_run_state(
            &mut running,
            &mut uncompiled,
            item,
            false,
            true,
            FailedScript::NoInstance,
        );
        assert!(uncompiled.contains(&item));
        set_run_state(
            &mut running,
            &mut uncompiled,
            item,
            true,
            false,
            FailedScript::NoInstance,
        );
        assert!(!uncompiled.contains(&item));
        assert!(!running.contains(&item));
    }
}
