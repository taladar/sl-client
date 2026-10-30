//! Library tranche: encodings and hashes (`server-lsl-lib-strings-lists`) —
//! URL escaping, Base64 and its XOR functions, and the digests.
//!
//! Second Life's answers are PyOptimizer's (`lslbasefuncs.py`), held to
//! aditi's `strings/functions` and `strings/newer` transcripts; OpenSim's are
//! ported from `LSL_Api.cs` (`Uri.EscapeDataString`,
//! `Convert.FromBase64String`, the `c2itable` of `llBase64ToInteger`) and
//! held to the local grid's.

use hmac::digest::Digest;
use hmac::digest::common::BlockSizeUser;
use hmac::{KeyInit as _, Mac as _, SimpleHmac};

use crate::library::text::until_nul;
use crate::library::{BuiltinId, CallError, Exception, PureCtx};

/// The hex digits, upper case.
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// The hex digits, lower case.
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// `byte` as two hex digits from `digits`.
fn push_hex(out: &mut String, byte: u8, digits: &[u8; 16]) {
    for nibble in [byte >> 4, byte & 0x0F] {
        out.push(char::from(
            digits.get(usize::from(nibble)).copied().unwrap_or(b'0'),
        ));
    }
}

/// The four bytes of a word, most significant first.
fn big_endian(word: u32) -> [u8; 4] {
    [24_u32, 16, 8, 0].map(|shift| u8::try_from(word >> shift & 0xFF).unwrap_or(0))
}

/// The Base64 alphabet.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The value of a Base64 character.
fn sextet(byte: u8) -> Option<u8> {
    ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .and_then(|at| u8::try_from(at).ok())
}

