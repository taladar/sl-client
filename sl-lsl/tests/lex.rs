//! Integration tests for the `sl-lsl` lexer over its public API.

#[cfg(test)]
mod tests {
    use core::ops::Range;

    use pretty_assertions::assert_eq;

    use sl_lsl::{Token, lex, tokens};

    /// Lex `source` into the bare sequence of token kinds (dropping spans), for
    /// concise structural assertions.
    fn kinds(source: &str) -> Vec<Token> {
        lex(source).into_iter().map(|t| t.token).collect()
    }

    /// Lex `source` into `(kind, matched-text)` pairs, exercising both the
    /// token classification and the spans that slice the text back out.
    fn kinds_text(source: &str) -> Vec<(Token, &str)> {
        lex(source)
            .into_iter()
            .map(|t| {
                let text = t.text(source).unwrap_or("<bad span>");
                (t.token, text)
            })
            .collect()
    }

    /// Lex `source` into `(kind, byte-range)` pairs, for exact span assertions.
    fn spanned(source: &str) -> Vec<(Token, Range<usize>)> {
        lex(source).into_iter().map(|t| (t.token, t.span)).collect()
    }

    #[test]
    fn empty_and_whitespace_produce_no_tokens() {
        assert_eq!(kinds(""), vec![]);
        assert_eq!(kinds("   \t\r\n\x0c  "), vec![]);
    }

    #[test]
    fn identifiers_are_plain_words_no_keyword_baked_in() {
        // `integer`, `default`, `llSay` and a user name all lex the same: the
        // library is not baked into the lexer.
        assert_eq!(
            kinds("integer default llSay myVar _x1"),
            vec![Token::Identifier; 5]
        );
    }

    #[test]
    fn integers_decimal_and_hex() {
        assert_eq!(
            kinds_text("0 42 0x2A 0XdeadBEEF"),
            vec![
                (Token::IntegerLiteral, "0"),
                (Token::IntegerLiteral, "42"),
                (Token::IntegerLiteral, "0x2A"),
                (Token::IntegerLiteral, "0XdeadBEEF"),
            ]
        );
    }

    #[test]
    fn floats_every_grammatical_form() {
        assert_eq!(
            kinds_text("1.0 .5 1. 1e10 1.5e-3 2.0f .25F 7E+2"),
            vec![
                (Token::FloatLiteral, "1.0"),
                (Token::FloatLiteral, ".5"),
                (Token::FloatLiteral, "1."),
                (Token::FloatLiteral, "1e10"),
                (Token::FloatLiteral, "1.5e-3"),
                (Token::FloatLiteral, "2.0f"),
                (Token::FloatLiteral, ".25F"),
                (Token::FloatLiteral, "7E+2"),
            ]
        );
    }

    #[test]
    fn bare_f_suffix_is_not_a_float() {
        // `10f` has no dot and no exponent, so per the LSL grammar it is an
        // integer `10` followed by the identifier `f`, not a float.
        assert_eq!(
            kinds_text("10f"),
            vec![(Token::IntegerLiteral, "10"), (Token::Identifier, "f")]
        );
    }

    #[test]
    fn member_access_dot_vs_float_dot() {
        // `v.x` is identifier, dot, identifier — the dot is member access, not
        // a float — while `.5` is a float.
        assert_eq!(
            kinds_text("v.x .5"),
            vec![
                (Token::Identifier, "v"),
                (Token::Dot, "."),
                (Token::Identifier, "x"),
                (Token::FloatLiteral, ".5"),
            ]
        );
    }

