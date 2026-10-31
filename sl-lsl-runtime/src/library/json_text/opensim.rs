//! OpenSim's `llJson*`: `LSL_Api.cs`'s five functions over LitJson, the JSON
//! library compiled into `OpenMetaverse.StructuredData.dll`.
//!
//! Nothing here is LSL's own dialect: OpenSim parses a document into LitJson's
//! tree (`JsonMapper.ToObject`), works on the tree and writes the result anew
//! (`JsonMapper.ToJson`), so what it answers is what that library reads and
//! writes (book chapter `gridspec/lsl.md`, § llJson* and the linkset data
//! store). This module ports the library's parts those functions reach — the
//! lexer's state machine (`Lexer.cs`), the table-driven reader
//! (`JsonReader.cs`), the tree (`JsonData.cs`, whose object keeps its keys in
//! insertion order and lets a repeated key overwrite the first one's value in
//! place) and the compact writer (`JsonWriter.cs`) — from OpenSim's own fork
//! of libopenmetaverse, checked against the DLL the local OpenSim runs.
//!
//! What follows from LitJson, and differs from Second Life:
//!
//! - a document must start with `[` or `{`; a number may not have a leading
//!   zero, a bare point or a point without digits after it; `true`, `false`
//!   and `null` are lower case; strings take single quotes as well as double,
//!   decode every escape (`\u` included) and refuse an unknown one; `/* */`
//!   and `//` comments are white space;
//! - only the lexer's look-ahead checks what follows a document, so `[1] x`
//!   is refused but `[1] 2` and `[1] [` are read as `[1]`, and a number at
//!   the very end of the text is lost (`7` reads as an empty tree, while `0`
//!   and `-0` there are refused: the lexer's zero state reads past the end);
//! - a number with a point or an exponent is a `double` — one past the
//!   `double` range fails to parse and falls through to `long.TryParse`,
//!   which also fails, so `1e400` reads as the integer **0**; an integer past
//!   `int` is a `long`, one past `long` an `ulong` the tree refuses, which
//!   fails the whole read;
//! - numbers are written as .NET Framework writes a `double`: fifteen
//!   significant digits, scientific notation from `1E+15` up and `1E-05`
//!   down, and `.0` appended to one that has neither point nor exponent
//!   (`-100.0`, `Infinity.0`); every character outside printable ASCII is
//!   written as `\uXXXX`, `/` is not escaped.
//!
//! `LSL_Api.cs` adds its own: `llJsonSetValue` reads a new value with
//! `float.TryParse` (so `1,000` is a thousand, `01` is 1, `1e39` overflows to
//! a string) and rounds it to six decimals; with no specifiers it sets index 0
//! and cuts the brackets off the result (`[1,2]` set to 5 is `5,2`);
//! `llJsonGetValue` and `llJsonValueType` read a document that does not start
//! with `[` or `{` as the first element of one that does; `llList2Json`
//! inserts any string starting with `[` or `{` as it is, writes a control
//! character as `\u000` followed by the character itself (a `String.Format`
//! of a `char`), and writes an infinite float as the string `"Inf"`.
//!
//! **Strings.** LitJson's strings are .NET's, UTF-16, and a `\u` escape can
//! leave half a surrogate pair. Inside this module a JSON string is kept as
//! its UTF-16 units, so such a half survives a parse and a write (`\uD83D`
//! comes back as `\uD83D`); where one reaches an LSL string it becomes
//! U+FFFD, the runtime's reading of an unpaired half on OpenSim (`text.rs`).
//!
//! **Numbers.** OpenSim's floats are `double`s, the runtime's `f32`: an
//! [`Element::Float`] reaches the library as its `f32` value and a number
//! leaves it rounded to one, so `1.0e20` in a list is
//! `100000002004088000000.000000` here and `100000000000000000000.000000`
//! there, and `(float)"1e39"`, infinite here, is written `"Inf"`. Only the
//! value model differs; the arithmetic is the library's, in `f64`.

use crate::num::{float_to_int, int_to_float, to_f32};
use crate::value::Element;

/// `JSON_INVALID`.
const INVALID: &str = "\u{FDD0}";
/// `JSON_OBJECT`.
const OBJECT: &str = "\u{FDD1}";
/// `JSON_ARRAY`.
const ARRAY: &str = "\u{FDD2}";
/// `JSON_NUMBER`.
const NUMBER: &str = "\u{FDD3}";
/// `JSON_STRING`.
const STRING: &str = "\u{FDD4}";
/// `JSON_NULL`.
const NULL: &str = "\u{FDD5}";
/// `JSON_TRUE`.
const TRUE: &str = "\u{FDD6}";
/// `JSON_FALSE`.
const FALSE: &str = "\u{FDD7}";
/// `JSON_DELETE`.
const DELETE: &str = "\u{FDD8}";
/// `JSON_APPEND`, which OpenSim declares a **string** (Second Life's is the
/// integer -1).
const APPEND: &str = "-1";

/// Text as .NET holds it: UTF-16 code units.
type Units = Vec<u16>;

/// An exception LitJson or `LSL_Api.cs` throws; every caller catches it and
/// answers `JSON_INVALID` or the text it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Thrown;

/// A `LitJson.JsonData`; a JSON `null` is no `JsonData` at all, so a slot that
/// may hold one is a [`Node`].
#[derive(Debug, Clone, PartialEq)]
enum Json {
    /// `JsonType.None`: a `JsonData` nothing was set on — what `ToObject`
    /// answers for text with no token in it, such as `""`, `"  "` or a lone
    /// number. It writes as nothing.
    Untyped,
    /// `JsonType.Int`.
    Int(i32),
    /// `JsonType.Long`.
    Long(i64),
    /// `JsonType.Double`.
    Double(f64),
    /// `JsonType.Boolean`.
    Boolean(bool),
    /// `JsonType.String`.
    Str(Units),
    /// `JsonType.Array`.
    Array(Vec<Node>),
    /// `JsonType.Object`, its pairs in insertion order and its keys unique.
    Object(Vec<(Units, Node)>),
}

/// A slot of the tree: [`None`] for a JSON `null`.
type Node = Option<Json>;

/// `text` as UTF-16 units.
fn units(text: &str) -> Units {
    text.encode_utf16().collect()
}

/// UTF-16 units as an LSL string, an unpaired surrogate as U+FFFD.
fn string(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|decoded| decoded.unwrap_or('\u{FFFD}'))
        .collect()
}

// ---------------------------------------------------------------------------
// .NET numbers
// ---------------------------------------------------------------------------

/// .NET's white space in a number (`Number.IsWhite`): tab, line feed,
/// vertical tab, form feed, carriage return and space.
const fn is_number_space(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ')
}

/// `Double.TryParse(text)` under `en-US`, `NumberStyles.Float |
/// AllowThousands`, as Mono answers it: white space round an optional sign,
/// digits with group separators (`,`) after the first digit and before the
/// point, an optional point and fraction, an optional exponent with its own
/// digits; or exactly `Infinity`, `-Infinity` or `NaN`. A finite text whose
/// value is past the `double` range **fails** (.NET Framework's
/// `OverflowException`); one below it is zero.
fn parse_double(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_number_space);
    match trimmed {
        "Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        "NaN" => return Some(f64::NAN),
        _ => {}
    }
    let mut cleaned = String::with_capacity(trimmed.len());
    let mut rest = trimmed;
    if let Some(sign) = rest.chars().next().filter(|c| matches!(c, '+' | '-')) {
        cleaned.push(sign);
        rest = rest.get(1..).unwrap_or_default();
    }
    let mut digits = 0_usize;
    let mut chars = rest.chars().peekable();
    while let Some(&character) = chars.peek() {
        if character.is_ascii_digit() {
            digits = digits.saturating_add(1);
            cleaned.push(character);
        } else if character == ',' && digits > 0 {
            // A group separator, dropped.
        } else {
            break;
        }
        let _consumed = chars.next();
    }
    if chars.peek() == Some(&'.') {
        cleaned.push('.');
        let _point = chars.next();
        while let Some(&character) = chars.peek() {
            if !character.is_ascii_digit() {
                break;
            }
            digits = digits.saturating_add(1);
            cleaned.push(character);
            let _digit = chars.next();
        }
    }
    if digits == 0 {
        return None;
    }
    if let Some(&marker) = chars.peek().filter(|c| matches!(c, 'e' | 'E')) {
        cleaned.push(marker);
        let _marker = chars.next();
        if let Some(&sign) = chars.peek().filter(|c| matches!(c, '+' | '-')) {
            cleaned.push(sign);
            let _sign = chars.next();
        }
        let mut exponent_digits = 0_usize;
        while let Some(&character) = chars.peek() {
            if !character.is_ascii_digit() {
                break;
            }
            exponent_digits = exponent_digits.saturating_add(1);
            cleaned.push(character);
            let _digit = chars.next();
        }
        if exponent_digits == 0 {
            return None;
        }
    }
    if chars.next().is_some() {
        return None;
    }
    let value: f64 = cleaned.parse().ok()?;
    value.is_finite().then_some(value)
}

/// `float.TryParse(text)`, as Mono answers it: [`parse_double`], then the
/// nearest `float`, failing where a finite text rounds to an infinity
/// (`3.5e38`; `-3.4028235e38`, which rounds to the least `float`, is read).
fn parse_float(text: &str) -> Option<f32> {
    let value = parse_double(text)?;
    let single = to_f32(value);
    (!single.is_infinite() || value.is_infinite()).then_some(single)
}

/// `long.TryParse(text)` under `NumberStyles.Integer`: an optional sign and
/// digits. The lexer's number text never has white space round it.
fn parse_long(text: &str) -> Option<i64> {
    text.parse().ok()
}

/// `Math.Round(value, 6)` as .NET Framework computes it: scaled by a million,
/// rounded half to even and scaled back, for a magnitude below 10¹⁶; a larger
/// one, an infinity and NaN unchanged.
fn round6(value: f64) -> f64 {
    if value.abs() < 1e16 {
        (value * 1e6).round_ties_even() / 1e6
    } else {
        value
    }
}

/// How many significant digits .NET Framework formats a `double` with.
const SIGNIFICANT: i32 = 15;

/// A `double` as .NET Framework's formatting sees it: fifteen significant
/// digits, rounded half away from zero from the exact value (so
/// `2224.668212890625` is `2224.66821289063`), without trailing zeros — `digits` with the
/// decimal point `point` digits from their start (`123.45` is `12345`, 3;
/// `0.001` is `1`, -2). Zero has no digits.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Decimal {
    /// Whether the value's sign bit is set.
    negative: bool,
    /// The significant digits, each 0–9, without trailing zeros.
    digits: Vec<u8>,
    /// Where the decimal point falls among them.
    point: i32,
}

