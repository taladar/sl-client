//! Error type for parsing a `message_template.msg` file.

use thiserror::Error;

/// An error encountered while tokenizing or parsing a message template.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The input ended while more tokens were expected.
    #[error("unexpected end of input while expecting {expected}")]
    UnexpectedEof {
        /// A description of what was expected.
        expected: String,
    },
    /// A token other than the expected one was found.
    #[error("line {line}: expected {expected}, found {found:?}")]
    Unexpected {
        /// The 1-based source line where the unexpected token appeared.
        line: usize,
        /// A description of what was expected.
        expected: String,
        /// The token that was actually found.
        found: String,
    },
    /// A word token was expected but a brace was found instead.
    #[error("line {line}: expected a word, found {found:?}")]
    ExpectedWord {
        /// The 1-based source line where the brace appeared.
        line: usize,
        /// The brace that was found.
        found: String,
    },
    /// An unknown message frequency keyword was encountered.
    #[error("line {line}: unknown frequency keyword {value:?}")]
    UnknownFrequency {
        /// The 1-based source line.
        line: usize,
        /// The offending keyword.
        value: String,
    },
    /// An unknown trust keyword was encountered.
    #[error("line {line}: unknown trust keyword {value:?}")]
    UnknownTrust {
        /// The 1-based source line.
        line: usize,
        /// The offending keyword.
        value: String,
    },
    /// An unknown encoding keyword was encountered.
    #[error("line {line}: unknown encoding keyword {value:?}")]
    UnknownEncoding {
        /// The 1-based source line.
        line: usize,
        /// The offending keyword.
        value: String,
    },
    /// An unknown block cardinality keyword was encountered.
    #[error("line {line}: unknown block cardinality keyword {value:?}")]
    UnknownCardinality {
        /// The 1-based source line.
        line: usize,
        /// The offending keyword.
        value: String,
    },
    /// An unknown field type keyword was encountered.
    #[error("line {line}: unknown field type keyword {value:?}")]
    UnknownFieldType {
        /// The 1-based source line.
        line: usize,
        /// The offending keyword.
        value: String,
    },
    /// A numeric token could not be parsed.
    #[error("line {line}: invalid number {value:?}")]
    InvalidNumber {
        /// The 1-based source line.
        line: usize,
        /// The offending token.
        value: String,
    },
}
