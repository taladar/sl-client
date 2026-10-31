//! Second Life's `llJson*`: a lenient scanner that keeps the text.
//!
//! The base is LSL PyOptimizer's `lsljson.py`, a deliberate reimplementation
//! of Linden's dialect written because the standard parsers disagree with it:
//! its tokenizer, its `llJsonGetValue` walk (which checks the syntax of the
//! level it is in and skips deeper ones by bracket matching), `llJson2List`
//! and `llList2Json`. Where aditi answered `data/json` otherwise, the probe
//! wins, and the module says so at each place:
//!
//! - **A string ends at the first `"` not preceded by `\`.** So a string
//!   whose text ends in an escaped backslash (`"a\\"`) does not end there,
//!   and runs on to the end of the document: `["A\n\/\"\\"]` reads as one
//!   string from its opening quote to the end of the text, `]` and all.
//!   Escapes are decoded, but only the one-letter ones (`\b \t \n \f \r`);
//!   any other escaped character is itself, so `\"` is `"` and `A` is
//!   `u0041` (`["😀"]` reads `ud83dude00`).
//! - **A number may not start with a needless zero** (`01`, `-01`, `0e0` are
//!   not numbers; `0`, `-0`, `0.5` are), as in PyOptimizer's variant of the
//!   pattern for BUG-6466 fixed — but an exponent still takes no `+`, as in
//!   the variant it keeps for today's grid. `1.`, `.5`, `1E-1.2` are numbers.
//! - **A word ends at `:` too**, not only at `,`, `]` and `}`: so `{'a':1}`
//!   is invalid rather than a pair of an empty key and the text `'a':1`.
//! - **`llJson2List` is lenient in an array**: the end of the text closes it
//!   (after a string that ran on), and a value with no comma before it takes
//!   the place of the one before (`["a" "b"]` is `["b"]`); a word is a string
//!   element (`[truth]`).
//! - **`llJsonSetValue`**, which PyOptimizer leaves unimplemented, is written
//!   from the probe's rows and LL's own test script (the wiki's
//!   `Json_usage_in_LSL/TestScript`, which PyOptimizer carries as
//!   `unit_tests/json.py`): see [`set_value`].

use sl_lsl::ast::TypeName;

use super::{
    JSON_ARRAY, JSON_DELETE, JSON_FALSE, JSON_INVALID, JSON_NULL, JSON_NUMBER, JSON_OBJECT,
    JSON_STRING, JSON_TRUE,
};
use crate::value::{Element, Value};

/// The kinds of token the scanner reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `,`.
    Comma,
    /// `:`.
    Colon,
    /// `{`, not yet matched to its `}`.
    OpenObject,
    /// `}`.
    CloseObject,
    /// `[`, not yet matched to its `]`.
    OpenArray,
    /// `]`.
    CloseArray,
    /// A string, quotes and all.
    String,
    /// A number, as Linden's pattern reads one.
    Number,
    /// The word `null`.
    Null,
    /// The word `true`.
    True,
    /// The word `false`.
    False,
    /// Anything else: a word, or a string that never ends.
    Invalid,
    /// A whole object, `{` to its matching `}`.
    Object,
    /// A whole array, `[` to its matching `]`.
    Array,
}

impl Kind {
    /// The sentinel `llJsonValueType` answers for a value of this kind.
    const fn sentinel(self) -> &'static str {
        match self {
            Self::String => JSON_STRING,
            Self::Number => JSON_NUMBER,
            Self::Null => JSON_NULL,
            Self::True => JSON_TRUE,
            Self::False => JSON_FALSE,
            Self::Object | Self::OpenObject => JSON_OBJECT,
            Self::Array | Self::OpenArray => JSON_ARRAY,
            Self::Comma | Self::Colon | Self::CloseObject | Self::CloseArray | Self::Invalid => {
                JSON_INVALID
            }
        }
    }

    /// Whether the token opens a container.
    const fn opens(self) -> bool {
        matches!(self, Self::OpenObject | Self::OpenArray)
    }
}

/// One token: where it starts and ends in the text (byte offsets, which are
/// always at ASCII characters or the end), and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Token {
    /// The offset of its first byte.
    start: usize,
    /// The offset just past its last byte.
    end: usize,
    /// What it is.
    kind: Kind,
}

impl Token {
    /// The token's text in `json`.
    fn text(self, json: &str) -> &str {
        json.get(self.start..self.end).unwrap_or_default()
    }
}

