//! Build script: reads the vendored `keywords_lsl_default.xml` — Linden Lab's
//! own `LSLSyntax` document, as Second Life serves it — and generates the
//! library table into `OUT_DIR`: the `BuiltinId` enum, the `BUILTINS`,
//! `CONSTANTS` and `EVENTS` descriptor arrays, and one `Signature` type per
//! function, which is what makes a hand-written implementation with the wrong
//! arity or argument types fail to compile.
//!
//! The served document leaves out functions the grid still compiles; those
//! come from `keywords_lsl_unserved.xml`, a document of the same format
//! holding only a `functions` map, kept apart so a refresh of the served one
//! stays a copy. A function in both stops the build: the grid serves it
//! again, and the supplement's entry should go.
//!
//! OpenSim's engines compile more: the OSSL (`os*`), LightShare (`ls*`) and
//! MOD (`mod*`) families and a few `ll*` functions and constants Second Life
//! has not got. They come from OpenSim's own served document,
//! `opensim/ScriptSyntax.xml`, and its supplement `opensim/ossl-unserved.xml`,
//! each entry marked with its family so the compiler can refuse it on Second
//! Life; an OSSL function also carries the permission check OpenSim makes
//! before it runs, read from `opensim/ossl-checks.txt` (derived from
//! `OSSL_Api.cs` by `opensim/extract_ossl_checks.py`). Where both documents
//! name a function or constant, Second Life's entry is the table's.
//!
//! The generated file is `include!`d by `src/library/generated.rs`, which
//! carries the lint relaxations (inner attributes cannot come through
//! `include!`).

use std::collections::HashMap;
use std::path::PathBuf;

use sl_llsd::{Llsd, parse_llsd_xml};

/// The vendored document, at the crate root.
const SOURCE: &str = "keywords_lsl_default.xml";

/// The functions the grid compiles and [`SOURCE`] does not list, at the
/// crate root.
const SUPPLEMENT: &str = "keywords_lsl_unserved.xml";

/// OpenSim's served document, its first line the syntax id.
const OPENSIM: &str = "opensim/ScriptSyntax.xml";

/// The functions OpenSim compiles and [`OPENSIM`] does not list.
const OPENSIM_SUPPLEMENT: &str = "opensim/ossl-unserved.xml";

/// Each OSSL function's permission check.
const OSSL_CHECKS: &str = "opensim/ossl-checks.txt";

/// A boxed error: this is a build script, and any failure should stop the
/// build with a message.
type BoxError = Box<dyn std::error::Error>;

/// Entry point: read the document, generate code, write it to `OUT_DIR`.
fn main() -> Result<(), BoxError> {
    println!("cargo:rerun-if-changed={SOURCE}");
    println!("cargo:rerun-if-changed={SUPPLEMENT}");
    println!("cargo:rerun-if-changed={OPENSIM}");
    println!("cargo:rerun-if-changed={OPENSIM_SUPPLEMENT}");
    println!("cargo:rerun-if-changed={OSSL_CHECKS}");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let read = |name: &str| fs_err::read_to_string(manifest_dir.join(name));
    let document = parse_llsd_xml(&read(SOURCE)?)?;
    let supplement = parse_llsd_xml(&read(SUPPLEMENT)?)?;
    let opensim_file = read(OPENSIM)?;
    let (_id, opensim_text) = opensim_file
        .split_once('\n')
        .ok_or_else(|| format!("{OPENSIM}: no syntax id line"))?;
    let opensim = parse_llsd_xml(&number_overloads(opensim_text))?;
    let opensim_supplement = parse_llsd_xml(&read(OPENSIM_SUPPLEMENT)?)?;
    let checks = ossl_checks(&read(OSSL_CHECKS)?)?;
    let code = generate(&Sources {
        document: &document,
        supplement: &supplement,
        opensim: &opensim,
        opensim_supplement: &opensim_supplement,
        checks: &checks,
    })?;

    let out_dir = std::env::var("OUT_DIR")?;
    fs_err::write(PathBuf::from(out_dir).join("library.rs"), code)?;
    Ok(())
}

