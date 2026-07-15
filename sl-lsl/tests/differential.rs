//! **Differential testing** of the `sl-lsl` semantic pass against Linden Lab's
//! own front-end, `tailslide`.
//!
//! The semantic pass ([`sl_lsl::analyze`]) is held to a no-false-positive bar: a
//! false *error* on code the grid would happily compile is worse than no error
//! at all. This test *proves* rather than asserts that bar by running both
//! `sl-lsl` and **`tailslide`** — the MIT-licensed LSL parser/compiler that
//! reproduces the legacy bytecode byte-for-byte, so its lexing, typing and
//! implicit-conversion quirks are the real ones — over a shared corpus of LSL
//! scripts and diffing the diagnostics.
//!
//! `tailslide` is used strictly as an **out-of-process oracle**, not a library
//! dependency of `sl-lsl` (the crate stays I/O-free): the CLI is invoked with
//! `--lint` per script and its textual findings are parsed back. The corpus in
//! `tests/corpus/` is committed, so this doubles as a regression guard as the
//! semantic rules grow.
//!
//! ## The property under test
//!
//! Classification is driven by the *oracle's* verdict, never by how the corpus
//! files are foldered:
//!
//! - **When `tailslide` compiles a script cleanly** (zero errors), `sl-lsl`
//!   **must** report zero error-severity diagnostics. A violation is a proven
//!   false positive and fails the test — this is the no-false-positive bar made
//!   concrete.
//! - **When `tailslide` reports errors**, `sl-lsl` is *expected* to agree that
//!   the script is broken, but a **miss** is tolerated (the pass is deliberately
//!   conservative and does not claim to implement every grid check). Misses are
//!   only reported, never failed.
//!
//! Warnings are never part of the gate: both tools emit advisory warnings
//! (unused variable, a value function that can fall off its end) on otherwise
//! valid scripts, and the bar speaks only to *errors*.
//!
//! ## Running the oracle
//!
//! The oracle run is **skipped** (the test passes) unless a built `tailslide` is
//! located, so CI without the C++ toolchain still goes green. Point the test at
//! a build with:
//!
//! - `SL_LSL_TAILSLIDE_BIN` — path to the `tailslide` executable (required to
//!   run the oracle).
//! - `SL_LSL_TAILSLIDE_BUILTINS` — path to tailslide's `builtins.txt`. Optional:
//!   if unset, it is derived from the binary's location
//!   (`<repo>/build/tailslide` ⇒ `<repo>/builtins.txt`).
//!
//! `builtins.txt` is tailslide's own authoritative library definition. Building
//! the [`LslSyntax`] table from *the same file the oracle uses* means any
//! diagnostic difference is a genuine semantic-rule difference, not an artefact
//! of a mismatched library version.
//!
//! The pure helpers (the `builtins.txt` parser, the lint-output parser, the
//! byte-offset-to-line map and the diff classification) are unit-tested below
//! and run with no `tailslide` present.

#![expect(
    clippy::print_stderr,
    reason = "an integration-test oracle reports its skip reason and result counts to the operator"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use sl_lsl::ast::TypeName;
use sl_lsl::syntax::{LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax};
use sl_lsl::{Severity, analyze, parse};

/// A boxed error, so the test bodies can use `?` for I/O and `return Err(..)`
/// for a proven false positive rather than reaching for a denied `unwrap` /
/// `panic`.
type BoxError = Box<dyn std::error::Error>;

/// The `ERROR::`/`WARN::` findings tailslide's `--lint` pass reported for one
/// script.
struct LintReport {
    /// The 1-based source lines carrying an `ERROR::` finding.
    error_lines: BTreeSet<u32>,
    /// The number of `ERROR::` findings; the gate keys off whether this is zero
    /// (the oracle considers the script compilable).
    error_count: usize,
}

// -- building the shared library table from tailslide's builtins.txt --------