/// Standard Base64 with padding.
fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|at| chunk.get(at).copied().unwrap_or(0));
        let word = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        for (index, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                let at = usize::try_from(word >> shift & 0x3F).unwrap_or(0);
                out.push(char::from(ALPHABET.get(at).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decode Base64 characters with no padding and no stray characters — two,
/// three or four of them make a group; whatever bits a short last group
/// leaves over are ignored.
fn decode(characters: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for group in characters.chunks(4) {
        let values: Vec<u32> = group
            .iter()
            .filter_map(|byte| sextet(*byte).map(u32::from))
            .collect();
        let word = values
            .iter()
            .chain(core::iter::repeat(&0))
            .take(4)
            .fold(0_u32, |word, value| word << 6 | value);
        let bytes = big_endian(word);
        let count = values.len().saturating_sub(1);
        out.extend(
            bytes
                .get(1..1_usize.saturating_add(count))
                .unwrap_or_default(),
        );
    }
    out
}

/// The Base64 Second Life reads from `text`: the leading run of Base64
/// characters, cut to whole groups and a last group of two or three
/// (PyOptimizer's `b64_re`) — so padding, and anything after the first
/// character that is not Base64, is ignored.
fn sl_base64_prefix(text: &str) -> &[u8] {
    let bytes = text.as_bytes();
    let run = bytes
        .iter()
        .position(|byte| sextet(*byte).is_none())
        .unwrap_or(bytes.len());
    let usable = if run % 4 == 1 {
        run.saturating_sub(1)
    } else {
        run
    };
    bytes.get(..usable).unwrap_or_default()
}

/// Bytes as text the way Second Life's Mono converts UTF-8: each byte of a
/// sequence that is not UTF-8 — an overlong form, a surrogate, past U+10FFFF,
/// or U+FFFE — becomes `?`, and the text ends at the first NUL
/// (PyOptimizer's `InternalUTF8toString`).
fn sl_utf8(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut at = 0_usize;
    while let Some(lead) = bytes.get(at) {
        let width = match lead {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => 0,
        };
        let decoded = bytes
            .get(at..at.saturating_add(width))
            .filter(|_| width > 0)
            .and_then(|sequence| core::str::from_utf8(sequence).ok())
            .filter(|sequence| *sequence != "\u{FFFE}");
        if let Some(sequence) = decoded {
            out.push_str(sequence);
            at = at.saturating_add(width);
        } else {
            out.push('?');
            at = at.saturating_add(1);
        }
    }
    until_nul(out)
}

/// `llEscapeURL`: every UTF-8 byte but the ASCII letters and digits as `%XX`
/// on Second Life — `~ - _ .` too; OpenSim leaves those four alone
/// (`Uri.EscapeDataString`).
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_escape_url<C: PureCtx>(ctx: &mut C, text: String) -> Result<String, CallError> {
    let keep_unreserved = ctx.flavour().is_opensim();
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric()
            || (keep_unreserved && matches!(byte, b'-' | b'_' | b'.' | b'~'))
        {
            out.push(char::from(byte));
        } else {
            out.push('%');
            push_hex(&mut out, byte, HEX_UPPER);
        }
    }
    Ok(out)
}

/// The value of an ASCII hex digit.
fn hex(character: char) -> Option<u8> {
    character
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

/// `llUnescapeURL`: each `%XX` decoded and the bytes read as UTF-8.
///
/// Second Life reads a `%` and the two characters after it as a byte
/// whatever they are — a character that is no hex digit counts as 0, and a
/// `%` in the second place ends the escape — so `%zz` is a NUL, at which the
/// text ends (`llUnescapeURL("%zz%4%e9%C3%A9%")` is `""`); a byte that is no
/// UTF-8 becomes `?`. OpenSim (`Uri.UnescapeDataString`) decodes only a
/// well-formed escape, and leaves one whose bytes are no UTF-8 as it was.
/// `+` is a plus on both.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_unescape_url<C: PureCtx>(ctx: &mut C, text: String) -> Result<String, CallError> {
    if ctx.flavour().is_opensim() {
        return Ok(os_unescape(&text));
    }
    let characters: Vec<char> = text.chars().collect();
    let mut bytes = Vec::new();
    let mut at = 0_usize;
    while let Some(character) = characters.get(at).copied() {
        at = at.saturating_add(1);
        if character != '%' {
            let mut buffer = [0_u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        let Some(first) = characters.get(at).copied() else {
            break;
        };
        at = at.saturating_add(1);
        let Some(second) = characters.get(at).copied() else {
            break;
        };
        let high = hex(first).unwrap_or(0) << 4;
        at = at.saturating_add(1);
        if second == '%' {
            bytes.push(high);
            continue;
        }
        bytes.push(high | hex(second).unwrap_or(0));
    }
    Ok(sl_utf8(&bytes))
}

/// `Uri.UnescapeDataString`: see [`ll_unescape_url`].
fn os_unescape(text: &str) -> String {
    let source = text.as_bytes();
    let escape = |at: usize| -> Option<u8> {
        if source.get(at) != Some(&b'%') {
            return None;
        }
        let digit = |offset: usize| {
            source
                .get(at.saturating_add(offset))
                .and_then(|byte| hex(char::from(*byte)))
        };
        Some(digit(1)? << 4 | digit(2)?)
    };
    let mut out = String::new();
    let mut at = 0_usize;
    while at < source.len() {
        match escape(at) {
            Some(byte) if byte < 0x80 => {
                out.push(char::from(byte));
                at = at.saturating_add(3);
            }
            Some(_) => {
                // A run of escaped bytes, decoded where it is UTF-8 and kept
                // as written where it is not.
                let mut run = Vec::new();
                let mut end = at;
                while let Some(byte) = escape(end).filter(|byte| *byte >= 0x80) {
                    run.push(byte);
                    end = end.saturating_add(3);
                }
                let mut index = 0_usize;
                while let Some(lead) = run.get(index) {
                    let width = match lead {
                        0xC2..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        0xF0..=0xF4 => 4,
                        _ => 0,
                    };
                    let decoded = run
                        .get(index..index.saturating_add(width))
                        .filter(|_| width > 0)
                        .and_then(|sequence| core::str::from_utf8(sequence).ok());
                    if let Some(sequence) = decoded {
                        out.push_str(sequence);
                        index = index.saturating_add(width);
                    } else {
                        // Kept as an escape, in upper case whatever it
                        // was written in (`%e9` comes back `%E9`).
                        out.push('%');
                        push_hex(&mut out, *lead, HEX_UPPER);
                        index = index.saturating_add(1);
                    }
                }
                at = end;
            }
            None => {
                let width = text
                    .get(at..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8);
                out.push_str(text.get(at..at.saturating_add(width)).unwrap_or_default());
                at = at.saturating_add(width);
            }
        }
    }
    out
}

/// `llStringToBase64`: the Base64 of the string's UTF-8.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_string_to_base64<C>(_ctx: &mut C, text: String) -> Result<String, CallError> {
    Ok(encode(text.as_bytes()))
}

/// Whether a byte is `[\x80-\xBF]`, a UTF-8 continuation.
const fn is_continuation(byte: u8) -> bool {
    matches!(byte, 0x80..=0xBF)
}

/// How many bytes at `at` Second Life's `llBase64ToString` lets through to
/// the UTF-8 conversion, or [`None`] when they are a sequence it replaces by
/// one `?` (the first group of PyOptimizer's `b64tostr_re`).
fn sl_passes(bytes: &[u8], at: usize) -> Option<usize> {
    let byte = |offset: usize| bytes.get(at.saturating_add(offset)).copied();
    let continuations = |from: usize, count: usize| {
        (from..from.saturating_add(count)).all(|offset| byte(offset).is_some_and(is_continuation))
    };
    let lead = byte(0)?;
    // A NUL is let through only as the last byte (or before a last line
    // feed: the pattern's `$`), where the NUL ending drops it.
    let last = bytes.len().saturating_sub(1);
    if lead == 0 && (at == last || (at.saturating_add(1) == last && byte(1) == Some(b'\n'))) {
        return Some(1);
    }
    let second = byte(1);
    match lead {
        0x09 | 0x0A | 0x0F | 0x1F..=0x7F | 0xFE | 0xFF => Some(1),
        0xC2..=0xDF if continuations(1, 1) => Some(2),
        0xE0 if second.is_some_and(|b| matches!(b, 0xA0..=0xBF)) && continuations(2, 1) => Some(3),
        0xE1..=0xEF if continuations(1, 2) => Some(3),
        0xF0 if second.is_some_and(|b| matches!(b, 0x90..=0xBF)) && continuations(2, 2) => Some(4),
        0xF1..=0xF7 if continuations(1, 3) => Some(4),
        0xF8 if second.is_some_and(|b| matches!(b, 0x88..=0xBF)) && continuations(2, 3) => Some(5),
        0xF9..=0xFB if continuations(1, 4) => Some(5),
        0xFC if second.is_some_and(|b| matches!(b, 0x84..=0xBF)) && continuations(2, 4) => Some(6),
        0xFD if continuations(1, 5) => Some(6),
        _ => None,
    }
}

/// How many bytes at `at` Second Life's `llBase64ToString` replaces by one
/// `?`: a control byte or stray continuation alone, or a lead byte with as
/// many continuations as follow it, up to its sequence's length (the second
/// group of `b64tostr_re`).
fn sl_replaced(bytes: &[u8], at: usize) -> usize {
    let Some(lead) = bytes.get(at).copied() else {
        return 0;
    };
    let most = match lead {
        0x00..=0x1F | 0x80..=0xBF => return 1,
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xF7 => 3,
        0xF8..=0xFB => 4,
        _ => 5,
    };
    let following = bytes
        .get(at.saturating_add(1)..)
        .unwrap_or_default()
        .iter()
        .take(most)
        .take_while(|byte| is_continuation(**byte))
        .count();
    following.saturating_add(1)
}

/// `llBase64ToString`: the Base64 decoded and read as UTF-8.
///
/// Second Life decodes the leading run of Base64 it can (padding is not
/// needed, `"YWJjZA"` is `abcd`; `"a!b"` is `""`), replaces a control byte
/// and a broken sequence by one `?` each before the conversion and every byte
/// that is still no UTF-8 by a `?` in it, and drops a last NUL. OpenSim
/// decodes only well-formed Base64 (`Convert.FromBase64String`) and refuses
/// anything else with `llBase64ToString: Error decoding string` — a second's
/// sleep — answering `""`; its conversion writes U+FFFD for what is not UTF-8
/// and keeps a NUL.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_base64_to_string<C: PureCtx>(ctx: &mut C, text: String) -> Result<String, CallError> {
    if ctx.flavour().is_opensim() {
        return Ok(if let Some(bytes) = os_base64(&text) {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            ctx.refuse("llBase64ToString: Error decoding string".to_owned());
            ctx.sleep(1.0);
            String::new()
        });
    }
    let mut bytes = decode(sl_base64_prefix(&text));
    let mut at = 0_usize;
    while at < bytes.len() {
        if let Some(width) = sl_passes(&bytes, at) {
            at = at.saturating_add(width);
        } else {
            let width = sl_replaced(&bytes, at);
            let _removed: Vec<u8> = bytes
                .splice(at..at.saturating_add(width).min(bytes.len()), *b"?")
                .collect();
            at = at.saturating_add(1);
        }
    }
    Ok(sl_utf8(&bytes))
}

/// `Convert.FromBase64String`: white space ignored, then groups of four with
/// at most two `=` and only at the very end; [`None`] for anything else.
fn os_base64(text: &str) -> Option<Vec<u8>> {
    let characters: Vec<u8> = text
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        .collect();
    if !characters.len().is_multiple_of(4) {
        return None;
    }
    let padding = characters
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    let body = characters.get(..characters.len().saturating_sub(padding))?;
    if padding > 2 || body.iter().any(|byte| sextet(*byte).is_none()) {
        return None;
    }
    Some(decode(body))
}

/// `llIntegerToBase64`: the integer's four bytes, big-endian, in Base64.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_integer_to_base64<C>(_ctx: &mut C, number: i32) -> Result<String, CallError> {
    Ok(encode(&big_endian(u32::from_ne_bytes(
        number.to_ne_bytes(),
    ))))
}

/// OpenSim's `c2itable`: a Base64 character's value plus one, `0` for `=`,
/// `-1` for anything else below 256.
fn os_digit(unit: u16) -> Option<i32> {
    let byte = u8::try_from(unit).ok()?;
    Some(match byte {
        b'=' => 0,
        other => sextet(other).map_or(-1, |value| i32::from(value).saturating_add(1)),
    })
}

/// `llBase64ToInteger`: up to the first four bytes the Base64 holds, as a
/// big-endian integer; `0` for a string longer than eight characters.
///
/// Second Life reads the Base64 it can and fills the rest with zero bytes
/// (PyOptimizer); it answers a string with no Base64 at the front with a
/// number that differs from run to run (`"!!!!"` gave 15204949, then
/// 12523626), for which this answers `0`. OpenSim reads six characters
/// itself, stops at padding, answers `0` for a character that is not
/// Base64, and throws reading past a string that ends sooner.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a string that ends before its
/// sixth character or padding, or holds a character past U+00FF.
pub fn ll_base64_to_integer<C: PureCtx>(ctx: &mut C, text: String) -> Result<i32, CallError> {
    if ctx.flavour().is_opensim() {
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.len() > 8 {
            return Ok(0);
        }
        let thrown = CallError::Exception {
            id: BuiltinId::LlBase64ToInteger,
            exception: Exception::IndexOutOfRange,
        };
        let mut number = 0_i32;
        for (at, shift) in [26_i32, 20, 14, 8, 2, -4].into_iter().enumerate() {
            let digit = units
                .get(at)
                .copied()
                .and_then(os_digit)
                .ok_or_else(|| thrown.clone())?;
            if digit <= 0 {
                return Ok(if digit < 0 { 0 } else { number });
            }
            let value = digit.saturating_sub(1);
            number = number.wrapping_add(if shift >= 0 {
                value.wrapping_shl(shift.unsigned_abs())
            } else {
                value >> shift.unsigned_abs()
            });
        }
        return Ok(number);
    }
    if text.chars().count() > 8 {
        return Ok(0);
    }
    let bytes = decode(sl_base64_prefix(&text));
    let word = (0..4).fold(0_u32, |word, at| {
        word << 8 | u32::from(bytes.get(at).copied().unwrap_or(0))
    });
    Ok(i32::from_ne_bytes(word.to_ne_bytes()))
}

/// The bytes of `text` and `key` for an XOR, as Second Life reads them: the
/// Base64 of each as [`sl_base64_prefix`] takes it, a key with none read as
/// one zero byte; [`None`] when the key is the empty string, which leaves the
/// text as it is.
fn xor_operands(text: &str, key: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    if key.is_empty() {
        return None;
    }
    let key_characters = sl_base64_prefix(key);
    let key = if key_characters.is_empty() {
        vec![0]
    } else {
        decode(key_characters)
    };
    Some((decode(sl_base64_prefix(text)), key))
}

/// `llXorBase64`: the bytes of the first Base64 XORed with those of the
/// second, cycled from the start whenever it runs out (`strings/functions`:
/// a key shorter, longer and empty — the empty key leaves the text as it
/// is); the same on every grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_xor_base64<C>(_ctx: &mut C, text: String, key: String) -> Result<String, CallError> {
    let Some((text_bytes, key)) = xor_operands(&text, &key) else {
        return Ok(text);
    };
    let mixed: Vec<u8> = text_bytes
        .iter()
        .zip(key.iter().cycle())
        .map(|(byte, with)| byte ^ with)
        .collect();
    Ok(encode(&mixed))
}

