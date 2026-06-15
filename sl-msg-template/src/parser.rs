//! A recursive-descent parser turning a token stream into a [`Template`].

use core::iter::Peekable;
use core::slice::Iter;

use crate::ast::{
    BlockDef, Cardinality, Encoding, FieldDef, FieldType, Frequency, MessageDef, Template, Trust,
};
use crate::error::ParseError;
use crate::lexer::{Token, TokenKind, tokenize};

/// Parses a complete message template source string into a [`Template`].
///
/// # Errors
///
/// Returns a [`ParseError`] if the input is not a well-formed message template
/// (unexpected tokens, unknown keywords, malformed numbers, or truncated input).
pub fn parse(input: &str) -> Result<Template, ParseError> {
    let tokens = tokenize(input);
    let mut parser = Parser {
        tokens: tokens.iter().peekable(),
    };
    parser.parse_template()
}

/// The parser state: a peekable cursor over the token slice.
struct Parser<'a> {
    /// The remaining tokens to consume.
    tokens: Peekable<Iter<'a, Token>>,
}

impl Parser<'_> {
    /// Parses the whole template: an optional `version` line followed by zero or
    /// more message definitions, until the input is exhausted.
    fn parse_template(&mut self) -> Result<Template, ParseError> {
        let mut version = None;
        if let Some(TokenKind::Word(word)) = self.peek_kind()
            && word == "version"
        {
            // Consume `version` and its value token.
            self.advance();
            version = Some(self.expect_word("a version number")?.to_owned());
        }

        let mut messages = Vec::new();
        while self.peek_is_open() {
            messages.push(self.parse_message()?);
        }

        // Any remaining token here is unexpected trailing content.
        if let Some(token) = self.tokens.next() {
            return Err(ParseError::Unexpected {
                line: token.line,
                expected: "end of input".to_owned(),
                found: describe(token),
            });
        }

        Ok(Template { version, messages })
    }

    /// Parses a single `{ Name Freq Number Trust Encoding [flags] { block }* }`.
    fn parse_message(&mut self) -> Result<MessageDef, ParseError> {
        self.expect_open("a message definition")?;
        let name = self.expect_word("a message name")?.to_owned();

        let (freq_word, freq_line) = self.expect_word_with_line("a message frequency")?;
        let frequency = parse_frequency(&freq_word, freq_line)?;

        let (number_word, number_line) = self.expect_word_with_line("a message number")?;
        let number = parse_number(&number_word, number_line)?;

        let (trust_word, trust_line) = self.expect_word_with_line("a trust attribute")?;
        let trust = parse_trust(&trust_word, trust_line)?;

        let (encoding_word, encoding_line) = self.expect_word_with_line("an encoding attribute")?;
        let encoding = parse_encoding(&encoding_word, encoding_line)?;

        // Zero or more trailing flag words before the first block or the close.
        let mut flags = Vec::new();
        while let Some(TokenKind::Word(word)) = self.peek_kind() {
            flags.push(word.clone());
            self.advance();
        }

        let mut blocks = Vec::new();
        while self.peek_is_open() {
            blocks.push(self.parse_block()?);
        }

        self.expect_close("the end of a message definition")?;

        Ok(MessageDef {
            name,
            frequency,
            number,
            trust,
            encoding,
            flags,
            blocks,
        })
    }

    /// Parses a single `{ Name Cardinality [N] { field }* }`.
    fn parse_block(&mut self) -> Result<BlockDef, ParseError> {
        self.expect_open("a block definition")?;
        let name = self.expect_word("a block name")?.to_owned();

        let (card_word, card_line) = self.expect_word_with_line("a block cardinality")?;
        let cardinality = match card_word.as_str() {
            "Single" => Cardinality::Single,
            "Variable" => Cardinality::Variable,
            "Multiple" => {
                let (count_word, count_line) = self.expect_word_with_line("a repetition count")?;
                Cardinality::Multiple(parse_number(&count_word, count_line)?)
            }
            _ => {
                return Err(ParseError::UnknownCardinality {
                    line: card_line,
                    value: card_word,
                });
            }
        };

        let mut fields = Vec::new();
        while self.peek_is_open() {
            fields.push(self.parse_field()?);
        }

        self.expect_close("the end of a block definition")?;

        Ok(BlockDef {
            name,
            cardinality,
            fields,
        })
    }

    /// Parses a single `{ Name Type [Size] }`.
    fn parse_field(&mut self) -> Result<FieldDef, ParseError> {
        self.expect_open("a field definition")?;
        let name = self.expect_word("a field name")?.to_owned();

        let (type_word, type_line) = self.expect_word_with_line("a field type")?;
        let ty = match type_word.as_str() {
            "U8" => FieldType::U8,
            "U16" => FieldType::U16,
            "U32" => FieldType::U32,
            "U64" => FieldType::U64,
            "S8" => FieldType::S8,
            "S16" => FieldType::S16,
            "S32" => FieldType::S32,
            "F32" => FieldType::F32,
            "F64" => FieldType::F64,
            "LLUUID" => FieldType::Uuid,
            "LLVector3" => FieldType::Vector3,
            "LLVector3d" => FieldType::Vector3d,
            "LLVector4" => FieldType::Vector4,
            "LLQuaternion" => FieldType::Quaternion,
            "BOOL" => FieldType::Bool,
            "IPADDR" => FieldType::IpAddr,
            "IPPORT" => FieldType::IpPort,
            "Variable" => {
                let (size_word, size_line) = self.expect_word_with_line("a length-prefix size")?;
                FieldType::Variable {
                    length_bytes: parse_u8(&size_word, size_line)?,
                }
            }
            "Fixed" => {
                let (size_word, size_line) = self.expect_word_with_line("a fixed byte count")?;
                FieldType::Fixed {
                    bytes: parse_number(&size_word, size_line)?,
                }
            }
            _ => {
                return Err(ParseError::UnknownFieldType {
                    line: type_line,
                    value: type_word,
                });
            }
        };

        self.expect_close("the end of a field definition")?;

        Ok(FieldDef { name, ty })
    }

    /// Advances past the next token, discarding it.
    fn advance(&mut self) {
        self.tokens.next();
    }

    /// Returns the kind of the next token without consuming it.
    fn peek_kind(&mut self) -> Option<&TokenKind> {
        self.tokens.peek().map(|token| &token.kind)
    }

    /// Returns `true` if the next token is an opening brace.
    fn peek_is_open(&mut self) -> bool {
        matches!(self.peek_kind(), Some(&TokenKind::OpenBrace))
    }

    /// Consumes an opening brace or returns an error described by `expected`.
    fn expect_open(&mut self, expected: &str) -> Result<(), ParseError> {
        match self.tokens.next() {
            Some(Token {
                kind: TokenKind::OpenBrace,
                ..
            }) => Ok(()),
            Some(token) => Err(ParseError::Unexpected {
                line: token.line,
                expected: format!("`{{` ({expected})"),
                found: describe(token),
            }),
            None => Err(ParseError::UnexpectedEof {
                expected: format!("`{{` ({expected})"),
            }),
        }
    }

    /// Consumes a closing brace or returns an error described by `expected`.
    fn expect_close(&mut self, expected: &str) -> Result<(), ParseError> {
        match self.tokens.next() {
            Some(Token {
                kind: TokenKind::CloseBrace,
                ..
            }) => Ok(()),
            Some(token) => Err(ParseError::Unexpected {
                line: token.line,
                expected: format!("`}}` ({expected})"),
                found: describe(token),
            }),
            None => Err(ParseError::UnexpectedEof {
                expected: format!("`}}` ({expected})"),
            }),
        }
    }

    /// Consumes a word token, returning its text, or errors described by `expected`.
    fn expect_word(&mut self, expected: &str) -> Result<String, ParseError> {
        self.expect_word_with_line(expected).map(|(word, _)| word)
    }

    /// Consumes a word token, returning its text and source line.
    fn expect_word_with_line(&mut self, expected: &str) -> Result<(String, usize), ParseError> {
        match self.tokens.next() {
            Some(Token {
                kind: TokenKind::Word(word),
                line,
            }) => Ok((word.clone(), *line)),
            Some(token) => Err(ParseError::ExpectedWord {
                line: token.line,
                found: describe(token),
            }),
            None => Err(ParseError::UnexpectedEof {
                expected: expected.to_owned(),
            }),
        }
    }
}

