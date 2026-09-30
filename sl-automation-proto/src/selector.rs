//! The string selector grammar: how a [`Locator`] or a [`WorldLocator`] is
//! written on a command line, and how either prints in a failure message.
//!
//! A selector is a chain of steps joined by `>>`, each looked for inside the
//! one before it:
//!
//! ```text
//! window[test_id=floater:build] >> button[name_key=build-apply][enabled=false]
//! ```
//!
//! A step is a role (or `*`, or nothing) followed by attributes in brackets.
//! Each attribute is named after the locator's JSON field and holds with `=`;
//! a name also takes `~=`, "contains". A value is written bare when it is
//! plain — letters, digits and `_ . : -` — and in double quotes otherwise,
//! with `\"`, `\\`, `\n`, `\r`, `\t` and `\u{…}` escapes:
//!
//! | attribute | UI | world |
//! | --- | --- | --- |
//! | `name` | `=` exact, `~=` contains | the same |
//! | `name_key`, `test_id` | text | — |
//! | `enabled`, `checked`, `selected`, `expanded`, `focused` | `true` / `false` | — |
//! | `own` | — | `true` / `false` |
//! | `full_id`, `owner` | — | a UUID |
//! | `local_id`, `pcode` | — | a number |
//! | `hover_text` | — | `=` exact, `~=` contains |
//! | `near` | — | `own_avatar`, or `x,y,z` region metres |
//! | `radius` | — | metres, with `near` |
//! | `nth` | a zero-based index | the same |
//!
//! A world selector is one step, its head a [`WorldKind`]:
//! `object[name=Door][near=own_avatar][radius=5][nth=0]`. Printing a locator
//! and parsing the print gives the same locator back; a misspelt role or
//! attribute, a repeated attribute and a malformed value are errors naming
//! the column.

use core::fmt;
use core::fmt::Write as _;
use core::str::FromStr;

use uuid::Uuid;

use crate::locator::{Locator, NameMatcher};
use crate::snapshot::Role;
use crate::world::{Anchor, Near, WorldKind, WorldLocator};

/// Why a selector string does not parse, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("selector {input:?}, column {column}: {message}")]
pub struct SelectorError {
    /// The selector as given.
    pub input: String,
    /// The one-based column (in characters) the problem starts at.
    pub column: usize,
    /// What is wrong there.
    pub message: String,
}

/// How an attribute compares its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// `=`: the whole value.
    Equals,
    /// `~=`: any part of the value.
    Contains,
}

/// One `[key=value]` of a step, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attr {
    /// The attribute's name.
    key: String,
    /// How it compares.
    op: Op,
    /// The value, unquoted and unescaped.
    value: String,
    /// The zero-based character index the attribute's name starts at.
    at: usize,
}

/// One step of a selector, as written: the head (a role or a kind; `None`
/// for `*` or none) and its attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    /// The head's text and where it starts.
    head: Option<(String, usize)>,
    /// The attributes, in the order written.
    attrs: Vec<Attr>,
}

/// A cursor over the selector's characters.
#[derive(Debug)]
struct Scanner<'input> {
    /// The selector as given, for errors.
    input: &'input str,
    /// Its characters.
    chars: Vec<char>,
    /// The index of the next character.
    pos: usize,
}

impl<'input> Scanner<'input> {
    /// A cursor at the start of `input`.
    fn new(input: &'input str) -> Self {
        Self {
            input,
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    /// The next character, without taking it.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    /// Take the next character.
    fn bump(&mut self) -> Option<char> {
        let next = self.peek()?;
        self.pos = self.pos.saturating_add(1);
        Some(next)
    }

    /// Whether every character is taken.
    const fn at_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    /// Take `text` if it comes next.
    fn eat(&mut self, text: &str) -> bool {
        let mut ahead = self.chars.iter().skip(self.pos);
        let matches = text.chars().all(|wanted| ahead.next() == Some(&wanted));
        if matches {
            self.pos = self.pos.saturating_add(text.chars().count());
        }
        matches
    }

    /// Skip whitespace.
    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos = self.pos.saturating_add(1);
        }
    }

