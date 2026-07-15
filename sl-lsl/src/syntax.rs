//! The **LSL library symbol table** — the grid-served language definition.
//!
//! The lexer ([`crate::lexer`]) deliberately emits every identifier-shaped word
//! as a single [`Token::Identifier`](crate::Token::Identifier); it does not know
//! which words name a library function, a constant, an event handler, or a
//! control-flow keyword. That classification is exactly what the grid ships in
//! the `LSLSyntax` capability, and this module is the typed home for the decoded
//! result: a [`LslSyntax`] holding the five groups a `llsd-lsl-syntax-version: 2`
//! document carries — **functions**, **constants**, **events**, **controls**
//! (the flow keywords `if`/`for`/`state`/…) and **types** (the seven type
//! keywords).
//!
//! The crate stays I/O-free: this module owns only the owned data types and the
//! lookup / classification methods over them. Turning the grid's LLSD document
//! into a [`LslSyntax`] is the wire layer's job (`sl-wire`'s `lsl_syntax`
//! module), so a linter or CI check that already holds a parsed table can reuse
//! this crate without pulling in an LLSD codec or a circuit.
//!
//! What the table is *for*:
//!
//! - **Highlighting** — [`LslSyntax::classify`] turns a bare identifier into the
//!   [`SymbolKind`] a syntax highlighter colours by (or [`None`] for a
//!   user-defined name), the "one layer up" lookup the lexer's own docs promise.
//! - **Tooltips / signature help** — every entry carries the grid's description
//!   text, ordered arguments with names and types, and (for functions) the
//!   energy and sleep costs.
//! - **The semantic pass** — [`LslFunction::arguments`] and
//!   [`LslFunction::return_type`] give the arity and types a call-site check
//!   needs, current for the grid the script will actually run on.

use std::collections::HashMap;

use crate::ast::TypeName;

/// The `llsd-lsl-syntax-version` this crate's model matches. A decoder must
/// refuse a document declaring any other version rather than parsing a schema it
/// does not understand — the version is bumped only when the *schema* changes,
/// not when Linden Lab adds a function, so an unknown value means the layout may
/// differ. Cross-checked against Firestorm's `LLSD_SYNTAX_LSL_VERSION_EXPECTED`
/// (`llsyntaxid.cpp`).
pub const LSL_SYNTAX_VERSION: i32 = 2;

/// Which of the five groups a library symbol belongs to — the classification a
/// syntax highlighter colours by. Returned by [`LslSyntax::classify`] for a word
/// the grid's table knows; a user-defined global, function, `state` or local is
/// *not* in the table and classifies to [`None`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// A library function — an `ll*` (or OpenSim `os*`) call.
    Function,
    /// A library constant, e.g. `TRUE`, `PI`, `AGENT`.
    Constant,
    /// An event-handler name, e.g. `touch_start`, `state_entry`.
    Event,
    /// A control-flow keyword: `if`, `else`, `for`, `while`, `do`, `jump`,
    /// `return`, `state`.
    Control,
    /// A type keyword: `integer`, `float`, `string`, `key`, `vector`,
    /// `rotation`, `list`.
    Type,
}

/// One ordered argument of a library [`LslFunction`] or [`LslEvent`]: the
/// parameter name the grid documents, its declared type, and its optional
/// per-argument tooltip.
///
/// `arg_type` is [`Option`] because the grid serves the type as a bare keyword
/// string: a value that is not one of LSL's seven type keywords decodes to
/// [`None`] (tolerant — an unfamiliar keyword does not discard the whole entry)
/// rather than being rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LslArgument {
    /// The parameter name (e.g. `avatarId`, `TargetNumber`).
    pub name: String,
    /// The parameter's LSL type, or [`None`] when the served type keyword is not
    /// one of the seven ([`TypeName`]).
    pub arg_type: Option<TypeName>,
    /// The grid's per-argument tooltip, if any (Second Life ships these; OpenSim
    /// typically does not).
    pub tooltip: Option<String>,
}

