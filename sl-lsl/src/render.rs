//! **Reader-facing diagnostic rendering** — turning the semantic pass's
//! [`Diagnostic`]s (and the grid's own compiler errors) into `rustc`-grade
//! output with a source excerpt, a caret under the offending span, and — where
//! the library table makes it possible — a *did-you-mean* suggestion or the
//! grid's own signature quoted back.
//!
//! LSL's native compiler errors are terse to the point of hostility
//! (`(12, 5) : ERROR : Syntax error`, and little else). Owning the parser
//! ([`crate::parser`]) *and* holding the grid's typed library ([`LslSyntax`])
//! lets a client do far better without inventing anything the grid disagrees
//! with:
//!
//! - a **labelled span** — the source line with a caret underlining exactly the
//!   bytes at fault, `--> line:col` above it, in the familiar `rustc` / `ariadne`
//!   shape;
//! - **"did you mean…?"** by edit distance against the grid's real symbol tables
//!   ([`closest`]), so a mistyped `llSy` suggests `llSay` and an `os*` typo
//!   suggests the OpenSim function that is actually served — automatically, with
//!   no baked-in name list;
//! - **honest type errors** that quote the tooltip the grid already gave us —
//!   *"`llSetTimerEvent` expects `(float rate)`"* — reconstructed from the
//!   [`LslFunction`](crate::syntax::LslFunction) signature.
//!
//! ## One renderer for local *and* grid-side diagnostics
//!
//! The same machinery renders the **server's** errors. A
//! `ScriptCompileError` (parsed by `sl-proto` into a 1-based line, column and
//! message) is fed through [`render_grid_error`], which resolves that
//! line/column to a byte span and renders it through the identical span
//! plumbing. Even an error only the grid can produce (the Mono/CIL back-end, an
//! OpenSim C#-specific message) then arrives with a caret and its source line
//! instead of a bare `(12, 5)`, and — because it is the same code — looks
//! identical to a locally-found one.
//!
//! The crate stays I/O-free and Bevy-free: this module borrows a `&str` source
//! and returns an owned `String`. Colour is opt-in ([`RenderStyle::color`]) and
//! off by default, so the output is safe to log, diff in a test, or hand to a
//! terminal that understands ANSI.
#![expect(
    clippy::module_name_repetitions,
    reason = "the `render_*` free functions are the crate's public entry points \
              (re-exported as `sl_lsl::render_diagnostic` etc.); the `render_` prefix \
              names what they do and is the established, discoverable API surface"
)]

use core::fmt::Write as _;
use core::ops::Range;

use crate::parser::ParseError;
use crate::semantics::{Diagnostic, DiagnosticKind, Severity};
use crate::syntax::LslSyntax;

/// How to render: whether to emit ANSI colour, and how wide a tab is when
/// aligning the caret under a source line that uses tabs (LSL scripts commonly
/// do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderStyle {
    /// Emit ANSI SGR colour codes (red for an error, yellow for a warning, blue
    /// for the gutter). Off by default so rendered text is plain and diffable.
    pub color: bool,
    /// The column width a tab expands to when the source line is displayed, so
    /// the caret lands under the right character. Four matches the LSL editor's
    /// convention.
    pub tab_width: usize,
}

impl Default for RenderStyle {
    /// Plain (no colour), four-space tabs — the safe default for logs and tests.
    fn default() -> Self {
        Self {
            color: false,
            tab_width: 4,
        }
    }
}

/// A secondary annotation printed below the source excerpt, in the shape
/// `rustc` uses for its `= help:` / `= note:` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Note {
    /// The lead-in word (`help` for an actionable suggestion, `note` for
    /// context).
    lead: &'static str,
    /// The note text.
    text: String,
}

/// Render one semantic [`Diagnostic`] against the grid library `syntax`, in the
/// default [`RenderStyle`].
///
/// The library is consulted for the two enrichments the bare
/// [`Diagnostic`] cannot carry: a *did-you-mean* suggestion (edit distance over
/// the real symbol names) and the grid's own signature for a type or arity
/// error. Pass an empty [`LslSyntax`] and the caret and message still render;
/// only the suggestions are omitted.
#[must_use]
pub fn render_diagnostic(source: &str, diag: &Diagnostic, syntax: &LslSyntax) -> String {
    render_diagnostic_styled(source, diag, syntax, RenderStyle::default())
}

