//! Build script: reads the vendored `keywords_lsl_default.xml` — Linden Lab's
//! own `LSLSyntax` document, as the viewer ships it — and generates the
//! library table into `OUT_DIR`: the `BuiltinId` enum, the `BUILTINS`,
//! `CONSTANTS` and `EVENTS` descriptor arrays, and one `Signature` type per
//! function, which is what makes a hand-written implementation with the wrong
//! arity or argument types fail to compile.
//!
//! The generated file is `include!`d by `src/library/generated.rs`, which
//! carries the lint relaxations (inner attributes cannot come through
//! `include!`).

use std::collections::HashMap;
use std::path::PathBuf;

use sl_llsd::{Llsd, parse_llsd_xml};

/// The vendored document, at the crate root.
const SOURCE: &str = "keywords_lsl_default.xml";

/// A boxed error: this is a build script, and any failure should stop the
/// build with a message.
type BoxError = Box<dyn std::error::Error>;

/// Entry point: read the document, generate code, write it to `OUT_DIR`.
fn main() -> Result<(), BoxError> {
    println!("cargo:rerun-if-changed={SOURCE}");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let source = fs_err::read_to_string(PathBuf::from(&manifest_dir).join(SOURCE))?;
    let document = parse_llsd_xml(&source)?;
    let code = generate(&document)?;

    let out_dir = std::env::var("OUT_DIR")?;
    fs_err::write(PathBuf::from(out_dir).join("library.rs"), code)?;
    Ok(())
}

/// Appends formatted text to `out`; writing to a `String` cannot fail.
fn emit(out: &mut String, args: core::fmt::Arguments<'_>) {
    use core::fmt::Write as _;
    out.write_fmt(args).unwrap_or_default();
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
}

/// A group of the document as a name-sorted list of entries, so the
/// generated order (and every index into it) is stable.
fn sorted_group<'a>(document: &'a Llsd, group: &str) -> Result<Vec<(&'a str, &'a Llsd)>, BoxError> {
    let map = document
        .get(group)
        .and_then(Llsd::as_map)
        .ok_or_else(|| format!("{SOURCE}: no `{group}` map"))?;
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

/// Reads every function.
fn functions(document: &Llsd) -> Result<Vec<Function>, BoxError> {
    let mut out = Vec::new();
    for (name, entry) in sorted_group(document, "functions")? {
        let ret = match text(entry, "return").as_str() {
            "" | "void" => None,
            other => {
                type_name(other).ok_or_else(|| format!("{name}: return type `{other}`"))?;
                Some(other.to_owned())
            }
        };
        out.push(Function {
            name: name.to_owned(),
            ret,
            args: arguments(name, entry)?,
            sleep: entry.get("sleep").and_then(Llsd::as_f64).unwrap_or(0.0),
            energy: entry.get("energy").and_then(Llsd::as_f64).unwrap_or(0.0),
            tooltip: text(entry, "tooltip"),
            deprecated: flag(entry, "deprecated"),
            god_mode: flag(entry, "god-mode"),
        });
    }
    Ok(out)
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

/// String constants the document spells in a notation rather than
/// literally, with the raw text expected — so a changed source stops the
/// build instead of being misread — and the value it stands for.
const STRING_OVERRIDES: [(&str, &str, &str); 1] = [("EOF", "\\\\n\\\\n\\\\n", "\n\n\n")];

/// A string constant's value: an override, a `U+XXXX` code point (the
/// `JSON_*` markers), or the text itself.
fn string_value(name: &str, raw: &str) -> Result<String, BoxError> {
    if let Some((_, expected, value)) = STRING_OVERRIDES.iter().find(|(n, ..)| *n == name) {
        if raw != *expected {
            return Err(format!("{name}: raw value `{raw}` changed; review the override").into());
        }
        return Ok((*value).to_owned());
    }
    if let Some(hex) = raw.strip_prefix("U+") {
        let code = u32::from_str_radix(hex, 16)?;
        let character = char::from_u32(code).ok_or_else(|| format!("{name}: bad code point"))?;
        return Ok(character.to_string());
    }
    Ok(raw.to_owned())
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
fn generate(document: &Llsd) -> Result<String, BoxError> {
    let functions = functions(document)?;
    let mut out = String::new();
    emit(
        &mut out,
        format_args!(
            "// Generated by build.rs from {SOURCE}; do not edit.\n\n\
             use sl_lsl::ast::TypeName;\n\
             use sl_types::lsl::{{Rotation, Vector}};\n\n\
             use crate::library::table::{{Argument, Builtin, Constant, ConstantValue, Event}};\n\
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
                "    /// `{}`{}.\n    {},\n",
                function.name,
                if function.deprecated {
                    " (deprecated)"
                } else {
                    ""
                },
                variant_name(&function.name)
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
            format_args!("        Self::{},\n", variant_name(&function.name)),
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
                variant_name(&function.name)
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
                 deprecated: {},\n        god_mode: {},\n        tooltip: {:?},\n    }},\n",
                variant_name(&function.name),
                function.name,
                argument_slice(&function.args),
                f32_expr(function.sleep),
                f32_expr(function.energy),
                function.deprecated,
                function.god_mode,
                function.tooltip,
            ),
        );
    }
    emit(&mut out, format_args!("];\n\n"));

    // Constants.
    let mut constants = Vec::new();
    for (name, entry) in sorted_group(document, "constants")? {
        if let Some(value) = constant_value(name, entry)? {
            constants.push((name, value, text(entry, "tooltip")));
        }
    }
    emit(
        &mut out,
        format_args!(
            "/// Every library constant, sorted by name.\npub static CONSTANTS: [Constant; {}] = [\n",
            constants.len()
        ),
    );
    for (name, value, tooltip) in &constants {
        emit(
            &mut out,
            format_args!(
                "    Constant {{ name: {name:?}, value: {value}, tooltip: {tooltip:?} }},\n"
            ),
        );
    }
    emit(&mut out, format_args!("];\n\n"));

    // Events.
    let events = sorted_group(document, "events")?;
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
        let variant = variant_name(&function.name);
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
        if let Some(other) = seen.insert(variant_name(&function.name), &function.name) {
            return Err(format!("{} and {other} share a variant name", function.name).into());
        }
    }
    Ok(out)
}