/// Appends formatted text to `out`; writing to a `String` cannot fail.
fn emit(out: &mut String, args: core::fmt::Arguments<'_>) {
    use core::fmt::Write as _;
    out.write_fmt(args).unwrap_or_default();
}

/// OpenSim's document lists each overload of a function under the same key
/// (`osApproxEquals` six times), which a map keeps only one of: this names
/// the second and later entries of the `functions` map `name#2`, `name#3`,
/// … so each survives as an entry of its own. The first keeps its name.
fn number_overloads(text: &str) -> String {
    let Some(functions) = text.find("<key>functions</key>") else {
        return text.to_owned();
    };
    let (head, tail) = text.split_at(functions);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = head.to_owned();
    // A function's own key is the only `<key>` at one space of indentation.
    for (index, line) in tail.split_inclusive('\n').enumerate() {
        let name = line
            .strip_prefix(" <key>")
            .and_then(|rest| rest.strip_suffix("</key>\n"))
            .filter(|_| index > 0);
        match name {
            Some(name) => {
                let count = seen.entry(name.to_owned()).or_insert(0);
                *count = count.saturating_add(1);
                if *count > 1 {
                    emit(&mut out, format_args!(" <key>{name}#{count}</key>\n"));
                } else {
                    out.push_str(line);
                }
            }
            None => out.push_str(line),
        }
    }
    out
}

/// One argument of a function or event, as read from the document.
struct Argument {
    /// The documented parameter name.
    name: String,
    /// The type keyword.
    ty: String,
}

/// One library function, as read from the document.
struct Function {
    /// The `ll*` name.
    name: String,
    /// Which overload of the name it is, from 1 (OpenSim's functions may
    /// have several; Second Life's have one).
    overload: usize,
    /// The return type keyword, or `None` for `void`.
    ret: Option<String>,
    /// The ordered arguments.
    args: Vec<Argument>,
    /// The forced delay after the call, in seconds.
    sleep: f64,
    /// The energy cost (kept for the syntax document only).
    energy: f64,
    /// The tooltip text.
    tooltip: String,
    /// Whether the document flags it deprecated.
    deprecated: bool,
    /// Whether the document flags it god-mode only.
    god_mode: bool,
    /// The generated `Family` variant: whose library it is.
    family: &'static str,
    /// The generated `Gate` expression: what OpenSim checks before it runs.
    gate: String,
    /// What a deprecated OSSL function's warning names in its place.
    replacement: Option<String>,
}

/// Everything the table is generated from.
struct Sources<'a> {
    /// Second Life's served document.
    document: &'a Llsd,
    /// Second Life's unserved functions.
    supplement: &'a Llsd,
    /// OpenSim's served document.
    opensim: &'a Llsd,
    /// OpenSim's unserved functions.
    opensim_supplement: &'a Llsd,
    /// Each OSSL function's check, by name.
    checks: &'a HashMap<String, Check>,
}

/// One line of [`OSSL_CHECKS`].
struct Check {
    /// `none`, `enabled` or a threat level.
    kind: String,
    /// The name the check looks the function's `Allow_` line up under.
    checked: String,
    /// What a deprecated function's warning names in its place.
    replacement: Option<String>,
}

/// The threat levels `OSSL_Api` names, lowest first.
const THREAT_LEVELS: [&str; 9] = [
    "NoAccess", "None", "Nuisance", "VeryLow", "Low", "Moderate", "High", "VeryHigh", "Severe",
];

/// Reads [`OSSL_CHECKS`]: `<function> <check> [<checked name>] [<replacement>]`
/// a line, `#` comments.
fn ossl_checks(text: &str) -> Result<HashMap<String, Check>, BoxError> {
    let mut checks = HashMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (name, kind) = match fields.as_slice() {
            [name, kind, ..] if fields.len() <= 4 => (*name, *kind),
            _ => return Err(format!("{OSSL_CHECKS}: `{line}`").into()),
        };
        if kind != "none" && kind != "enabled" && !THREAT_LEVELS.contains(&kind) {
            return Err(format!("{OSSL_CHECKS}: {name}: check `{kind}`").into());
        }
        let checked = fields.get(2).copied().unwrap_or(name).to_owned();
        let replacement = fields.get(3).map(|text| (*text).to_owned());
        let _old = checks.insert(
            name.to_owned(),
            Check {
                kind: kind.to_owned(),
                checked,
                replacement,
            },
        );
    }
    Ok(checks)
}