    /// Take a run of identifier characters (ASCII letters, digits, `_`).
    fn ident(&mut self) -> String {
        let mut ident = String::new();
        while let Some(next) = self.peek()
            && (next.is_ascii_alphanumeric() || next == '_')
        {
            ident.push(next);
            self.pos = self.pos.saturating_add(1);
        }
        ident
    }

    /// An error at character index `at`.
    fn error_at(&self, at: usize, message: impl Into<String>) -> SelectorError {
        SelectorError {
            input: self.input.to_owned(),
            column: at.saturating_add(1),
            message: message.into(),
        }
    }

    /// An error at the cursor.
    fn error(&self, message: impl Into<String>) -> SelectorError {
        self.error_at(self.pos, message)
    }

    /// The whole selector: steps joined by `>>`.
    fn selector(&mut self) -> Result<Vec<Step>, SelectorError> {
        let mut steps = Vec::new();
        loop {
            self.skip_space();
            steps.push(self.step()?);
            self.skip_space();
            if self.at_end() {
                return Ok(steps);
            }
            if !self.eat(">>") {
                return Err(self.error("expected `>>` or the end of the selector"));
            }
        }
    }

    /// One step: a head (a name or `*`, or nothing) and its attributes.
    fn step(&mut self) -> Result<Step, SelectorError> {
        let start = self.pos;
        let head = if self.eat("*") {
            None
        } else {
            let ident = self.ident();
            (!ident.is_empty()).then_some((ident, start))
        };
        let star = head.is_none() && self.pos > start;
        let mut attrs = Vec::new();
        loop {
            self.skip_space();
            if !self.eat("[") {
                break;
            }
            attrs.push(self.attr()?);
        }
        if head.is_none() && !star && attrs.is_empty() {
            return Err(self.error("expected a role, `*` or `[`"));
        }
        Ok(Step { head, attrs })
    }

    /// One attribute, its `[` already taken.
    fn attr(&mut self) -> Result<Attr, SelectorError> {
        self.skip_space();
        let at = self.pos;
        let key = self.ident();
        if key.is_empty() {
            return Err(self.error("expected an attribute name"));
        }
        self.skip_space();
        let op = if self.eat("~=") {
            Op::Contains
        } else if self.eat("=") {
            Op::Equals
        } else {
            return Err(self.error(format!("expected `=` or `~=` after `{key}`")));
        };
        self.skip_space();
        let value = if self.peek() == Some('"') {
            let value = self.quoted()?;
            self.skip_space();
            if !self.eat("]") {
                return Err(self.error("expected `]` after the quoted value"));
            }
            value
        } else {
            let start = self.pos;
            let mut value = String::new();
            loop {
                match self.bump() {
                    Some(']') => break,
                    Some(next) => value.push(next),
                    None => return Err(self.error_at(start, "unclosed `[`")),
                }
            }
            let value = value.trim().to_owned();
            if value.is_empty() {
                return Err(self.error_at(start, format!("expected a value for `{key}`")));
            }
            value
        };
        Ok(Attr { key, op, value, at })
    }

    /// A double-quoted string, its escapes resolved.
    fn quoted(&mut self) -> Result<String, SelectorError> {
        let start = self.pos;
        let _quote = self.bump();
        let mut value = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error_at(start, "unclosed `\"`")),
                Some('"') => return Ok(value),
                Some('\\') => value.push(self.escape()?),
                Some(next) => value.push(next),
            }
        }
    }

    /// The character an escape stands for, its `\` already taken.
    fn escape(&mut self) -> Result<char, SelectorError> {
        let at = self.pos.saturating_sub(1);
        match self.bump() {
            Some('"') => Ok('"'),
            Some('\\') => Ok('\\'),
            Some('n') => Ok('\n'),
            Some('r') => Ok('\r'),
            Some('t') => Ok('\t'),
            Some('u') => {
                if !self.eat("{") {
                    return Err(self.error("expected `{` after `\\u`"));
                }
                let mut hex = String::new();
                loop {
                    match self.bump() {
                        Some('}') => break,
                        Some(digit) if digit.is_ascii_hexdigit() => hex.push(digit),
                        _ => return Err(self.error_at(at, "malformed `\\u{…}` escape")),
                    }
                }
                u32::from_str_radix(&hex, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| self.error_at(at, format!("`\\u{{{hex}}}` is no character")))
            }
            _ => Err(self.error_at(at, "unknown escape")),
        }
    }
}

