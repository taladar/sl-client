//! What kind of token a highlighted range is, in **tree-sitter's capture
//! vocabulary**.
//!
//! The names are tree-sitter's (and Helix's and Neovim's, which grew from it):
//! dotted and hierarchical, most general first, so `function.builtin` is a
//! `function` to anything that knows no finer name. That is the property a
//! colour scheme leans on — a scheme that colours `function` and nothing more
//! still colours a library call — and it is why a syntax scheme written for LSL
//! colours a Lua script too, when there is one.
//!
//! Two names here are not tree-sitter's own, because LSL has a thing no
//! tree-sitter grammar names: **`function.event`**, an LSL event handler, which
//! is neither a call nor a definition of the resident's own; and the state a
//! script moves between, which takes tree-sitter's **`namespace`**, the nearest
//! fit (a state is a named scope of handlers). Both fall back the usual way.

/// The kind of token a highlighted range is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capture {
    /// A comment, line or block.
    Comment,
    /// A string literal.
    String,
    /// An escape sequence inside a string literal (`\n`, `\"`).
    StringEscape,
    /// A numeric literal.
    Number,
    /// A keyword of the language — control flow, and the words that declare.
    Keyword,
    /// A type name.
    Type,
    /// A function of the file's own, where it is defined or called.
    Function,
    /// A function of the language's library.
    FunctionBuiltin,
    /// An event handler — LSL's `touch_start`, `timer`.
    FunctionEvent,
    /// A constant of the file's own.
    Constant,
    /// A constant of the language's library — `TRUE`, `PI`.
    ConstantBuiltin,
    /// A jump label.
    Label,
    /// A named scope — LSL's states.
    Namespace,
    /// An operator.
    Operator,
    /// A bracket of any kind.
    PunctuationBracket,
    /// A separator — `,`, `;`, `.`.
    PunctuationDelimiter,
    /// Punctuation with a meaning of its own — LSL's `@` before a label.
    PunctuationSpecial,
    /// Something that is not a token at all.
    Error,
}

impl Capture {
    /// Every capture, in a fixed order — what an editor indexes its style
    /// classes by ([`Self::index`]).
    pub const ALL: [Self; 18] = [
        Self::Comment,
        Self::String,
        Self::StringEscape,
        Self::Number,
        Self::Keyword,
        Self::Type,
        Self::Function,
        Self::FunctionBuiltin,
        Self::FunctionEvent,
        Self::Constant,
        Self::ConstantBuiltin,
        Self::Label,
        Self::Namespace,
        Self::Operator,
        Self::PunctuationBracket,
        Self::PunctuationDelimiter,
        Self::PunctuationSpecial,
        Self::Error,
    ];

    /// The capture's tree-sitter name, dotted: `function.builtin`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::String => "string",
            Self::StringEscape => "string.escape",
            Self::Number => "number",
            Self::Keyword => "keyword",
            Self::Type => "type",
            Self::Function => "function",
            Self::FunctionBuiltin => "function.builtin",
            Self::FunctionEvent => "function.event",
            Self::Constant => "constant",
            Self::ConstantBuiltin => "constant.builtin",
            Self::Label => "label",
            Self::Namespace => "namespace",
            Self::Operator => "operator",
            Self::PunctuationBracket => "punctuation.bracket",
            Self::PunctuationDelimiter => "punctuation.delimiter",
            Self::PunctuationSpecial => "punctuation.special",
            Self::Error => "error",
        }
    }

    /// The capture's name with `-` for `.`: what a stylesheet class or a custom
    /// property is named after (`function-builtin`), since a `.` would start a
    /// second class.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::StringEscape => "string-escape",
            Self::FunctionBuiltin => "function-builtin",
            Self::FunctionEvent => "function-event",
            Self::ConstantBuiltin => "constant-builtin",
            Self::PunctuationBracket => "punctuation-bracket",
            Self::PunctuationDelimiter => "punctuation-delimiter",
            Self::PunctuationSpecial => "punctuation-special",
            other => other.name(),
        }
    }

    /// The more general capture this one falls back to, or `None` for a
    /// top-level one: `function.builtin` → `function`.
    #[must_use]
    pub const fn parent(self) -> Option<Self> {
        match self {
            Self::StringEscape => Some(Self::String),
            Self::FunctionBuiltin | Self::FunctionEvent => Some(Self::Function),
            Self::ConstantBuiltin => Some(Self::Constant),
            _ => None,
        }
    }

    /// This capture's place in [`Self::ALL`].
    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|capture| *capture == self)
            .unwrap_or_default()
    }

    /// The capture named `name`, or the most specific one a dotted prefix of it
    /// names (`keyword.control.repeat` → `keyword`); `None` when not even its
    /// first segment is known.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let mut candidate = name;
        loop {
            if let Some(capture) = Self::ALL
                .iter()
                .copied()
                .find(|capture| capture.name() == candidate)
            {
                return Some(capture);
            }
            let (head, _tail) = candidate.rsplit_once('.')?;
            candidate = head;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Capture;
    use pretty_assertions::assert_eq;

    /// Every capture's index is its place, names and slugs are unique, and a
    /// parent's name is a prefix of its child's.
    #[test]
    fn the_vocabulary_is_consistent() {
        let mut names = std::collections::BTreeSet::new();
        let mut slugs = std::collections::BTreeSet::new();
        for (place, capture) in Capture::ALL.iter().enumerate() {
            assert_eq!(capture.index(), place);
            assert!(names.insert(capture.name()), "{capture:?}");
            assert!(slugs.insert(capture.slug()), "{capture:?}");
            assert_eq!(capture.slug(), capture.name().replace('.', "-"));
            if let Some(parent) = capture.parent() {
                assert!(
                    capture.name().starts_with(&format!("{}.", parent.name())),
                    "{capture:?}"
                );
            }
        }
    }

    /// A finer name than any here falls back to the longest known prefix.
    #[test]
    fn a_finer_name_falls_back() {
        assert_eq!(
            Capture::from_name("function.builtin"),
            Some(Capture::FunctionBuiltin)
        );
        assert_eq!(
            Capture::from_name("keyword.control.repeat"),
            Some(Capture::Keyword)
        );
        assert_eq!(
            Capture::from_name("string.special.url"),
            Some(Capture::String)
        );
        assert_eq!(Capture::from_name("variable.parameter"), None);
    }
}