/// Parse tailslide's `builtins.txt` into an [`LslSyntax`] table: the function,
/// constant and event definitions the semantic pass checks calls against, plus
/// the fixed control-flow and type keyword groups.
fn builtins_library(text: &str) -> LslSyntax {
    let mut syntax = LslSyntax::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("const ") {
            if let Some((name, constant)) = parse_constant(rest) {
                let _prev = syntax.constants.insert(name, constant);
            }
        } else if let Some(rest) = line.strip_prefix("event ") {
            if let Some((name, event)) = parse_event(rest) {
                let _prev = syntax.events.insert(name, event);
            }
        } else if let Some((name, function)) = parse_function(line) {
            let _prev = syntax.functions.insert(name, function);
        }
    }
    for keyword in [
        "integer", "float", "string", "key", "vector", "rotation", "list",
    ] {
        let _prev = syntax
            .types
            .insert(keyword.to_owned(), LslKeyword::default());
    }
    for keyword in [
        "if", "else", "for", "while", "do", "jump", "return", "state",
    ] {
        let _prev = syntax
            .controls
            .insert(keyword.to_owned(), LslKeyword::default());
    }
    syntax
}

/// Parse a function line — `<ret> <name>( <args> )`, e.g.
/// `integer llAbs( integer val )` — into its name and [`LslFunction`]. A `void`
/// return keyword decodes to [`None`] (the same as [`TypeName::from_keyword`]).
fn parse_function(line: &str) -> Option<(String, LslFunction)> {
    let (head, tail) = line.split_once('(')?;
    let args = tail.rsplit_once(')').map_or(tail, |(before, _)| before);
    let mut tokens = head.split_whitespace();
    let return_keyword = tokens.next()?;
    let name = tokens.next()?;
    if tokens.next().is_some() {
        // Not a `<ret> <name>(` shape (e.g. an unexpected line); skip it.
        return None;
    }
    Some((
        name.to_owned(),
        LslFunction {
            return_type: TypeName::from_keyword(return_keyword),
            arguments: parse_args(args),
            ..LslFunction::default()
        },
    ))
}

/// Parse an event line — the text after the `event ` prefix, e.g.
/// `at_target( integer tnum, vector targetpos, vector ourpos )` — into its name
/// and [`LslEvent`].
fn parse_event(rest: &str) -> Option<(String, LslEvent)> {
    let (head, tail) = rest.split_once('(')?;
    let args = tail.rsplit_once(')').map_or(tail, |(before, _)| before);
    let name = head.split_whitespace().next()?;
    Some((
        name.to_owned(),
        LslEvent {
            arguments: parse_args(args),
            ..LslEvent::default()
        },
    ))
}

/// Parse a constant line — the text after the `const ` prefix, e.g.
/// `integer ACTIVE = 0x2` — into its name and [`LslConstant`] (value kept as
/// served).
fn parse_constant(rest: &str) -> Option<(String, LslConstant)> {
    let (decl, value) = rest.split_once('=')?;
    let mut tokens = decl.split_whitespace();
    let type_keyword = tokens.next()?;
    let name = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some((
        name.to_owned(),
        LslConstant {
            constant_type: TypeName::from_keyword(type_keyword),
            value: Some(value.trim().to_owned()),
            ..LslConstant::default()
        },
    ))
}

/// Parse a comma-separated parameter list (the text between the parentheses)
/// into ordered [`LslArgument`]s. An empty list yields no arguments.
fn parse_args(args: &str) -> Vec<LslArgument> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed.split(',').filter_map(parse_arg).collect()
}

/// Parse a single `<type> <name>` parameter into an [`LslArgument`]; an
/// unfamiliar type keyword decodes to [`None`] rather than dropping the entry.
fn parse_arg(part: &str) -> Option<LslArgument> {
    let mut tokens = part.split_whitespace();
    let type_keyword = tokens.next()?;
    let name = tokens.next()?;
    Some(LslArgument {
        name: name.to_owned(),
        arg_type: TypeName::from_keyword(type_keyword),
        tooltip: None,
    })
}

// -- parsing tailslide's lint output ----------------------------------------

