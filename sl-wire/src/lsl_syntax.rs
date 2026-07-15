//! The **`LSLSyntax`** capability: decode (and build) the grid's LSL language
//! definition.
//!
//! `SimulatorFeatures` advertises an `LSLSyntaxId`; a plain GET on the
//! `LSLSyntax` capability returns the document it identifies — an LLSD map
//! tagged `llsd-lsl-syntax-version: 2` with five groups (**functions**,
//! **constants**, **events**, **controls**, **types**). This module turns that
//! document into the owned [`LslSyntax`] symbol table `sl-lsl` defines, and
//! builds the inverse (server side / round-trip). The schema and its keys are
//! cross-checked against Firestorm's `llkeywords.cpp` (`processTokensGroup`,
//! `getArguments`) and `llsyntaxid.cpp` (the version gate), and against
//! OpenSim's `bin/ScriptSyntax.xml`.
//!
//! The decoder is **version-gated**: a document declaring any version other than
//! [`LSL_SYNTAX_VERSION`] is refused ([`WireError::UnsupportedLslSyntaxVersion`])
//! rather than parsed against a schema it may not match — the version is bumped
//! only when the *layout* changes, not when the grid's function list does. Every
//! other field is lenient: an absent optional key takes its default, and a type
//! keyword that is not one of LSL's seven decodes to [`None`] rather than
//! discarding the whole entry, because the document is grid-served and older or
//! customised grids vary.
//!
//! An **empty top-level document is not treated as a decode error** — a grid may
//! legitimately serve fewer groups — so the caller decides whether an empty
//! table means "use a shipped default" (see the fetch/cache layer).

use std::collections::HashMap;

use sl_llsd::Llsd;
use sl_lsl::ast::TypeName;
use sl_lsl::{
    LSL_SYNTAX_VERSION, LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax,
};

use crate::WireError;

/// The LLSD key carrying the document's schema version.
const VERSION_KEY: &str = "llsd-lsl-syntax-version";