/// The family a function OpenSim adds belongs to, by its prefix.
fn opensim_family(name: &str) -> Result<&'static str, BoxError> {
    Ok(if name.starts_with("ll") {
        "OpenSim"
    } else if name.starts_with("os") {
        "Ossl"
    } else if name.starts_with("ls") {
        "LightShare"
    } else if name.starts_with("mod") {
        "Mod"
    } else {
        return Err(format!("{name}: no OpenSim family has that prefix").into());
    })
}

/// The `Gate` expression for a function of `family` named `name`.
fn gate(family: &str, name: &str, checks: &HashMap<String, Check>) -> Result<String, BoxError> {
    Ok(match family {
        "Ossl" => {
            let check = checks
                .get(name)
                .ok_or_else(|| format!("{name}: no line in {OSSL_CHECKS}"))?;
            match check.kind.as_str() {
                "none" => "Gate::Unchecked".to_owned(),
                "enabled" => "Gate::Enabled".to_owned(),
                level => format!(
                    "Gate::Threat {{ level: ThreatLevel::{level}, name: {:?} }}",
                    check.checked
                ),
            }
        }
        "LightShare" => "Gate::LightShare".to_owned(),
        "Mod" => "Gate::Mod".to_owned(),
        _ => "Gate::Unchecked".to_owned(),
    })
}

/// A group of the document as a name-sorted list of entries, so the
/// generated order (and every index into it) is stable.
fn sorted_group<'a>(document: &'a Llsd, group: &str) -> Result<Vec<(&'a str, &'a Llsd)>, BoxError> {
    let map = document
        .get(group)
        .and_then(Llsd::as_map)
        .ok_or_else(|| format!("no `{group}` map"))?;
    let mut entries: Vec<(&str, &Llsd)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    Ok(entries)
}

/// The text of a string member, or empty.
fn text(entry: &Llsd, key: &str) -> String {
    entry
        .get(key)
        .and_then(Llsd::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// A boolean flag member (absent is false).
fn flag(entry: &Llsd, key: &str) -> bool {
    matches!(entry.get(key), Some(Llsd::Boolean(true)))
}

/// The ordered `arguments` array: single-key maps `{ name: { type, … } }`.
/// An absent or `undef` array is no arguments.
fn arguments(owner: &str, entry: &Llsd) -> Result<Vec<Argument>, BoxError> {
    let Some(list) = entry.get("arguments").and_then(Llsd::as_array) else {
        return Ok(Vec::new());
    };
    let mut args = Vec::new();
    for item in list {
        let map = item
            .as_map()
            .ok_or_else(|| format!("{owner}: an argument is not a map"))?;
        let (name, detail) = map
            .iter()
            .next()
            .filter(|_| map.len() == 1)
            .ok_or_else(|| format!("{owner}: an argument map must have one key"))?;
        let ty = text(detail, "type");
        type_name(&ty).ok_or_else(|| format!("{owner}: argument `{name}` has type `{ty}`"))?;
        args.push(Argument {
            name: name.clone(),
            ty,
        });
    }
    Ok(args)
}

/// The `sl_lsl::ast::TypeName` variant a type keyword names.
fn type_name(keyword: &str) -> Option<&'static str> {
    Some(match keyword {
        "integer" => "Integer",
        "float" => "Float",
        "string" => "String",
        "key" => "Key",
        "vector" => "Vector",
        "rotation" | "quaternion" => "Rotation",
        "list" => "List",
        _ => return None,
    })
}

/// The Rust type a hand-written implementation takes or returns for a type
/// keyword (`None`, `void`, is `()`).
fn rust_type(keyword: Option<&str>) -> &'static str {
    match keyword {
        Some("integer") => "i32",
        Some("float") => "f32",
        Some("string") => "String",
        Some("key") => "Key",
        Some("vector") => "Vector",
        Some("rotation" | "quaternion") => "Rotation",
        Some("list") => "Vec<Element>",
        _ => "()",
    }
}