    #[test]
    fn strings_with_escapes() {
        // The `\"` escape does not end the string; the `\\` before the closing
        // quote is an escaped backslash, so the string terminates normally.
        assert_eq!(
            kinds_text(r#""hello" "a\"b" "c\\""#),
            vec![
                (Token::StringLiteral, r#""hello""#),
                (Token::StringLiteral, r#""a\"b""#),
                (Token::StringLiteral, r#""c\\""#),
            ]
        );
    }

    #[test]
    fn unterminated_string_runs_to_end() {
        assert_eq!(
            kinds_text("x = \"oops"),
            vec![
                (Token::Identifier, "x"),
                (Token::Assign, "="),
                (Token::StringLiteral, "\"oops"),
            ]
        );
    }

    #[test]
    fn line_comment_stops_at_newline() {
        assert_eq!(
            kinds_text("a // comment\nb"),
            vec![
                (Token::Identifier, "a"),
                (Token::LineComment, "// comment"),
                (Token::Identifier, "b"),
            ]
        );
    }

    #[test]
    fn block_comment_terminated_and_unterminated() {
        assert_eq!(
            kinds_text("a /* x\ny */ b"),
            vec![
                (Token::Identifier, "a"),
                (Token::BlockComment, "/* x\ny */"),
                (Token::Identifier, "b"),
            ]
        );
        // Unterminated `/*` runs to end-of-input.
        assert_eq!(
            kinds_text("a /* never closed"),
            vec![
                (Token::Identifier, "a"),
                (Token::BlockComment, "/* never closed"),
            ]
        );
    }

    #[test]
    fn operators_maximal_munch() {
        // Every multi-character operator must win over its single-character
        // prefixes (`<<` over two `<`, `<=` over `<` then `=`, etc.).
        assert_eq!(
            kinds("<< >> <= >= == != && || ++ -- += -= *= /= %="),
            vec![
                Token::ShiftLeft,
                Token::ShiftRight,
                Token::LessEq,
                Token::GreaterEq,
                Token::EqEq,
                Token::NotEq,
                Token::AndAnd,
                Token::OrOr,
                Token::PlusPlus,
                Token::MinusMinus,
                Token::PlusAssign,
                Token::MinusAssign,
                Token::StarAssign,
                Token::SlashAssign,
                Token::PercentAssign,
            ]
        );
    }

    #[test]
    fn single_char_operators_and_punctuation() {
        assert_eq!(
            kinds("+ - * / % = < > ! & | ^ ~ . , ; ( ) { } [ ] @"),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::Slash,
                Token::Percent,
                Token::Assign,
                Token::Less,
                Token::Greater,
                Token::Bang,
                Token::Amp,
                Token::Pipe,
                Token::Caret,
                Token::Tilde,
                Token::Dot,
                Token::Comma,
                Token::Semicolon,
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
                Token::At,
            ]
        );
    }

    #[test]
    fn slash_disambiguation() {
        // `/` alone is Slash; `/=` is SlashAssign; `//` starts a comment; `/*`
        // starts a block comment.
        assert_eq!(
            kinds("a / b"),
            vec![Token::Identifier, Token::Slash, Token::Identifier]
        );
        assert_eq!(
            kinds("a /= b"),
            vec![Token::Identifier, Token::SlashAssign, Token::Identifier]
        );
        assert_eq!(kinds("a //b"), vec![Token::Identifier, Token::LineComment]);
        assert_eq!(
            kinds("a /*b*/"),
            vec![Token::Identifier, Token::BlockComment]
        );
    }

    #[test]
    fn stray_bytes_become_error_tokens() {
        // `#` begins no LSL token.
        assert_eq!(
            kinds_text("a # b"),
            vec![
                (Token::Identifier, "a"),
                (Token::Error, "#"),
                (Token::Identifier, "b"),
            ]
        );
        assert!(kinds("$?`").iter().all(|t| t.is_error()));
    }

    #[test]
    fn spans_are_exact_and_on_char_boundaries() {
        let src = "n = 42;";
        assert_eq!(
            spanned(src),
            vec![
                (Token::Identifier, 0..1),
                (Token::Assign, 2..3),
                (Token::IntegerLiteral, 4..6),
                (Token::Semicolon, 6..7),
            ]
        );
        // Every span slices cleanly out of the (utf-8) source.
        for t in lex(src) {
            assert!(t.text(src).is_some());
        }
    }

    #[test]
    fn multibyte_content_in_strings_and_comments() {
        // A non-ASCII glyph inside a string/comment must not split a char
        // boundary — the span must slice back out intact.
        assert_eq!(
            kinds_text("\"héllo→\" // café"),
            vec![
                (Token::StringLiteral, "\"héllo→\""),
                (Token::LineComment, "// café"),
            ]
        );
    }

    #[test]
    fn tokens_iterator_matches_lex_vector() {
        let src = "default { state_entry() { llSay(0, \"hi\"); } }";
        let streamed: Vec<_> = tokens(src).collect();
        assert_eq!(streamed, lex(src));
    }

    #[test]
    fn realistic_snippet_has_no_errors_and_keeps_comment() {
        let src = "\
default
{
    state_entry()
    {
        llSay(PUBLIC_CHANNEL, \"Hello, Avatar!\"); // greet
    }
}
";
        let toks = lex(src);

        // Concatenating every token's text (in order, spaces removed)
        // reproduces the source with its whitespace removed.
        let joined: String = toks
            .iter()
            .filter_map(|t| t.text(src))
            .collect::<String>()
            .replace(' ', "");
        let expected: String = src.split_whitespace().collect();
        assert_eq!(joined, expected);

        // No error tokens in valid source.
        assert!(toks.iter().all(|t| !t.token.is_error()));

        // The trailing comment is present and marked as trivia.
        let comment = toks.iter().find(|t| t.token == Token::LineComment);
        assert_eq!(comment.and_then(|t| t.text(src)), Some("// greet"));
        assert!(comment.is_some_and(sl_lsl::SpannedToken::is_trivia));
    }
}