/// Whether `byte` is white space to the scanner: the six ASCII characters
/// `llStringTrim` trims.
const fn is_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
}

/// `text` without its leading and trailing ASCII white space
/// (`llStringTrim(…, STRING_TRIM)` on Second Life).
fn trim(text: &str) -> &str {
    text.trim_matches(|character: char| u8::try_from(character).is_ok_and(is_space))
}

/// `text` without its trailing ASCII white space.
fn trim_end(text: &str) -> &str {
    text.trim_end_matches(|character: char| u8::try_from(character).is_ok_and(is_space))
}

/// The byte at `at`, if there is one.
fn byte(bytes: &[u8], at: usize) -> Option<u8> {
    bytes.get(at).copied()
}

/// The offset just past the run of ASCII digits starting at `at`.
fn digits(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    while byte(bytes, end).is_some_and(|byte| byte.is_ascii_digit()) {
        end = end.saturating_add(1);
    }
    end
}

/// The number starting at `at`, if one does: the offset just past it, and
/// whether it is an integer (digits after an optional minus, nothing else).
///
/// The pattern is `lsljson.py`'s `jsonnum_re` — an optional minus, then
/// either digits, an `E`, an optional minus, digits, an optional point and
/// digits; or digits, an optional point, digits, an optional `E` with an
/// optional minus, digits — behind the guard of its BUG-6466-fixed variant,
/// that what follows the minus is a digit 1–9, a point not followed by `e`,
/// or a `0` followed by neither a digit nor `e`.
fn number_end(bytes: &[u8], at: usize) -> Option<(usize, bool)> {
    let start = if byte(bytes, at) == Some(b'-') {
        at.saturating_add(1)
    } else {
        at
    };
    let after = byte(bytes, start.saturating_add(1));
    let guarded = match byte(bytes, start)? {
        b'1'..=b'9' => true,
        b'.' => after != Some(b'e'),
        b'0' => !matches!(after, Some(b'0'..=b'9' | b'e')),
        _ => false,
    };
    if !guarded {
        return None;
    }
    let whole = digits(bytes, start);
    // The first form: an exponent straight after the digits.
    if matches!(byte(bytes, whole), Some(b'E' | b'e')) {
        let mut end = whole.saturating_add(1);
        if byte(bytes, end) == Some(b'-') {
            end = end.saturating_add(1);
        }
        end = digits(bytes, end);
        if byte(bytes, end) == Some(b'.') {
            end = end.saturating_add(1);
        }
        return Some((digits(bytes, end), false));
    }
    // The second form: a point, digits, an exponent, digits, each optional.
    let mut end = whole;
    if byte(bytes, end) == Some(b'.') {
        end = end.saturating_add(1);
    }
    end = digits(bytes, end);
    if matches!(byte(bytes, end), Some(b'E' | b'e')) {
        end = end.saturating_add(1);
        if byte(bytes, end) == Some(b'-') {
            end = end.saturating_add(1);
        }
    }
    end = digits(bytes, end);
    Some((end, end == whole))
}

/// The offset just past the string whose opening quote is at `at`: the
/// first `"` after it not preceded by `\` — or [`None`] when there is no
/// such quote and the string runs on to the end.
fn string_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut previous = b'"';
    let mut index = at.saturating_add(1);
    while let Some(current) = byte(bytes, index) {
        if current == b'"' && previous != b'\\' {
            return Some(index.saturating_add(1));
        }
        previous = current;
        index = index.saturating_add(1);
    }
    None
}