impl FromStr for Locator {
    type Err = SelectorError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let scanner = &mut Scanner::new(input);
        let steps = scanner.selector()?;
        let mut locator: Option<Self> = None;
        for step in steps {
            let own = ui_step(scanner, step)?;
            locator = Some(match locator {
                Some(scope) => own.within(scope),
                None => own,
            });
        }
        locator.ok_or_else(|| scanner.error_at(0, "empty selector"))
    }
}

/// Fill `slot` from `attr`, refusing a second attribute of one name.
fn set<T>(
    scanner: &Scanner<'_>,
    slot: &mut Option<T>,
    attr: &Attr,
    value: T,
) -> Result<(), SelectorError> {
    if slot.is_some() {
        return Err(scanner.error_at(attr.at, format!("`{}` given twice", attr.key)));
    }
    *slot = Some(value);
    Ok(())
}

/// `attr`'s value, refusing `~=`.
fn exact<'attr>(scanner: &Scanner<'_>, attr: &'attr Attr) -> Result<&'attr str, SelectorError> {
    match attr.op {
        Op::Equals => Ok(&attr.value),
        Op::Contains => {
            Err(scanner.error_at(attr.at, format!("`{}` takes `=`, not `~=`", attr.key)))
        }
    }
}

/// `attr`'s value as a name comparison.
fn matcher(attr: &Attr) -> NameMatcher {
    match attr.op {
        Op::Equals => NameMatcher::Exact(attr.value.clone()),
        Op::Contains => NameMatcher::Contains(attr.value.clone()),
    }
}

/// `attr`'s value parsed as a `T`, which is described as `what` when it
/// does not parse.
fn parsed<T: FromStr>(scanner: &Scanner<'_>, attr: &Attr, what: &str) -> Result<T, SelectorError> {
    exact(scanner, attr)?.parse().map_err(|_error| {
        scanner.error_at(
            attr.at,
            format!("`{}` wants {what}, not {:?}", attr.key, attr.value),
        )
    })
}

/// One UI step as its own locator, without a scope.
fn ui_step(scanner: &Scanner<'_>, step: Step) -> Result<Locator, SelectorError> {
    let mut locator = Locator::default();
    if let Some((head, at)) = step.head {
        let role = Role::ALL
            .into_iter()
            .find(|role| role.as_str() == head)
            .ok_or_else(|| {
                let known: Vec<&str> = Role::ALL.into_iter().map(Role::as_str).collect();
                scanner.error_at(
                    at,
                    format!("unknown role `{head}` (known: {})", known.join(", ")),
                )
            })?;
        locator.role = Some(role);
    }
    for attr in &step.attrs {
        match attr.key.as_str() {
            "name" => set(scanner, &mut locator.name, attr, matcher(attr))?,
            "name_key" => {
                let key = exact(scanner, attr)?.to_owned();
                set(scanner, &mut locator.name_key, attr, key)?;
            }
            "test_id" => {
                let id = exact(scanner, attr)?.to_owned();
                set(scanner, &mut locator.test_id, attr, id)?;
            }
            "enabled" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.enabled, attr, wanted)?;
            }
            "checked" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.checked, attr, wanted)?;
            }
            "selected" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.selected, attr, wanted)?;
            }
            "expanded" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.expanded, attr, wanted)?;
            }
            "focused" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.focused, attr, wanted)?;
            }
            "nth" => {
                let index = parsed(scanner, attr, "a zero-based index")?;
                set(scanner, &mut locator.nth, attr, index)?;
            }
            other => {
                return Err(scanner.error_at(
                    attr.at,
                    format!(
                        "unknown attribute `{other}` (known: name, name_key, test_id, enabled, \
                         checked, selected, expanded, focused, nth)"
                    ),
                ));
            }
        }
    }
    Ok(locator)
}