/// Render one semantic [`Diagnostic`] with an explicit [`RenderStyle`].
#[must_use]
pub fn render_diagnostic_styled(
    source: &str,
    diag: &Diagnostic,
    syntax: &LslSyntax,
    style: RenderStyle,
) -> String {
    let (inline, notes) = enrich(diag, syntax);
    render_labelled(
        source,
        diag.severity,
        &diag.message,
        &diag.span,
        inline.as_deref(),
        &notes,
        style,
    )
}

/// Render a whole batch of [`Diagnostic`]s (as [`crate::analyze`] returns them),
/// one block each, separated by a blank line and in the order given.
#[must_use]
pub fn render_diagnostics(source: &str, diags: &[Diagnostic], syntax: &LslSyntax) -> String {
    let style = RenderStyle::default();
    let mut out = String::new();
    for (index, diag) in diags.iter().enumerate() {
        if index != 0 {
            out.push('\n');
        }
        out.push_str(&render_diagnostic_styled(source, diag, syntax, style));
    }
    out
}

/// Render a recovered syntax error ([`ParseError`]) through the same span
/// machinery. A parse error needs no library, so there is no suggestion — just
/// the message, the caret, and the source line.
#[must_use]
pub fn render_parse_error(source: &str, error: &ParseError) -> String {
    render_labelled(
        source,
        Severity::Error,
        &error.message,
        &error.span,
        None,
        &[],
        RenderStyle::default(),
    )
}

/// Render a **grid-side** compiler error — one whose position the grid gave as a
/// 1-based `line` and (optional) 1-based `column`, as `sl-proto` parses a
/// `ScriptCompileError` — through the identical caret plumbing.
///
/// The `(line, column)` pair is resolved against `source` to a byte span (a
/// zero-width point when no column is given, or when the position lies past the
/// end of the source), so the rendered block is indistinguishable from a
/// locally-found diagnostic at the same place. `severity` lets the caller mark a
/// grid *warning* distinctly from a hard compile error.
#[must_use]
pub fn render_grid_error(
    source: &str,
    line: u32,
    column: Option<u32>,
    severity: Severity,
    message: &str,
) -> String {
    let span = grid_span(source, line, column);
    render_labelled(
        source,
        severity,
        message,
        &span,
        None,
        &[],
        RenderStyle::default(),
    )
}

/// Resolve a grid's 1-based `(line, column)` to a byte span into `source`. A
/// missing or out-of-range column yields a zero-width span at the line start (or
/// at the clamped column); an out-of-range line clamps to the end of the source.
fn grid_span(source: &str, line: u32, column: Option<u32>) -> Range<usize> {
    let line_index = line.saturating_sub(1);
    let mut start = source.len();
    for (index, line_span) in line_spans(source).enumerate() {
        if u32::try_from(index).is_ok_and(|number| number == line_index) {
            let column_offset = column
                .map(|col| col.saturating_sub(1))
                .and_then(|col| usize::try_from(col).ok())
                .unwrap_or(0);
            // Step `column_offset` characters into the line, clamped to its end.
            let text = source.get(line_span.clone()).unwrap_or("");
            let byte = text
                .char_indices()
                .nth(column_offset)
                .map_or(text.len(), |(offset, _ch)| offset);
            start = line_span.start.saturating_add(byte);
            break;
        }
    }
    start..start
}

/// The did-you-mean inline label and any signature notes a diagnostic earns
/// from the library. Returns `(inline_label, notes)`: the inline label is
/// printed after the caret, the notes below.
fn enrich(diag: &Diagnostic, syntax: &LslSyntax) -> (Option<String>, Vec<Note>) {
    match &diag.kind {
        DiagnosticKind::UndefinedFunction { name } => (
            suggestion(name, syntax.functions.keys().map(String::as_str)),
            Vec::new(),
        ),
        DiagnosticKind::UndefinedVariable { name } => (
            suggestion(name, syntax.constants.keys().map(String::as_str)),
            Vec::new(),
        ),
        DiagnosticKind::UnknownEvent { name } => (
            suggestion(name, syntax.events.keys().map(String::as_str)),
            Vec::new(),
        ),
        DiagnosticKind::WrongArgCount { callee, .. }
        | DiagnosticKind::ArgTypeMismatch { callee, .. } => {
            (None, signature_note(callee, syntax).into_iter().collect())
        }
        DiagnosticKind::WrongEventArgCount { event, .. }
        | DiagnosticKind::EventArgTypeMismatch { event, .. } => (
            None,
            event_signature_note(event, syntax).into_iter().collect(),
        ),
        _ => (None, Vec::new()),
    }
}