impl Decimal {
    /// The fifteen-digit decimal of a finite `value`.
    fn of(value: f64) -> Self {
        let negative = value.is_sign_negative();
        if value == 0.0 {
            return Self {
                negative,
                digits: Vec::new(),
                point: 0,
            };
        }
        // Every digit of the exact value (a `double` has at most 767), so a
        // tie is seen as one.
        let text = format!("{:.766e}", value.abs());
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let digits: Vec<u8> = mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|byte| byte.saturating_sub(b'0'))
            .collect();
        let exponent: i32 = exponent.parse().unwrap_or(0);
        let mut decimal = Self {
            negative,
            digits,
            point: exponent.saturating_add(1),
        };
        decimal.round_at(SIGNIFICANT.saturating_sub(decimal.point));
        decimal
    }

    /// The digit at `index` from the start, zero outside the digits.
    fn digit(&self, index: i32) -> u8 {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.digits.get(index))
            .copied()
            .unwrap_or(0)
    }

    /// Rounded half up at `decimals` places after the point, as a custom
    /// format rounds its fifteen digits (`Number.RoundNumber`).
    fn round_at(&mut self, decimals: i32) {
        let keep = self.point.saturating_add(decimals);
        let Ok(keep) = usize::try_from(keep) else {
            self.digits.clear();
            return;
        };
        if keep >= self.digits.len() {
            return;
        }
        let up = self.digits.get(keep).is_some_and(|digit| *digit >= 5);
        self.digits.truncate(keep);
        if up {
            loop {
                match self.digits.last_mut() {
                    Some(9) => {
                        let _nine = self.digits.pop();
                    }
                    Some(last) => {
                        *last = last.saturating_add(1);
                        break;
                    }
                    None => {
                        self.digits.push(1);
                        self.point = self.point.saturating_add(1);
                        break;
                    }
                }
            }
        }
        while self.digits.last() == Some(&0) {
            let _zero = self.digits.pop();
        }
    }
}

/// The ASCII digit of a value 0–9.
fn digit_char(digit: u8) -> char {
    char::from(b'0'.saturating_add(digit))
}

/// `Convert.ToString(value, NumberFormatInfo.InvariantInfo)` — .NET
/// Framework's `"G"` for a `double`, fifteen significant digits: fixed
/// notation for a decimal exponent from -4 to 14, scientific (`1E+25`,
/// `1E-05`, the exponent in at least two digits) outside it; negative zero is
/// `0`.
fn general(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let decimal = Decimal::of(value);
    if decimal.digits.is_empty() {
        return "0".to_owned();
    }
    let mut out = String::new();
    if decimal.negative {
        out.push('-');
    }
    let exponent = decimal.point.saturating_sub(1);
    if !(-4..15).contains(&exponent) {
        let mut digits = decimal.digits.iter().copied().map(digit_char);
        if let Some(first) = digits.next() {
            out.push(first);
        }
        let rest: String = digits.collect();
        if !rest.is_empty() {
            out.push('.');
            out.push_str(&rest);
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        let magnitude = exponent.unsigned_abs();
        if magnitude < 10 {
            out.push('0');
        }
        out.push_str(&magnitude.to_string());
        return out;
    }
    if decimal.point <= 0 {
        out.push_str("0.");
        for _ in decimal.point..0 {
            out.push('0');
        }
        out.extend(decimal.digits.iter().copied().map(digit_char));
        return out;
    }
    for index in 0..decimal.point {
        out.push(digit_char(decimal.digit(index)));
    }
    let point = usize::try_from(decimal.point).unwrap_or(usize::MAX);
    if let Some(fraction) = decimal.digits.get(point..).filter(|rest| !rest.is_empty()) {
        out.push('.');
        out.extend(fraction.iter().copied().map(digit_char));
    }
    out
}

/// A custom numeric format of a `double` with `least` to `most` decimals —
/// `"0.000000"` (6, 6), `LSLFloat.ToString`'s, and `"0.0#####"` (1, 6),
/// `JsonElementToString`'s: the fifteen digits rounded half up at `most`
/// places, the whole integer part, the decimals down to `least`; a value that
/// rounds to zero has no sign.
fn custom(value: f64, least: i32, most: i32) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let mut decimal = Decimal::of(value);
    decimal.round_at(most);
    let mut out = String::new();
    if decimal.negative && !decimal.digits.is_empty() {
        out.push('-');
    }
    if decimal.point <= 0 {
        out.push('0');
    } else {
        for index in 0..decimal.point {
            out.push(digit_char(decimal.digit(index)));
        }
    }
    let mut fraction: Vec<char> = (0..most)
        .map(|place| digit_char(decimal.digit(decimal.point.saturating_add(place))))
        .collect();
    let least = usize::try_from(least).unwrap_or(0);
    while fraction.len() > least && fraction.last() == Some(&'0') {
        let _zero = fraction.pop();
    }
    if !fraction.is_empty() {
        out.push('.');
        out.extend(fraction);
    }
    out
}

// ---------------------------------------------------------------------------
// LitJson's lexer
// ---------------------------------------------------------------------------

/// A lexer token and a parser symbol (`ParserToken`, whose values lie above
/// every `char`, together with the single characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symbol {
    /// A single character: `[`, `]`, `{`, `}`, `,`, `:` or `"` (which a
    /// single-quoted string also reports).
    Char(char),
    /// `ParserToken.Number`.
    Number,
    /// `ParserToken.True`.
    True,
    /// `ParserToken.False`.
    False,
    /// `ParserToken.Null`.
    Null,
    /// `ParserToken.CharSeq`: a string's characters.
    CharSeq,
    /// `ParserToken.Text`: a whole document.
    Text,
    /// `ParserToken.Object`.
    Object,
    /// `ParserToken.ObjectPrime`.
    ObjectPrime,
    /// `ParserToken.Pair`.
    Pair,
    /// `ParserToken.PairRest`.
    PairRest,
    /// `ParserToken.Array`.
    Array,
    /// `ParserToken.ArrayPrime`.
    ArrayPrime,
    /// `ParserToken.Value`.
    Value,
    /// `ParserToken.ValueRest`.
    ValueRest,
    /// `ParserToken.String`.
    String,
    /// `ParserToken.End`: the bottom of the parser's stack.
    End,
    /// `ParserToken.Epsilon`: the empty rule.
    Epsilon,
}

/// What a lexer state reports when it ends a token (`fsm_return_table`):
/// `Char` stands for "the character just read", and a state that never ends
/// one reports nothing.
const fn returned(state: u8) -> Option<Symbol> {
    match state {
        1 | 20 | 24 => Some(Symbol::Char('\0')),
        3 | 4 | 6 | 8 => Some(Symbol::Number),
        11 => Some(Symbol::True),
        15 => Some(Symbol::False),
        18 => Some(Symbol::Null),
        19 | 23 => Some(Symbol::CharSeq),
        _ => None,
    }
}

/// White space between tokens: space and tab to carriage return.
const fn is_space(character: char) -> bool {
    matches!(character, ' ' | '\t'..='\r')
}

/// A `\u` escape's digit, as `HexValue` reads it.
fn hex_value(character: char) -> u32 {
    character.to_digit(16).unwrap_or(0)
}

/// `ProcessEscChar`: the character a one-letter escape stands for.
const fn escaped(character: char) -> char {
    match character {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'b' => '\u{08}',
        'f' => '\u{0C}',
        other => other,
    }
}

/// The UTF-16 unit of a character the lexer appends; only characters of the
/// Basic Multilingual Plane reach it.
fn unit_of(character: char) -> u16 {
    u16::try_from(u32::from(character)).unwrap_or(0xFFFD)
}

/// `LitJson.Lexer`: a finite state machine over UTF-16 units, with comments
/// and single-quoted strings allowed (the constructor's defaults).
#[derive(Debug)]
struct Lexer<'text> {
    /// The text.
    text: &'text [u16],
    /// The next unit to read.
    at: usize,
    /// A unit put back (`input_buffer`).
    buffered: Option<u16>,
    /// The unit last read, [`None`] past the end (`input_char` of -1).
    current: Option<u16>,
    /// Whether the text has run out.
    end_of_input: bool,
    /// The state, 1–28.
    state: u8,
    /// The state to go on in after an escape (`StateStack`).
    resume: u8,
    /// The state the handler chose (`NextState`).
    next: u8,
    /// Whether the handler ended a token (`Return`).
    done: bool,
    /// The token's characters so far.
    buffer: Units,
    /// The last token's characters (`StringValue`).
    value: Units,
    /// The last token.
    token: Symbol,
    /// A `\u` escape's value so far.
    unichar: u32,
}

impl<'text> Lexer<'text> {
    /// A lexer at the start of `text`, in state 1.
    const fn new(text: &'text [u16]) -> Self {
        Self {
            text,
            at: 0,
            buffered: None,
            current: None,
            end_of_input: false,
            state: 1,
            resume: 0,
            next: 1,
            done: false,
            buffer: Vec::new(),
            value: Vec::new(),
            token: Symbol::Char('\0'),
            unichar: 0,
        }
    }

    /// `GetChar`: read the next unit; false, and the end noted, past the
    /// end.
    fn get(&mut self) -> bool {
        self.current = self.buffered.take().or_else(|| {
            let unit = self.text.get(self.at).copied();
            if unit.is_some() {
                self.at = self.at.saturating_add(1);
            }
            unit
        });
        if self.current.is_none() {
            self.end_of_input = true;
        }
        self.current.is_some()
    }

    /// `UngetChar`: the unit last read is read again next.
    const fn unget(&mut self) {
        self.buffered = self.current;
    }

    /// The unit last read as a character; half a surrogate pair is U+FFFD,
    /// which no state treats as anything but a string's character.
    fn character(&self) -> char {
        self.current
            .and_then(|unit| char::from_u32(u32::from(unit)))
            .unwrap_or('\u{FFFD}')
    }

    /// Append the unit last read to the token.
    fn keep(&mut self) {
        if let Some(unit) = self.current {
            self.buffer.push(unit);
        }
    }

    /// End the token and go on in `next`.
    const fn finish(&mut self, next: u8) {
        self.done = true;
        self.next = next;
    }

    /// After a number's digits: white space ends it, `,` `]` `}` end it and
    /// are read again, and `point` / `exponent` say whether a point or an
    /// exponent may follow.
    fn after_digits(&mut self, point: bool, exponent: bool) -> Result<(), Thrown> {
        let character = self.character();
        if is_space(character) {
            self.finish(1);
            return Ok(());
        }
        match character {
            ',' | ']' | '}' => {
                self.unget();
                self.finish(1);
                Ok(())
            }
            '.' if point => {
                self.keep();
                self.next = 5;
                Ok(())
            }
            'e' | 'E' if exponent => {
                self.keep();
                self.next = 7;
                Ok(())
            }
            _ => Err(Thrown),
        }
    }

    /// One letter of `true`, `false` or `null`: `expected`, then `next` — or,
    /// with `last`, the end of the word's token.
    fn letter(&mut self, expected: char, next: u8, last: bool) -> Result<(), Thrown> {
        let _read = self.get();
        if self.character() != expected || self.current.is_none() {
            return Err(Thrown);
        }
        if last {
            self.finish(1);
        } else {
            self.next = next;
        }
        Ok(())
    }