/// `llGetListLength` → `LlGetListLength`.
fn variant_name(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A number as a Rust expression of the exact `f32` it rounds to, spelled
/// by its bits so no literal is ever re-rounded (or trips a precision lint).
fn f32_expr(value: f64) -> String {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "rounding to single precision is exactly what the table stores"
    )]
    let single = value as f32;
    format!("f32::from_bits(0x{:08x})", single.to_bits())
}

/// The `functions` map of a supplement, which holds nothing else.
fn supplement_functions<'a>(
    supplement: &'a Llsd,
    what: &str,
) -> Result<&'a HashMap<String, Llsd>, BoxError> {
    let map = supplement
        .as_map()
        .ok_or_else(|| format!("{what}: not a map"))?;
    if let Some(other) = map.keys().find(|key| *key != "functions") {
        return Err(format!("{what}: `{other}` — it holds functions only").into());
    }
    supplement
        .get("functions")
        .and_then(Llsd::as_map)
        .ok_or_else(|| format!("{what}: no `functions` map").into())
}

/// One function read from `entry`.
fn function(
    name: &str,
    entry: &Llsd,
    family: &'static str,
    checks: &HashMap<String, Check>,
) -> Result<Function, BoxError> {
    let ret = match text(entry, "return").as_str() {
        "" | "void" => None,
        other => {
            type_name(other).ok_or_else(|| format!("{name}: return type `{other}`"))?;
            Some(other.to_owned())
        }
    };
    let (name, overload) = match name.split_once('#') {
        Some((name, overload)) => (name, overload.parse()?),
        None => (name, 1),
    };
    let replacement = if family == "Ossl" {
        checks.get(name).and_then(|check| check.replacement.clone())
    } else {
        None
    };
    Ok(Function {
        name: name.to_owned(),
        overload,
        ret,
        args: arguments(name, entry)?,
        sleep: entry.get("sleep").and_then(Llsd::as_f64).unwrap_or(0.0),
        energy: entry.get("energy").and_then(Llsd::as_f64).unwrap_or(0.0),
        tooltip: text(entry, "tooltip"),
        deprecated: flag(entry, "deprecated") || replacement.is_some(),
        god_mode: flag(entry, "god-mode"),
        family,
        gate: gate(family, name, checks)?,
        replacement,
    })
}

/// Reads every function: Second Life's served document's and its
/// supplement's, then those OpenSim's add, sorted by name.
fn functions(sources: &Sources<'_>) -> Result<Vec<Function>, BoxError> {
    let served = sorted_group(sources.document, "functions")?;
    let unserved = supplement_functions(sources.supplement, SUPPLEMENT)?;
    if let Some(name) = unserved
        .keys()
        .find(|name| served.iter().any(|(served, _entry)| served == name))
    {
        return Err(format!("{name} is in {SOURCE} now; remove it from {SUPPLEMENT}").into());
    }
    let mut out = Vec::new();
    for (name, entry) in served
        .into_iter()
        .chain(unserved.iter().map(|(name, entry)| (name.as_str(), entry)))
    {
        out.push(function(name, entry, "Linden", sources.checks)?);
    }

    let opensim_served = sorted_group(sources.opensim, "functions")?;
    let opensim_unserved = supplement_functions(sources.opensim_supplement, OPENSIM_SUPPLEMENT)?;
    if let Some(name) = opensim_unserved
        .keys()
        .find(|name| opensim_served.iter().any(|(served, _entry)| served == name))
    {
        return Err(
            format!("{name} is in {OPENSIM} now; remove it from {OPENSIM_SUPPLEMENT}").into(),
        );
    }
    for (name, entry) in opensim_served.into_iter().chain(
        opensim_unserved
            .iter()
            .map(|(name, entry)| (name.as_str(), entry)),
    ) {
        let base = name.split('#').next().unwrap_or(name);
        if out
            .iter()
            .any(|function| function.family == "Linden" && function.name == base)
        {
            continue;
        }
        out.push(function(
            name,
            entry,
            opensim_family(name)?,
            sources.checks,
        )?);
    }
    out.sort_by(|a, b| (&a.name, a.overload).cmp(&(&b.name, b.overload)));
    Ok(out)
}