/// A library function — one `ll*` / `os*` call — with its signature and costs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LslFunction {
    /// The return type, or [`None`] for a `void` function (the document omits
    /// the `return` key, or serves an empty / unrecognised type keyword).
    pub return_type: Option<TypeName>,
    /// The ordered argument list (may be empty).
    pub arguments: Vec<LslArgument>,
    /// The script-time energy cost (`energy`), if the grid advertises it. Second
    /// Life ships this; OpenSim's document omits it.
    pub energy: Option<f32>,
    /// The forced script sleep in seconds after the call (`sleep`), if
    /// advertised. Second Life ships this as a number; OpenSim folds it into the
    /// tooltip text (`"Sleep 0.1"`) instead, so it decodes to [`None`] there.
    pub sleep: Option<f32>,
    /// The grid's free-text description / tooltip, if any.
    pub tooltip: Option<String>,
    /// Whether the grid flags the function `deprecated`.
    pub deprecated: bool,
    /// Whether the grid flags the function `god-mode` (only usable by an estate
    /// god).
    pub god_mode: bool,
}

/// A library constant — a name bound to a fixed typed value (e.g. `TRUE`, `PI`,
/// `CHANGED_INVENTORY`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LslConstant {
    /// The constant's type, or [`None`] when the served type keyword is not one
    /// of the seven ([`TypeName`]).
    pub constant_type: Option<TypeName>,
    /// The constant's value **as served** — a textual rendering that preserves
    /// the grid's own formatting (`"0x1000"`, `"2"`, `"<0., 0., 0.>"`), since a
    /// tooltip shows it verbatim and a flag constant reads best in hex. [`None`]
    /// when the document omits the `value` key.
    pub value: Option<String>,
    /// The grid's free-text description / tooltip, if any.
    pub tooltip: Option<String>,
    /// Whether the grid flags the constant `deprecated`.
    pub deprecated: bool,
    /// Whether the grid flags the constant `god-mode`.
    pub god_mode: bool,
}

/// An event-handler signature — the name and ordered parameters of a `state`
/// event such as `touch_start(integer num_detected)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LslEvent {
    /// The ordered argument list the handler receives.
    pub arguments: Vec<LslArgument>,
    /// The grid's free-text description / tooltip, if any.
    pub tooltip: Option<String>,
    /// Whether the grid flags the event `deprecated`.
    pub deprecated: bool,
    /// Whether the grid flags the event `god-mode`.
    pub god_mode: bool,
}

/// A bare keyword entry — a control-flow keyword (`controls` group) or a type
/// keyword (`types` group). Neither carries a signature or a value, only a
/// tooltip and the two flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LslKeyword {
    /// The grid's free-text description / tooltip, if any.
    pub tooltip: Option<String>,
    /// Whether the grid flags the keyword `deprecated`.
    pub deprecated: bool,
    /// Whether the grid flags the keyword `god-mode`.
    pub god_mode: bool,
}

/// The grid's LSL library, decoded from an `LSLSyntax` document: the five symbol
/// groups a highlighter, a tooltip provider and the semantic pass all read.
///
/// The maps are keyed by the symbol name exactly as the grid serves it (LSL is
/// case-sensitive, so `llSay` and `llsay` are distinct). Construct one from the
/// wire document with `sl-wire`'s `parse_lsl_syntax`; this crate only reads it.
#[derive(Debug, Clone, PartialEq, Default)]
#[expect(
    clippy::module_name_repetitions,
    reason = "`LslSyntax` mirrors the `LSLSyntax` capability's own name and is the \
              established public type; the `syntax` module groups the whole library model"
)]
pub struct LslSyntax {
    /// The library functions, keyed by name.
    pub functions: HashMap<String, LslFunction>,
    /// The library constants, keyed by name.
    pub constants: HashMap<String, LslConstant>,
    /// The event-handler signatures, keyed by name.
    pub events: HashMap<String, LslEvent>,
    /// The control-flow keywords, keyed by name.
    pub controls: HashMap<String, LslKeyword>,
    /// The type keywords, keyed by name.
    pub types: HashMap<String, LslKeyword>,
}