    /// The handler of the current state (`State1` … `State28`); an error is
    /// the handler's `false`, which `NextToken` throws as a `JsonException`.
    fn step(&mut self) -> Result<(), Thrown> {
        match self.state {
            1 => {
                while self.get() {
                    let character = self.character();
                    if is_space(character) {
                        continue;
                    }
                    if ('1'..='9').contains(&character) {
                        self.keep();
                        self.next = 3;
                        return Ok(());
                    }
                    return match character {
                        '"' => {
                            self.finish(19);
                            Ok(())
                        }
                        ',' | ':' | '[' | ']' | '{' | '}' => {
                            self.finish(1);
                            Ok(())
                        }
                        '-' => {
                            self.keep();
                            self.next = 2;
                            Ok(())
                        }
                        '0' => {
                            self.keep();
                            self.next = 4;
                            Ok(())
                        }
                        'f' => {
                            self.next = 12;
                            Ok(())
                        }
                        'n' => {
                            self.next = 16;
                            Ok(())
                        }
                        't' => {
                            self.next = 9;
                            Ok(())
                        }
                        '\'' => {
                            self.current = Some(unit_of('"'));
                            self.finish(23);
                            Ok(())
                        }
                        '/' => {
                            self.next = 25;
                            Ok(())
                        }
                        _ => Err(Thrown),
                    };
                }
                Ok(())
            }
            2 => {
                let _read = self.get();
                match self.character() {
                    '1'..='9' if self.current.is_some() => {
                        self.keep();
                        self.next = 3;
                        Ok(())
                    }
                    '0' if self.current.is_some() => {
                        self.keep();
                        self.next = 4;
                        Ok(())
                    }
                    _ => Err(Thrown),
                }
            }
            3 => {
                while self.get() {
                    if self.character().is_ascii_digit() {
                        self.keep();
                        continue;
                    }
                    return self.after_digits(true, true);
                }
                Ok(())
            }
            4 => {
                if !self.get() {
                    // `State4` reads past the end as -1, which no case matches.
                    return Err(Thrown);
                }
                self.after_digits(true, true)
            }
            5 => {
                let _read = self.get();
                if self.current.is_some() && self.character().is_ascii_digit() {
                    self.keep();
                    self.next = 6;
                    Ok(())
                } else {
                    Err(Thrown)
                }
            }
            6 => {
                while self.get() {
                    if self.character().is_ascii_digit() {
                        self.keep();
                        continue;
                    }
                    return self.after_digits(false, true);
                }
                Ok(())
            }
            7 => {
                let _read = self.get();
                match self.character() {
                    '0'..='9' | '+' | '-' if self.current.is_some() => {
                        self.keep();
                        self.next = 8;
                        Ok(())
                    }
                    _ => Err(Thrown),
                }
            }
            8 => {
                while self.get() {
                    if self.character().is_ascii_digit() {
                        self.keep();
                        continue;
                    }
                    return self.after_digits(false, false);
                }
                Ok(())
            }
            9 => self.letter('r', 10, false),
            10 => self.letter('u', 11, false),
            11 | 15 => self.letter('e', 1, true),
            12 => self.letter('a', 13, false),
            13 => self.letter('l', 14, false),
            14 => self.letter('s', 15, false),
            16 => self.letter('u', 17, false),
            17 => self.letter('l', 18, false),
            18 => self.letter('l', 1, true),
            19 | 23 => {
                let close = if self.state == 19 { '"' } else { '\'' };
                while self.get() {
                    let character = self.character();
                    if character == close && self.current.is_some() {
                        self.unget();
                        self.finish(if self.state == 19 { 20 } else { 24 });
                        return Ok(());
                    }
                    if character == '\\' {
                        self.resume = self.state;
                        self.next = 21;
                        return Ok(());
                    }
                    self.keep();
                }
                Ok(())
            }
            20 | 24 => {
                let close = if self.state == 20 { '"' } else { '\'' };
                let _read = self.get();
                if self.current.is_some() && self.character() == close {
                    self.current = Some(unit_of('"'));
                    self.finish(1);
                    Ok(())
                } else {
                    Err(Thrown)
                }
            }
            21 => {
                let _read = self.get();
                match self.character() {
                    'u' => {
                        self.next = 22;
                        Ok(())
                    }
                    character @ ('"' | '\'' | '/' | '\\' | 'b' | 'f' | 'n' | 'r' | 't')
                        if self.current.is_some() =>
                    {
                        self.buffer.push(unit_of(escaped(character)));
                        self.next = self.resume;
                        Ok(())
                    }
                    _ => Err(Thrown),
                }
            }
            22 => {
                let mut counter = 0_u32;
                let mut multiplier = 4096_u32;
                self.unichar = 0;
                while self.get() {
                    let character = self.character();
                    if !character.is_ascii_hexdigit() {
                        return Err(Thrown);
                    }
                    self.unichar = self
                        .unichar
                        .saturating_add(hex_value(character).saturating_mul(multiplier));
                    counter = counter.saturating_add(1);
                    multiplier >>= 4;
                    if counter == 4 {
                        self.buffer
                            .push(u16::try_from(self.unichar).unwrap_or(0xFFFD));
                        self.next = self.resume;
                        return Ok(());
                    }
                }
                Ok(())
            }
            25 => {
                let _read = self.get();
                match self.character() {
                    '*' if self.current.is_some() => {
                        self.next = 27;
                        Ok(())
                    }
                    '/' if self.current.is_some() => {
                        self.next = 26;
                        Ok(())
                    }
                    _ => Err(Thrown),
                }
            }
            26 => {
                while self.get() {
                    if self.character() == '\n' {
                        self.next = 1;
                        return Ok(());
                    }
                }
                Ok(())
            }
            27 => {
                while self.get() {
                    if self.character() == '*' {
                        self.next = 28;
                        return Ok(());
                    }
                }
                Ok(())
            }
            28 => {
                while self.get() {
                    let character = self.character();
                    if character == '*' {
                        continue;
                    }
                    self.next = if character == '/' { 1 } else { 27 };
                    return Ok(());
                }
                Ok(())
            }
            _ => Err(Thrown),
        }
    }

    /// `NextToken`: run the machine to the end of the next token; false at
    /// the end of the text, which drops a token not yet ended (a number
    /// with nothing after it).
    fn next_token(&mut self) -> Result<bool, Thrown> {
        self.done = false;
        loop {
            self.step()?;
            if self.end_of_input {
                return Ok(false);
            }
            if self.done {
                self.value = core::mem::take(&mut self.buffer);
                self.token = match returned(self.state) {
                    Some(Symbol::Char(_)) => Symbol::Char(self.character()),
                    Some(symbol) => symbol,
                    None => return Err(Thrown),
                };
                self.state = self.next;
                return Ok(true);
            }
            self.state = self.next;
        }
    }
}

// ---------------------------------------------------------------------------
// LitJson's reader and mapper
// ---------------------------------------------------------------------------

/// `JsonToken`: what one `JsonReader.Read` produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// Nothing (yet).
    None,
    /// `[`.
    ArrayStart,
    /// `]`.
    ArrayEnd,
    /// `{`.
    ObjectStart,
    /// An object's key.
    PropertyName,
    /// `}`.
    ObjectEnd,
    /// A number that fits an `int`.
    Int,
    /// A number past `int` (or, as an `ulong`, past `long`).
    Long,
    /// A number with a point or an exponent.
    Double,
    /// A string.
    String,
    /// `true` or `false`.
    Boolean,
    /// `null`.
    Null,
}

/// The value of a [`Token`] (`JsonReader.Value`).
#[derive(Debug, Clone, PartialEq)]
enum Scalar {
    /// No value.
    None,
    /// A string or a key.
    Text(Units),
    /// An `int`.
    Int(i32),
    /// A `long`.
    Long(i64),
    /// An `ulong` too large for a `long`, which the mapper's `(long)` cast
    /// refuses.
    ULong,
    /// A `double`.
    Double(f64),
    /// A boolean.
    Boolean(bool),
}

/// The productions of `JsonReader`'s parse table (`PopulateParseTable`): what
/// `row` expands to when the next token is `column`, [`None`] where the table
/// has no entry (a `KeyNotFoundException`, thrown on).
const fn production(row: Symbol, column: Symbol) -> Option<&'static [Symbol]> {
    use Symbol::{
        Array, ArrayPrime, CharSeq, Epsilon, False, Null, Number, Object, ObjectPrime, Pair,
        PairRest, String, True, Value, ValueRest,
    };
    const VALUES: &[Symbol] = &[Value, ValueRest, Symbol::Char(']')];
    match (row, column) {
        (Array, Symbol::Char('[')) => Some(&[Symbol::Char('['), ArrayPrime]),
        (ArrayPrime, Symbol::Char('"' | '[' | '{') | Number | True | False | Null) => Some(VALUES),
        (ArrayPrime, Symbol::Char(']')) => Some(&[Symbol::Char(']')]),
        (Object, Symbol::Char('{')) => Some(&[Symbol::Char('{'), ObjectPrime]),
        (ObjectPrime, Symbol::Char('"')) => Some(&[Pair, PairRest, Symbol::Char('}')]),
        (ObjectPrime, Symbol::Char('}')) => Some(&[Symbol::Char('}')]),
        (Pair, Symbol::Char('"')) => Some(&[String, Symbol::Char(':'), Value]),
        (PairRest, Symbol::Char(',')) => Some(&[Symbol::Char(','), Pair, PairRest]),
        (PairRest, Symbol::Char('}')) | (ValueRest, Symbol::Char(']')) => Some(&[Epsilon]),
        (String, Symbol::Char('"')) => Some(&[Symbol::Char('"'), CharSeq, Symbol::Char('"')]),
        (Symbol::Text | Value, Symbol::Char('[')) => Some(&[Array]),
        (Symbol::Text | Value, Symbol::Char('{')) => Some(&[Object]),
        (Value, Symbol::Char('"')) => Some(&[String]),
        (Value, Number) => Some(&[Number]),
        (Value, True) => Some(&[True]),
        (Value, False) => Some(&[False]),
        (Value, Null) => Some(&[Null]),
        (ValueRest, Symbol::Char(',')) => Some(&[Symbol::Char(','), Value, ValueRest]),
        _ => None,
    }
}

/// How far a [`Reader`] has got through its text (`read_started`,
/// `end_of_json` and `end_of_input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    /// Nothing read yet.
    Fresh,
    /// Inside a document.
    Reading,
    /// A whole document read; the next `Read` starts another.
    DocumentDone,
    /// The text has run out.
    Exhausted,
}

/// `LitJson.JsonReader`: an LL(1) parser over the [`Lexer`]'s tokens, one
/// token of look-ahead.
#[derive(Debug)]
struct Reader<'text> {
    /// The lexer.
    lexer: Lexer<'text>,
    /// The parser's stack (`automaton_stack`), its top last.
    stack: Vec<Symbol>,
    /// The look-ahead token (`current_input`).
    input: Symbol,
    /// How far the text has been read.
    stream: Stream,
    /// Whether the parser is between a string's quotes.
    in_string: bool,
    /// Whether the current `Read` has its token.
    ready: bool,
    /// The token.
    token: Token,
    /// Its value.
    value: Scalar,
}

impl<'text> Reader<'text> {
    /// A reader at the start of `text`.
    fn new(text: &'text [u16]) -> Self {
        Self {
            lexer: Lexer::new(text),
            stack: vec![Symbol::End, Symbol::Text],
            input: Symbol::Char('\0'),
            stream: Stream::Fresh,
            in_string: false,
            ready: false,
            token: Token::None,
            value: Scalar::None,
        }
    }

    /// `ProcessNumber`: a number's text as an `int`, `long`, `ulong` or
    /// `double` token — a `double` only for text with a point or an
    /// exponent that `double.TryParse` reads, and `int` 0 for text nothing
    /// reads (`1e400`, `1e+`).
    fn number(&mut self) {
        let text = string(&self.lexer.value);
        if text.contains(['.', 'e', 'E'])
            && let Some(double) = parse_double(&text)
        {
            self.token = Token::Double;
            self.value = Scalar::Double(double);
            return;
        }
        if let Some(long) = parse_long(&text) {
            if let Ok(int) = i32::try_from(long) {
                self.token = Token::Int;
                self.value = Scalar::Int(int);
            } else {
                self.token = Token::Long;
                self.value = Scalar::Long(long);
            }
            return;
        }
        if text.parse::<u64>().is_ok() {
            self.token = Token::Long;
            self.value = Scalar::ULong;
            return;
        }
        self.token = Token::Int;
        self.value = Scalar::Int(0);
    }