/// `llXorBase64StringsCorrect` (deprecated): as [`ll_xor_base64`], except
/// that the key starts again at its first zero byte rather than at its end
/// (PyOptimizer, after SCR-35). Not measured on any grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_xor_base64_strings_correct<C>(
    _ctx: &mut C,
    text: String,
    key: String,
) -> Result<String, CallError> {
    let Some((text_bytes, mut key)) = xor_operands(&text, &key) else {
        return Ok(text);
    };
    key.push(0);
    let mut at = 0_usize;
    let mut mixed = Vec::with_capacity(text_bytes.len());
    for byte in text_bytes {
        mixed.push(byte ^ key.get(at).copied().unwrap_or(0));
        at = at.saturating_add(1);
        if key.get(at).copied().unwrap_or(0) == 0 {
            at = 0;
        }
    }
    Ok(encode(&mixed))
}

/// `llXorBase64Strings` (deprecated): the Base64 *characters* XORed as
/// six-bit values, with Linden's quirks — a character of the text that is
/// not Base64 becomes `=`; a key character that is not Base64 counts as the
/// key's first character's value and restarts the key after it, and a first
/// character that is not Base64 is read as `+` when it is `=` and `/`
/// otherwise (PyOptimizer). Not measured on any grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_xor_base64_strings<C>(
    _ctx: &mut C,
    text: String,
    key: String,
) -> Result<String, CallError> {
    if key.is_empty() {
        return Ok(text);
    }
    let mut key: Vec<char> = key.chars().collect();
    let value = |character: char| u8::try_from(character).ok().and_then(sextet);
    let first = key.first().copied().unwrap_or('A');
    let base = value(first).unwrap_or_else(|| {
        let (replacement, base) = if first == '=' { ('+', 62) } else { ('/', 63) };
        if let Some(slot) = key.first_mut() {
            *slot = replacement;
        }
        base
    });
    let mut at = 0_usize;
    let mut out = String::new();
    for character in text.chars() {
        let key_character = key.get(at).copied().unwrap_or('A');
        at = at.saturating_add(1);
        if at >= key.len() {
            at = 0;
        }
        let Some(own) = value(character) else {
            out.push('=');
            continue;
        };
        let with = value(key_character).unwrap_or_else(|| {
            at = 1;
            base
        });
        let mixed = usize::from(own ^ with);
        out.push(char::from(ALPHABET.get(mixed).copied().unwrap_or(b'A')));
    }
    Ok(out)
}