/// The token at or after `from`, white space skipped; [`None`] at the end of
/// the text.
fn token(json: &str, from: usize) -> Option<Token> {
    let bytes = json.as_bytes();
    let mut start = from;
    while byte(bytes, start).is_some_and(is_space) {
        start = start.saturating_add(1);
    }
    let first = byte(bytes, start)?;
    let next = start.saturating_add(1);
    let punctuation = match first {
        b',' => Some(Kind::Comma),
        b':' => Some(Kind::Colon),
        b'{' => Some(Kind::OpenObject),
        b'}' => Some(Kind::CloseObject),
        b'[' => Some(Kind::OpenArray),
        b']' => Some(Kind::CloseArray),
        _ => None,
    };
    if let Some(kind) = punctuation {
        return Some(Token {
            start,
            end: next,
            kind,
        });
    }
    if first == b'"' {
        return Some(string_end(bytes, start).map_or(
            Token {
                start,
                end: bytes.len(),
                kind: Kind::Invalid,
            },
            |end| Token {
                start,
                end,
                kind: Kind::String,
            },
        ));
    }
    if let Some((end, _integer)) = number_end(bytes, start) {
        return Some(Token {
            start,
            end,
            kind: Kind::Number,
        });
    }
    // A word: up to the next `,`, `:`, `]` or `}`, its trailing space
    // dropped.
    let mut stop = start;
    while byte(bytes, stop).is_some_and(|byte| !matches!(byte, b',' | b':' | b']' | b'}')) {
        stop = stop.saturating_add(1);
    }
    let word = trim_end(json.get(start..stop).unwrap_or_default());
    let kind = match word {
        "null" => Kind::Null,
        "true" => Kind::True,
        "false" => Kind::False,
        _ => Kind::Invalid,
    };
    Some(Token {
        start,
        end: start.saturating_add(word.len()),
        kind,
    })
}

/// The offset just past the `}` or `]` matching the `{` or `[` at `at`,
/// skipping strings as [`string_end`] reads them; [`None`] when it is never
/// matched. Nothing inside is checked.
fn matching(json: &str, at: usize) -> Option<usize> {
    let bytes = json.as_bytes();
    let open = byte(bytes, at)?;
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 1_u32;
    let mut in_string = false;
    let mut previous = open;
    let mut index = at.saturating_add(1);
    while let Some(current) = byte(bytes, index) {
        if in_string {
            if current == b'"' && previous != b'\\' {
                in_string = false;
            }
        } else if current == b'"' {
            in_string = true;
        } else if current == open {
            depth = depth.saturating_add(1);
        } else if current == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index.saturating_add(1));
            }
        }
        previous = current;
        index = index.saturating_add(1);
    }
    None
}

/// [`token`], with a `{` or `[` that is matched read as the whole container.
fn token_full(json: &str, from: usize) -> Option<Token> {
    let found = token(json, from)?;
    let kind = match found.kind {
        Kind::OpenObject => Kind::Object,
        Kind::OpenArray => Kind::Array,
        _ => return Some(found),
    };
    Some(matching(json, found.start).map_or(found, |end| Token {
        start: found.start,
        end,
        kind,
    }))
}