/// Returns a short human-readable description of a token for error messages.
fn describe(token: &Token) -> String {
    match &token.kind {
        TokenKind::OpenBrace => "{".to_owned(),
        TokenKind::CloseBrace => "}".to_owned(),
        TokenKind::Word(word) => word.clone(),
    }
}

/// Parses a message frequency keyword.
fn parse_frequency(word: &str, line: usize) -> Result<Frequency, ParseError> {
    match word {
        "High" => Ok(Frequency::High),
        "Medium" => Ok(Frequency::Medium),
        "Low" => Ok(Frequency::Low),
        "Fixed" => Ok(Frequency::Fixed),
        _ => Err(ParseError::UnknownFrequency {
            line,
            value: word.to_owned(),
        }),
    }
}

/// Parses a trust keyword.
fn parse_trust(word: &str, line: usize) -> Result<Trust, ParseError> {
    match word {
        "Trusted" => Ok(Trust::Trusted),
        "NotTrusted" => Ok(Trust::NotTrusted),
        _ => Err(ParseError::UnknownTrust {
            line,
            value: word.to_owned(),
        }),
    }
}

/// Parses an encoding keyword.
fn parse_encoding(word: &str, line: usize) -> Result<Encoding, ParseError> {
    match word {
        "Zerocoded" => Ok(Encoding::Zerocoded),
        "Unencoded" => Ok(Encoding::Unencoded),
        _ => Err(ParseError::UnknownEncoding {
            line,
            value: word.to_owned(),
        }),
    }
}

/// Parses a `u32` number token, accepting either decimal or `0x`-prefixed hex.
fn parse_number(word: &str, line: usize) -> Result<u32, ParseError> {
    let parsed = match word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => word.parse::<u32>(),
    };
    parsed.map_err(|_ignored| ParseError::InvalidNumber {
        line,
        value: word.to_owned(),
    })
}

/// Parses a `u8` number token (used for variable-field length-prefix sizes).
fn parse_u8(word: &str, line: usize) -> Result<u8, ParseError> {
    word.parse::<u8>()
        .map_err(|_ignored| ParseError::InvalidNumber {
            line,
            value: word.to_owned(),
        })
}