/// A digest as lower-case hex.
fn hex_digest(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        push_hex(&mut out, *byte, HEX_LOWER);
    }
    out
}

/// `llMD5String`: the MD5 of `text:nonce` in UTF-8, as hex; the same on every
/// grid.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_md5_string<C>(_ctx: &mut C, text: String, nonce: i32) -> Result<String, CallError> {
    let salted = format!("{}:{nonce}", until_nul(text));
    Ok(hex_digest(&md5::Md5::digest(salted.as_bytes())))
}

/// `llSHA1String`: the SHA-1 of the UTF-8, as hex.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sha1_string<C>(_ctx: &mut C, text: String) -> Result<String, CallError> {
    Ok(hex_digest(&sha1::Sha1::digest(text.as_bytes())))
}

/// `llSHA256String`: the SHA-256 of the UTF-8, as hex.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_sha256_string<C>(_ctx: &mut C, text: String) -> Result<String, CallError> {
    Ok(hex_digest(&sha2::Sha256::digest(text.as_bytes())))
}

/// The digests `llComputeHash` and `llHMAC` name, in any case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    /// `md5`.
    Md5,
    /// `sha1`.
    Sha1,
    /// `sha224`.
    Sha224,
    /// `sha256`.
    Sha256,
    /// `sha384`.
    Sha384,
    /// `sha512`.
    Sha512,
}