/// A string's text, its quotes (the first and last characters) dropped and
/// its escapes decoded: `\b \t \n \f \r` as their characters, any other
/// escaped character as itself, and a backslash left at the end kept
/// (`lsljson.py`'s `InternalJsonUnquote`).
fn unquote(quoted: &str) -> String {
    let mut inner = quoted.chars();
    let _open = inner.next();
    let _close = inner.next_back();
    let mut out = String::new();
    let mut escaped = false;
    for character in inner {
        if escaped {
            out.push(match character {
                'b' => '\u{08}',
                't' => '\t',
                'n' => '\n',
                'f' => '\u{0C}',
                'r' => '\r',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            out.push(character);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}

/// Whether `text` is quoted: at least two characters, the first and last a
/// `"`.
fn is_quoted(text: &str) -> bool {
    text.len() > 1 && text.starts_with('"') && text.ends_with('"')
}

/// `text` as a JSON string: quoted, with backspace, tab, newline, form feed,
/// carriage return, `"`, `/` and `\` escaped — and nothing else, so a
/// control character or a non-ASCII one goes in as it is.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for character in text.chars() {
        match character {
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '/' => out.push_str("\\/"),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Whether all of `text` is one number.
fn is_number(text: &str) -> bool {
    number_end(text.as_bytes(), 0).is_some_and(|(end, _integer)| end == text.len())
}

/// A float as the library writes one into JSON: C's `%.6f` of its exact
/// value (`1.0e20` is `100000002004087734272.000000`, not Mono's seven
/// digits), `inf`, `-inf` and `nan` — none of which is JSON.
fn float(value: f32) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    format!("{:.6}", f64::from(value))
}

/// One list element as JSON (`lsljson.py`'s `InternalElement2Json`): a
/// string by [`string_value`] (`numbers` says whether text that is a number
/// goes in as one, as for `llJsonSetValue` but not `llList2Json`), a key
/// quoted as it is, a vector or rotation quoted in its `%.6f` form, a float
/// by [`float`], an integer in decimal.
fn element(element: &Element, numbers: bool) -> String {
    match element {
        Element::String(text) => string_value(text, numbers),
        Element::Key(text) => format!("\"{text}\""),
        Element::Integer(integer) => integer.to_string(),
        Element::Float(value) => float(*value),
        Element::Vector(vector) => format!(
            "\"<{}, {}, {}>\"",
            float(vector.x),
            float(vector.y),
            float(vector.z)
        ),
        Element::Rotation(rotation) => format!(
            "\"<{}, {}, {}, {}>\"",
            float(rotation.x),
            float(rotation.y),
            float(rotation.z),
            float(rotation.s)
        ),
    }
}

/// A string as JSON. Trimmed first; then empty is `""`, the sentinels of
/// `null`, `true` and `false` and those words are themselves, text that is
/// already quoted or bracketed (`[` … `]`, `{` … `}`) goes in as written —
/// whether or not it is JSON — and so does a number when `numbers` says so;
/// `JSON_INVALID` writes nothing at all (`[,"…"]`, `{"b":}`); anything else
/// is quoted.
fn string_value(text: &str, numbers: bool) -> String {
    let text = trim(text);
    if text.is_empty() {
        return "\"\"".to_owned();
    }
    match text {
        _ if text == JSON_NULL => return "null".to_owned(),
        _ if text == JSON_TRUE => return "true".to_owned(),
        _ if text == JSON_FALSE => return "false".to_owned(),
        "null" | "true" | "false" => return text.to_owned(),
        _ => {}
    }
    let bracketed = (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with('{') && text.ends_with('}'));
    if is_quoted(text) || bracketed || (numbers && is_number(text)) {
        return text.to_owned();
    }
    if text == JSON_INVALID {
        return String::new();
    }
    quote(text)
}

/// `llList2Json`.
pub(super) fn list2_json(kind: &str, values: &[Element]) -> String {
    if kind == JSON_ARRAY {
        let items: Vec<String> = values.iter().map(|value| element(value, false)).collect();
        return format!("[{}]", items.join(","));
    }
    if kind != JSON_OBJECT || !values.len().is_multiple_of(2) {
        return JSON_INVALID.to_owned();
    }
    let mut pairs = Vec::with_capacity(values.len() / 2);
    let (pairs_of, _rest) = values.as_chunks::<2>();
    for pair in pairs_of {
        let [Element::String(key), value] = pair else {
            return JSON_INVALID.to_owned();
        };
        pairs.push(format!("{}:{}", quote(key), element(value, false)));
    }
    format!("{{{}}}", pairs.join(","))
}

/// The most digits a number may have and still be read as written — the
/// rest are clamped (`lsljson.py`: "surely overflown").
const INTEGER_DIGITS: usize = 11;

/// One element of `llJson2List`'s answer from its text
/// (`lsljson.py`'s `InternalJson2Elem`): the words' sentinels, an integer
/// clamped to the integer's range, a float as LSL casts the text, a quoted
/// string's text, or the text itself.
fn list_element(text: &str) -> Element {
    match text {
        "" => return Element::String(String::new()),
        "null" => return Element::String(JSON_NULL.to_owned()),
        "false" => return Element::String(JSON_FALSE.to_owned()),
        "true" => return Element::String(JSON_TRUE.to_owned()),
        _ => {}
    }
    if let Some((end, integer)) = number_end(text.as_bytes(), 0)
        && end == text.len()
    {
        if integer {
            let negative = text.starts_with('-');
            let clamped = if text.len() > INTEGER_DIGITS {
                None
            } else {
                text.parse::<i64>().ok()
            };
            let value = clamped.map_or(if negative { i32::MIN } else { i32::MAX }, |wide| {
                i32::try_from(wide).unwrap_or(if wide < 0 { i32::MIN } else { i32::MAX })
            });
            return Element::Integer(value);
        }
        if let Ok(Value::Float(value)) =
            crate::cast(Value::String(text.to_owned()), TypeName::Float)
        {
            return Element::Float(value);
        }
    }
    if is_quoted(text) {
        return Element::String(unquote(text));
    }
    Element::String(text.to_owned())
}

/// `[JSON_INVALID]`, what `llJson2List` answers a document it gives up on.
fn invalid_list() -> Vec<Element> {
    vec![Element::String(JSON_INVALID.to_owned())]
}

/// Whether `found` closes the container `json` is: the closing bracket of
/// `kind` at the end of the text, or the end of the text itself.
fn closes(json: &str, found: Option<Token>, kind: Kind) -> bool {
    found.is_none_or(|token| token.kind == kind && token.end == json.len())
}

/// `llJson2List`.
pub(super) fn json2_list(json: &str) -> Vec<Element> {
    let json = trim(json);
    if json.is_empty() {
        return Vec::new();
    }
    if json.starts_with('[') && json.ends_with(']') {
        return array_list(json);
    }
    if json.starts_with('{') && json.ends_with('}') {
        return object_list(json);
    }
    vec![list_element(json)]
}

/// `llJson2List` of a document that starts with `[` and ends with `]`.
fn array_list(json: &str) -> Vec<Element> {
    let mut out = Vec::new();
    let mut found = token_full(json, 1);
    if closes(json, found, Kind::CloseArray) {
        return out;
    }
    match found {
        Some(Token {
            kind: Kind::Colon, ..
        }) => return invalid_list(),
        Some(Token {
            kind: Kind::Comma, ..
        }) => out.push(Element::String(String::new())),
        Some(first) => {
            out.push(list_element(first.text(json)));
            found = token_full(json, first.end);
        }
        None => return out,
    }
    loop {
        if closes(json, found, Kind::CloseArray) {
            return out;
        }
        let Some(current) = found else {
            return out;
        };
        match current.kind {
            Kind::Comma => {}
            Kind::Colon => return invalid_list(),
            // A value with no comma before it takes the place of the one
            // before (`["a" "b"]` is `["b"]` on aditi).
            _ => {
                let _replaced = out.pop();
                out.push(list_element(current.text(json)));
                found = token_full(json, current.end);
                continue;
            }
        }
        found = token_full(json, current.end);
        let empty = found.is_some_and(|next| next.kind == Kind::Comma)
            || closes(json, found, Kind::CloseArray);
        if empty {
            out.push(Element::String(String::new()));
            continue;
        }
        let Some(value) = found else {
            return out;
        };
        if value.kind == Kind::Colon {
            return invalid_list();
        }
        out.push(list_element(value.text(json)));
        found = token_full(json, value.end);
    }
}

/// `llJson2List` of a document that starts with `{` and ends with `}`: the
/// grammar of `lsljson.py`, which takes a last pair without a key
/// (`{"a"}` is `["", "a"]`) and a trailing comma (two empty elements).
fn object_list(json: &str) -> Vec<Element> {
    let mut out = Vec::new();
    let mut found = token_full(json, 1);
    if closes(json, found, Kind::CloseObject) {
        return out;
    }
    if found.is_some_and(|first| matches!(first.kind, Kind::Colon | Kind::Comma)) {
        return invalid_list();
    }
    loop {
        if closes(json, found, Kind::CloseObject) {
            out.push(Element::String(String::new()));
            out.push(Element::String(String::new()));
            return out;
        }
        let Some(mut current) = found else {
            return out;
        };
        let mut key = String::new();
        if current.kind == Kind::String {
            let colon = token_full(json, current.end);
            if let Some(colon) = colon.filter(|colon| colon.kind == Kind::Colon) {
                key = unquote(current.text(json));
                let Some(value) = token_full(json, colon.end) else {
                    return invalid_list();
                };
                current = value;
            }
        }
        if matches!(current.kind, Kind::Comma | Kind::Colon) {
            return invalid_list();
        }
        let keyless = key.is_empty();
        out.push(Element::String(key));
        out.push(list_element(current.text(json)));
        found = token_full(json, current.end);
        if closes(json, found, Kind::CloseObject) {
            return out;
        }
        let Some(comma) = found.filter(|comma| comma.kind == Kind::Comma && !keyless) else {
            return invalid_list();
        };
        found = token_full(json, comma.end);
    }
}

/// The key of the value the walk is at: an index in an array, a name in an
/// object.
#[derive(Debug, Clone, PartialEq, Eq)]
enum At {
    /// The index in an array.
    Index(i32),
    /// The (unquoted) name in an object.
    Name(String),
}

/// Whether the key `at` is the one the specifier `spec` names; [`None`] when
/// the two cannot be compared, which makes the whole call `JSON_INVALID`
/// (`lsljson.py`'s `InternalJsonPathMatches`): a name against an integer, an
/// index against a string with no digits at its start, anything against a
/// float, vector or rotation. A string reads as an index by the up to nine
/// digits it starts with — `["b", "0"]` reads `b`'s first element.
fn names(at: &At, spec: &Element) -> Option<bool> {
    match (at, spec) {
        (At::Index(index), Element::Integer(wanted)) => Some(index == wanted),
        (At::Name(name), Element::String(wanted) | Element::Key(wanted)) => Some(name == wanted),
        (At::Index(index), Element::String(wanted) | Element::Key(wanted)) => {
            let lead: String = wanted
                .chars()
                .take_while(char::is_ascii_digit)
                .take(9)
                .collect();
            let wanted: i32 = lead.parse().ok()?;
            Some(*index == wanted)
        }
        _ => None,
    }
}

/// The value the empty path names: the document, when it is one value — a
/// scalar alone, or a container whose last character (trailing space
/// dropped) closes it; nothing inside is checked.
fn whole(json: &str) -> Option<Token> {
    let first = token(json, 0)?;
    if first.kind.opens() {
        let trimmed = trim_end(json);
        let (close, kind) = if first.kind == Kind::OpenObject {
            ('}', Kind::Object)
        } else {
            (']', Kind::Array)
        };
        return trimmed.ends_with(close).then_some(Token {
            start: first.start,
            end: trimmed.len(),
            kind,
        });
    }
    let scalar = matches!(
        first.kind,
        Kind::Number | Kind::String | Kind::Null | Kind::True | Kind::False | Kind::Invalid
    );
    (scalar && token(json, first.end).is_none()).then_some(first)
}

/// The token of the value `path` names in `json` (`lsljson.py`'s
/// `InternalJsonFindValue`), or [`None`] for `JSON_INVALID`.
///
/// One pass over the text: every level the walk enters is checked as it is
/// read (keys are strings followed by `:`, values are separated by commas,
/// containers close), while a value the path does not enter is skipped by
/// bracket matching alone, unchecked. A repeated key names its last value.
fn find(json: &str, path: &[Element]) -> Option<Token> {
    if path.is_empty() {
        return whole(json);
    }
    let first = token(json, 0)?;
    if !first.kind.opens() {
        return None;
    }
    // How many levels of the path the walk is inside; the value it found.
    let mut level = 0_usize;
    let mut found = None;
    let mut open = vec![first.kind];
    let mut at = At::Index(0);
    let mut just_opened = true;
    let mut just_closed = false;
    let mut current = token(json, first.end);
    loop {
        let entering_empty = just_opened
            && current
                .is_some_and(|close| matches!(close.kind, Kind::CloseObject | Kind::CloseArray));
        if !(just_closed || entering_empty) {
            just_opened = false;
            let mut value = current?;
            if matches!(
                value.kind,
                Kind::Comma | Kind::Colon | Kind::CloseObject | Kind::CloseArray | Kind::Invalid
            ) {
                return None;
            }
            if open.last() == Some(&Kind::OpenObject) {
                if value.kind != Kind::String {
                    return None;
                }
                let colon = token(json, value.end)?;
                if colon.kind != Kind::Colon {
                    return None;
                }
                at = At::Name(unquote(value.text(json)));
                value = token(json, colon.end)?;
            }
            let entered = match path.get(level) {
                Some(spec) => names(&at, spec)?,
                None => false,
            };
            if entered {
                level = level.saturating_add(1);
                found = None;
                if level == path.len() {
                    if value.kind.opens() {
                        let end = matching(json, value.start)?;
                        value = Token {
                            start: value.start,
                            end,
                            kind: if value.kind == Kind::OpenObject {
                                Kind::Object
                            } else {
                                Kind::Array
                            },
                        };
                    }
                    found = Some(value);
                    level = level.saturating_sub(1);
                } else if value.kind.opens() {
                    open.push(value.kind);
                    at = At::Index(0);
                    just_opened = true;
                    current = token(json, value.end);
                    continue;
                }
            } else if value.kind.opens() {
                // A value the path does not enter is skipped whole.
                value.end = matching(json, value.start)?;
            }
            current = token(json, value.end);
        }
        just_closed = false;
        let kind = current.map(|next| next.kind);
        if !just_opened && kind == Some(Kind::Comma) {
            current = token(json, current?.end);
            if open.last() == Some(&Kind::OpenArray)
                && let At::Index(index) = &mut at
            {
                *index = index.saturating_add(1);
            }
            continue;
        }
        let closing = matches!(
            (kind, open.last()),
            (Some(Kind::CloseObject), Some(Kind::OpenObject))
                | (Some(Kind::CloseArray), Some(Kind::OpenArray))
        );
        if !closing {
            return None;
        }
        let _closed = open.pop();
        level = level.saturating_sub(1);
        let close = current?;
        if open.is_empty() {
            return token(json, close.end).is_none().then_some(found?);
        }
        just_closed = true;
        current = token(json, close.end);
    }
}

/// `llJsonGetValue`.
pub(super) fn get_value(json: &str, specifiers: &[Element]) -> String {
    let Some(value) = find(json, specifiers) else {
        return JSON_INVALID.to_owned();
    };
    let text = value.text(json);
    match value.kind {
        Kind::String => unquote(text),
        Kind::Number | Kind::Object | Kind::Array => text.to_owned(),
        // A whole document that is a string never closed but still starts
        // and ends with a quote (`"a\"`) is read as one.
        Kind::Invalid if is_quoted(text) => unquote(text),
        other => other.sentinel().to_owned(),
    }
}

/// `llJsonValueType`.
pub(super) fn value_type(json: &str, specifiers: &[Element]) -> String {
    find(json, specifiers)
        .map_or(JSON_INVALID, |value| value.kind.sentinel())
        .to_owned()
}

/// One step of a path `llJsonSetValue` writes along.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// An index, `JSON_APPEND` (-1) for one past the end.
    Index(i32),
    /// A key.
    Name(String),
}

impl Step {
    /// The step a specifier names: an integer an index, a string or key a
    /// name; [`None`] for anything else, which `JSON_INVALID` answers.
    fn of(spec: &Element) -> Option<Self> {
        match spec {
            Element::Integer(index) => Some(Self::Index(*index)),
            Element::String(name) | Element::Key(name) => Some(Self::Name(name.clone())),
            Element::Float(_) | Element::Vector(_) | Element::Rotation(_) => None,
        }
    }
}

/// `JSON_APPEND`, Second Life's integer.
const APPEND: i32 = -1;

/// A document as `llJsonSetValue` reads the level it writes to: the members
/// of an object or an array, each value kept as written, or something else —
/// a scalar, an empty or broken document — which the write replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Level {
    /// An object's members: the key as written, its text, the value as
    /// written.
    Object(Vec<(String, String, String)>),
    /// An array's elements, as written.
    Array(Vec<String>),
    /// No object or array.
    Other,
}

/// A value the level reader keeps: a string, a number, a word, or a whole
/// container (unchecked inside).
fn member(json: &str, from: usize) -> Option<Token> {
    token_full(json, from).filter(|value| {
        matches!(
            value.kind,
            Kind::String
                | Kind::Number
                | Kind::Null
                | Kind::True
                | Kind::False
                | Kind::Object
                | Kind::Array
        )
    })
}

/// `json` read as one level: an object or array that closes where the text
/// does, with every member well formed; [`Level::Other`] otherwise.
fn level(json: &str) -> Level {
    let Some(first) = token(json, 0).filter(|first| first.kind.opens()) else {
        return Level::Other;
    };
    let Some(end) = matching(json, first.start) else {
        return Level::Other;
    };
    if token(json, end).is_some() {
        return Level::Other;
    }
    let close = if first.kind == Kind::OpenObject {
        Kind::CloseObject
    } else {
        Kind::CloseArray
    };
    let mut keys = Vec::new();
    let mut values = Vec::new();
    let mut at = first.end;
    let empty = token(json, at).is_some_and(|next| next.kind == close && next.end == end);
    if !empty {
        loop {
            if first.kind == Kind::OpenObject {
                let Some(key) = token(json, at).filter(|key| key.kind == Kind::String) else {
                    return Level::Other;
                };
                let Some(colon) = token(json, key.end).filter(|colon| colon.kind == Kind::Colon)
                else {
                    return Level::Other;
                };
                keys.push(key.text(json).to_owned());
                at = colon.end;
            }
            let Some(value) = member(json, at) else {
                return Level::Other;
            };
            values.push(value.text(json).to_owned());
            match token(json, value.end) {
                Some(next) if next.kind == Kind::Comma => at = next.end,
                Some(next) if next.kind == close && next.end == end => break,
                _ => return Level::Other,
            }
        }
    }
    if first.kind == Kind::OpenArray {
        return Level::Array(values);
    }
    Level::Object(
        keys.into_iter()
            .zip(values)
            .map(|(raw, value)| {
                let name = unquote(&raw);
                (raw, name, value)
            })
            .collect(),
    )
}

/// The containers `path` makes, holding `value` at its end: an index of 0
/// (or `JSON_APPEND`) makes an array, a name an object; any other index
/// cannot be made.
fn build(path: &[Step], value: &str) -> Option<String> {
    let Some((step, rest)) = path.split_first() else {
        return Some(string_value(value, true));
    };
    match step {
        Step::Index(0 | APPEND) => Some(format!("[{}]", build(rest, value)?)),
        Step::Index(_) => None,
        Step::Name(name) => Some(format!("{{{}:{}}}", quote(name), build(rest, value)?)),
    }
}

/// `llJsonSetValue` along `path` in `json`; [`None`] for `JSON_INVALID`.
fn set(json: &str, path: &[Step], value: &str) -> Option<String> {
    let delete = value == JSON_DELETE;
    let Some((step, rest)) = path.split_first() else {
        return Some(string_value(value, true));
    };
    match (level(json), step) {
        (Level::Object(mut members), Step::Name(name)) => {
            let mut hits = members
                .iter()
                .enumerate()
                .filter(|(_, (_, key, _))| key == name)
                .map(|(index, _)| index);
            let first = hits.next();
            // A repeated key becomes one pair, where it first stood.
            let later: Vec<usize> = hits.collect();
            for index in later.into_iter().rev() {
                let _dropped = members.remove(index);
            }
            match first {
                Some(index) if rest.is_empty() && delete => {
                    let _removed = members.remove(index);
                }
                Some(index) => {
                    let (_, _, old) = members.get_mut(index)?;
                    *old = if rest.is_empty() {
                        string_value(value, true)
                    } else {
                        set(old, rest, value)?
                    };
                }
                None if delete => return None,
                None => members.push((quote(name), name.clone(), build(rest, value)?)),
            }
            let written: Vec<String> = members
                .into_iter()
                .map(|(raw, _, value)| format!("{raw}:{value}"))
                .collect();
            Some(format!("{{{}}}", written.join(",")))
        }
        (Level::Array(mut elements), Step::Index(index)) => {
            let len = i32::try_from(elements.len()).unwrap_or(i32::MAX);
            if *index == APPEND || *index == len {
                if delete {
                    return None;
                }
                elements.push(build(rest, value)?);
            } else {
                let at = usize::try_from(*index)
                    .ok()
                    .filter(|at| *at < elements.len())?;
                if rest.is_empty() && delete {
                    let _removed = elements.remove(at);
                } else {
                    let old = elements.get_mut(at)?;
                    *old = if rest.is_empty() {
                        string_value(value, true)
                    } else {
                        set(old, rest, value)?
                    };
                }
            }
            Some(format!("[{}]", elements.join(",")))
        }
        // An index on an object, a name on an array, or no container at all:
        // what is there is replaced by what the path makes.
        _ if delete => None,
        _ => build(path, value),
    }
}

/// `llJsonSetValue`: what aditi and LL's test script answer.
///
/// - The path is followed through the levels that are an object or an array
///   ([`level`]); each level written to is written anew, compact, around the
///   members it keeps as they were written (`{"a" : 1 , "c" : [ 1 ] }` with
///   `a` set to 2 is `{"a":2,"c":[ 1 ]}`).
/// - A name that is not there is added at the end, an index equal to the
///   length or `JSON_APPEND` appends, with the containers the rest of the
///   path needs made on the way (an index of 0 an array, a name an object);
///   any other index past the end, or a negative one, is `JSON_INVALID`.
/// - What stands where the path goes on but is no container, an index on an
///   object, a name on an array, and a document that is no object or array
///   — empty, a word, broken (`[1`) — is replaced by what the path makes.
/// - `JSON_DELETE` removes the member the path names, and is `JSON_INVALID`
///   when it is not there.
/// - The value goes in as `llList2Json` writes a string, except that a
///   number is written as one: `"1.50"` is `1.50`, `"01"` is the string
///   `"01"`, `"[1, 2]"` is the array as written.
pub(super) fn set_value(json: &str, specifiers: &[Element], value: &str) -> String {
    if specifiers.is_empty() && value == JSON_DELETE {
        // `lsljson.py`'s special case: the empty path answers the sentinel.
        return JSON_DELETE.to_owned();
    }
    specifiers
        .iter()
        .map(Step::of)
        .collect::<Option<Vec<Step>>>()
        .and_then(|path| set(json, &path, value))
        .unwrap_or_else(|| JSON_INVALID.to_owned())
}

#[cfg(test)]
mod tests;