/// Parse tailslide's `--lint` output (emitted on stderr) into a [`LintReport`].
/// Each finding line reads `LEVEL:: ( line, col): [CODE] message`; only
/// `ERROR::` lines feed the gate, `WARN::` and the `TOTAL::` summary are
/// ignored.
fn parse_lint(stderr: &str) -> LintReport {
    let mut error_lines = BTreeSet::new();
    let mut error_count: usize = 0;
    for raw in stderr.lines() {
        let line = raw.trim_start();
        if line.starts_with("ERROR::") {
            error_count = error_count.saturating_add(1);
            if let Some(number) = lint_line_number(line) {
                let _new = error_lines.insert(number);
            }
        }
    }
    LintReport {
        error_lines,
        error_count,
    }
}

/// Extract the 1-based source line from a tailslide finding line, reading the
/// first number of the `( line, col)` location field.
fn lint_line_number(line: &str) -> Option<u32> {
    let after_paren = line.split_once('(')?.1;
    let field = after_paren.split(',').next()?.trim();
    field.parse::<u32>().ok()
}

// -- the sl-lsl side --------------------------------------------------------

/// The 1-based source lines `sl-lsl` flags as **errors** for `src`: every
/// recovered syntax error (the grid's front-end would reject these) plus every
/// [`Severity::Error`] semantic diagnostic. Warnings are excluded — they are not
/// part of the no-false-positive gate.
fn sl_lsl_error_lines(src: &str, syntax: &LslSyntax) -> BTreeSet<u32> {
    let parsed = parse(src);
    let mut lines = BTreeSet::new();
    for error in &parsed.errors {
        let _new = lines.insert(offset_to_line(src, error.span.start));
    }
    for diagnostic in analyze(&parsed.script, syntax) {
        if diagnostic.severity == Severity::Error {
            let _new = lines.insert(offset_to_line(src, diagnostic.span.start));
        }
    }
    lines
}

/// Map a byte offset into `src` to its 1-based line number, so a `sl-lsl` byte
/// span can be compared against tailslide's line-numbered findings.
fn offset_to_line(src: &str, offset: usize) -> u32 {
    let newlines = src
        .bytes()
        .take(offset)
        .filter(|&byte| byte == b'\n')
        .count();
    u32::try_from(newlines).map_or(u32::MAX, |count| count.saturating_add(1))
}

// -- locating the oracle and the corpus -------------------------------------

/// Locate the `tailslide` binary and read the matching `builtins.txt`, or
/// [`None`] to skip the oracle run. The binary comes from
/// `SL_LSL_TAILSLIDE_BIN`; `builtins.txt` from `SL_LSL_TAILSLIDE_BUILTINS` or,
/// failing that, derived from the binary's location.
fn oracle() -> Option<(PathBuf, String)> {
    let bin = PathBuf::from(std::env::var_os("SL_LSL_TAILSLIDE_BIN")?);
    if !bin.exists() {
        return None;
    }
    let builtins = std::env::var_os("SL_LSL_TAILSLIDE_BUILTINS")
        .map(PathBuf::from)
        .or_else(|| derive_builtins(&bin))?;
    let source = fs_err::read_to_string(&builtins).ok()?;
    Some((bin, source))
}

/// Derive `builtins.txt` from the binary path: a CMake build places the binary
/// at `<repo>/build/tailslide`, with `builtins.txt` at the repo root.
fn derive_builtins(bin: &Path) -> Option<PathBuf> {
    let build_dir = bin.parent()?;
    let repo = build_dir.parent()?;
    let candidate = repo.join("builtins.txt");
    candidate.exists().then_some(candidate)
}

/// Run `tailslide --lint <script>` and parse its findings.
fn run_tailslide(bin: &Path, script: &Path) -> Result<LintReport, BoxError> {
    let output = Command::new(bin).arg("--lint").arg(script).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_lint(&stderr))
}

/// The corpus of `.lsl` scripts to diff, sorted for a deterministic run order.
///
/// Defaults to the committed corpus (`tests/corpus/{valid,error}/`), the
/// regression guard. `SL_LSL_DIFFTEST_CORPUS` overrides it with an arbitrary
/// directory tree — point it at a larger real-world corpus (e.g. tailslide's own
/// `tests/scripts/`) to exercise the no-false-positive bar at scale. Caveat: the
/// recursive-descent parser does not yet guard its recursion depth, so a
/// deeply-nested torture input (tailslide's `parserstackdepth*.lsl`) can
/// overflow the native stack — a known parser bug, not a harness one.
fn corpus_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(dir) = std::env::var_os("SL_LSL_DIFFTEST_CORPUS") {
        collect_lsl_recursive(Path::new(&dir), &mut files);
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("corpus");
        for sub in ["valid", "error"] {
            collect_lsl(&root.join(sub), &mut files);
        }
    }
    files.sort();
    files
}