    /// `ProcessSymbol`: what popping `symbol` contributes to the token.
    fn process(&mut self, symbol: Symbol) {
        match symbol {
            Symbol::Char('[') => {
                self.token = Token::ArrayStart;
                self.ready = true;
            }
            Symbol::Char(']') => {
                self.token = Token::ArrayEnd;
                self.ready = true;
            }
            Symbol::Char('{') => {
                self.token = Token::ObjectStart;
                self.ready = true;
            }
            Symbol::Char('}') => {
                self.token = Token::ObjectEnd;
                self.ready = true;
            }
            Symbol::Char('"') => {
                if self.in_string {
                    self.in_string = false;
                    self.ready = true;
                } else {
                    if self.token == Token::None {
                        self.token = Token::String;
                    }
                    self.in_string = true;
                }
            }
            Symbol::CharSeq => self.value = Scalar::Text(self.lexer.value.clone()),
            Symbol::False => {
                self.token = Token::Boolean;
                self.value = Scalar::Boolean(false);
                self.ready = true;
            }
            Symbol::True => {
                self.token = Token::Boolean;
                self.value = Scalar::Boolean(true);
                self.ready = true;
            }
            Symbol::Null => {
                self.token = Token::Null;
                self.ready = true;
            }
            Symbol::Number => {
                self.number();
                self.ready = true;
            }
            Symbol::Pair => self.token = Token::PropertyName,
            _ => {}
        }
    }

    /// `ReadToken`: the next look-ahead token; false at the end of the text.
    fn read_token(&mut self) -> Result<bool, Thrown> {
        if self.stream == Stream::Exhausted {
            return Ok(false);
        }
        let _more = self.lexer.next_token()?;
        if self.lexer.end_of_input {
            self.stream = Stream::Exhausted;
            return Ok(false);
        }
        self.input = self.lexer.token;
        Ok(true)
    }

    /// `Read`: the next token into [`Reader::token`] and
    /// [`Reader::value`]; false when there is none.
    fn read(&mut self) -> Result<bool, Thrown> {
        if self.stream == Stream::Exhausted {
            return Ok(false);
        }
        if self.stream == Stream::DocumentDone {
            self.stream = Stream::Reading;
            self.stack = vec![Symbol::End, Symbol::Text];
        }
        self.in_string = false;
        self.ready = false;
        self.token = Token::None;
        self.value = Scalar::None;
        if self.stream == Stream::Fresh {
            self.stream = Stream::Reading;
            if !self.read_token()? {
                return Ok(false);
            }
        }
        loop {
            if self.ready {
                if self.stack.last() == Some(&Symbol::End) && self.stream != Stream::Exhausted {
                    self.stream = Stream::DocumentDone;
                }
                return Ok(true);
            }
            let symbol = self.stack.pop().ok_or(Thrown)?;
            self.process(symbol);
            if symbol == self.input {
                if !self.read_token()? {
                    if self.stack.last().ok_or(Thrown)? != &Symbol::End {
                        // "Input doesn't evaluate to proper JSON text".
                        return Err(Thrown);
                    }
                    return Ok(self.ready);
                }
                continue;
            }
            let entry = production(symbol, self.input).ok_or(Thrown)?;
            if entry.first() == Some(&Symbol::Epsilon) {
                continue;
            }
            self.stack.extend(entry.iter().rev());
        }
    }
}

/// `JsonMapper.ReadValue` into a `JsonData`: [`None`] for a `null` and for
/// the `]` that ends an array (the caller tells them apart by the token).
fn read_value(reader: &mut Reader<'_>) -> Result<Node, Thrown> {
    let _read = reader.read()?;
    match reader.token {
        Token::ArrayEnd | Token::Null => Ok(None),
        Token::String => match &reader.value {
            Scalar::Text(text) => Ok(Some(Json::Str(text.clone()))),
            _ => Err(Thrown),
        },
        Token::Double | Token::Int | Token::Long | Token::Boolean => {
            match reader.value {
                Scalar::Double(double) => Ok(Some(Json::Double(double))),
                Scalar::Int(int) => Ok(Some(Json::Int(int))),
                Scalar::Long(long) => Ok(Some(Json::Long(long))),
                Scalar::Boolean(boolean) => Ok(Some(Json::Boolean(boolean))),
                // `(long)reader.Value` of a boxed `ulong`: an
                // `InvalidCastException`.
                Scalar::ULong | Scalar::None | Scalar::Text(_) => Err(Thrown),
            }
        }
        Token::ArrayStart => {
            let mut items = Vec::new();
            loop {
                let item = read_value(reader)?;
                if item.is_none() && reader.token == Token::ArrayEnd {
                    break;
                }
                if reader.stream == Stream::Exhausted && reader.token == Token::None {
                    // A read past the end inside an array, which `Read`
                    // throws on before it can happen; kept from looping.
                    return Err(Thrown);
                }
                items.push(item);
            }
            Ok(Some(Json::Array(items)))
        }
        Token::ObjectStart => {
            let mut pairs: Vec<(Units, Node)> = Vec::new();
            loop {
                let more = reader.read()?;
                if reader.token == Token::ObjectEnd {
                    break;
                }
                let Scalar::Text(key) = reader.value.clone() else {
                    // A null key: `ArgumentNullException`.
                    return Err(Thrown);
                };
                if !more {
                    return Err(Thrown);
                }
                let value = read_value(reader)?;
                insert(&mut pairs, key, value);
            }
            Ok(Some(Json::Object(pairs)))
        }
        Token::None | Token::PropertyName | Token::ObjectEnd => Ok(Some(Json::Untyped)),
    }
}

/// `JsonData[key] = value`: a key already there keeps its place and takes
/// the new value; a new one goes last.
fn insert(pairs: &mut Vec<(Units, Node)>, key: Units, value: Node) {
    if let Some(slot) = pairs.iter_mut().find(|(existing, _)| *existing == key) {
        slot.1 = value;
    } else {
        pairs.push((key, value));
    }
}

/// `JsonMapper.ToObject(text)`: the document's tree, or what LitJson throws.
fn to_object(text: &str) -> Result<Node, Thrown> {
    let text = units(text);
    let mut reader = Reader::new(&text);
    read_value(&mut reader)
}

// ---------------------------------------------------------------------------
// LitJson's writer
// ---------------------------------------------------------------------------

/// `JsonWriter.PutString`: a string in double quotes, with `\n` `\r` `\t`
/// `\"` `\\` `\f` `\b` escaped, printable ASCII as it is and every other
/// UTF-16 unit as `\uXXXX` in upper-case hexadecimal.
fn put_string(text: &[u16], out: &mut String) {
    out.push('"');
    for unit in text {
        match char::from_u32(u32::from(*unit)) {
            Some('\n') => out.push_str("\\n"),
            Some('\r') => out.push_str("\\r"),
            Some('\t') => out.push_str("\\t"),
            Some(character @ ('"' | '\\')) => {
                out.push('\\');
                out.push(character);
            }
            Some('\u{0C}') => out.push_str("\\f"),
            Some('\u{08}') => out.push_str("\\b"),
            Some(character @ ' '..='~') => out.push(character),
            _ => {
                out.push_str("\\u");
                for shift in [12_u16, 8, 4, 0] {
                    let nibble = u32::from((*unit >> shift) & 0xF);
                    out.extend(char::from_digit(nibble, 16).map(|hex| hex.to_ascii_uppercase()));
                }
            }
        }
    }
    out.push('"');
}

/// `JsonData.WriteJson`, compact: a `null` as `null`, an untyped
/// `JsonData` as nothing, a `double` in [`general`] form with `.0` appended
/// when that has neither point nor exponent.
fn write(node: &Node, out: &mut String) {
    match node {
        None => out.push_str("null"),
        Some(Json::Untyped) => {}
        Some(Json::Str(text)) => put_string(text, out),
        Some(Json::Boolean(boolean)) => out.push_str(if *boolean { "true" } else { "false" }),
        Some(Json::Double(double)) => {
            let text = general(*double);
            out.push_str(&text);
            if !text.contains(['.', 'e', 'E']) {
                out.push_str(".0");
            }
        }
        Some(Json::Int(int)) => out.push_str(&int.to_string()),
        Some(Json::Long(long)) => out.push_str(&long.to_string()),
        Some(Json::Array(items)) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(item, out);
            }
            out.push(']');
        }
        Some(Json::Object(pairs)) => {
            out.push('{');
            for (index, (key, value)) in pairs.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                put_string(key, out);
                out.push(':');
                write(value, out);
            }
            out.push('}');
        }
    }
}

/// `JsonMapper.ToJson`: the tree as compact JSON text.
fn to_json(node: &Node) -> String {
    let mut out = String::new();
    write(node, &mut out);
    out
}

// ---------------------------------------------------------------------------
// LSL_Api.cs
// ---------------------------------------------------------------------------

/// `JsonParseTop`: the elements of the document's top level — a scalar as
/// itself, an array's items and an object's keys and values through
/// [`parse_node`]. A `long` anywhere in it throws.
fn parse_top(node: &Node) -> Result<Vec<Element>, Thrown> {
    let Some(json) = node else {
        // `retl.Add(JSON_NULL)`, then `elem.GetJsonType()` of null.
        return Err(Thrown);
    };
    match json {
        Json::Int(int) => Ok(vec![Element::Integer(*int)]),
        Json::Boolean(boolean) => Ok(vec![Element::String(
            if *boolean { TRUE } else { FALSE }.to_owned(),
        )]),
        Json::Double(double) => Ok(vec![Element::Float(to_f32(*double))]),
        Json::Untyped => Ok(vec![Element::String(NULL.to_owned())]),
        Json::Str(text) => Ok(vec![Element::String(string(text))]),
        Json::Array(items) => items.iter().map(parse_node).collect(),
        Json::Object(pairs) => {
            let mut out = Vec::new();
            for (key, value) in pairs {
                out.push(Element::String(string(key)));
                out.push(parse_node(value)?);
            }
            Ok(out)
        }
        Json::Long(_) => Err(Thrown),
    }
}

/// `JsonParseTopNodes`: one element of the top level — `null` as
/// `JSON_NULL`, a boolean as `JSON_TRUE` / `JSON_FALSE`, a nested array or
/// object as its JSON text written anew; a `long` throws.
fn parse_node(node: &Node) -> Result<Element, Thrown> {
    match node {
        None | Some(Json::Untyped) => Ok(Element::String(NULL.to_owned())),
        Some(Json::Int(int)) => Ok(Element::Integer(*int)),
        Some(Json::Boolean(boolean)) => Ok(Element::String(
            if *boolean { TRUE } else { FALSE }.to_owned(),
        )),
        Some(Json::Double(double)) => Ok(Element::Float(to_f32(*double))),
        Some(Json::Str(text)) => Ok(Element::String(string(text))),
        Some(Json::Array(_) | Json::Object(_)) => Ok(Element::String(to_json(node))),
        Some(Json::Long(_)) => Err(Thrown),
    }
}

/// `llJson2List`: the empty string, `[]` and `{}` give the empty list; text
/// that does not start with `[` or `{`, or that LitJson cannot read, is one
/// string of the text; a document with a `long` in its top level is
/// `[JSON_INVALID]`.
pub(super) fn json2_list(json: &str) -> Vec<Element> {
    if json.is_empty() || json == "[]" || json == "{}" {
        return Vec::new();
    }
    if !json.starts_with(['[', '{']) {
        return vec![Element::String(json.to_owned())];
    }
    match to_object(json) {
        Err(Thrown) => vec![Element::String(json.to_owned())],
        Ok(node) => {
            parse_top(&node).unwrap_or_else(|Thrown| vec![Element::String(INVALID.to_owned())])
        }
    }
}