impl Algorithm {
    /// The algorithm `name` names, ignoring case.
    fn named(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha224" => Some(Self::Sha224),
            "sha256" => Some(Self::Sha256),
            "sha384" => Some(Self::Sha384),
            "sha512" => Some(Self::Sha512),
            _ => None,
        }
    }

    /// The digest of `data`.
    fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Md5 => md5::Md5::digest(data).to_vec(),
            Self::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Self::Sha224 => sha2::Sha224::digest(data).to_vec(),
            Self::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Self::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Self::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }

    /// The HMAC of `data` under `key`.
    fn hmac(self, key: &[u8], data: &[u8]) -> Vec<u8> {
        match self {
            Self::Md5 => hmac_with::<md5::Md5>(key, data),
            Self::Sha1 => hmac_with::<sha1::Sha1>(key, data),
            Self::Sha224 => hmac_with::<sha2::Sha224>(key, data),
            Self::Sha256 => hmac_with::<sha2::Sha256>(key, data),
            Self::Sha384 => hmac_with::<sha2::Sha384>(key, data),
            Self::Sha512 => hmac_with::<sha2::Sha512>(key, data),
        }
    }
}

/// The HMAC of `data` under `key` with the digest `D`. A key of any length
/// is taken (it is hashed when longer than a block), so the empty result
/// for a refused key cannot happen.
fn hmac_with<D: Digest + BlockSizeUser + Clone>(key: &[u8], data: &[u8]) -> Vec<u8> {
    SimpleHmac::<D>::new_from_slice(key).map_or_else(
        |_refused| Vec::new(),
        |mut mac| {
            mac.update(data);
            mac.finalize().into_bytes().to_vec()
        },
    )
}