/// The `BuiltinId` variant of a function: `llGetListLength` →
/// `LlGetListLength`, its second overload `…2`.
fn function_variant(function: &Function) -> String {
    if function.overload > 1 {
        format!("{}{}", variant_name(&function.name), function.overload)
    } else {
        variant_name(&function.name)
    }
}

/// A constant's value as the raw text the document serves (a string, an
/// integer, a real or a UUID member).
fn raw_value(entry: &Llsd) -> Option<String> {
    match entry.get("value")? {
        Llsd::String(text) => Some(text.clone()),
        Llsd::Integer(value) => Some(value.to_string()),
        Llsd::Real(value) => Some(value.to_string()),
        Llsd::Uuid(value) => Some(value.to_string()),
        _ => None,
    }
}

/// A string constant's value. The document spells a character no XML text
/// can carry as an escape behind two backslashes — `\\n` (`EOF`, `NAK`),
/// `\\x15` (`NAK`), `\\ufdd0` (the `JSON_*` markers) — and every other value
/// literally. Any other escape stops the build rather than being misread.
fn string_value(name: &str, raw: &str) -> Result<String, BoxError> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('\\') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let escape = rest
            .get(at..)
            .and_then(|tail| tail.strip_prefix("\\\\"))
            .ok_or_else(|| format!("{name}: `{raw}`: a backslash that is not `\\\\`"))?;
        // The escape's width: its letter, then the hex digits after it.
        let (character, width) = if escape.starts_with('n') {
            ('\n', 1)
        } else {
            let width = match escape.get(..1) {
                Some("x") => 3,
                Some("u") => 5,
                _ => {
                    return Err(format!("{name}: `{raw}`: an escape this build cannot read").into());
                }
            };
            let hex = escape
                .get(1..width)
                .ok_or_else(|| format!("{name}: `{raw}`: a short escape"))?;
            let code = u32::from_str_radix(hex, 16)?;
            let character =
                char::from_u32(code).ok_or_else(|| format!("{name}: bad code point"))?;
            (character, width)
        };
        out.push(character);
        rest = escape.get(width..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

/// An integer constant: decimal or `0x` hex, possibly negative; hex is a
/// 32-bit pattern (`0x80000000` is `-2147483648`).
fn integer_value(name: &str, raw: &str) -> Result<i32, BoxError> {
    let (negative, digits) = raw
        .strip_prefix('-')
        .map_or((false, raw), |rest| (true, rest));
    let magnitude = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16)?.cast_signed()
    } else {
        digits
            .parse::<i32>()
            .map_err(|error| format!("{name}: `{raw}`: {error}"))?
    };
    Ok(if negative {
        magnitude.wrapping_neg()
    } else {
        magnitude
    })
}

/// `<a, b, c>` (or four components) as floats.
fn components(name: &str, raw: &str, count: usize) -> Result<Vec<f64>, BoxError> {
    let inner = raw
        .trim()
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .ok_or_else(|| format!("{name}: `{raw}` is not <…>"))?;
    let values = inner
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != count {
        return Err(format!("{name}: `{raw}` needs {count} components").into());
    }
    Ok(values)
}