/// `EscapeForJSON(s, true)`: `s` in double quotes with `\`, `"` and `/`
/// backslashed, `\b` `\t` `\n` `\f` `\r` escaped, any other control
/// character as `\u000` followed by the character itself — `String.Format`
/// of a `char` with `{0:X}` formats nothing — and the rest as it is.
fn escape_for_json(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '\\' | '"' | '/' => {
                out.push('\\');
                out.push(character);
            }
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            control if control < ' ' => {
                out.push_str("\\u000");
                out.push(control);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// `LSLFloat.ToString`, `"0.000000"`, of a component or a float.
fn float_text(value: f32) -> String {
    custom(f64::from(value), 6, 6)
}

/// `ListToJson`: one list element as JSON — a float with six decimals (an
/// infinity as `"Inf"`, NaN as `"NaN"`), an integer, a vector or rotation as
/// the string of its six-decimal text, and a string or key as `true`,
/// `false` or `null` for those words and the sentinels, `""` when empty, as
/// it is when it starts with `[` or `{`, and escaped otherwise. Unlike
/// Second Life it trims nothing: `str.Trim()`'s result is discarded.
fn list_to_json(element: &Element) -> String {
    match element {
        Element::Float(float) => {
            if float.is_infinite() {
                "\"Inf\"".to_owned()
            } else if float.is_nan() {
                "\"NaN\"".to_owned()
            } else {
                float_text(*float)
            }
        }
        Element::Integer(integer) => integer.to_string(),
        Element::Rotation(rotation) => format!(
            "\"<{}, {}, {}, {}>\"",
            float_text(rotation.x),
            float_text(rotation.y),
            float_text(rotation.z),
            float_text(rotation.s)
        ),
        Element::Vector(vector) => format!(
            "\"<{}, {}, {}>\"",
            float_text(vector.x),
            float_text(vector.y),
            float_text(vector.z)
        ),
        Element::String(text) | Element::Key(text) => {
            if text == TRUE || text == "true" {
                "true".to_owned()
            } else if text == FALSE || text == "false" {
                "false".to_owned()
            } else if text == NULL || text == "null" {
                "null".to_owned()
            } else if text.is_empty() {
                "\"\"".to_owned()
            } else if text.starts_with(['{', '[']) {
                text.clone()
            } else {
                escape_for_json(text)
            }
        }
    }
}

/// `llList2Json`: `JSON_ARRAY` writes each element through
/// [`list_to_json`]; `JSON_OBJECT` takes the list as key, value pairs, a key
/// a string or a key (OpenSim's keys are strings) and every key escaped;
/// an odd count, a key of another type and any other kind are
/// `JSON_INVALID`. Pairs are written as given, a repeated key twice.
pub(super) fn list2_json(kind: &str, values: &[Element]) -> String {
    if kind == ARRAY {
        let items: Vec<String> = values.iter().map(list_to_json).collect();
        return format!("[{}]", items.join(","));
    }
    if kind != OBJECT {
        return INVALID.to_owned();
    }
    let mut out = String::from("{");
    for (index, pair) in values.chunks(2).enumerate() {
        let (Some(key), Some(value)) = (pair.first(), pair.get(1)) else {
            // `values.Data[i+1]` past the end: `IndexOutOfRangeException`.
            return INVALID.to_owned();
        };
        let (Element::String(key) | Element::Key(key)) = key else {
            return INVALID.to_owned();
        };
        if index > 0 {
            out.push(',');
        }
        out.push_str(&escape_for_json(key));
        out.push(':');
        out.push_str(&list_to_json(value));
    }
    out.push('}');
    out
}

/// A specifier as `JsonSetSpecific` reads it: a string (or key) or an
/// integer; anything else throws.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Spec {
    /// A key, or `JSON_APPEND`.
    Key(Units),
    /// An index.
    Index(i32),
}

/// The specifier at `level`, [`None`] past the end; an element of another
/// type throws.
fn spec_at(specifiers: &[Element], level: usize) -> Result<Option<Spec>, Thrown> {
    match specifiers.get(level) {
        None => Ok(None),
        Some(Element::String(text) | Element::Key(text)) => Ok(Some(Spec::Key(units(text)))),
        Some(Element::Integer(index)) => Ok(Some(Spec::Index(*index))),
        Some(_) => Err(Thrown),
    }
}

/// Whether `spec` is `JSON_APPEND`, OpenSim's string `"-1"`.
fn is_append(spec: &[u16]) -> bool {
    spec == units(APPEND).as_slice()
}

/// The tree a value written by `llJsonSetValue` becomes: `null`, `true` and
/// `false` for those words and sentinels; a number when `float.TryParse`
/// reads the text — an integer when it is integral and the text has no
/// point (`01` is 1, `1e5` 100000), otherwise the float rounded to six
/// decimals; and the text as a string otherwise. `JSON_DELETE` throws.
fn new_value(value: &str) -> Result<Node, Thrown> {
    if value == NULL || value == "null" {
        return Ok(None);
    }
    if value == DELETE {
        return Err(Thrown);
    }
    if value == TRUE || value == "true" {
        return Ok(Some(Json::Boolean(true)));
    }
    if value == FALSE || value == "false" {
        return Ok(Some(Json::Boolean(false)));
    }
    if let Some(number) = parse_float(value) {
        // `num - (int)num == 0.0f`, in float, with x86's (int) of a value
        // out of range being `int.MinValue`.
        let truncated = float_to_int(number);
        if number - int_to_float(truncated) == 0.0 && !value.contains('.') {
            return Ok(Some(Json::Int(truncated)));
        }
        let rounded = to_f32(round6(f64::from(number)));
        return Ok(Some(Json::Double(f64::from(rounded))));
    }
    Ok(Some(Json::Str(units(value))))
}

/// `JsonBuildRestOfSpec`: the value made from the specifiers from `level`
/// on — an index (only 0) or `JSON_APPEND` a one-element array, a key a
/// one-pair object, and past the last one [`new_value`]. Any other index
/// throws.
fn build(specifiers: &[Element], level: usize, value: &str) -> Result<Node, Thrown> {
    match spec_at(specifiers, level)? {
        None => new_value(value),
        Some(Spec::Index(0)) => Ok(Some(Json::Array(vec![build(
            specifiers,
            level.saturating_add(1),
            value,
        )?]))),
        Some(Spec::Key(key)) if is_append(&key) => Ok(Some(Json::Array(vec![build(
            specifiers,
            level.saturating_add(1),
            value,
        )?]))),
        Some(Spec::Index(_)) => Err(Thrown),
        Some(Spec::Key(key)) => Ok(Some(Json::Object(vec![(
            key,
            build(specifiers, level.saturating_add(1), value)?,
        )]))),
    }
}

/// Where `JsonSetSpecific` found an existing value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    /// At this index of an array.
    Index(usize),
    /// Under this key of an object.
    Key(Units),
}

/// `JsonSetSpecific`: set `value` at the specifiers from `level` on inside
/// `elem`, changing it in place, or answer the tree that replaces it (an
/// array given a key while it has fewer than two items, an object given
/// index 0, a scalar given anything). An array given a key while it has two
/// items or more, and an object given another index, are left as they are —
/// unless the value is `JSON_DELETE`, which throws wherever nothing was
/// deleted.
fn set_specific(
    elem: &mut Json,
    specifiers: &[Element],
    level: usize,
    value: &str,
) -> Result<Node, Thrown> {
    let spec = spec_at(specifiers, level)?.ok_or(Thrown)?;
    let last = specifiers.len().saturating_sub(1);
    let next = level.saturating_add(1);
    let mut found = None;
    match elem {
        Json::Array(items) => match &spec {
            Spec::Index(index) => {
                let count = items.len();
                let at = usize::try_from(*index).map_err(|_negative| Thrown)?;
                if at != 0 && at > count {
                    return Err(Thrown);
                }
                if at == count {
                    items.push(build(specifiers, next, value)?);
                } else {
                    found = Some(Found::Index(at));
                }
            }
            Spec::Key(key) => {
                if is_append(key) {
                    items.push(build(specifiers, next, value)?);
                } else if items.len() < 2 {
                    return Ok(Some(Json::Object(vec![(
                        key.clone(),
                        build(specifiers, next, value)?,
                    )])));
                }
            }
        },
        Json::Object(pairs) => match &spec {
            Spec::Key(key) => {
                if pairs.iter().any(|(existing, _)| existing == key) {
                    found = Some(Found::Key(key.clone()));
                } else {
                    pairs.push((key.clone(), build(specifiers, next, value)?));
                }
            }
            Spec::Index(0) => {
                return Ok(Some(Json::Array(vec![build(specifiers, next, value)?])));
            }
            Spec::Index(_) => {}
        },
        Json::Untyped
        | Json::Int(_)
        | Json::Long(_)
        | Json::Double(_)
        | Json::Boolean(_)
        | Json::Str(_) => return build(specifiers, level, value),
    }
    if let Some(found) = found {
        let slot = slot(elem, &found).ok_or(Thrown)?;
        if level < last {
            // `value` may be a JSON null: `JsonSetSpecific(null, …)` throws.
            let child = slot.as_mut().ok_or(Thrown)?;
            if let Some(replacement) = set_specific(child, specifiers, next, value)? {
                *slot = Some(replacement);
            }
            return Ok(None);
        }
        if value == DELETE {
            match (elem, found) {
                (Json::Array(items), Found::Index(at)) => {
                    let _removed = items.remove(at);
                }
                (Json::Object(pairs), Found::Key(key)) => {
                    pairs.retain(|(existing, _)| *existing != key);
                }
                _ => {}
            }
            return Ok(None);
        }
        *slot = new_value(value)?;
    }
    if value == DELETE {
        return Err(Thrown);
    }
    Ok(None)
}

/// The slot of `elem` that `found` names.
fn slot<'tree>(elem: &'tree mut Json, found: &Found) -> Option<&'tree mut Node> {
    match (elem, found) {
        (Json::Array(items), Found::Index(at)) => items.get_mut(*at),
        (Json::Object(pairs), Found::Key(key)) => pairs
            .iter_mut()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value),
        _ => None,
    }
}

/// `llJsonSetValue`: `value` written at the specifiers in `json` and the
/// document written anew; with no specifiers, index 0 is written and the
/// result's first and last characters are cut off (`[1,2]` set to 5 is
/// `5,2`, `{"a":1}` is `5`). The empty string starts as an empty array;
/// text LitJson cannot read, a path it cannot follow and a `JSON_DELETE` of
/// what is not there are `JSON_INVALID`.
pub(super) fn set_value(json: &str, specifiers: &[Element], value: &str) -> String {
    let no_specifiers = specifiers.is_empty();
    let zero = [Element::Integer(0)];
    let specifiers = if no_specifiers { &zero[..] } else { specifiers };
    let mut work = if json.is_empty() {
        Some(Json::Array(Vec::new()))
    } else {
        match to_object(json) {
            Ok(node) => node,
            Err(Thrown) => return INVALID.to_owned(),
        }
    };
    let Some(tree) = work.as_mut() else {
        return INVALID.to_owned();
    };
    match set_specific(tree, specifiers, 0, value) {
        Err(Thrown) => return INVALID.to_owned(),
        Ok(Some(replacement)) => work = Some(replacement),
        Ok(None) => {}
    }
    let written = to_json(&work);
    if !no_specifiers {
        return written;
    }
    // `r.Substring(1, r.Length - 2)`; the writer's output is ASCII.
    written
        .len()
        .checked_sub(1)
        .and_then(|end| written.get(1..end))
        .map_or_else(|| INVALID.to_owned(), str::to_owned)
}