/// Append every `.lsl` file under `dir` and its sub-directories to `out`, for an
/// external corpus that may nest scripts (a missing directory is skipped).
fn collect_lsl_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs_err::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lsl_recursive(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lsl") {
            out.push(path);
        }
    }
}

/// Append every `.lsl` file directly under `dir` to `out` (a missing directory
/// is simply skipped).
fn collect_lsl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs_err::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("lsl") {
            out.push(path);
        }
    }
}

// -- the oracle-driven test and pure unit tests -----------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use sl_lsl::ast::TypeName;

    use super::{
        BoxError, builtins_library, corpus_files, lint_line_number, offset_to_line, oracle,
        parse_lint, run_tailslide, sl_lsl_error_lines,
    };

    /// Diff `sl-lsl` against `tailslide` over the committed corpus, proving the
    /// no-false-positive bar: no error-severity diagnostic on a script the
    /// oracle compiles cleanly. Skips (passing) when no `tailslide` build is
    /// configured.
    #[test]
    fn differential_oracle_matches_tailslide() -> Result<(), BoxError> {
        let Some((bin, builtins_src)) = oracle() else {
            eprintln!(
                "differential_oracle_matches_tailslide: skipped — set SL_LSL_TAILSLIDE_BIN (and \
                 optionally SL_LSL_TAILSLIDE_BUILTINS) to a built tailslide to run the oracle"
            );
            return Ok(());
        };
        let syntax = builtins_library(&builtins_src);
        assert!(
            !syntax.functions.is_empty(),
            "builtins.txt yielded no functions — is the path correct?"
        );

        let files = corpus_files();
        assert!(!files.is_empty(), "the LSL corpus is empty");

        let mut clean_agreements: usize = 0;
        let mut error_agreements: usize = 0;
        let mut skipped: usize = 0;
        let mut missed: Vec<PathBuf> = Vec::new();
        let mut false_positives: Vec<(PathBuf, BTreeSet<u32>)> = Vec::new();

        for path in &files {
            // An external corpus may hold non-UTF-8 files (e.g. compiled
            // bytecode fixtures); skip anything that is not readable source
            // rather than aborting the whole diff.
            let Ok(src) = fs_err::read_to_string(path) else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            let oracle_report = run_tailslide(&bin, path)?;
            let our_errors = sl_lsl_error_lines(&src, &syntax);

            if oracle_report.error_count == 0 {
                if our_errors.is_empty() {
                    clean_agreements = clean_agreements.saturating_add(1);
                } else {
                    false_positives.push((path.clone(), our_errors));
                }
            } else if our_errors.is_empty() {
                missed.push(path.clone());
            } else {
                error_agreements = error_agreements.saturating_add(1);
            }
        }

        eprintln!(
            "differential oracle: {} file(s) — clean-agree {}, error-agree {}, missed {}, \
             false-positive {}, skipped {}",
            files.len(),
            clean_agreements,
            error_agreements,
            missed.len(),
            false_positives.len(),
            skipped
        );
        for path in &missed {
            eprintln!(
                "  missed (tailslide flags, sl-lsl silent): {}",
                path.display()
            );
        }

        if !false_positives.is_empty() {
            let detail = false_positives
                .iter()
                .map(|(path, lines)| format!("{} at lines {:?}", path.display(), lines))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!(
                "sl-lsl reported errors on {} script(s) tailslide compiles cleanly (false \
                 positives): {detail}",
                false_positives.len()
            )
            .into());
        }
        Ok(())
    }

    /// The `builtins.txt` parser recovers a function's return type and argument
    /// types, a void return, an empty argument list, an event and a constant.
    #[test]
    fn builtins_parser_covers_each_shape() -> Result<(), String> {
        let text = "\
// Generated by the LSL2 Derived Files Generator.
integer llAbs( integer val )
void llSay( integer channel, string msg )
key llGetOwner(  )
event at_target( integer tnum, vector targetpos, vector ourpos )
const integer ACTIVE = 0x2";
        let syntax = builtins_library(text);

        let ll_abs = syntax.function("llAbs").ok_or("llAbs missing")?;
        assert_eq!(ll_abs.return_type, Some(TypeName::Integer));
        assert_eq!(ll_abs.arguments.len(), 1);
        assert_eq!(
            ll_abs.arguments.first().map(|a| a.arg_type),
            Some(Some(TypeName::Integer))
        );

        let ll_say = syntax.function("llSay").ok_or("llSay missing")?;
        assert_eq!(ll_say.return_type, None);
        assert_eq!(ll_say.arguments.len(), 2);

        let ll_get_owner = syntax.function("llGetOwner").ok_or("llGetOwner missing")?;
        assert_eq!(ll_get_owner.return_type, Some(TypeName::Key));
        assert!(ll_get_owner.arguments.is_empty());

        let at_target = syntax.event("at_target").ok_or("at_target missing")?;
        assert_eq!(at_target.arguments.len(), 3);

        let active = syntax.constant("ACTIVE").ok_or("ACTIVE missing")?;
        assert_eq!(active.constant_type, Some(TypeName::Integer));
        assert_eq!(active.value.as_deref(), Some("0x2"));

        // The fixed keyword groups are populated too.
        assert!(syntax.is_type("integer"));
        assert!(syntax.is_control("if"));
        Ok(())
    }

    /// The lint-output parser extracts the error lines and count from real
    /// tailslide output and ignores warnings and the total summary.
    #[test]
    fn lint_parser_reads_errors_only() {
        let stderr = "\
ERROR:: (  5,  9): [E10006] `llNotAFunction' is undeclared.
 WARN:: (  5,  9): [E20009] variable `x' declared but never used.
ERROR:: ( 12,  3): [E10013] Too few arguments to function `llSay'.
TOTAL:: Errors: 2  Warnings: 1";
        let report = parse_lint(stderr);
        assert_eq!(report.error_count, 2);
        assert_eq!(
            report.error_lines.iter().copied().collect::<Vec<_>>(),
            vec![5, 12]
        );
    }

    /// A clean run reports no errors at all.
    #[test]
    fn lint_parser_clean_run_is_empty() {
        let report = parse_lint("TOTAL:: Errors: 0  Warnings: 0\n");
        assert_eq!(report.error_count, 0);
        assert!(report.error_lines.is_empty());
    }

    /// The location-field reader pulls the line number out of a finding line.
    #[test]
    fn lint_line_number_reads_the_location() {
        assert_eq!(
            lint_line_number("ERROR:: (  5,  9): [E10006] `x' is undeclared."),
            Some(5)
        );
        assert_eq!(lint_line_number("TOTAL:: Errors: 0  Warnings: 0"), None);
    }

    /// Byte offsets map to 1-based lines, counting the newlines before them.
    #[test]
    fn offset_to_line_counts_newlines() {
        let src = "a\nbb\nccc";
        assert_eq!(offset_to_line(src, 0), 1);
        assert_eq!(offset_to_line(src, 2), 2);
        assert_eq!(offset_to_line(src, 5), 3);
    }

    /// The sl-lsl side flags a syntax error's line, and reports nothing on a
    /// clean script (against a representative hand-built library).
    #[test]
    fn sl_lsl_error_lines_flag_a_syntax_error() {
        let library =
            builtins_library("void llSay( integer channel, string msg )\nkey llGetOwner(  )");
        let clean = "default\n{\n    state_entry()\n    {\n        llSay(0, \"hi\");\n    }\n}\n";
        assert!(sl_lsl_error_lines(clean, &library).is_empty());

        let broken = "default\n{\n    state_entry()\n    {\n        llSay(0 \"hi\")\n    }\n}\n";
        let lines = sl_lsl_error_lines(broken, &library);
        assert!(
            lines.contains(&5),
            "expected an error on line 5, got {lines:?}"
        );
    }
}
