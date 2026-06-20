//! The tokenizer and typed argument accessors shared by every registry build
//! function.
//!
//! A command's arguments are parsed from the rest of the line into [`Args`]: a
//! list of *positional* tokens plus a map of `key=value` *keyword* tokens.
//! Tokens are whitespace-separated, with double quotes protecting embedded
//! spaces (`im $self "hello there"`); a `key=value` token whose key is a bare
//! identifier becomes a keyword argument, so optional struct fields can be set
//! by name in any order.
//!
//! Each typed accessor (`req_uuid`, `opt_i32`, `vector`, …) looks a field up by
//! its keyword name first, then by positional index, resolves any leading
//! `$placeholder` through the [`ReplContext`], and finally parses the literal.

use std::collections::BTreeMap;
use std::str::FromStr;

use sl_proto::{Rotation, Uuid, Vector};

use crate::context::ReplContext;
use crate::error::ReplError;

/// The parsed arguments of a single command line: positional tokens plus
/// `key=value` keyword tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    /// The command name these arguments belong to (for error messages); empty
    /// until the registry assigns it at dispatch.
    command: String,
    /// The positional argument tokens, in order.
    positional: Vec<String>,
    /// The `key=value` keyword arguments.
    keyword: BTreeMap<String, String>,
}

/// One accumulator state while tokenizing a line.
#[derive(Debug, Default)]
struct TokenAcc {
    /// The keyword key, set once an unquoted `=` has split the token.
    key: Option<String>,
    /// The characters accumulated so far (the value, or the whole token before
    /// a split).
    buf: String,
    /// Whether any character (including an empty quoted span) has been seen.
    started: bool,
}

impl TokenAcc {
    /// Flush the accumulated token into `positional`/`keyword`, resetting state.
    fn flush(&mut self, positional: &mut Vec<String>, keyword: &mut BTreeMap<String, String>) {
        if !self.started {
            return;
        }
        match self.key.take() {
            Some(key) => {
                drop(keyword.insert(key, std::mem::take(&mut self.buf)));
            }
            None => positional.push(std::mem::take(&mut self.buf)),
        }
        self.started = false;
    }
}

/// Whether `s` is a non-empty bare identifier (ASCII alphanumeric or `_`),
/// eligible to be a `key=value` keyword key.
fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Resolve a single raw token, expanding a leading `$placeholder` through `ctx`.
fn resolve(ctx: &dyn ReplContext, raw: &str) -> Result<String, ReplError> {
    match raw.strip_prefix('$') {
        Some(name) => ctx
            .resolve_placeholder(name)
            .ok_or_else(|| ReplError::Unresolved(format!("${name}"))),
        None => Ok(raw.to_owned()),
    }
}

/// Parse `value` into `T` via [`FromStr`], mapping a failure to
/// [`ReplError::InvalidArg`] describing `field` and `expected`.
fn parse_scalar<T>(field: &str, value: &str, expected: &str) -> Result<T, ReplError>
where
    T: FromStr,
{
    value
        .parse::<T>()
        .ok()
        .ok_or_else(|| ReplError::InvalidArg {
            field: field.to_owned(),
            value: value.to_owned(),
            expected: expected.to_owned(),
        })
}

/// Parse an already-resolved literal into `T` via [`FromStr`] (a public helper
/// for parsing the colon-separated fields returned by [`Args::vec_records`]).
pub(crate) fn literal<T>(field: &str, value: &str, expected: &str) -> Result<T, ReplError>
where
    T: FromStr,
{
    parse_scalar::<T>(field, value, expected)
}

/// Parse an already-resolved literal into a [`Uuid`].
pub(crate) fn literal_uuid(field: &str, value: &str) -> Result<Uuid, ReplError> {
    Uuid::parse_str(value)
        .ok()
        .ok_or_else(|| ReplError::InvalidArg {
            field: field.to_owned(),
            value: value.to_owned(),
            expected: "UUID".to_owned(),
        })
}