/// `JsonFind`: the value at the specifiers from `level` on, an index
/// reading only an array and a string only an object; [`None`] where the
/// path leaves the tree. The answer may be a JSON null.
fn find(node: &Node, specifiers: &[Element], level: usize) -> Option<Node> {
    let json = node.as_ref()?;
    let next = match (json, specifiers.get(level)?) {
        (Json::Array(items), Element::Integer(index)) => {
            items.get(usize::try_from(*index).ok()?)?
        }
        (Json::Object(pairs), Element::String(key) | Element::Key(key)) => {
            let key = units(key);
            &pairs.iter().find(|(existing, _)| *existing == key)?.1
        }
        _ => return None,
    };
    if level.saturating_add(1) == specifiers.len() {
        return Some(next.clone());
    }
    match next {
        Some(Json::Array(_) | Json::Object(_)) => find(next, specifiers, level.saturating_add(1)),
        _ => None,
    }
}

/// The value `llJsonGetValue` and `llJsonValueType` read: [`None`] for
/// `JSON_INVALID`. Empty or white text, a path into `{}` or `[]`, a path
/// into text that does not start with `[` or `{`, text LitJson cannot read
/// and a path that leaves the tree are all invalid; text not starting with
/// `[` or `{` is read with no path as the first element of `[text]`.
fn lookup(json: &str, specifiers: &[Element]) -> Option<Node> {
    if json.chars().all(char::is_whitespace) {
        return None;
    }
    if !specifiers.is_empty() && (json == "{}" || json == "[]") {
        return None;
    }
    let zero = [Element::Integer(0)];
    let (text, specifiers) = if json.starts_with(['[', '{']) {
        (json.to_owned(), specifiers)
    } else if specifiers.is_empty() {
        (format!("[{json}]"), &zero[..])
    } else {
        return None;
    };
    let tree = to_object(&text).ok()?;
    if specifiers.is_empty() {
        return Some(tree);
    }
    find(&tree, specifiers, 0)
}

/// `llJsonGetValue`: the value as `JsonElementToString` writes it — an array
/// or object as JSON written anew, a string as itself, a boolean or `null`
/// as its sentinel, an integer as itself and a `double` with one to six
/// decimals (`2.5`, `-100.0`, `0.0` for `1e-7`).
pub(super) fn get_value(json: &str, specifiers: &[Element]) -> String {
    let Some(node) = lookup(json, specifiers) else {
        return INVALID.to_owned();
    };
    match &node {
        None | Some(Json::Untyped) => NULL.to_owned(),
        Some(Json::Array(_) | Json::Object(_)) => to_json(&node),
        Some(Json::Boolean(boolean)) => if *boolean { TRUE } else { FALSE }.to_owned(),
        Some(Json::Double(double)) => custom(*double, 1, 6),
        Some(Json::Int(int)) => int.to_string(),
        Some(Json::Long(long)) => long.to_string(),
        Some(Json::Str(text)) => string(text),
    }
}