/// Reads a `deprecated` / `god-mode` boolean flag from an entry map. The grid
/// serves these two ways — a real boolean (`SimulatorFeaturesModule`-style) or
/// the string `"true"` (Firestorm reads every attribute as a string and compares
/// `== "true"`) — so both are accepted; anything else, and absence, is `false`.
fn flag(map: &Llsd, key: &str) -> bool {
    match map.get(key) {
        Some(Llsd::Boolean(value)) => *value,
        Some(Llsd::String(text)) => text.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// Reads an optional numeric cost (`energy` / `sleep`) from a function entry.
/// Second Life serves these as numbers; a grid that renders them as a decimal
/// string is tolerated too. A value that is neither decodes to [`None`], as does
/// an absent key.
fn cost(map: &Llsd, key: &str) -> Option<f32> {
    match map.get(key) {
        Some(value @ (Llsd::Real(_) | Llsd::Integer(_))) => value.as_f32(),
        Some(Llsd::String(text)) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

/// Renders a constant's `value` member as text, preserving the grid's own
/// formatting. Almost always already a string (`"0x1000"`, `"<0., 0., 0.>"`);
/// a numeric encoding is rendered so callers need not special-case it. Absent
/// decodes to [`None`].
fn value_text(map: &Llsd) -> Option<String> {
    match map.get("value") {
        Some(Llsd::String(text)) => Some(text.clone()),
        Some(Llsd::Integer(number)) => Some(number.to_string()),
        Some(Llsd::Real(number)) => Some(number.to_string()),
        Some(Llsd::Boolean(boolean)) => Some(boolean.to_string()),
        _ => None,
    }
}

/// Reads a `tooltip` string member, if present and of string kind.
fn tooltip(map: &Llsd) -> Option<String> {
    match map.get("tooltip") {
        Some(Llsd::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// Classifies a bare type-keyword string into a [`TypeName`], or [`None`] for an
/// empty / unrecognised keyword (`void` returns, or a keyword an unusual grid
/// adds).
fn type_of(map: &Llsd, key: &str) -> Option<TypeName> {
    map.get(key)
        .and_then(Llsd::as_str)
        .and_then(TypeName::from_keyword)
}

/// Decodes an `arguments` array — an ordered array of single-key maps
/// `{ name: { type, tooltip? } }` — into an ordered [`LslArgument`] list. A
/// non-array (or absent) `arguments` yields an empty list; a malformed element
/// is skipped rather than failing the whole entry, keeping the decoder tolerant
/// of a grid that ships an odd row.
fn arguments(entry: &Llsd) -> Vec<LslArgument> {
    let Some(array) = entry.get("arguments").and_then(Llsd::as_array) else {
        return Vec::new();
    };
    let mut arguments = Vec::new();
    for element in array {
        let Some(fields) = element.as_map() else {
            continue;
        };
        // Each element is a single-key map keyed by the argument name; iterate
        // its entries (there is one in practice) so the argument name is
        // captured verbatim.
        for (name, detail) in fields {
            let (arg_type, arg_tooltip) = match detail {
                Llsd::Map(_) => (type_of(detail, "type"), tooltip(detail)),
                _ => (None, None),
            };
            arguments.push(LslArgument {
                name: name.clone(),
                arg_type,
                tooltip: arg_tooltip,
            });
        }
    }
    arguments
}

/// Decodes the `functions` group.
fn functions(group: &Llsd) -> HashMap<String, LslFunction> {
    let Some(map) = group.as_map() else {
        return HashMap::new();
    };
    let mut functions = HashMap::new();
    for (name, entry) in map {
        if !matches!(entry, Llsd::Map(_)) {
            continue;
        }
        let _previous = functions.insert(
            name.clone(),
            LslFunction {
                return_type: type_of(entry, "return"),
                arguments: arguments(entry),
                energy: cost(entry, "energy"),
                sleep: cost(entry, "sleep"),
                tooltip: tooltip(entry),
                deprecated: flag(entry, "deprecated"),
                god_mode: flag(entry, "god-mode"),
            },
        );
    }
    functions
}

/// Decodes the `constants` group.
fn constants(group: &Llsd) -> HashMap<String, LslConstant> {
    let Some(map) = group.as_map() else {
        return HashMap::new();
    };
    let mut constants = HashMap::new();
    for (name, entry) in map {
        if !matches!(entry, Llsd::Map(_)) {
            continue;
        }
        let _previous = constants.insert(
            name.clone(),
            LslConstant {
                constant_type: type_of(entry, "type"),
                value: value_text(entry),
                tooltip: tooltip(entry),
                deprecated: flag(entry, "deprecated"),
                god_mode: flag(entry, "god-mode"),
            },
        );
    }
    constants
}

/// Decodes the `events` group.
fn events(group: &Llsd) -> HashMap<String, LslEvent> {
    let Some(map) = group.as_map() else {
        return HashMap::new();
    };
    let mut events = HashMap::new();
    for (name, entry) in map {
        if !matches!(entry, Llsd::Map(_)) {
            continue;
        }
        let _previous = events.insert(
            name.clone(),
            LslEvent {
                arguments: arguments(entry),
                tooltip: tooltip(entry),
                deprecated: flag(entry, "deprecated"),
                god_mode: flag(entry, "god-mode"),
            },
        );
    }
    events
}

/// Decodes a bare-keyword group (`controls` or `types`) — each entry carries at
/// most a tooltip and the two flags.
fn keywords(group: &Llsd) -> HashMap<String, LslKeyword> {
    let Some(map) = group.as_map() else {
        return HashMap::new();
    };
    let mut keywords = HashMap::new();
    for (name, entry) in map {
        if !matches!(entry, Llsd::Map(_)) {
            continue;
        }
        let _previous = keywords.insert(
            name.clone(),
            LslKeyword {
                tooltip: tooltip(entry),
                deprecated: flag(entry, "deprecated"),
                god_mode: flag(entry, "god-mode"),
            },
        );
    }
    keywords
}

/// Decodes an `LSLSyntax` GET reply into the [`LslSyntax`] symbol table.
///
/// The document is **version-gated**: a `llsd-lsl-syntax-version` other than
/// [`LSL_SYNTAX_VERSION`] (or an absent version key) is refused. Past the gate
/// every field is lenient — absent groups are empty, an unrecognised type
/// keyword decodes to [`None`], and a malformed row is skipped — so a grid that
/// serves a partial or slightly-off document still yields a usable table.
///
/// # Errors
/// Returns [`WireError::UnsupportedLslSyntaxVersion`] if the document declares an
/// unimplemented (or absent) schema version, or [`WireError::Llsd`] if the
/// version key is present but of a non-integer LLSD kind.
pub fn parse_lsl_syntax(body: &Llsd) -> Result<LslSyntax, WireError> {
    match body.field_i32(VERSION_KEY, VERSION_KEY)? {
        Some(version) if version == LSL_SYNTAX_VERSION => {}
        Some(version) => {
            return Err(WireError::UnsupportedLslSyntaxVersion {
                version,
                expected: LSL_SYNTAX_VERSION,
            });
        }
        None => {
            return Err(WireError::UnsupportedLslSyntaxVersion {
                version: -1,
                expected: LSL_SYNTAX_VERSION,
            });
        }
    }
    let group = |key: &str| body.get(key).cloned().unwrap_or(Llsd::Undef);
    Ok(LslSyntax {
        functions: functions(&group("functions")),
        constants: constants(&group("constants")),
        events: events(&group("events")),
        controls: keywords(&group("controls")),
        types: keywords(&group("types")),
    })
}

// ---------------------------------------------------------------------------
// The inverse: build an `LSLSyntax` document (server side / round-trip).
// ---------------------------------------------------------------------------

/// Builds the `type` (+ optional `tooltip`) detail map for one argument.
fn argument_detail(argument: &LslArgument) -> Llsd {
    let mut detail: HashMap<String, Llsd> = HashMap::new();
    if let Some(type_name) = argument.arg_type {
        let _previous = detail.insert(
            "type".to_owned(),
            Llsd::String(type_name.keyword().to_owned()),
        );
    }
    if let Some(text) = &argument.tooltip {
        let _previous = detail.insert("tooltip".to_owned(), Llsd::String(text.clone()));
    }
    Llsd::Map(detail)
}

/// Builds an ordered `arguments` array of single-key `{ name: detail }` maps.
fn arguments_array(list: &[LslArgument]) -> Llsd {
    Llsd::Array(
        list.iter()
            .map(|argument| {
                Llsd::Map(HashMap::from([(
                    argument.name.clone(),
                    argument_detail(argument),
                )]))
            })
            .collect(),
    )
}

/// Adds the shared `tooltip` / `deprecated` / `god-mode` members to an entry
/// map when they carry non-default values (so a round-trip stays minimal).
fn put_common(
    entry: &mut HashMap<String, Llsd>,
    tooltip: Option<&str>,
    deprecated: bool,
    god_mode: bool,
) {
    if let Some(text) = tooltip {
        let _previous = entry.insert("tooltip".to_owned(), Llsd::String(text.to_owned()));
    }
    if deprecated {
        let _previous = entry.insert("deprecated".to_owned(), Llsd::Boolean(true));
    }
    if god_mode {
        let _previous = entry.insert("god-mode".to_owned(), Llsd::Boolean(true));
    }
}

/// Builds an `LSLSyntax` LLSD document from a [`LslSyntax`] — the inverse of
/// [`parse_lsl_syntax`], serialised as LLSD XML. Used by the round-trip tests
/// and available to a server that answers the `LSLSyntax` capability. The output
/// round-trips through [`parse_lsl_syntax`].
#[must_use]
pub fn build_lsl_syntax_document(syntax: &LslSyntax) -> String {
    let mut root: HashMap<String, Llsd> = HashMap::new();
    let _previous = root.insert(VERSION_KEY.to_owned(), Llsd::Integer(LSL_SYNTAX_VERSION));

    let mut functions: HashMap<String, Llsd> = HashMap::new();
    for (name, function) in &syntax.functions {
        let mut entry: HashMap<String, Llsd> = HashMap::new();
        if let Some(return_type) = function.return_type {
            let _previous = entry.insert(
                "return".to_owned(),
                Llsd::String(return_type.keyword().to_owned()),
            );
        }
        let _previous = entry.insert("arguments".to_owned(), arguments_array(&function.arguments));
        if let Some(energy) = function.energy {
            let _previous = entry.insert("energy".to_owned(), Llsd::Real(f64::from(energy)));
        }
        if let Some(sleep) = function.sleep {
            let _previous = entry.insert("sleep".to_owned(), Llsd::Real(f64::from(sleep)));
        }
        put_common(
            &mut entry,
            function.tooltip.as_deref(),
            function.deprecated,
            function.god_mode,
        );
        let _previous = functions.insert(name.clone(), Llsd::Map(entry));
    }
    let _previous = root.insert("functions".to_owned(), Llsd::Map(functions));

    let mut constants: HashMap<String, Llsd> = HashMap::new();
    for (name, constant) in &syntax.constants {
        let mut entry: HashMap<String, Llsd> = HashMap::new();
        if let Some(constant_type) = constant.constant_type {
            let _previous = entry.insert(
                "type".to_owned(),
                Llsd::String(constant_type.keyword().to_owned()),
            );
        }
        if let Some(value) = &constant.value {
            let _previous = entry.insert("value".to_owned(), Llsd::String(value.clone()));
        }
        put_common(
            &mut entry,
            constant.tooltip.as_deref(),
            constant.deprecated,
            constant.god_mode,
        );
        let _previous = constants.insert(name.clone(), Llsd::Map(entry));
    }
    let _previous = root.insert("constants".to_owned(), Llsd::Map(constants));

    let mut events: HashMap<String, Llsd> = HashMap::new();
    for (name, event) in &syntax.events {
        let mut entry: HashMap<String, Llsd> = HashMap::new();
        let _previous = entry.insert("arguments".to_owned(), arguments_array(&event.arguments));
        put_common(
            &mut entry,
            event.tooltip.as_deref(),
            event.deprecated,
            event.god_mode,
        );
        let _previous = events.insert(name.clone(), Llsd::Map(entry));
    }
    let _previous = root.insert("events".to_owned(), Llsd::Map(events));

    let build_keywords = |group: &HashMap<String, LslKeyword>| -> Llsd {
        let mut out: HashMap<String, Llsd> = HashMap::new();
        for (name, keyword) in group {
            let mut entry: HashMap<String, Llsd> = HashMap::new();
            put_common(
                &mut entry,
                keyword.tooltip.as_deref(),
                keyword.deprecated,
                keyword.god_mode,
            );
            let _previous = out.insert(name.clone(), Llsd::Map(entry));
        }
        Llsd::Map(out)
    };
    let _previous = root.insert("controls".to_owned(), build_keywords(&syntax.controls));
    let _previous = root.insert("types".to_owned(), build_keywords(&syntax.types));

    Llsd::Map(root).to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use sl_lsl::ast::TypeName;
    use sl_lsl::{LslArgument, LslConstant, LslFunction, LslKeyword, LslSyntax, SymbolKind};

    use super::{build_lsl_syntax_document, parse_lsl_syntax};
    use crate::WireError;
    use crate::llsd::parse_llsd_xml;

    /// A representative hand-built table round-trips through the builder and the
    /// parser, preserving signatures, costs, values, flags and per-argument
    /// tooltips.
    #[test]
    fn round_trip_preserves_every_group() -> Result<(), String> {
        let mut syntax = LslSyntax::default();
        let _previous = syntax.functions.insert(
            "llSetTimerEvent".to_owned(),
            LslFunction {
                return_type: None,
                arguments: vec![LslArgument {
                    name: "sec".to_owned(),
                    arg_type: Some(TypeName::Float),
                    tooltip: Some("seconds".to_owned()),
                }],
                energy: Some(10.0),
                sleep: None,
                tooltip: Some("Set a repeating timer.".to_owned()),
                deprecated: false,
                god_mode: false,
            },
        );
        let _previous = syntax.functions.insert(
            "llMakeExplosion".to_owned(),
            LslFunction {
                return_type: None,
                arguments: Vec::new(),
                energy: None,
                sleep: Some(0.1),
                tooltip: None,
                deprecated: true,
                god_mode: false,
            },
        );
        let _previous = syntax.constants.insert(
            "AGENT_ALWAYS_RUN".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some("0x1000".to_owned()),
                tooltip: None,
                deprecated: false,
                god_mode: false,
            },
        );
        let _previous = syntax.events.insert(
            "touch_start".to_owned(),
            sl_lsl::LslEvent {
                arguments: vec![LslArgument {
                    name: "num_detected".to_owned(),
                    arg_type: Some(TypeName::Integer),
                    tooltip: None,
                }],
                tooltip: Some("Triggered on touch.".to_owned()),
                deprecated: false,
                god_mode: false,
            },
        );
        let _previous = syntax.controls.insert(
            "state".to_owned(),
            LslKeyword {
                tooltip: Some("Change state.".to_owned()),
                deprecated: false,
                god_mode: true,
            },
        );
        let _previous = syntax
            .types
            .insert("vector".to_owned(), LslKeyword::default());

        let xml = build_lsl_syntax_document(&syntax);
        let decoded = parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?;
        let parsed = parse_lsl_syntax(&decoded).map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, syntax);
        Ok(())
    }

    /// A document declaring a version other than 2 is refused, and an absent
    /// version key is refused the same way (`version: -1`).
    #[test]
    fn wrong_or_missing_version_is_refused() -> Result<(), String> {
        let wrong = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>llsd-lsl-syntax-version</key><integer>3</integer>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        match parse_lsl_syntax(&wrong) {
            Err(WireError::UnsupportedLslSyntaxVersion { version, expected }) => {
                assert_eq!(version, 3);
                assert_eq!(expected, 2);
            }
            other => return Err(format!("expected version rejection, got {other:?}")),
        }

        let missing =
            parse_llsd_xml("<llsd><map></map></llsd>").map_err(|error| format!("{error:?}"))?;
        match parse_lsl_syntax(&missing) {
            Err(WireError::UnsupportedLslSyntaxVersion { version, .. }) => assert_eq!(version, -1),
            other => return Err(format!("expected missing-version rejection, got {other:?}")),
        }
        Ok(())
    }

    /// A version-2 document in the exact shape OpenSim's `ScriptSyntax.xml`
    /// serves (functions with/without a `return`, ordered typed arguments,
    /// constants with a `value`, a keyword group) decodes into a queryable table.
    #[test]
    fn open_sim_shaped_document_decodes() -> Result<(), String> {
        let body = parse_llsd_xml(concat!(
            "<llsd><map>",
            "<key>llsd-lsl-syntax-version</key><integer>2</integer>",
            "<key>functions</key><map>",
            "  <key>llAbs</key><map>",
            "    <key>return</key><string>integer</string>",
            "    <key>arguments</key><array>",
            "      <map><key>val</key><map><key>type</key><string>integer</string></map></map>",
            "    </array>",
            "  </map>",
            "  <key>llAdjustSoundVolume</key><map>",
            "    <key>arguments</key><array>",
            "      <map><key>volume</key><map><key>type</key><string>float</string></map></map>",
            "    </array>",
            "    <key>tooltip</key><string>Sleep 0.1</string>",
            "  </map>",
            "</map>",
            "<key>constants</key><map>",
            "  <key>ACTIVE</key><map>",
            "    <key>type</key><string>integer</string>",
            "    <key>value</key><string>2</string>",
            "    <key>tooltip</key><string>Objects running a script</string>",
            "  </map>",
            "</map>",
            "<key>controls</key><map>",
            "  <key>while</key><map><key>tooltip</key><string>while loop</string></map>",
            "</map>",
            "</map></llsd>"
        ))
        .map_err(|error| format!("{error:?}"))?;
        let syntax = parse_lsl_syntax(&body).map_err(|error| format!("{error:?}"))?;

        let abs = syntax.function("llAbs").ok_or("expected llAbs")?;
        assert_eq!(abs.return_type, Some(TypeName::Integer));
        assert_eq!(abs.arguments.len(), 1);
        assert_eq!(abs.arguments.first().map(|a| a.name.as_str()), Some("val"));
        assert_eq!(
            abs.arguments.first().and_then(|a| a.arg_type),
            Some(TypeName::Integer)
        );

        // A void function omits the `return` key.
        let volume = syntax
            .function("llAdjustSoundVolume")
            .ok_or("expected llAdjustSoundVolume")?;
        assert_eq!(volume.return_type, None);
        assert_eq!(volume.tooltip.as_deref(), Some("Sleep 0.1"));

        let active = syntax.constant("ACTIVE").ok_or("expected ACTIVE")?;
        assert_eq!(active.constant_type, Some(TypeName::Integer));
        assert_eq!(active.value.as_deref(), Some("2"));

        assert!(syntax.is_control("while"));
        assert_eq!(syntax.classify("llAbs"), Some(SymbolKind::Function));
        assert_eq!(syntax.classify("ACTIVE"), Some(SymbolKind::Constant));
        Ok(())
    }
}