/// Parse an already-resolved literal into a `bool`.
pub(crate) fn literal_bool(field: &str, value: &str) -> Result<bool, ReplError> {
    parse_bool(field, value)
}

/// Parse an LSL-style `<x,y,z>` vector.
fn parse_vector(field: &str, value: &str) -> Result<Vector, ReplError> {
    let invalid = || ReplError::InvalidArg {
        field: field.to_owned(),
        value: value.to_owned(),
        expected: "vector <x,y,z>".to_owned(),
    };
    let inner = value
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .ok_or_else(invalid)?;
    let mut parts = inner.split(',');
    let mut next = || -> Result<f32, ReplError> {
        parts
            .next()
            .and_then(|p| p.trim().parse::<f32>().ok())
            .ok_or_else(invalid)
    };
    let x = next()?;
    let y = next()?;
    let z = next()?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    Ok(Vector { x, y, z })
}

/// Parse an LSL-style `<x,y,z,s>` rotation (quaternion).
fn parse_rotation(field: &str, value: &str) -> Result<Rotation, ReplError> {
    let invalid = || ReplError::InvalidArg {
        field: field.to_owned(),
        value: value.to_owned(),
        expected: "rotation <x,y,z,s>".to_owned(),
    };
    let inner = value
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .ok_or_else(invalid)?;
    let mut parts = inner.split(',');
    let mut next = || -> Result<f32, ReplError> {
        parts
            .next()
            .and_then(|p| p.trim().parse::<f32>().ok())
            .ok_or_else(invalid)
    };
    let x = next()?;
    let y = next()?;
    let z = next()?;
    let s = next()?;
    if parts.next().is_some() {
        return Err(invalid());
    }
    Ok(Rotation { x, y, z, s })
}

/// Parse a hex string (an even number of `[0-9a-fA-F]` digits) into bytes.
pub(crate) fn parse_hex(field: &str, value: &str) -> Result<Vec<u8>, ReplError> {
    let invalid = || ReplError::InvalidArg {
        field: field.to_owned(),
        value: value.to_owned(),
        expected: "hex bytes".to_owned(),
    };
    let digits: Vec<char> = value.chars().collect();
    if digits.len().checked_rem(2) != Some(0) {
        return Err(invalid());
    }
    let mut out = Vec::new();
    for pair in digits.chunks(2) {
        let text: String = pair.iter().collect();
        out.push(u8::from_str_radix(&text, 16).ok().ok_or_else(invalid)?);
    }
    Ok(out)
}

/// Parse a boolean from `true`/`false`/`1`/`0`/`yes`/`no` (case-insensitive).
pub(crate) fn parse_bool(field: &str, value: &str) -> Result<bool, ReplError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(ReplError::InvalidArg {
            field: field.to_owned(),
            value: value.to_owned(),
            expected: "boolean".to_owned(),
        }),
    }
}

impl Args {
    /// Tokenize the argument portion of a line into positional and keyword
    /// arguments. The command name must already be stripped.
    pub(crate) fn parse(input: &str) -> Result<Self, ReplError> {
        let mut positional = Vec::new();
        let mut keyword = BTreeMap::new();
        let mut acc = TokenAcc::default();
        let mut in_quotes = false;
        for c in input.chars() {
            if in_quotes {
                if c == '"' {
                    in_quotes = false;
                } else {
                    acc.buf.push(c);
                }
                continue;
            }
            match c {
                '"' => {
                    in_quotes = true;
                    acc.started = true;
                }
                c if c.is_whitespace() => acc.flush(&mut positional, &mut keyword),
                '=' if acc.key.is_none() && is_identifier(&acc.buf) => {
                    acc.key = Some(std::mem::take(&mut acc.buf));
                    acc.started = true;
                }
                other => {
                    acc.buf.push(other);
                    acc.started = true;
                }
            }
        }
        if in_quotes {
            return Err(ReplError::UnterminatedQuote);
        }
        acc.flush(&mut positional, &mut keyword);
        Ok(Self {
            command: String::new(),
            positional,
            keyword,
        })
    }