impl FromStr for WorldLocator {
    type Err = SelectorError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let scanner = &mut Scanner::new(input);
        let mut steps = scanner.selector()?.into_iter();
        let (Some(step), None) = (steps.next(), steps.next()) else {
            return Err(scanner.error_at(0, "a world selector is one step, without `>>`"));
        };
        world_step(scanner, step)
    }
}

/// A world step as its locator.
fn world_step(scanner: &Scanner<'_>, step: Step) -> Result<WorldLocator, SelectorError> {
    let mut locator = WorldLocator::default();
    if let Some((head, at)) = step.head {
        let kind = WorldKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == head)
            .ok_or_else(|| {
                scanner.error_at(
                    at,
                    format!("unknown kind `{head}` (known: avatar, object, attachment)"),
                )
            })?;
        locator.kind = Some(kind);
    }
    let mut near: Option<Anchor> = None;
    let mut radius: Option<f32> = None;
    let mut radius_at: Option<usize> = None;
    for attr in &step.attrs {
        match attr.key.as_str() {
            "own" => {
                let wanted = parsed(scanner, attr, "true or false")?;
                set(scanner, &mut locator.own, attr, wanted)?;
            }
            "name" => set(scanner, &mut locator.name, attr, matcher(attr))?,
            "full_id" => {
                let id = parsed(scanner, attr, "a UUID")?;
                set(scanner, &mut locator.full_id, attr, id)?;
            }
            "local_id" => {
                let id = parsed(scanner, attr, "a region-local id")?;
                set(scanner, &mut locator.local_id, attr, id)?;
            }
            "owner" => {
                let owner: Uuid = parsed(scanner, attr, "a UUID")?;
                set(scanner, &mut locator.owner, attr, owner)?;
            }
            "pcode" => {
                let pcode = parsed(scanner, attr, "an object class byte")?;
                set(scanner, &mut locator.pcode, attr, pcode)?;
            }
            "hover_text" => set(scanner, &mut locator.hover_text, attr, matcher(attr))?,
            "near" => {
                let anchor = anchor(scanner, attr)?;
                set(scanner, &mut near, attr, anchor)?;
            }
            "radius" => {
                let metres = parsed(scanner, attr, "metres")?;
                set(scanner, &mut radius, attr, metres)?;
                radius_at = Some(attr.at);
            }
            "nth" => {
                let index = parsed(scanner, attr, "a zero-based index")?;
                set(scanner, &mut locator.nth, attr, index)?;
            }
            other => {
                return Err(scanner.error_at(
                    attr.at,
                    format!(
                        "unknown attribute `{other}` (known: own, name, full_id, local_id, owner, \
                         pcode, hover_text, near, radius, nth)"
                    ),
                ));
            }
        }
    }
    locator.near = match (near, radius_at) {
        (Some(to), _at) => Some(Near { to, radius }),
        (None, Some(at)) => return Err(scanner.error_at(at, "`radius` needs `near`")),
        (None, None) => None,
    };
    Ok(locator)
}

/// A `near` value: `own_avatar`, or `x,y,z`.
fn anchor(scanner: &Scanner<'_>, attr: &Attr) -> Result<Anchor, SelectorError> {
    let value = exact(scanner, attr)?;
    if value == "own_avatar" {
        return Ok(Anchor::OwnAvatar);
    }
    let parts: Vec<f32> = value
        .split(',')
        .map(|part| part.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_error| {
            scanner.error_at(
                attr.at,
                format!("`near` wants own_avatar or x,y,z, not {value:?}"),
            )
        })?;
    match parts.as_slice() {
        &[x, y, z] => Ok(Anchor::Point([x, y, z])),
        _ => Err(scanner.error_at(
            attr.at,
            format!("`near` wants three coordinates, not {value:?}"),
        )),
    }
}

/// Whether `value` prints without quotes: plain, and not empty.
fn is_bare(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|next| next.is_ascii_alphanumeric() || matches!(next, '_' | '.' | ':' | '-'))
}