/// `llComputeHash`: the digest of the UTF-8 by the algorithm named (in any
/// case; `md5_sha1` is the MD5 and then the SHA-1), as hex. Any other name
/// is refused on `DEBUG_CHANNEL` with the list of valid ones and answers
/// `""` (`strings/newer`). OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_compute_hash<C: PureCtx>(
    ctx: &mut C,
    text: String,
    algorithm: String,
) -> Result<String, CallError> {
    if algorithm.eq_ignore_ascii_case("md5_sha1") {
        let mut both = Algorithm::Md5.digest(text.as_bytes());
        both.extend(Algorithm::Sha1.digest(text.as_bytes()));
        return Ok(hex_digest(&both));
    }
    Ok(if let Some(algorithm) = Algorithm::named(&algorithm) {
        hex_digest(&algorithm.digest(text.as_bytes()))
    } else {
        ctx.refuse(
            "llComputeHash valid algorithms are: md5, md5_sha1, sha1, sha224, sha256, sha384, \
             sha512."
                .to_owned(),
        );
        String::new()
    })
}

/// `llHMAC`: the HMAC of the message's UTF-8 under the key's, by the
/// algorithm named (in any case), in Base64. Any other name is refused on
/// `DEBUG_CHANNEL` with the list of valid ones and answers `""`
/// (`strings/newer`). OpenSim does not compile it.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_hmac<C: PureCtx>(
    ctx: &mut C,
    key: String,
    message: String,
    algorithm: String,
) -> Result<String, CallError> {
    Ok(if let Some(algorithm) = Algorithm::named(&algorithm) {
        encode(&algorithm.hmac(key.as_bytes(), message.as_bytes()))
    } else {
        ctx.refuse(
            "llHMAC valid algorithms are: md5, sha1, sha224, sha256, sha384, sha512".to_owned(),
        );
        String::new()
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::library::bare::{Bare, os, sl};

    #[test]
    fn escaping_keeps_letters_and_digits_and_on_opensim_the_unreserved_four() {
        let text = "a b/c?d=e&f~g*h-i_j.k!l'm(n)o+p,q;r:s@t#u$v%w";
        // strings/functions, `escape ascii` on each grid.
        assert_eq!(
            ll_escape_url(&mut sl(), text.to_owned()),
            Ok("a%20b%2Fc%3Fd%3De%26f%7Eg%2Ah%2Di%5Fj%2Ek%21l%27m%28n%29o%2Bp%2Cq%3Br%3As%40t%23u%24v%25w"
                .to_owned())
        );
        assert_eq!(
            ll_escape_url(&mut os(), text.to_owned()),
            Ok(
                "a%20b%2Fc%3Fd%3De%26f~g%2Ah-i_j.k%21l%27m%28n%29o%2Bp%2Cq%3Br%3As%40t%23u%24v%25w"
                    .to_owned()
            )
        );
        assert_eq!(
            ll_escape_url(&mut sl(), "é😀".to_owned()),
            Ok("%C3%A9%F0%9F%98%80".to_owned())
        );
    }

    #[test]
    fn unescaping_a_bad_escape() {
        let bad = || "%zz%4%e9%C3%A9%".to_owned();
        assert_eq!(ll_unescape_url(&mut sl(), bad()), Ok(String::new()));
        assert_eq!(
            ll_unescape_url(&mut os(), bad()),
            Ok("%zz%4%E9é%".to_owned())
        );
        for ctx in [&mut sl(), &mut os()] {
            assert_eq!(
                ll_unescape_url(ctx, "a+b%20c".to_owned()),
                Ok("a+b c".to_owned())
            );
        }
        assert_eq!(
            ll_unescape_url(&mut sl(), "%c3%a9".to_owned()),
            Ok("é".to_owned())
        );
        // PyOptimizer's: invalid bytes each become `?`.
        assert_eq!(
            ll_unescape_url(&mut sl(), "%80%C3".to_owned()),
            Ok("??".to_owned())
        );
        assert_eq!(
            ll_unescape_url(&mut sl(), "%F4%C3%80".to_owned()),
            Ok("?À".to_owned())
        );
    }

    #[test]
    fn base64_to_string_per_grid() {
        let decode = |ctx: &mut Bare, text: &str| ll_base64_to_string(ctx, text.to_owned());
        // strings/functions.
        assert_eq!(decode(&mut sl(), "/w=="), Ok("?".to_owned()));
        assert_eq!(decode(&mut sl(), "a!b"), Ok(String::new()));
        assert_eq!(decode(&mut sl(), "YWJj"), Ok("abc".to_owned()));
        assert_eq!(decode(&mut sl(), "YWJjZA"), Ok("abcd".to_owned()));
        assert_eq!(decode(&mut sl(), "YQBi"), Ok("a?b".to_owned()));
        assert_eq!(decode(&mut os(), "/w=="), Ok("\u{FFFD}".to_owned()));
        assert_eq!(decode(&mut os(), "YQBi"), Ok("a\0b".to_owned()));
        let mut ctx = os();
        assert_eq!(decode(&mut ctx, "YWJjZA"), Ok(String::new()));
        assert_eq!(decode(&mut ctx, "a!b"), Ok(String::new()));
        assert_eq!(ctx.refusals.len(), 2);
        assert!((ctx.slept - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn base64_and_integers() {
        assert_eq!(ll_integer_to_base64(&mut (), -1), Ok("/////w==".to_owned()));
        assert_eq!(ll_integer_to_base64(&mut (), 1), Ok("AAAAAQ==".to_owned()));
        for ctx in [&mut sl(), &mut os()] {
            let mut read = |text: &str| ll_base64_to_integer(ctx, text.to_owned());
            assert_eq!(read("AAAAAQ=="), Ok(1));
            assert_eq!(read("AQ=="), Ok(0x0100_0000));
            assert_eq!(read("AQAAAAAA"), Ok(0x0100_0000));
            assert_eq!(read("AAAAAAAAA"), Ok(0));
        }
        assert_eq!(ll_base64_to_integer(&mut os(), "!!!!".to_owned()), Ok(0));
        assert_eq!(
            ll_base64_to_integer(&mut os(), "AQ".to_owned()),
            Err(CallError::Exception {
                id: BuiltinId::LlBase64ToInteger,
                exception: Exception::IndexOutOfRange,
            })
        );
        assert_eq!(
            ll_base64_to_integer(&mut sl(), "AQ".to_owned()),
            Ok(0x0100_0000)
        );
    }

    #[test]
    fn xor_cycles_the_key() {
        let text = || ll_string_to_base64(&mut (), "hello world".to_owned()).unwrap_or_default();
        let key = |key: &str| ll_string_to_base64(&mut (), key.to_owned()).unwrap_or_default();
        // strings/functions, every grid.
        assert_eq!(
            ll_xor_base64(&mut (), text(), key("ab")),
            Ok("CQcNDg5CFg0TDgU=".to_owned())
        );
        assert_eq!(
            ll_xor_base64(&mut (), text(), key("a much longer key than text")),
            Ok("CUUBGQxIVwMdAgM=".to_owned())
        );
        assert_eq!(ll_xor_base64(&mut (), text(), String::new()), Ok(text()));
        assert_eq!(
            ll_xor_base64(&mut (), text(), "YQ==".to_owned()),
            Ok("CQQNDQ5BFg4TDQU=".to_owned())
        );
        // The SCR-35 variant restarts at the key's zero byte: with no zero
        // byte in the key, it is the same as the plain XOR.
        assert_eq!(
            ll_xor_base64_strings_correct(&mut (), text(), key("ab")),
            Ok("CQcNDg5CFg0TDgU=".to_owned())
        );
        assert_eq!(
            ll_xor_base64_strings(&mut (), "AAAA".to_owned(), "BC".to_owned()),
            Ok("BCBC".to_owned())
        );
    }

    #[test]
    fn digests_match_the_transcripts() {
        // strings/functions and strings/newer on aditi.
        assert_eq!(
            ll_md5_string(&mut (), "abc".to_owned(), 0),
            Ok("cf4bab410c5a562ddef8587f22c939ca".to_owned())
        );
        assert_eq!(
            ll_md5_string(&mut (), "abc".to_owned(), 5),
            Ok("12c9d3edd5e3f8776b3f66e0a78ccd5a".to_owned())
        );
        assert_eq!(
            ll_sha1_string(&mut (), "é".to_owned()),
            Ok("bf15be717ac1b080b4f1c456692825891ff5073d".to_owned())
        );
        assert_eq!(
            ll_sha256_string(&mut (), "abc".to_owned()),
            Ok("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned())
        );
        let mut ctx = sl();
        assert_eq!(
            ll_compute_hash(&mut ctx, "abc".to_owned(), "SHA1".to_owned()),
            Ok("a9993e364706816aba3e25717850c26c9cd0d89d".to_owned())
        );
        assert_eq!(
            ll_compute_hash(&mut ctx, "abc".to_owned(), "md4".to_owned()),
            Ok(String::new())
        );
        assert_eq!(
            ll_hmac(
                &mut ctx,
                "key".to_owned(),
                "abc".to_owned(),
                "SHA256".to_owned()
            ),
            Ok("nBluMtwBdfhvSxy4konWYZ3mvuaZ5MN45oMJ7Zehpqs=".to_owned())
        );
        assert_eq!(
            ll_hmac(&mut ctx, String::new(), "abc".to_owned(), "sha1".to_owned()),
            Ok("m0qRjzmNdNPjZ5cKujy+VOTStdk=".to_owned())
        );
        assert_eq!(
            ll_hmac(&mut ctx, "k".to_owned(), "abc".to_owned(), "md4".to_owned()),
            Ok(String::new())
        );
        assert_eq!(ctx.refusals.len(), 2);
    }
}