impl LslSyntax {
    /// The library function named `name`, if the grid advertises one.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<&LslFunction> {
        self.functions.get(name)
    }

    /// The library constant named `name`, if the grid advertises one.
    #[must_use]
    pub fn constant(&self, name: &str) -> Option<&LslConstant> {
        self.constants.get(name)
    }

    /// The event-handler signature named `name`, if the grid advertises one.
    #[must_use]
    pub fn event(&self, name: &str) -> Option<&LslEvent> {
        self.events.get(name)
    }

    /// Whether `name` is a control-flow keyword the grid advertises.
    #[must_use]
    pub fn is_control(&self, name: &str) -> bool {
        self.controls.contains_key(name)
    }

    /// Whether `name` is a type keyword the grid advertises.
    #[must_use]
    pub fn is_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    /// Classifies a bare identifier against the library, for a highlighter or a
    /// scope-aware completion filter: the [`SymbolKind`] the grid's table gives
    /// the word, or [`None`] for a name the library does not know (a user global,
    /// function, `state`, event parameter or local).
    ///
    /// The groups are probed in the order a viewer colours them — functions and
    /// constants first (the common case), then events, then the keyword groups.
    /// A word cannot legitimately appear in two groups, so the order only affects
    /// how many lookups a hit costs, not the result.
    #[must_use]
    pub fn classify(&self, word: &str) -> Option<SymbolKind> {
        if self.functions.contains_key(word) {
            Some(SymbolKind::Function)
        } else if self.constants.contains_key(word) {
            Some(SymbolKind::Constant)
        } else if self.events.contains_key(word) {
            Some(SymbolKind::Event)
        } else if self.controls.contains_key(word) {
            Some(SymbolKind::Control)
        } else if self.types.contains_key(word) {
            Some(SymbolKind::Type)
        } else {
            None
        }
    }

    /// The total number of symbols across all five groups — handy for a "loaded
    /// N keywords" log line and for asserting a decode was non-empty.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions
            .len()
            .saturating_add(self.constants.len())
            .saturating_add(self.events.len())
            .saturating_add(self.controls.len())
            .saturating_add(self.types.len())
    }

    /// Whether the table carries no symbols at all (every group empty).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.constants.is_empty()
            && self.events.is_empty()
            && self.controls.is_empty()
            && self.types.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        LslArgument, LslConstant, LslEvent, LslFunction, LslKeyword, LslSyntax, SymbolKind,
    };
    use crate::ast::TypeName;

    /// A hand-built table classifies each group's members and reports `None` for
    /// a user-defined name.
    #[test]
    fn classify_covers_every_group() {
        let mut syntax = LslSyntax::default();
        let _prev = syntax.functions.insert(
            "llSay".to_owned(),
            LslFunction {
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
        let _prev = syntax.constants.insert(
            "TRUE".to_owned(),
            LslConstant {
                constant_type: Some(TypeName::Integer),
                value: Some("1".to_owned()),
                ..LslConstant::default()
            },
        );
        let _prev = syntax
            .events
            .insert("state_entry".to_owned(), LslEvent::default());
        let _prev = syntax
            .controls
            .insert("if".to_owned(), LslKeyword::default());
        let _prev = syntax
            .types
            .insert("integer".to_owned(), LslKeyword::default());

        assert_eq!(syntax.classify("llSay"), Some(SymbolKind::Function));
        assert_eq!(syntax.classify("TRUE"), Some(SymbolKind::Constant));
        assert_eq!(syntax.classify("state_entry"), Some(SymbolKind::Event));
        assert_eq!(syntax.classify("if"), Some(SymbolKind::Control));
        assert_eq!(syntax.classify("integer"), Some(SymbolKind::Type));
        // A user-defined name is not in the library table.
        assert_eq!(syntax.classify("myGlobal"), None);
        // LSL is case-sensitive: a differently-cased spelling is not a hit.
        assert_eq!(syntax.classify("llsay"), None);

        assert_eq!(syntax.len(), 5);
        assert!(!syntax.is_empty());
        assert!(LslSyntax::default().is_empty());
    }
}