/// `llJsonValueType`: the sentinel of the value's type — and of a string
/// that holds `JSON_NULL`, `JSON_TRUE` or `JSON_FALSE`, that sentinel.
pub(super) fn value_type(json: &str, specifiers: &[Element]) -> String {
    let Some(node) = lookup(json, specifiers) else {
        return INVALID.to_owned();
    };
    match node {
        None | Some(Json::Untyped) => NULL,
        Some(Json::Array(_)) => ARRAY,
        Some(Json::Boolean(true)) => TRUE,
        Some(Json::Boolean(false)) => FALSE,
        Some(Json::Double(_) | Json::Int(_) | Json::Long(_)) => NUMBER,
        Some(Json::Object(_)) => OBJECT,
        Some(Json::Str(text)) => {
            let text = string(&text);
            match text.as_str() {
                NULL => NULL,
                TRUE => TRUE,
                FALSE => FALSE,
                _ => STRING,
            }
        }
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    //! The rows of `data/json` on both OpenSim engines (which answered
    //! alike), and LitJson's corners as OpenSim's own `LSL_Api` answered
    //! them when called under Mono with the DLLs the local OpenSim runs.

    use pretty_assertions::assert_eq;
    use sl_types::lsl::{Rotation, Vector};

    use super::*;

    /// A string element.
    fn s(text: &str) -> Element {
        Element::String(text.to_owned())
    }

    /// An integer element.
    const fn i(value: i32) -> Element {
        Element::Integer(value)
    }

    /// A float element.
    const fn f(value: f32) -> Element {
        Element::Float(value)
    }

    /// `NULL_KEY` as a key element.
    fn null_key() -> Element {
        Element::Key(crate::value::NULL_KEY.to_owned())
    }

    #[test]
    fn list2json_rows() {
        let vector = Element::Vector(Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        });
        let rotation = Element::Rotation(Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            s: 1.0,
        });
        let rows: Vec<(&str, Vec<Element>, &str)> = vec![
            (
                ARRAY,
                vec![i(1), f(2.5), s("a"), vector.clone(), rotation, null_key()],
                "[1,2.500000,\"a\",\"<1.000000, 2.000000, 3.000000>\",\
                 \"<0.000000, 0.000000, 0.000000, 1.000000>\",\
                 \"00000000-0000-0000-0000-000000000000\"]",
            ),
            (
                ARRAY,
                vec![s("true"), s("false"), s("null"), s(TRUE), s(FALSE), s(NULL)],
                "[true,false,null,true,false,null]",
            ),
            (
                ARRAY,
                vec![
                    s("\"q\""),
                    s("[1]"),
                    s("{\"a\":1}"),
                    s(" [2] "),
                    s("{bad"),
                    s("[1,]"),
                ],
                "[\"\\\"q\\\"\",[1],{\"a\":1},\" [2] \",{bad,[1,]]",
            ),
            (
                ARRAY,
                [
                    "7", " 7 ", "-0", "1e3", "01", "1.", ".5", "0x10", "+1", "1.50",
                ]
                .iter()
                .map(|text| s(text))
                .collect(),
                "[\"7\",\" 7 \",\"-0\",\"1e3\",\"01\",\"1.\",\".5\",\"0x10\",\"+1\",\"1.50\"]",
            ),
            (
                ARRAY,
                vec![
                    s(""),
                    s("a\nb"),
                    s("a\\b"),
                    s("a/b"),
                    s("\u{1}\u{7F}"),
                    s("é€"),
                    s("😀"),
                ],
                "[\"\",\"a\\nb\",\"a\\\\b\",\"a\\/b\",\"\\u000\u{1}\u{7F}\",\"é€\",\"😀\"]",
            ),
            (
                ARRAY,
                vec![i(i32::MIN), i(i32::MAX), i(0), i(-1)],
                "[-2147483648,2147483647,0,-1]",
            ),
            (ARRAY, vec![], "[]"),
            (
                ARRAY,
                vec![s(INVALID), s(DELETE), s(OBJECT), s(STRING)],
                "[\"\u{FDD0}\",\"\u{FDD8}\",\"\u{FDD1}\",\"\u{FDD4}\"]",
            ),
            (
                OBJECT,
                vec![
                    s("a"),
                    i(1),
                    s("b"),
                    s("x"),
                    s("c"),
                    s("[1,2]"),
                    s("d"),
                    f(2.5),
                ],
                "{\"a\":1,\"b\":\"x\",\"c\":[1,2],\"d\":2.500000}",
            ),
            (
                OBJECT,
                vec![s("a"), i(1), s("a"), i(2)],
                "{\"a\":1,\"a\":2}",
            ),
            (OBJECT, vec![s("a")], INVALID),
            (OBJECT, vec![i(1), i(2)], INVALID),
            (OBJECT, vec![vector, i(1)], INVALID),
            (
                OBJECT,
                vec![null_key(), i(1)],
                "{\"00000000-0000-0000-0000-000000000000\":1}",
            ),
            (OBJECT, vec![s(""), i(1)], "{\"\":1}"),
            (OBJECT, vec![], "{}"),
            (STRING, vec![s("a")], INVALID),
            ("x", vec![s("a")], INVALID),
        ];
        for (kind, values, expected) in rows {
            assert_eq!(list2_json(kind, &values), expected, "{values:?}");
        }
    }

    /// `floats`: `[3.141593,100000000000000000000.000000,0.000000,0.000000,
    /// 0.000000,1000000000000000000000000000000000000000.000000,-1000…]` on
    /// OpenSim, whose floats are doubles. The runtime's are `f32`: `1.0e20`
    /// is `100000002004087734272` and `(float)"1e39"` an infinity, which the
    /// library writes as `"Inf"` — the value model, not the library.
    #[test]
    fn list2json_floats_differ_only_by_the_value_model() {
        let values = [
            f(core::f32::consts::PI),
            f(1.0e20),
            f(-0.0),
            f(0.000_000_1),
            f(1.0e-20),
            f(f32::INFINITY),
            f(f32::NEG_INFINITY),
        ];
        assert_eq!(
            list2_json(ARRAY, &values),
            "[3.141593,100000002004088000000.000000,0.000000,0.000000,0.000000,\"Inf\",\"Inf\"]"
        );
    }

    #[test]
    fn json2list_rows() {
        let rows: Vec<(&str, Vec<Element>)> = vec![
            (
                "[1,2.5,\"a\",true,false,null,[1,2],{\"a\":1}]",
                vec![
                    i(1),
                    f(2.5),
                    s("a"),
                    s(TRUE),
                    s(FALSE),
                    s(NULL),
                    s("[1,2]"),
                    s("{\"a\":1}"),
                ],
            ),
            (
                "{\"a\":1,\"b\":[2,3],\"c\":{\"d\":null}}",
                vec![s("a"), i(1), s("b"), s("[2,3]"), s("c"), s("{\"d\":null}")],
            ),
            ("  [ 1 , 2 ]  ", vec![s("  [ 1 , 2 ]  ")]),
            ("1", vec![s("1")]),
            ("\"s\"", vec![s("\"s\"")]),
            ("true", vec![s("true")]),
            ("", vec![]),
            ("garbage", vec![s("garbage")]),
            ("[1,", vec![s("[1,")]),
            ("[1,]", vec![s("[1,]")]),
            ("{\"a\"}", vec![s("{\"a\"}")]),
            ("[01]", vec![s("[01]")]),
            ("[1.]", vec![s("[1.]")]),
            ("[.5]", vec![s("[.5]")]),
            (
                "[1e5,1E-2,-0,2.50]",
                vec![f(100_000.0), f(0.01), i(0), f(2.5)],
            ),
            ("[\"\\u0041\\n\\/\\\"\\\\\"]", vec![s("A\n/\"\\")]),
            ("[\"a\" \"b\"]", vec![s("[\"a\" \"b\"]")]),
            (
                "[12345678901234567890,2147483648,1.5e300,-1e400]",
                vec![s("[12345678901234567890,2147483648,1.5e300,-1e400]")],
            ),
            ("{\"a\":1,\"a\":2}", vec![s("a"), i(2)]),
            ("[[],{}]", vec![s("[]"), s("{}")]),
            ("{}", vec![]),
            ("[]", vec![]),
            ("[\"\\ud83d\\ude00\"]", vec![s("😀")]),
            // A lone surrogate on OpenSim; U+FFFD here.
            ("[\"\\ud83d\"]", vec![s("\u{FFFD}")]),
            ("[\"é\"]", vec![s("é")]),
            ("[truth]", vec![s("[truth]")]),
            ("{'a':1}", vec![s("a"), i(1)]),
            ("[1] x", vec![s("[1] x")]),
            ("[\"a\u{1}b\"]", vec![s("a\u{1}b")]),
            ("[\"a\nb\"]", vec![s("a\nb")]),
        ];
        for (json, expected) in rows {
            assert_eq!(json2_list(json), expected, "{json}");
        }
    }

    /// The `get` rows: the value and its type.
    #[test]
    fn get_rows() {
        let document =
            "{\"a\":1,\"b\":[10,\"x\",true,null,{\"c\":2.50}],\"s\":\"t\\\"u\",\"n\":-1.0e2}";
        let rows: Vec<(&str, Vec<Element>, &str, &str)> = vec![
            (
                document,
                vec![],
                "{\"a\":1,\"b\":[10,\"x\",true,null,{\"c\":2.5}],\"s\":\"t\\\"u\",\"n\":-100.0}",
                OBJECT,
            ),
            (document, vec![s("a")], "1", NUMBER),
            (
                document,
                vec![s("b")],
                "[10,\"x\",true,null,{\"c\":2.5}]",
                ARRAY,
            ),
            (document, vec![s("b"), i(0)], "10", NUMBER),
            (document, vec![s("b"), i(1)], "x", STRING),
            (document, vec![s("b"), i(2)], TRUE, TRUE),
            (document, vec![s("b"), i(3)], NULL, NULL),
            (document, vec![s("b"), i(4), s("c")], "2.5", NUMBER),
            (document, vec![s("b"), i(5)], INVALID, INVALID),
            (document, vec![s("b"), i(-1)], INVALID, INVALID),
            (document, vec![s("b"), s(APPEND)], INVALID, INVALID),
            (document, vec![s("b"), s("0")], INVALID, INVALID),
            (document, vec![s("b"), f(1.0)], INVALID, INVALID),
            (document, vec![s("a"), i(0)], INVALID, INVALID),
            (document, vec![s("b"), i(0), i(0)], INVALID, INVALID),
            (document, vec![s("z")], INVALID, INVALID),
            (document, vec![i(0)], INVALID, INVALID),
            (document, vec![s("s")], "t\"u", STRING),
            (document, vec![s("n")], "-100.0", NUMBER),
            ("[1,2", vec![i(0)], INVALID, INVALID),
            ("[1,2", vec![], INVALID, INVALID),
            ("\"str\"", vec![], "str", STRING),
            ("  7 ", vec![], "7", NUMBER),
            ("abc", vec![], INVALID, INVALID),
            ("", vec![], INVALID, INVALID),
            ("{\"a\":1,\"a\":2}", vec![s("a")], "2", NUMBER),
            ("[1,2,}", vec![i(0)], INVALID, INVALID),
        ];
        for (json, path, value, kind) in rows {
            assert_eq!(get_value(json, &path), value, "{json} {path:?}");
            assert_eq!(value_type(json, &path), kind, "{json} {path:?}");
        }
    }

    #[test]
    fn set_rows() {
        let o = "{\"a\":1}";
        let a = "[1,2]";
        let rows: Vec<(&str, Vec<Element>, &str, &str)> = vec![
            (o, vec![s("a")], "2", "{\"a\":2}"),
            (o, vec![s("b")], "x", "{\"a\":1,\"b\":\"x\"}"),
            (o, vec![s("b")], "true", "{\"a\":1,\"b\":true}"),
            (o, vec![s("b")], "1.50", "{\"a\":1,\"b\":1.5}"),
            (o, vec![s("b")], "[1, 2]", "{\"a\":1,\"b\":\"[1, 2]\"}"),
            (o, vec![s("b")], "\"q\"", "{\"a\":1,\"b\":\"\\\"q\\\"\"}"),
            (o, vec![s("b")], "{bad", "{\"a\":1,\"b\":\"{bad\"}"),
            (o, vec![s("b")], "  7 ", "{\"a\":1,\"b\":7}"),
            (o, vec![s("b")], "-0", "{\"a\":1,\"b\":0}"),
            (o, vec![s("b")], "01", "{\"a\":1,\"b\":1}"),
            (o, vec![s("b")], "", "{\"a\":1,\"b\":\"\"}"),
            (o, vec![s("b")], TRUE, "{\"a\":1,\"b\":true}"),
            (o, vec![s("b")], NULL, "{\"a\":1,\"b\":null}"),
            (o, vec![s("b")], INVALID, "{\"a\":1,\"b\":\"\\uFDD0\"}"),
            (o, vec![s("a")], DELETE, "{}"),
            (o, vec![s("z")], DELETE, INVALID),
            (o, vec![s("x"), s("y")], "1", "{\"a\":1,\"x\":{\"y\":1}}"),
            (o, vec![s("x"), i(0)], "1", "{\"a\":1,\"x\":[1]}"),
            (o, vec![s("x"), i(1)], "1", INVALID),
            (o, vec![], "5", "5"),
            (o, vec![i(0)], "1", "[1]"),
            (o, vec![s("a"), s("b")], "1", "{\"a\":{\"b\":1}}"),
            (a, vec![i(0)], "9", "[9,2]"),
            (a, vec![i(2)], "3", "[1,2,3]"),
            (a, vec![i(3)], "4", INVALID),
            (a, vec![s(APPEND)], "5", "[1,2,5]"),
            (a, vec![i(1)], DELETE, "[1]"),
            (a, vec![i(5)], DELETE, INVALID),
            (a, vec![s("k")], "1", "[1,2]"),
            (a, vec![i(-2)], "x", INVALID),
            (a, vec![i(5), i(0)], "x", INVALID),
            (a, vec![i(2), i(0)], "x", "[1,2,[\"x\"]]"),
            ("[1", vec![i(0)], "2", INVALID),
            ("", vec![s("a")], "1", "{\"a\":1}"),
            ("", vec![i(0)], "1", "[1]"),
            ("abc", vec![s("a")], "1", INVALID),
            ("  {\"a\" : 1 }  ", vec![s("b")], "2", "{\"a\":1,\"b\":2}"),
            (
                "  {\"a\" : 1 , \"c\" : [ 1 ] }  ",
                vec![s("a")],
                "2",
                "{\"a\":2,\"c\":[1]}",
            ),
            ("{\"a\":1,\"a\":2}", vec![s("a")], "3", "{\"a\":3}"),
        ];
        for (json, path, value, expected) in rows {
            assert_eq!(
                set_value(json, &path, value),
                expected,
                "{json} {path:?} {value}"
            );
        }
    }

    /// LitJson's reading, as OpenSim's `llJson2List` answered it under Mono.
    #[test]
    fn litjson_reads() {
        let rows: Vec<(&str, Vec<Element>)> = vec![
            // `1e400` fails `double.TryParse` and falls through to `int` 0;
            // `1.5e300` is a double (an infinity once a float here).
            ("[-1e400]", vec![i(0)]),
            ("[1e400]", vec![i(0)]),
            ("[1.5e300]", vec![f(f32::INFINITY)]),
            // A `long` in the top level throws in `JsonParseTopNodes`.
            ("[2147483648]", vec![s(INVALID)]),
            // Comments are white space.
            ("[1,/*c*/2]", vec![i(1), i(2)]),
            ("[1]//x", vec![i(1)]),
            ("[1]/*x*/", vec![i(1)]),
            // Only the look-ahead token after the document is read.
            ("[1] 2", vec![i(1)]),
            ("[1] [", vec![i(1)]),
            ("[1 ]", vec![i(1)]),
            ("[1 2]", vec![s("[1 2]")]),
            ("[null,1]", vec![s(NULL), i(1)]),
            (
                "[1,[null],{\"b\":null}]",
                vec![i(1), s("[null]"), s("{\"b\":null}")],
            ),
            ("[ ]", vec![]),
            ("{ }", vec![]),
            ("[", vec![s("[")]),
            ("[\"a\",]", vec![s("[\"a\",]")]),
            ("{\"a\":1,}", vec![s("{\"a\":1,}")]),
            ("[-]", vec![s("[-]")]),
            ("[-0]", vec![i(0)]),
            ("[-0.0]", vec![f(-0.0)]),
            ("[0.5e]", vec![s("[0.5e]")]),
            ("[1e+2]", vec![f(100.0)]),
            ("[1e+]", vec![i(0)]),
            ("[tr]", vec![s("[tr]")]),
            ("[True]", vec![s("[True]")]),
            ("[\"a\\u0041\\x\"]", vec![s("[\"a\\u0041\\x\"]")]),
            ("['a\"b']", vec![s("a\"b")]),
            ("[\"a'b\"]", vec![s("a'b")]),
            ("[\"\\'\"]", vec![s("'")]),
            ("/*c*/[1]", vec![s("/*c*/[1]")]),
            (
                "[1,{\"a\":[1,2.5,\"x\\n\"]}]",
                vec![i(1), s("{\"a\":[1,2.5,\"x\\n\"]}")],
            ),
            ("{\"z\":1,\"a\":2}", vec![s("z"), i(1), s("a"), i(2)]),
            // A repeated key keeps the first one's place.
            (
                "{\"a\":1,\"b\":2,\"a\":3}",
                vec![s("a"), i(3), s("b"), i(2)],
            ),
            ("[[1,2,3]]", vec![s("[1,2,3]")]),
            ("[{\"k\":\"\\u00e9\"}]", vec![s("{\"k\":\"\\u00E9\"}")]),
        ];
        for (json, expected) in rows {
            assert_eq!(json2_list(json), expected, "{json}");
        }
    }

    /// LitJson's writing and `JsonElementToString`'s numbers, as OpenSim
    /// answered them under Mono.
    #[test]
    fn litjson_writes_and_reads_values() {
        let numbers = "[1.0E+25,1e-7,123456789012.5,0.1,100,1e15,1e14,123456789012345678]";
        assert_eq!(
            get_value(numbers, &[]),
            "[1E+25,1E-07,123456789012.5,0.1,100,1E+15,100000000000000.0,123456789012345678]"
        );
        assert_eq!(get_value(numbers, &[i(0)]), "10000000000000000000000000.0");
        assert_eq!(get_value(numbers, &[i(1)]), "0.0");
        assert_eq!(get_value(numbers, &[i(2)]), "123456789012.5");
        assert_eq!(get_value(numbers, &[i(6)]), "100000000000000.0");
        assert_eq!(get_value(numbers, &[i(7)]), "123456789012345678");
        assert_eq!(value_type(numbers, &[i(7)]), NUMBER);
        assert_eq!(
            get_value(
                "[0.00001,0.0001,1.5e-5,123456789012345.6,1234567890123456.7]",
                &[]
            ),
            "[1E-05,0.0001,1.5E-05,123456789012346.0,1.23456789012346E+15]"
        );
        assert_eq!(
            get_value("[true,false,null,\"\\u00e9\\u0001/\"]", &[]),
            "[true,false,null,\"\\u00E9\\u0001/\"]"
        );
        assert_eq!(get_value("{\"a\":null}", &[s("a")]), NULL);
        assert_eq!(get_value("[\"\u{FDD5}\",\"\u{FDD6}\"]", &[i(0)]), NULL);
        assert_eq!(value_type("[\"\u{FDD5}\",\"\u{FDD6}\"]", &[i(1)]), TRUE);
        assert_eq!(value_type("[\"x\"]", &[i(0)]), STRING);
        assert_eq!(get_value("[1,2]", &[s("0")]), INVALID);
        assert_eq!(get_value("7", &[]), "7");
        assert_eq!(get_value("7", &[i(0)]), INVALID);
        assert_eq!(get_value("\"a\"", &[]), "a");
        assert_eq!(get_value("  [1]", &[]), "[1]");
        assert_eq!(get_value(" ", &[]), INVALID);
        assert_eq!(value_type("true", &[]), TRUE);
    }

    /// `llJsonSetValue`'s `float.TryParse` and six-decimal rounding, as
    /// OpenSim answered them under Mono.
    #[test]
    fn set_value_reads_numbers_as_float_try_parse() {
        let rows = [
            ("1,000", "[1000]"),
            ("1e39", "[\"1e39\"]"),
            ("3.4e38", "[3.39999995214436E+38]"),
            ("0x10", "[\"0x10\"]"),
            ("1e", "[\"1e\"]"),
            ("Infinity", "[Infinity.0]"),
            ("NaN", "[NaN.0]"),
            ("-0", "[0]"),
            ("-0.0", "[0.0]"),
            (" 7 ", "[7]"),
            ("1.25", "[1.25]"),
            ("123456.7", "[123456.703125]"),
            ("0.0000001", "[0.0]"),
            ("2147483648", "[2147483648.0]"),
            ("-2147483649", "[-2147483648]"),
            ("1e10", "[10000000000.0]"),
            ("$5", "[\"$5\"]"),
            ("(5)", "[\"(5)\"]"),
            ("5-", "[\"5-\"]"),
            ("+5", "[5]"),
            (".5", "[0.5]"),
            ("5.", "[5.0]"),
            ("1E5", "[100000]"),
            ("1e25", "[9.99999956202353E+24]"),
            ("0.1", "[0.100000001490116]"),
            ("16777217", "[16777216]"),
            ("a/b", "[\"a/b\"]"),
        ];
        for (value, expected) in rows {
            assert_eq!(set_value("[1]", &[i(0)], value), expected, "{value}");
        }
    }

    /// `llJsonSetValue`'s paths, as OpenSim answered them under Mono.
    #[test]
    fn set_value_paths() {
        let rows: Vec<(&str, Vec<Element>, &str, &str)> = vec![
            ("", vec![s("a"), i(0)], "1", "{\"a\":[1]}"),
            ("", vec![i(1)], "1", INVALID),
            (" ", vec![s("a")], "1", "{\"a\":1}"),
            ("[]", vec![i(0)], "x", "[\"x\"]"),
            ("[]", vec![i(1)], "x", INVALID),
            ("[]", vec![s(APPEND)], "x", "[\"x\"]"),
            ("[1,2]", vec![s(APPEND), s(APPEND)], "x", "[1,2,[\"x\"]]"),
            ("{}", vec![s("a")], DELETE, INVALID),
            ("[1,[2,3]]", vec![i(1), i(0)], DELETE, "[1,[3]]"),
            ("[1,[2,3]]", vec![i(1), i(5)], DELETE, INVALID),
            ("[1,[2,3]]", vec![i(1), i(2)], DELETE, INVALID),
            (
                "{\"a\":{\"b\":1}}",
                vec![s("a"), s("b")],
                DELETE,
                "{\"a\":{}}",
            ),
            ("{\"a\":{\"b\":1}}", vec![s("a"), s("c")], DELETE, INVALID),
            ("{\"a\":1}", vec![i(1)], "x", "{\"a\":1}"),
            ("{\"a\":1}", vec![i(0), i(0)], "x", "[[\"x\"]]"),
            (
                "{\"a\":[1]}",
                vec![s("a"), s("k")],
                "x",
                "{\"a\":{\"k\":\"x\"}}",
            ),
            ("{\"a\":[1,2]}", vec![s("a"), s("k")], "x", "{\"a\":[1,2]}"),
            ("[1]", vec![s("k")], "x", "{\"k\":\"x\"}"),
            ("[1,2]", vec![i(0), s("k")], "x", "[{\"k\":\"x\"},2]"),
            ("[[1,2],2]", vec![i(0), s("k")], "x", "[[1,2],2]"),
            ("[[1],2]", vec![i(0), s("k")], "x", "[{\"k\":\"x\"},2]"),
            ("{\"a\":1}", vec![f(1.0)], "x", INVALID),
            ("{\"a\":1}", vec![s("a")], NULL, "{\"a\":null}"),
            ("{\"a\":1}", vec![s("a")], "null", "{\"a\":null}"),
            ("{\"a\":1}", vec![s("a")], FALSE, "{\"a\":false}"),
            ("{\"a\":1}", vec![s("a")], "false", "{\"a\":false}"),
            ("{\"a\":1}", vec![s("a")], OBJECT, "{\"a\":\"\\uFDD1\"}"),
            ("[1,2]", vec![], "5", "5,2"),
            ("7", vec![], "5", "5"),
            ("\"x\"", vec![], "5", INVALID),
            ("{\"a\":1}", vec![], DELETE, INVALID),
            (
                "{\"a\":1}",
                vec![s("x"), i(0), s("y")],
                "true",
                "{\"a\":1,\"x\":[{\"y\":true}]}",
            ),
            ("{\"a\":1}", vec![s("x"), i(1)], "true", INVALID),
            (
                "{\"a\":1}",
                vec![s("x"), s(APPEND)],
                "true",
                "{\"a\":1,\"x\":[true]}",
            ),
            ("{\"a\":1}", vec![s("x"), f(1.0)], "true", INVALID),
            (
                "{\"a\":\"é\"}",
                vec![s("b")],
                "é",
                "{\"a\":\"\\u00E9\",\"b\":\"\\u00E9\"}",
            ),
            (
                "{\"a\":1}",
                vec![s("b")],
                "\u{1}",
                "{\"a\":1,\"b\":\"\\u0001\"}",
            ),
            ("{\"a\":[1, 2]}", vec![s("b")], "1", "{\"a\":[1,2],\"b\":1}"),
            // Setting through a JSON null throws (`JsonSetSpecific(null, …)`).
            ("{\"a\":null}", vec![s("a"), s("b")], "1", INVALID),
            // Half a surrogate pair survives a parse and a write.
            ("[\"\\ud83d\"]", vec![i(1)], "1", "[\"\\uD83D\",1]"),
        ];
        for (json, path, value, expected) in rows {
            assert_eq!(
                set_value(json, &path, value),
                expected,
                "{json} {path:?} {value}"
            );
        }
    }

    /// `llList2Json`'s corners, as OpenSim answered them under Mono.
    #[test]
    fn list2json_corners() {
        assert_eq!(list2_json(OBJECT, &[s("a"), i(1), i(2), i(3)]), INVALID);
        assert_eq!(
            list2_json(OBJECT, &[s("k\u{1}\"/"), i(1)]),
            "{\"k\\u000\u{1}\\\"\\/\":1}"
        );
        assert_eq!(
            list2_json(
                ARRAY,
                &[
                    s(TRUE),
                    s(FALSE),
                    s(NULL),
                    s("true"),
                    s("{x"),
                    s("["),
                    s("")
                ]
            ),
            "[true,false,null,true,{x,[,\"\"]"
        );
        assert_eq!(
            list2_json(
                ARRAY,
                &[f(-0.0), f(0.000_000_1), f(-0.000_000_1), f(0.5), f(-2.5)]
            ),
            "[0.000000,0.000000,0.000000,0.500000,-2.500000]"
        );
        assert_eq!(list2_json(ARRAY, &[f(f32::NAN)]), "[\"NaN\"]");
    }

    /// .NET Framework's `"G"` and custom formats of a `double`, as Mono
    /// printed them.
    #[test]
    fn dotnet_number_formats() {
        let rows: [(f64, &str, &str, &str); 24] = [
            (0.0, "0", "0.000000", "0.0"),
            (-0.0, "0", "0.000000", "0.0"),
            (
                1e25,
                "1E+25",
                "10000000000000000000000000.000000",
                "10000000000000000000000000.0",
            ),
            (1e-7, "1E-07", "0.000000", "0.0"),
            (
                1e15,
                "1E+15",
                "1000000000000000.000000",
                "1000000000000000.0",
            ),
            (
                1e14,
                "100000000000000",
                "100000000000000.000000",
                "100000000000000.0",
            ),
            (
                123_456_789_012_345.6,
                "123456789012346",
                "123456789012346.000000",
                "123456789012346.0",
            ),
            (
                1_234_567_890_123_456.7,
                "1.23456789012346E+15",
                "1234567890123460.000000",
                "1234567890123460.0",
            ),
            (1.0 / 3.0, "0.333333333333333", "0.333333", "0.333333"),
            (2.0 / 3.0, "0.666666666666667", "0.666667", "0.666667"),
            (1e-5, "1E-05", "0.000010", "0.00001"),
            (0.0001, "0.0001", "0.000100", "0.0001"),
            (-1.5e-10, "-1.5E-10", "0.000000", "0.0"),
            (0.000_000_5, "5E-07", "0.000001", "0.000001"),
            (0.000_001_5, "1.5E-06", "0.000002", "0.000002"),
            (0.000_002_5, "2.5E-06", "0.000003", "0.000003"),
            (1.000_000_5, "1.0000005", "1.000001", "1.000001"),
            (
                3.4e38,
                "3.4E+38",
                "340000000000000000000000000000000000000.000000",
                "340000000000000000000000000000000000000.0",
            ),
            (123.456_789_5, "123.4567895", "123.456790", "123.45679"),
            (0.123_456_5, "0.1234565", "0.123457", "0.123457"),
            (
                999_999.999_999_5,
                "999999.9999995",
                "1000000.000000",
                "1000000.0",
            ),
            (
                1e16,
                "1E+16",
                "10000000000000000.000000",
                "10000000000000000.0",
            ),
            (0.300_000_000_000_000_04, "0.3", "0.300000", "0.3"),
            (1.000_000_000_000_000_2, "1", "1.000000", "1.0"),
        ];
        for (value, g, six, short) in rows {
            assert_eq!(general(value), g, "{value:e}");
            assert_eq!(custom(value, 6, 6), six, "{value:e}");
            assert_eq!(custom(value, 1, 6), short, "{value:e}");
        }
        assert_eq!(general(f64::MAX), "1.79769313486232E+308");
        assert_eq!(general(5e-324), "4.94065645841247E-324");
    }

    /// `Math.Round(value, 6)`, as Mono rounded it.
    #[test]
    fn math_round_six() {
        let rows = [
            (0.000_001_5, 2e-6),
            (0.000_002_5, 2e-6),
            (0.123_456_5, 0.123_456),
            (123.456_789_5, 123.456_79),
            (999_999.999_999_5, 1_000_000.0),
            (1e-7, 0.0),
        ];
        for (value, rounded) in rows {
            assert_eq!(round6(value).to_bits(), f64::to_bits(rounded), "{value:e}");
        }
    }

    /// `float.TryParse` and `double.TryParse` under `en-US`, as Mono
    /// answered them.
    #[test]
    fn dotnet_parses() {
        let rows: [(&str, Option<f32>, Option<f64>); 26] = [
            ("1,000", Some(1000.0), Some(1000.0)),
            (",5", None, None),
            ("1,,0", Some(10.0), Some(10.0)),
            ("1,0.5", Some(10.5), Some(10.5)),
            ("1.5,0", None, None),
            ("1e-400", Some(0.0), Some(0.0)),
            ("1e39", None, Some(1e39)),
            ("3.5e38", None, Some(3.5e38)),
            ("-3.4028235e38", Some(f32::MIN), Some(-3.402_823_5e38)),
            ("1e-50", Some(0.0), Some(1e-50)),
            (" 1 ", Some(1.0), Some(1.0)),
            ("1 2", None, None),
            ("\t1\n", Some(1.0), Some(1.0)),
            ("1e+5", Some(100_000.0), Some(100_000.0)),
            ("infinity", None, None),
            (
                "-Infinity",
                Some(f32::NEG_INFINITY),
                Some(f64::NEG_INFINITY),
            ),
            ("∞", None, None),
            ("1d", None, None),
            ("1f", None, None),
            ("0x1", None, None),
            ("00012", Some(12.0), Some(12.0)),
            ("1.e5", Some(100_000.0), Some(100_000.0)),
            ("e5", None, None),
            ("-", None, None),
            (".", None, None),
            ("1_0", None, None),
        ];
        for (text, single, double) in rows {
            assert_eq!(parse_float(text), single, "{text}");
            assert_eq!(parse_double(text), double, "{text}");
        }
        assert_eq!(parse_double("1e400"), None);
    }
}