/// The generated `ConstantValue` expression for one constant, or `None` for
/// an entry with no type (the document lists `default` among the constants
/// for highlighting only).
fn constant_value(name: &str, entry: &Llsd) -> Result<Option<String>, BoxError> {
    let ty = text(entry, "type");
    if ty.is_empty() {
        return Ok(None);
    }
    let raw = raw_value(entry).ok_or_else(|| format!("{name}: no value"))?;
    let expr = match ty.as_str() {
        "integer" => format!("ConstantValue::Integer({})", integer_value(name, &raw)?),
        "float" => format!("ConstantValue::Float({})", f32_expr(raw.parse::<f64>()?)),
        "string" => format!("ConstantValue::String({:?})", string_value(name, &raw)?),
        "key" => format!("ConstantValue::Key({raw:?})"),
        "vector" | "rotation" => {
            let count = if ty == "vector" { 3 } else { 4 };
            let parts: Vec<String> = components(name, &raw, count)?
                .into_iter()
                .map(f32_expr)
                .collect();
            let variant = if ty == "vector" { "Vector" } else { "Rotation" };
            format!("ConstantValue::{variant}([{}])", parts.join(", "))
        }
        other => return Err(format!("{name}: constant type `{other}`").into()),
    };
    Ok(Some(expr))
}

/// The `&[Argument { … }]` slice literal for an argument list.
fn argument_slice(args: &[Argument]) -> String {
    let items: Vec<String> = args
        .iter()
        .map(|arg| {
            format!(
                "Argument {{ name: {:?}, ty: TypeName::{} }}",
                arg.name,
                type_name(&arg.ty).unwrap_or("Integer")
            )
        })
        .collect();
    format!("&[{}]", items.join(", "))
}