/// Write `value` bare when it may be, else quoted and escaped.
fn write_value(f: &mut fmt::Formatter<'_>, value: &str) -> fmt::Result {
    if is_bare(value) {
        return f.write_str(value);
    }
    f.write_str("\"")?;
    for next in value.chars() {
        match next {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            control if control.is_control() => write!(f, "\\u{{{:x}}}", u32::from(control))?,
            other => f.write_char(other)?,
        }
    }
    f.write_str("\"")
}

/// Write `[key=value]`, the value bare or quoted.
fn write_text(f: &mut fmt::Formatter<'_>, key: &str, value: &str) -> fmt::Result {
    write!(f, "[{key}=")?;
    write_value(f, value)?;
    f.write_str("]")
}

/// Write a name comparison as `[key=value]` or `[key~=value]`.
fn write_matcher(f: &mut fmt::Formatter<'_>, key: &str, matcher: &NameMatcher) -> fmt::Result {
    let (op, value) = match matcher {
        NameMatcher::Exact(value) => ("=", value),
        NameMatcher::Contains(value) => ("~=", value),
    };
    write!(f, "[{key}{op}")?;
    write_value(f, value)?;
    f.write_str("]")
}

/// Write `[key=value]` for a value that prints plainly.
fn write_plain(f: &mut fmt::Formatter<'_>, key: &str, value: impl fmt::Display) -> fmt::Result {
    write!(f, "[{key}={value}]")
}

/// Prints the locator in the selector grammar, the scope first — `window
/// [test_id=floater:preferences] >> button[name_key=button-ok]` without the
/// space — which [`Locator::from_str`] reads back as the same locator.
impl fmt::Display for Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(scope) = &self.within {
            write!(f, "{scope} >> ")?;
        }
        match self.role {
            Some(role) => f.write_str(role.as_str())?,
            None if names_nothing(self) => f.write_str("*")?,
            None => {}
        }
        if let Some(id) = &self.test_id {
            write_text(f, "test_id", id)?;
        }
        if let Some(key) = &self.name_key {
            write_text(f, "name_key", key)?;
        }
        if let Some(name) = &self.name {
            write_matcher(f, "name", name)?;
        }
        for (key, filter) in [
            ("enabled", self.enabled),
            ("checked", self.checked),
            ("selected", self.selected),
            ("expanded", self.expanded),
            ("focused", self.focused),
        ] {
            if let Some(wanted) = filter {
                write_plain(f, key, wanted)?;
            }
        }
        if let Some(index) = self.nth {
            write_plain(f, "nth", index)?;
        }
        Ok(())
    }
}

/// Whether the locator's own step names nothing — every criterion but its
/// scope unset — so it prints as `*`.
fn names_nothing(locator: &Locator) -> bool {
    Locator {
        within: None,
        ..locator.clone()
    } == Locator::default()
}

/// Prints the locator in the selector grammar — `object[name=Door]
/// [near=own_avatar][radius=5][nth=0]` without the space — which
/// [`WorldLocator::from_str`] reads back as the same locator.
impl fmt::Display for WorldLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            Some(kind) => f.write_str(kind.as_str())?,
            None if *self == Self::default() => f.write_str("*")?,
            None => {}
        }
        if let Some(own) = self.own {
            write_plain(f, "own", own)?;
        }
        if let Some(name) = &self.name {
            write_matcher(f, "name", name)?;
        }
        if let Some(id) = self.full_id {
            write_plain(f, "full_id", id)?;
        }
        if let Some(id) = self.local_id {
            write_plain(f, "local_id", id)?;
        }
        if let Some(owner) = self.owner {
            write_plain(f, "owner", owner)?;
        }
        if let Some(pcode) = self.pcode {
            write_plain(f, "pcode", pcode)?;
        }
        if let Some(text) = &self.hover_text {
            write_matcher(f, "hover_text", text)?;
        }
        if let Some(near) = self.near {
            match near.to {
                Anchor::Point([x, y, z]) => write!(f, "[near={x},{y},{z}]")?,
                Anchor::OwnAvatar => f.write_str("[near=own_avatar]")?,
            }
            if let Some(radius) = near.radius {
                write_plain(f, "radius", radius)?;
            }
        }
        if let Some(index) = self.nth {
            write_plain(f, "nth", index)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