    /// Set the command name used in error messages (called at dispatch).
    #[must_use]
    pub(crate) fn with_command(mut self, command: &str) -> Self {
        command.clone_into(&mut self.command);
        self
    }

    /// The keyword arguments.
    #[must_use]
    pub(crate) const fn keyword(&self) -> &BTreeMap<String, String> {
        &self.keyword
    }

    /// The raw (unresolved) value for `field`, preferring the keyword argument
    /// then the positional slot `pos`.
    fn raw(&self, field: &str, pos: usize) -> Option<&str> {
        self.keyword
            .get(field)
            .map(String::as_str)
            .or_else(|| self.positional.get(pos).map(String::as_str))
    }

    /// Build a [`ReplError::MissingArg`] for `field`.
    fn missing(&self, field: &str) -> ReplError {
        ReplError::MissingArg {
            command: self.command.clone(),
            field: field.to_owned(),
        }
    }

    /// The resolved string value for a required `field` (keyword or positional
    /// `pos`).
    pub(crate) fn req_str(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<String, ReplError> {
        let raw = self.raw(field, pos).ok_or_else(|| self.missing(field))?;
        resolve(ctx, raw)
    }

    /// The resolved string value for an optional `field`, or `None` if absent.
    pub(crate) fn opt_str(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Option<String>, ReplError> {
        match self.raw(field, pos) {
            Some(raw) => Ok(Some(resolve(ctx, raw)?)),
            None => Ok(None),
        }
    }

    /// The resolved string value for an optional `field`, or `default` if
    /// absent.
    pub(crate) fn str_or(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        default: &str,
    ) -> Result<String, ReplError> {
        Ok(self
            .opt_str(ctx, field, pos)?
            .unwrap_or_else(|| default.to_owned()))
    }

    /// A required [`Uuid`] argument.
    pub(crate) fn req_uuid(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Uuid, ReplError> {
        let value = self.req_str(ctx, field, pos)?;
        Uuid::parse_str(&value)
            .ok()
            .ok_or_else(|| ReplError::InvalidArg {
                field: field.to_owned(),
                value,
                expected: "UUID".to_owned(),
            })
    }

    /// An optional [`Uuid`] argument, defaulting to [`Uuid::nil`] if absent.
    pub(crate) fn uuid_or_nil(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Uuid, ReplError> {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => Uuid::parse_str(&value)
                .ok()
                .ok_or_else(|| ReplError::InvalidArg {
                    field: field.to_owned(),
                    value,
                    expected: "UUID".to_owned(),
                }),
            None => Ok(Uuid::nil()),
        }
    }

    /// A required, non-empty list of [`Uuid`]s taken from every *positional*
    /// token at index `from_pos` and beyond (each resolved through `ctx` and
    /// parsed as a UUID). Keyword arguments are ignored. Errors if no positional
    /// token is present at `from_pos` or any token is not a UUID.
    pub(crate) fn req_uuid_list(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        from_pos: usize,
    ) -> Result<Vec<Uuid>, ReplError> {
        let mut ids = Vec::new();
        for raw in self.positional.iter().skip(from_pos) {
            let value = resolve(ctx, raw)?;
            let id = Uuid::parse_str(&value)
                .ok()
                .ok_or_else(|| ReplError::InvalidArg {
                    field: field.to_owned(),
                    value,
                    expected: "UUID".to_owned(),
                })?;
            ids.push(id);
        }
        if ids.is_empty() {
            return Err(self.missing(field));
        }
        Ok(ids)
    }

    /// A required argument parsed via [`FromStr`] into `T`, described as
    /// `expected` on failure.
    pub(crate) fn req_parse<T>(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        expected: &str,
    ) -> Result<T, ReplError>
    where
        T: FromStr,
    {
        let value = self.req_str(ctx, field, pos)?;
        parse_scalar::<T>(field, &value, expected)
    }

    /// An optional argument parsed via [`FromStr`] into `T`, defaulting to
    /// `default` when absent.
    pub(crate) fn parse_or<T>(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        expected: &str,
        default: T,
    ) -> Result<T, ReplError>
    where
        T: FromStr,
    {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => parse_scalar::<T>(field, &value, expected),
            None => Ok(default),
        }
    }

    /// An optional argument parsed via [`FromStr`] into `T`, yielding `None` when
    /// the field is absent.
    pub(crate) fn opt_parse<T>(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        expected: &str,
    ) -> Result<Option<T>, ReplError>
    where
        T: FromStr,
    {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => parse_scalar::<T>(field, &value, expected).map(Some),
            None => Ok(None),
        }
    }

    /// A required `bool` argument (`true`/`false`/`1`/`0`/`yes`/`no`).
    pub(crate) fn req_bool(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<bool, ReplError> {
        let value = self.req_str(ctx, field, pos)?;
        parse_bool(field, &value)
    }

    /// An optional `bool` argument, defaulting to `default` when absent.
    pub(crate) fn bool_or(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        default: bool,
    ) -> Result<bool, ReplError> {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => parse_bool(field, &value),
            None => Ok(default),
        }
    }

    /// A required `<x,y,z>` vector argument.
    pub(crate) fn req_vector(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Vector, ReplError> {
        let value = self.req_str(ctx, field, pos)?;
        parse_vector(field, &value)
    }

    /// An optional `<x,y,z>` vector argument.
    pub(crate) fn opt_vector(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Option<Vector>, ReplError> {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => Ok(Some(parse_vector(field, &value)?)),
            None => Ok(None),
        }
    }

    /// A required `<x,y,z,s>` rotation argument.
    pub(crate) fn req_rotation(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Rotation, ReplError> {
        let value = self.req_str(ctx, field, pos)?;
        parse_rotation(field, &value)
    }

    /// An optional `<x,y,z,s>` rotation argument.
    pub(crate) fn opt_rotation(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Option<Rotation>, ReplError> {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => Ok(Some(parse_rotation(field, &value)?)),
            None => Ok(None),
        }
    }

    /// An optional hex-encoded byte string argument, defaulting to empty.
    pub(crate) fn bytes_or_empty(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Vec<u8>, ReplError> {
        match self.opt_str(ctx, field, pos)? {
            Some(value) => parse_hex(field, &value),
            None => Ok(Vec::new()),
        }
    }

    /// A comma-separated list of [`Uuid`]s (empty when the field is absent).
    pub(crate) fn vec_uuid(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Vec<Uuid>, ReplError> {
        let Some(raw) = self.raw(field, pos) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for token in raw.split(',') {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resolved = resolve(ctx, trimmed)?;
            out.push(
                Uuid::parse_str(&resolved)
                    .ok()
                    .ok_or_else(|| ReplError::InvalidArg {
                        field: field.to_owned(),
                        value: resolved,
                        expected: "UUID".to_owned(),
                    })?,
            );
        }
        Ok(out)
    }

    /// A comma-separated list parsed via [`FromStr`] into `T` (empty when the
    /// field is absent).
    pub(crate) fn vec_parse<T>(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
        expected: &str,
    ) -> Result<Vec<T>, ReplError>
    where
        T: FromStr,
    {
        let Some(raw) = self.raw(field, pos) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for token in raw.split(',') {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resolved = resolve(ctx, trimmed)?;
            out.push(parse_scalar::<T>(field, &resolved, expected)?);
        }
        Ok(out)
    }

    /// A comma-separated list of `a:b:c…` records: each comma item is split on
    /// `:` into its colon-separated, individually-resolved fields. Empty when
    /// the argument is absent. Used to build small vectors of multi-field
    /// records (wearables, role-member changes, animation toggles, …).
    pub(crate) fn vec_records(
        &self,
        ctx: &dyn ReplContext,
        field: &str,
        pos: usize,
    ) -> Result<Vec<Vec<String>>, ReplError> {
        let Some(raw) = self.raw(field, pos) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for token in raw.split(',') {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut fields = Vec::new();
            for part in trimmed.split(':') {
                fields.push(resolve(ctx, part.trim())?);
            }
            out.push(fields);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;

    use super::Args;
    use crate::context::{NoContext, ReplContext};
    use crate::error::ReplError;

    /// A [`ReplContext`] backed by a fixed placeholder map.
    struct MapContext(BTreeMap<String, String>);

    impl ReplContext for MapContext {
        fn resolve_placeholder(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    /// Parse arguments, failing the assertion on a tokenizer error.
    fn args(input: &str) -> Args {
        Args::parse(input).unwrap_or_default()
    }

    #[test]
    fn separates_positional_and_keyword_tokens() {
        let parsed = args("alpha beta name=Cool gamma");
        assert_eq!(parsed.req_str(&NoContext, "x", 0), Ok("alpha".to_owned()));
        assert_eq!(parsed.req_str(&NoContext, "x", 2), Ok("gamma".to_owned()));
        assert_eq!(parsed.keyword().get("name"), Some(&"Cool".to_owned()));
    }

    #[test]
    fn quotes_protect_spaces_and_equals() {
        let parsed = args(r#"chat "hello = world" channel=5"#);
        assert_eq!(
            parsed.req_str(&NoContext, "x", 1),
            Ok("hello = world".to_owned())
        );
        assert_eq!(parsed.keyword().get("channel"), Some(&"5".to_owned()));
    }

    #[test]
    fn keyword_value_may_be_quoted() {
        let parsed = args(r#"name="Da Boom""#);
        assert_eq!(parsed.keyword().get("name"), Some(&"Da Boom".to_owned()));
    }

    #[test]
    fn unterminated_quote_errors() {
        assert_eq!(Args::parse("\"oops"), Err(ReplError::UnterminatedQuote));
    }

    #[test]
    fn typed_accessors_parse_literals() {
        let parsed = args("11111111-1111-1111-1111-111111111111 <1,2,3> true");
        let ctx = NoContext;
        assert_eq!(
            parsed.req_uuid(&ctx, "id", 0).map(|u| u.to_string()),
            Ok("11111111-1111-1111-1111-111111111111".to_owned())
        );
        let v = parsed.req_vector(&ctx, "v", 1);
        assert_eq!(v.map(|v| (v.x, v.y, v.z)), Ok((1.0, 2.0, 3.0)));
        assert_eq!(parsed.req_bool(&ctx, "flag", 2), Ok(true));
    }

    #[test]
    fn placeholder_resolves_through_context() {
        let mut map = BTreeMap::new();
        drop(map.insert(
            "self".to_owned(),
            "11111111-1111-1111-1111-111111111111".to_owned(),
        ));
        let ctx = MapContext(map);
        let parsed = args("$self");
        assert_eq!(
            parsed.req_uuid(&ctx, "id", 0).map(|u| u.to_string()),
            Ok("11111111-1111-1111-1111-111111111111".to_owned())
        );
    }

    #[test]
    fn unresolved_placeholder_errors() {
        let parsed = args("$missing");
        let err = parsed.req_str(&NoContext, "field", 0);
        assert_eq!(err, Err(ReplError::Unresolved("$missing".to_owned())));
    }

    #[test]
    fn vec_uuid_splits_on_commas() {
        let parsed =
            args("11111111-1111-1111-1111-111111111111,22222222-2222-2222-2222-222222222222");
        let ids = parsed.vec_uuid(&NoContext, "ids", 0);
        assert_eq!(ids.map(|v| v.len()), Ok(2));
    }
}