/// Generates the whole include file.
fn generate(sources: &Sources<'_>) -> Result<String, BoxError> {
    let functions = functions(sources)?;
    let mut out = String::new();
    emit(
        &mut out,
        format_args!(
            "// Generated by build.rs from {SOURCE}, {SUPPLEMENT}, {OPENSIM}, \
             {OPENSIM_SUPPLEMENT} and {OSSL_CHECKS}; do not edit.\n\n\
             use sl_lsl::ast::TypeName;\n\
             use sl_types::lsl::{{Rotation, Vector}};\n\n\
             use crate::library::table::{{\n    \
             Argument, Builtin, Constant, ConstantValue, Event, Family, Gate, ThreatLevel,\n}};\n\
             use crate::library::dispatch::{{Key, Signature}};\n\
             use crate::value::Element;\n\n"
        ),
    );

    // The id enum.
    emit(
        &mut out,
        format_args!(
            "/// One library function, by name — an index into [`BUILTINS`].\n\
             #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]\n\
             pub enum BuiltinId {{\n"
        ),
    );
    for function in &functions {
        emit(
            &mut out,
            format_args!(
                "    /// `{}`{}{}.\n    {},\n",
                function.name,
                if function.overload > 1 {
                    format!(", overload {}", function.overload)
                } else {
                    String::new()
                },
                if function.deprecated {
                    " (deprecated)"
                } else {
                    ""
                },
                function_variant(function)
            ),
        );
    }
    emit(&mut out, format_args!("}}\n\nimpl BuiltinId {{\n"));
    emit(
        &mut out,
        format_args!(
            "    /// Every function, in [`BUILTINS`] order.\n    pub const ALL: [Self; {}] = [\n",
            functions.len()
        ),
    );
    for function in &functions {
        emit(
            &mut out,
            format_args!("        Self::{},\n", function_variant(function)),
        );
    }
    emit(
        &mut out,
        format_args!(
            "    ];\n\n    /// This function's index into [`BUILTINS`].\n    \
             #[must_use]\n    pub const fn index(self) -> usize {{\n        match self {{\n"
        ),
    );
    for (index, function) in functions.iter().enumerate() {
        emit(
            &mut out,
            format_args!(
                "            Self::{} => {index},\n",
                function_variant(function)
            ),
        );
    }
    emit(
        &mut out,
        format_args!(
            "        }}\n    }}\n\n    /// This function's descriptor.\n    #[must_use]\n    \
             pub fn descriptor(self) -> &'static Builtin {{\n        &BUILTINS[self.index()]\n    }}\n}}\n\n"
        ),
    );

    // The descriptors.
    emit(
        &mut out,
        format_args!(
            "/// Every library function's descriptor, sorted by name.\n\
             pub static BUILTINS: [Builtin; {}] = [\n",
            functions.len()
        ),
    );
    for function in &functions {
        let ret = function
            .ret
            .as_deref()
            .and_then(type_name)
            .map_or_else(|| "None".to_owned(), |ty| format!("Some(TypeName::{ty})"));
        emit(
            &mut out,
            format_args!(
                "    Builtin {{\n        id: BuiltinId::{},\n        name: {:?},\n        \
                 args: {},\n        ret: {ret},\n        sleep: {},\n        energy: {},\n        \
                 deprecated: {},\n        god_mode: {},\n        tooltip: {:?},\n        \
                 family: Family::{},\n        gate: {},\n        replacement: {:?},\n    }},\n",
                function_variant(function),
                function.name,
                argument_slice(&function.args),
                f32_expr(function.sleep),
                f32_expr(function.energy),
                function.deprecated,
                function.god_mode,
                function.tooltip,
                function.family,
                function.gate,
                function.replacement,
            ),
        );
    }
    emit(&mut out, format_args!("];\n\n"));

    // Constants.
    let mut constants = Vec::new();
    for (name, entry) in sorted_group(sources.document, "constants")? {
        if let Some(value) = constant_value(name, entry)? {
            constants.push((name, value, text(entry, "tooltip"), false));
        }
    }
    for (name, entry) in sorted_group(sources.opensim, "constants")? {
        if constants.iter().any(|(known, ..)| *known == name) {
            continue;
        }
        if let Some(value) = constant_value(name, entry)? {
            constants.push((name, value, text(entry, "tooltip"), true));
        }
    }
    constants.sort_by(|a, b| a.0.cmp(b.0));
    emit(
        &mut out,
        format_args!(
            "/// Every library constant, sorted by name.\npub static CONSTANTS: [Constant; {}] = [\n",
            constants.len()
        ),
    );
    for (name, value, tooltip, opensim_only) in &constants {
        emit(
            &mut out,
            format_args!(
                "    Constant {{ name: {name:?}, value: {value}, tooltip: {tooltip:?}, \
                 opensim_only: {opensim_only} }},\n"
            ),
        );
    }
    emit(&mut out, format_args!("];\n\n"));

    // Events.
    let events = sorted_group(sources.document, "events")?;
    emit(
        &mut out,
        format_args!(
            "/// Every event, sorted by name.\npub static EVENTS: [Event; {}] = [\n",
            events.len()
        ),
    );
    for (name, entry) in &events {
        emit(
            &mut out,
            format_args!(
                "    Event {{ name: {name:?}, args: {}, tooltip: {:?} }},\n",
                argument_slice(&arguments(name, entry)?),
                text(entry, "tooltip")
            ),
        );
    }
    emit(&mut out, format_args!("];\n\n"));

    // One signature type per function.
    emit(
        &mut out,
        format_args!(
            "/// One type per library function, stating its argument and return types\n\
             /// as the Rust types an implementation takes and returns.\n\
             pub mod signatures {{\n    use super::*;\n\n"
        ),
    );
    for function in &functions {
        let variant = function_variant(function);
        let args: Vec<&str> = function
            .args
            .iter()
            .map(|arg| rust_type(Some(&arg.ty)))
            .collect();
        let tuple = if args.len() == 1 {
            format!("({},)", args.join(""))
        } else {
            format!("({})", args.join(", "))
        };
        emit(
            &mut out,
            format_args!(
                "    /// The signature of `{name}`.\n    #[derive(Debug)]\n    pub struct {variant};\n\n    \
                 impl Signature for {variant} {{\n        type Args = {tuple};\n        \
                 type Ret = {};\n        const ID: BuiltinId = BuiltinId::{variant};\n    }}\n\n",
                rust_type(function.ret.as_deref()),
                name = function.name,
            ),
        );
    }
    emit(&mut out, format_args!("}}\n"));

    // Every name must map to a distinct variant.
    let mut seen: HashMap<String, &str> = HashMap::new();
    for function in &functions {
        if let Some(other) = seen.insert(function_variant(function), &function.name) {
            return Err(format!("{} and {other} share a variant name", function.name).into());
        }
    }
    Ok(out)
}
