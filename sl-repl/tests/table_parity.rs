//! The REPL's two hand-maintained command tables must agree.
//!
//! `format::command_name` (a match arm per [`Command`](sl_proto::Command)
//! variant) decides what a dispatched command is *called* in the log and the
//! transcript; `registry::all_specs` decides what a typed line is *parsed* as.
//! Nothing in the type system ties them together, and they had drifted: fifteen
//! commands the formatter could print had no registry entry at all, and
//! `UploadScript` printed an ambiguous `upload_script` that neither of its two
//! registry entries answers to. A transcript containing any of them could not
//! be replayed.
//!
//! The registry side is read at runtime, through the public
//! [`Registry`](sl_repl::Registry). The formatter side has no runtime
//! enumeration — `command_name` maps a *value* to a name, and there is no way to
//! conjure one of every `Command` variant — so it is read out of the source
//! text, with the extraction itself checked
//! (`arms_and_names_are_extracted_in_step`) so a reformatting that hid arms
//! would fail rather than quietly shrink the comparison.

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use sl_proto::CircuitId;
    use sl_repl::{Registry, ReplAction, ReplContext, ReplError, format_command, parse_line};

    /// A context that resolves no placeholders but reports a circuit, so the
    /// object- and parcel-scoped commands can scope their region-local ids the
    /// way they would in a live session.
    struct CircuitContext;

    impl ReplContext for CircuitContext {
        fn resolve_placeholder(&self, _name: &str) -> Option<String> {
            None
        }

        fn current_circuit_id(&self) -> Option<CircuitId> {
            Some(CircuitId::new(7))
        }
    }

    /// The `format.rs` source, scanned for the `command_name` arms.
    const FORMAT_SOURCE: &str = include_str!("../src/format.rs");

    /// The `registry.rs` source, scanned for each spec's `usage` and the
    /// argument names its build closure reads.
    const REGISTRY_SOURCE: &str = include_str!("../src/registry.rs");

    /// The signature line `command_name`'s body follows.
    const COMMAND_NAME: &str = "fn command_name(command: &Command) -> &'static str {";

    /// The body of the named function in `source`: everything between its
    /// signature line and the first line that closes it at column zero.
    fn function_body<'a>(source: &'a str, signature: &str) -> Result<&'a str, String> {
        let (_before, after) = source
            .split_once(signature)
            .ok_or_else(|| format!("`{signature}` not found in the scanned source"))?;
        let end = after
            .find("\n}\n")
            .ok_or_else(|| format!("`{signature}` is never closed at column zero"))?;
        Ok(after.get(..end).unwrap_or_default())
    }

    /// The string literal on the right of each `=>` in `command_name`'s body —
    /// the REPL name the formatter prints for one `Command` variant.
    fn printable_names() -> Result<Vec<String>, String> {
        Ok(function_body(FORMAT_SOURCE, COMMAND_NAME)?
            .lines()
            .filter_map(|line| line.split_once("=> \""))
            .filter_map(|(_pattern, tail)| tail.split_once('"'))
            .map(|(name, _tail)| name.to_owned())
            .collect())
    }

    /// Every command name the registry parses.
    fn registered_names() -> Vec<String> {
        Registry::shared()
            .specs()
            .iter()
            .map(|spec| spec.name.to_owned())
            .collect()
    }

    #[test]
    fn arms_and_names_are_extracted_in_step() -> Result<(), String> {
        let arms = function_body(FORMAT_SOURCE, COMMAND_NAME)?
            .matches("Command::")
            .count();
        let names = printable_names()?.len();
        assert_eq!(
            arms, names,
            "the scan found {arms} `Command::` patterns but {names} names; the \
             extraction no longer sees every arm, so the parity checks below \
             would be comparing a truncated table"
        );
        assert!(
            names > 300,
            "only {names} names extracted — `command_name` has ~350 arms, so the \
             scan is broken rather than the table being small"
        );
        Ok(())
    }

    #[test]
    fn every_printable_name_is_a_command_the_registry_parses() -> Result<(), String> {
        let registered: BTreeSet<String> = registered_names().into_iter().collect();
        let orphans: Vec<String> = printable_names()?
            .into_iter()
            .filter(|name| !registered.contains(name))
            .collect();
        assert!(
            orphans.is_empty(),
            "the formatter prints {} command name(s) no registry entry answers \
             to, so a transcript containing one cannot be replayed: {orphans:?}",
            orphans.len()
        );
        Ok(())
    }

    #[test]
    fn every_registered_name_is_one_the_formatter_prints() -> Result<(), String> {
        let printable: BTreeSet<String> = printable_names()?.into_iter().collect();
        let orphans: Vec<String> = registered_names()
            .into_iter()
            .filter(|name| !printable.contains(name))
            .collect();
        assert!(
            orphans.is_empty(),
            "the registry parses {} command name(s) the formatter never prints, \
             so dispatching one logs it under a different name: {orphans:?}",
            orphans.len()
        );
        Ok(())
    }

    #[test]
    fn command_names_are_unique_on_both_sides() -> Result<(), String> {
        for (side, names) in [
            ("the formatter", printable_names()?),
            ("the registry", registered_names()),
        ] {
            let unique: BTreeSet<&String> = names.iter().collect();
            assert_eq!(
                unique.len(),
                names.len(),
                "{side} uses one name for two different commands"
            );
        }
        Ok(())
    }

    /// One `CommandSpec { … }` literal, as scanned out of `all_specs`.
    struct ScannedSpec {
        /// The spec's command name.
        name: String,
        /// Its `usage` string with the line-continuation escapes collapsed.
        usage: String,
        /// The argument names its build closure passes to the typed accessors.
        fields: BTreeSet<String>,
    }

    /// Every `CommandSpec` literal in `all_specs`, split on the
    /// eight-space-indented `CommandSpec {` that starts each one.
    fn scanned_specs() -> Result<Vec<ScannedSpec>, String> {
        /// Collapse a Rust string literal's `\` line continuations and strip
        /// the surrounding quotes.
        fn literal(text: &str) -> String {
            let mut out = String::new();
            let mut continuing = false;
            for line in text.lines() {
                let line = if continuing { line.trim_start() } else { line };
                continuing = line.ends_with('\\');
                out.push_str(line.trim_end_matches('\\'));
            }
            out.trim().trim_matches('"').to_owned()
        }

        /// The argument name in an accessor call, given the text just after its
        /// `ctx,` argument: the next `"…"` literal, if it opens before the line
        /// ends.
        fn accessor_field(rest: &str) -> Option<String> {
            let start = rest.find('"')?;
            if start > rest.find('\n').unwrap_or(usize::MAX) {
                return None;
            }
            let after = rest.get(start.checked_add(1)?..)?;
            let end = after.find('"')?;
            after.get(..end).map(str::to_owned)
        }

        let body = function_body(REGISTRY_SOURCE, "fn all_specs() -> Vec<CommandSpec> {")?;
        let mut specs = Vec::new();
        for block in body.split("\n        CommandSpec {").skip(1) {
            let Some((_head, after_name)) = block.split_once("name: \"") else {
                continue;
            };
            let Some((name, after)) = after_name.split_once('"') else {
                continue;
            };
            let usage = after
                .split_once("usage: ")
                .and_then(|(_before, tail)| tail.split_once(",\n"))
                .map_or_else(String::new, |(text, _rest)| literal(text));
            let mut fields = BTreeSet::new();
            for piece in block.split("ctx,").skip(1) {
                if let Some(field) = accessor_field(piece) {
                    let _inserted = fields.insert(field);
                }
            }
            specs.push(ScannedSpec {
                name: name.to_owned(),
                usage,
                fields,
            });
        }
        Ok(specs)
    }

    #[test]
    fn every_spec_literal_is_scanned() -> Result<(), String> {
        let scanned = scanned_specs()?.len();
        let registered = registered_names().len();
        assert_eq!(
            scanned, registered,
            "the scan found {scanned} `CommandSpec` literals but the registry \
             holds {registered} specs; the usage check below would be skipping \
             some"
        );
        Ok(())
    }

    #[test]
    fn every_usage_names_the_fields_its_builder_reads() -> Result<(), String> {
        let mut drifted = Vec::new();
        for spec in scanned_specs()? {
            let words: BTreeSet<&str> = spec
                .usage
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .collect();
            let missing: Vec<&String> = spec
                .fields
                .iter()
                .filter(|field| !words.contains(field.as_str()))
                .collect();
            if !missing.is_empty() {
                drifted.push(format!(
                    "{} reads {missing:?} but its usage is {:?}",
                    spec.name, spec.usage
                ));
            }
        }
        assert!(
            drifted.is_empty(),
            "a usage hint that does not name a field its builder reads is a lie \
             `help` prints and a keyword the user cannot guess:\n{}",
            drifted.join("\n")
        );
        Ok(())
    }

    /// A required-argument value that stands a chance of parsing, guessed from
    /// one `<…>` token of a usage string.
    fn sample_value(token: &str) -> String {
        /// The word before the first `|` of an `a|b|c` alternation in `token`.
        fn first_alternative(token: &str) -> Option<String> {
            let (before, _after) = token.split_once('|')?;
            let start = before
                .rfind([' ', ':', '<', '='])
                .map_or(0, |index| index.saturating_add(1));
            let word = before.get(start..)?;
            (!word.is_empty()).then(|| word.to_owned())
        }

        let name = token
            .trim_matches(['<', '>'])
            .split([':', ' ', '='])
            .next()
            .unwrap_or(token);
        if token.contains("x,y,z,s") || name.contains("rotation") {
            return "<0,0,0,1>".to_owned();
        }
        if token.contains("-vec")
            || token.contains("x,y,z")
            || name.contains("position")
            || name.contains("offset")
            || name.starts_with("ray_")
        {
            return "<1,2,3>".to_owned();
        }
        if token.contains("hex") {
            return "deadbeef".to_owned();
        }
        if let Some(alternative) = first_alternative(token) {
            return alternative;
        }
        // A region-local id is a small integer, not a UUID, so it has to be
        // caught before the `…id` suffix rule below.
        if name.contains("local") || name.contains("spawn_index") {
            return "1".to_owned();
        }
        if name.ends_with("id") || name.ends_with("ids") {
            return "11111111-1111-1111-1111-111111111111".to_owned();
        }
        if token.contains("bool") {
            return "true".to_owned();
        }
        "1".to_owned()
    }

    /// A line for the named command with a guessed value for each required
    /// (`<…>`) argument of its usage, in order.
    fn sample_line(name: &str, usage: &str) -> String {
        let mut line = name.to_owned();
        let mut rest = usage;
        while let Some((before, open)) = rest.split_once('<') {
            let Some(close) = open.find('>') else { break };
            let token = open.get(..close).unwrap_or_default();
            // Only a `<…>` that does not sit inside a `[…]` is required.
            if !before.ends_with('[') && !before.contains("=[") {
                line.push(' ');
                line.push_str(&sample_value(token));
            }
            rest = open.get(close.saturating_add(1)..).unwrap_or_default();
        }
        line
    }

    #[test]
    fn every_build_closure_runs_and_fails_only_on_its_arguments() -> Result<(), String> {
        let mut built = 0_usize;
        let mut rejected = Vec::new();
        for spec in scanned_specs()? {
            let line = sample_line(&spec.name, &spec.usage);
            let Ok(Some(ReplAction::Command(pending))) = parse_line(&line) else {
                return Err(format!(
                    "the generated line for `{}` does not parse: {line}",
                    spec.name
                ));
            };
            match Registry::shared().build(&pending, &CircuitContext) {
                Ok(command) => {
                    built = built.saturating_add(1);
                    let rendered = format_command(&command, &CircuitContext);
                    assert!(
                        rendered.starts_with(&spec.name),
                        "`{line}` built a command the formatter calls \
                         {rendered:?}, not `{}` — the two tables disagree for a \
                         real value, not just by name",
                        spec.name
                    );
                }
                // An argument the guesser could not supply is expected; a build
                // function failing for any other reason is not.
                Err(
                    ReplError::MissingArg { .. }
                    | ReplError::InvalidArg { .. }
                    | ReplError::Unresolved(_)
                    | ReplError::NotSupported(..),
                ) => rejected.push(spec.name),
                Err(other) => {
                    return Err(format!(
                        "`{line}` failed for a non-argument reason: {other}"
                    ));
                }
            }
        }
        // 300 of the 352 build from their own usage hint; the rest need a value
        // the guesser cannot invent (an enum with no alternatives spelled out, a
        // colon-separated record). The floor is a backstop against the generator
        // silently degrading into a no-op, not a coverage target.
        assert!(
            built > 280,
            "only {built} of the {} registered commands built from their own \
             usage hints; the generator is broken, not the registry (rejected: \
             {rejected:?})",
            built.saturating_add(rejected.len())
        );
        Ok(())
    }
}