/// A `did you mean \`X\`?` inline label for `name`, if a library symbol is within
/// edit distance (see [`closest`]).
fn suggestion<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    closest(name, candidates).map(|best| format!("did you mean `{best}`?"))
}

/// A `note` quoting the grid's own signature for a function call error, e.g.
/// `` `llSetTimerEvent` expects `(float rate)` `` — reconstructed from the
/// library entry so the type error reads the way the grid documents the call.
fn signature_note(callee: &str, syntax: &LslSyntax) -> Option<Note> {
    let func = syntax.function(callee)?;
    let params = func
        .arguments
        .iter()
        .map(|arg| match arg.arg_type {
            Some(ty) => format!("{} {}", ty.keyword(), arg.name),
            None => arg.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(Note {
        lead: "note",
        text: format!("`{callee}` expects `({params})`"),
    })
}

/// A `note` quoting the grid's signature for an event-handler error, e.g.
/// `` `touch_start` is `touch_start(integer num_detected)` ``.
fn event_signature_note(event: &str, syntax: &LslSyntax) -> Option<Note> {
    let entry = syntax.event(event)?;
    let params = entry
        .arguments
        .iter()
        .map(|arg| match arg.arg_type {
            Some(ty) => format!("{} {}", ty.keyword(), arg.name),
            None => arg.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(Note {
        lead: "note",
        text: format!("event `{event}` is `{event}({params})`"),
    })
}

/// The library symbol closest to `target` by Levenshtein distance, if one is
/// near enough to be a plausible typo. The threshold scales with the target's
/// length (a longer name tolerates more slips) but never suggests a wildly
/// different word: at most three edits, and always fewer than half the name's
/// length, so a one-character identifier suggests nothing.
#[must_use]
pub fn closest<'a>(target: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let target_len = target.chars().count();
    let max_dist = (target_len / 3).clamp(1, 3);
    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let dist = levenshtein(target, candidate);
        if dist == 0 {
            // An exact match is not a typo; nothing to suggest.
            return None;
        }
        if dist > max_dist || dist.saturating_mul(2) >= target_len {
            continue;
        }
        if best.is_none_or(|(best_dist, _name)| dist < best_dist) {
            best = Some((dist, candidate));
        }
    }
    best.map(|(_dist, name)| name)
}

/// The Levenshtein edit distance between two strings (character-wise), by the
/// classic single-row dynamic-programming recurrence. Used only for the
/// did-you-mean suggestion, over the few-hundred-entry library table.
fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    // `row[j]` is the distance between the processed prefix of `a` and the
    // first `j` characters of `b`; it starts as the distance from the empty `a`.
    let mut row: Vec<usize> = (0..=b_chars.len()).collect();
    for (i, a_ch) in a.chars().enumerate() {
        let mut diagonal = i;
        let mut left = i.saturating_add(1);
        if let Some(first) = row.first_mut() {
            *first = left;
        }
        for (j, b_ch) in b_chars.iter().enumerate() {
            let up = row.get(j.saturating_add(1)).copied().unwrap_or(0);
            let cost = usize::from(a_ch != *b_ch);
            let here = diagonal
                .saturating_add(cost)
                .min(left.saturating_add(1))
                .min(up.saturating_add(1));
            if let Some(slot) = row.get_mut(j.saturating_add(1)) {
                *slot = here;
            }
            diagonal = up;
            left = here;
        }
    }
    row.last().copied().unwrap_or(0)
}

/// The core renderer: a header line, a `--> line:col` locator, the source line
/// with a caret under `span`, an optional inline label after the caret, and any
/// `notes` below — the one code path both local and grid-side diagnostics take.
fn render_labelled(
    source: &str,
    severity: Severity,
    message: &str,
    span: &Range<usize>,
    inline: Option<&str>,
    notes: &[Note],
    style: RenderStyle,
) -> String {
    let (line_number, line_span) = locate(source, span.start);
    let line_text = source.get(line_span.clone()).unwrap_or("");
    // Column (1-based, in characters) of the span start within its line.
    let prefix = source.get(line_span.start..span.start).unwrap_or("");
    let column = prefix.chars().count().saturating_add(1);

    let (line_display, caret_indent) = expand_tabs(line_text, prefix, style.tab_width);
    let caret_width = caret_span_width(source, line_span.clone(), span);

    let gutter_width = line_number.to_string().len();
    let pad = " ".repeat(gutter_width);

    let mut out = String::new();
    // Header: `error: <message>` / `warning: <message>`.
    let label = severity_label(severity);
    let _ignored = writeln!(
        out,
        "{}: {message}",
        paint(label, severity_color(severity), style)
    );
    // Locator: `  --> line:col`, aligned under the header.
    let _ignored = writeln!(
        out,
        "{pad}{} {line_number}:{column}",
        paint("-->", GUTTER, style)
    );
    // Blank gutter line, then the source line, then the caret line.
    let bar = paint("|", GUTTER, style);
    let _ignored = writeln!(out, "{pad} {bar}");
    let number = paint(&line_number.to_string(), GUTTER, style);
    let _ignored = writeln!(out, "{number} {bar} {line_display}");
    let carets = "^".repeat(caret_width.max(1));
    let underline = match inline {
        Some(text) => format!("{carets} {text}"),
        None => carets,
    };
    let _ignored = writeln!(
        out,
        "{pad} {bar} {caret_indent}{}",
        paint(&underline, severity_color(severity), style)
    );
    // `= note:` / `= help:` lines, aligned under the bar.
    for note in notes {
        let _ignored = writeln!(
            out,
            "{pad} {} {}: {}",
            paint("=", GUTTER, style),
            paint(note.lead, GUTTER, style),
            note.text
        );
    }
    out
}

/// The 1-based line number containing byte `offset`, and the byte span of that
/// whole line (excluding its trailing newline).
fn locate(source: &str, offset: usize) -> (usize, Range<usize>) {
    let clamped = offset.min(source.len());
    for (number, span) in line_spans(source).enumerate() {
        if clamped <= span.end {
            return (number.saturating_add(1), span);
        }
    }
    // An empty source, or an offset past the last line: point at the final line.
    let start = source
        .rfind('\n')
        .map_or(0, |index| index.saturating_add(1));
    (
        source.matches('\n').count().saturating_add(1),
        start..source.len(),
    )
}

/// The byte spans of each line in `source`, excluding the newline terminator.
/// Yields one span per line; a trailing newline does *not* produce a spurious
/// empty final line, matching how an editor numbers lines.
fn line_spans(source: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0usize;
    let mut done = false;
    core::iter::from_fn(move || {
        if done {
            return None;
        }
        match source.get(start..).and_then(|rest| rest.find('\n')) {
            Some(offset) => {
                let end = start.saturating_add(offset);
                let span = start..end;
                start = end.saturating_add(1);
                Some(span)
            }
            None => {
                done = true;
                // The last line (no trailing newline), unless the source ended
                // exactly on a newline (then there is no further line).
                if start <= source.len() && (start < source.len() || source.is_empty()) {
                    Some(start..source.len())
                } else {
                    None
                }
            }
        }
    })
}

/// The visual width (in caret characters) to underline for `span` on the line
/// `line_span`: from the span start to its end, clamped to the line, at least
/// one, measured in characters (tabs count as one caret, matching the single
/// space a tab expands the caret indent by).
fn caret_span_width(source: &str, line_span: Range<usize>, span: &Range<usize>) -> usize {
    let end = span.end.min(line_span.end).max(span.start);
    let text = source.get(span.start..end).unwrap_or("");
    text.chars().count().max(1)
}

/// Expand tabs in the displayed source line to spaces, and build the matching
/// caret indent (the run of spaces before the caret), so the caret lands under
/// the right character regardless of tabs. `prefix` is the source text before
/// the span start on this line.
fn expand_tabs(line_text: &str, prefix: &str, tab_width: usize) -> (String, String) {
    let width = tab_width.max(1);
    let mut display = String::new();
    for ch in line_text.chars() {
        if ch == '\t' {
            display.push_str(&" ".repeat(width));
        } else {
            display.push(ch);
        }
    }
    let mut indent = String::new();
    for ch in prefix.chars() {
        if ch == '\t' {
            indent.push_str(&" ".repeat(width));
        } else {
            indent.push(' ');
        }
    }
    (display, indent)
}

/// The header word for a severity.
const fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// The ANSI colour for a severity's header and caret.
const fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => RED,
        Severity::Warning => YELLOW,
    }
}

/// ANSI SGR: bold red (an error).
const RED: &str = "\u{1b}[1;31m";
/// ANSI SGR: bold yellow (a warning).
const YELLOW: &str = "\u{1b}[1;33m";
/// ANSI SGR: bold blue (the gutter: `-->`, `|`, `=`, line numbers).
const GUTTER: &str = "\u{1b}[1;34m";
/// ANSI SGR: reset all attributes.
const RESET: &str = "\u{1b}[0m";

/// Wrap `text` in an ANSI colour when [`RenderStyle::color`] is set, else return
/// it untouched.
fn paint(text: &str, color: &str, style: RenderStyle) -> String {
    if style.color {
        format!("{color}{text}{RESET}")
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        RenderStyle, closest, levenshtein, render_diagnostic, render_grid_error, render_parse_error,
    };
    use crate::ast::TypeName;
    use crate::parse;
    use crate::parser::ParseError;
    use crate::semantics::{DiagnosticKind, Severity, analyze};
    use crate::syntax::{LslArgument, LslConstant, LslEvent, LslFunction, LslSyntax};

    /// A small library table: `llSay`, `llSetTimerEvent`, a constant and an
    /// event, enough to drive suggestions and signature notes.
    fn library() -> LslSyntax {
        let mut syntax = LslSyntax::default();
        let _prev = syntax.functions.insert(
            "llSay".to_owned(),
            LslFunction {
                return_type: None,
                arguments: vec![
                    LslArgument {
                        name: "channel".to_owned(),
                        arg_type: Some(TypeName::Integer),
                        tooltip: None,
                    },
                    LslArgument {
                        name: "msg".to_owned(),
                        arg_type: Some(TypeName::String),
                        tooltip: None,
                    },
                ],
                ..LslFunction::default()
            },
        );
        let _prev = syntax.functions.insert(
            "llSetTimerEvent".to_owned(),
            LslFunction {
                return_type: None,
                arguments: vec![LslArgument {
                    name: "rate".to_owned(),
                    arg_type: Some(TypeName::Float),
                    tooltip: None,
                }],
                ..LslFunction::default()
            },
        );
        let _prev = syntax.constants.insert(
            "TRUE".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some("1".to_owned()),
                ..LslConstant::default()
            },
        );
        let _prev = syntax.events.insert(
            "touch_start".to_owned(),
            LslEvent {
                arguments: vec![LslArgument {
                    name: "num_detected".to_owned(),
                    arg_type: Some(TypeName::Integer),
                    tooltip: None,
                }],
                ..LslEvent::default()
            },
        );
        syntax
    }

    /// Render every diagnostic whose kind matches `keep`, joined — so a test can
    /// assert on the rendering of a specific kind without an `expect`/`unwrap`
    /// (both denied by the workspace lints) on a `find`.
    fn render_kind(source: &str, keep: impl Fn(&DiagnosticKind) -> bool) -> String {
        let syntax = library();
        let parsed = parse(source);
        analyze(&parsed.script, &syntax)
            .iter()
            .filter(|diag| keep(&diag.kind))
            .map(|diag| render_diagnostic(source, diag, &syntax))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Levenshtein matches known distances (identity, one substitution, one
    /// insertion, a full rewrite).
    #[test]
    fn levenshtein_known_distances() {
        assert_eq!(levenshtein("llSay", "llSay"), 0);
        assert_eq!(levenshtein("llSy", "llSay"), 1);
        assert_eq!(levenshtein("llSays", "llSay"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    /// `closest` suggests a near name, declines an exact match, and declines a
    /// one-character target (which would suggest noise).
    #[test]
    fn closest_suggests_only_plausible() {
        let names = ["llSay", "llSetTimerEvent", "llGetOwner"];
        assert_eq!(closest("llSy", names.into_iter()), Some("llSay"));
        assert_eq!(
            closest("llSetTimerEvnt", names.into_iter()),
            Some("llSetTimerEvent")
        );
        // An exact match is not a typo.
        assert_eq!(closest("llSay", names.into_iter()), None);
        // Nothing close enough.
        assert_eq!(closest("llTeleportAgent", names.into_iter()), None);
        // A single character never suggests.
        assert_eq!(closest("x", names.into_iter()), None);
    }

    /// A rendered undefined-function error carries the header, the `-->`
    /// locator, the source line, a caret of the right width, and a
    /// did-you-mean suggestion drawn from the library.
    #[test]
    fn undefined_function_renders_with_suggestion() {
        let source = "default\n{\n    state_entry()\n    {\n        llSy(0, \"hi\");\n    }\n}\n";
        let rendered = render_kind(source, |kind| {
            matches!(kind, DiagnosticKind::UndefinedFunction { .. })
        });

        assert!(rendered.contains("error: call to undefined function `llSy`"));
        assert!(rendered.contains("--> 5:9"));
        assert!(rendered.contains("        llSy(0, \"hi\");"));
        assert!(rendered.contains("did you mean `llSay`?"));
        // The caret underlines the four bytes of `llSy`.
        assert!(rendered.contains("^^^^ did you mean `llSay`?"));
    }

    /// An argument-type mismatch quotes the grid's own signature back as a note.
    #[test]
    fn arg_type_mismatch_quotes_signature() {
        let source =
            "default\n{\n    state_entry()\n    {\n        llSetTimerEvent(\"x\");\n    }\n}\n";
        let rendered = render_kind(source, |kind| {
            matches!(kind, DiagnosticKind::ArgTypeMismatch { .. })
        });

        assert!(rendered.contains("note: `llSetTimerEvent` expects `(float rate)`"));
    }

    /// A grid-side compiler error (1-based line/column, no local diagnostic)
    /// renders through the same machinery: a caret at the reported position over
    /// the real source line.
    #[test]
    fn grid_error_renders_at_line_col() {
        let source = "default\n{\n    state_entry()\n    {\n        bogus();\n    }\n}\n";
        let rendered = render_grid_error(source, 5, Some(9), Severity::Error, "Syntax error");
        assert!(rendered.contains("error: Syntax error"));
        assert!(rendered.contains("--> 5:9"));
        assert!(rendered.contains("        bogus();"));
        assert!(rendered.contains('^'));
    }

    /// A grid error with no column points a zero-width caret at the line start.
    #[test]
    fn grid_error_without_column_points_at_line_start() {
        let source = "default\n{\n}\n";
        let rendered = render_grid_error(source, 2, None, Severity::Error, "unexpected");
        assert!(rendered.contains("--> 2:1"));
    }

    /// A tab-indented line aligns the caret under the offending token (tabs
    /// expand to `tab_width` columns in both the display and the caret indent).
    #[test]
    fn tabs_align_the_caret() {
        // A tab-indented call inside an event handler. Two leading tabs put the
        // call at eight display columns.
        let source = "default\n{\n\tstate_entry()\n\t{\n\t\tllSy();\n\t}\n}\n";
        let rendered = render_kind(source, |kind| {
            matches!(kind, DiagnosticKind::UndefinedFunction { .. })
        });
        // Two leading tabs expand to eight spaces in the displayed source line.
        assert!(rendered.contains("        llSy();"));
        // The caret indent matches: eight spaces, then four carets.
        assert!(rendered.contains("|         ^^^^"));
    }

    /// A recovered syntax error renders with a caret and message, no library
    /// needed.
    #[test]
    fn parse_error_renders() {
        let error = ParseError {
            message: "expected `}`".to_owned(),
            span: 8..9,
        };
        let source = "default\n{";
        let rendered = render_parse_error(source, &error);
        assert!(rendered.contains("error: expected `}`"));
        assert!(rendered.contains("--> 2:1"));
    }

    /// With colour on, the header and caret carry ANSI codes; with it off, the
    /// output is plain.
    #[test]
    fn color_is_opt_in() {
        let error = ParseError {
            message: "boom".to_owned(),
            span: 0..3,
        };
        let source = "abc\n";
        let colored = super::render_labelled(
            source,
            Severity::Error,
            "boom",
            &(0..3),
            None,
            &[],
            RenderStyle {
                color: true,
                tab_width: 4,
            },
        );
        assert!(colored.contains("\u{1b}[1;31m"));
        let plain = render_parse_error(source, &error);
        assert!(!plain.contains("\u{1b}["));
    }
}
