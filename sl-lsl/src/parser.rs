//! An **error-tolerant recursive-descent parser** that turns the
//! [`crate::lexer`] token stream into an [`crate::ast`] tree.
//!
//! This is the piece that differs from a compiler front-end: an editor parses
//! *broken* code on every keystroke, so **recovery matters more than a clean
//! grammar**. The parser never aborts — where the input does not parse it
//! records a [`ParseError`], drops an [`Expr::Error`] or [`Stmt::Error`]
//! placeholder into the tree and carries on, so a half-typed statement does not
//! discard the rest of the file. Every call to [`parse`] returns *both* a tree
//! and the list of errors.
//!
//! The parser holds the small set of LSL **keywords** (the types `integer` …
//! `list`, the control words `if`/`while`/`state` …) and classifies the
//! lexer's uniform [`Token::Identifier`] words against them. It does **not**
//! hold the LSL *library*: a called name, an event name or a constant is left
//! as a plain identifier for the semantic pass to resolve against the grid's
//! symbol table.
//!
//! ## Precedence
//!
//! Expressions use precedence-climbing (a Pratt loop). LSL's operator
//! precedence is transcribed from Linden Lab's own grammar, including its
//! famous quirk that `&&` and `||` share a single left-associative level.
//!
//! ## The `<` / `>` ambiguity
//!
//! `<a, b, c>` is a vector and `<` is also less-than, so the angle brackets
//! collide with the relational and shift operators (the only ones spelled with
//! `<` or `>`). A `<` is read as a vector/rotation constructor **only in
//! operand position** (where a primary expression is expected); after an
//! operand it is the relational operator. Inside a constructor's
//! components the operators that consume a `<`/`>` — `<`, `<=`, `>`, `>=`,
//! `<<`, `>>` — are suppressed so the closing `>` wins, exactly as real LSL
//! requires those comparisons to be parenthesised (`<(a > b), 0, 0>`); a
//! parenthesised sub-expression lifts the suppression again.

use core::ops::Range;

use crate::ast::{
    AssignOp, BinaryOp, Block, EventHandler, Expr, FunctionDef, GlobalItem, GlobalVar, Ident,
    Param, PostfixOp, PrefixOp, Script, StateDef, StateName, Stmt, TypeName, TypeRef,
};
use crate::lexer::{SpannedToken, lex};
use crate::token::Token;

/// A single recovered syntax error: a human-readable message and the byte span
/// it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// The diagnostic message.
    pub message: String,
    /// The byte range in the source the error points at (may be zero-width at
    /// the point a token was expected).
    pub span: Range<usize>,
}

/// The result of [`parse`]: the (always-present) syntax tree and any recovered
/// errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    /// The parsed script. Present even when `errors` is non-empty, carrying
    /// [`Expr::Error`] / [`Stmt::Error`] placeholders where recovery kicked in.
    pub script: Script,
    /// The recovered syntax errors, in source order.
    pub errors: Vec<ParseError>,
}

impl Parse {
    /// Whether the parse recovered from at least one syntax error.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Parse LSL `source` into a [`Parse`] (tree plus recovered errors).
///
/// Total and error-tolerant: it always returns a tree, never panics and never
/// stops early on a syntax error.
#[must_use]
pub fn parse(source: &str) -> Parse {
    let mut parser = Parser::new(source);
    let script = parser.parse_script();
    Parse {
        script,
        errors: parser.errors,
    }
}

/// An infix operator recognised by the Pratt loop: a binary operator or an
/// assignment.
#[derive(Debug, Clone, Copy)]
enum InfixOp {
    /// A binary operator combining two values.
    Binary(BinaryOp),
    /// An assignment operator storing into an lvalue.
    Assign(AssignOp),
}

/// The recursive-descent parser state: the token stream, a cursor into it and
/// the accumulated errors.
struct Parser<'src> {
    /// The source string the tokens index into.
    source: &'src str,
    /// The non-trivia tokens (comments filtered out).
    tokens: Vec<SpannedToken>,
    /// The index of the current token in `tokens`.
    pos: usize,
    /// The recovered syntax errors, in source order.
    errors: Vec<ParseError>,
}

impl<'src> Parser<'src> {
    /// Build a parser over `source`, lexing it and dropping trivia (comments)
    /// so the grammar never has to mention them.
    fn new(source: &'src str) -> Self {
        let tokens = lex(source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .collect();
        Self {
            source,
            tokens,
            pos: 0,
            errors: Vec::new(),
        }
    }

    // -- cursor / span helpers --------------------------------------------

    /// The current token, if any remain.
    fn peek(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.pos)
    }

    /// The kind of the current token, if any remain.
    fn peek_kind(&self) -> Option<Token> {
        self.peek().map(|token| token.token)
    }

    /// The kind of the token `offset` positions ahead of the cursor.
    fn peek_kind_at(&self, offset: usize) -> Option<Token> {
        self.tokens
            .get(self.pos.saturating_add(offset))
            .map(|token| token.token)
    }

    /// Whether the current token is of `kind`.
    fn at(&self, kind: Token) -> bool {
        self.peek_kind() == Some(kind)
    }

    /// The current token's source text, or `""` at end of input.
    fn cur_text(&self) -> &'src str {
        self.peek()
            .and_then(|token| token.text(self.source))
            .unwrap_or("")
    }

    /// The start byte offset of the current token, or the source length at end
    /// of input.
    fn cur_start(&self) -> usize {
        self.peek()
            .map_or(self.source.len(), |token| token.span.start)
    }

    /// The end byte offset of the current token, or the source length at end of
    /// input.
    fn cur_end(&self) -> usize {
        self.peek()
            .map_or(self.source.len(), |token| token.span.end)
    }

    /// The byte span of the current token, or a zero-width span at the source
    /// end at end of input.
    fn cur_span(&self) -> Range<usize> {
        self.peek().map_or_else(
            || self.source.len()..self.source.len(),
            |token| token.span.start..token.span.end,
        )
    }

    /// The end byte offset of the previously consumed token, or 0 before any
    /// token has been consumed.
    fn prev_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|prev| self.tokens.get(prev))
            .map_or(0, |token| token.span.end)
    }

    /// Advance the cursor past the current token.
    const fn bump(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos = self.pos.saturating_add(1);
        }
    }

    /// If the current token is `kind`, consume it and return `true`.
    fn eat(&mut self, kind: Token) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consume `kind` if present; otherwise record an "expected …" error.
    fn expect(&mut self, kind: Token, what: &str) {
        if !self.eat(kind) {
            self.error_here(format!("expected {what}"));
        }
    }

    /// Record an error at the current position with the given message.
    fn error_here(&mut self, message: String) {
        let span = self.cur_span();
        self.errors.push(ParseError { message, span });
    }

    /// Record an error at an explicit span.
    fn error_at(&mut self, span: Range<usize>, message: String) {
        self.errors.push(ParseError { message, span });
    }

    /// Whether the current token is an identifier whose text equals `keyword`.
    fn at_keyword(&self, keyword: &str) -> bool {
        self.at(Token::Identifier) && self.cur_text() == keyword
    }

    /// Consume the current token as an identifier node (assumes the caller has
    /// checked it is an identifier).
    fn make_ident(&mut self) -> Ident {
        let span = self.cur_span();
        let name = self.cur_text().to_owned();
        self.bump();
        Ident { name, span }
    }

    /// Consume an identifier, or record an error and return an empty
    /// placeholder identifier at the current position.
    fn expect_ident(&mut self, what: &str) -> Ident {
        if self.at(Token::Identifier) {
            self.make_ident()
        } else {
            self.error_here(format!("expected {what}"));
            let at = self.cur_start();
            Ident {
                name: String::new(),
                span: at..at,
            }
        }
    }

    /// Consume the current type keyword into a [`TypeRef`], or `None` if the
    /// current token is not a type keyword.
    fn parse_type_ref(&mut self) -> Option<TypeRef> {
        let kind = TypeName::from_keyword(self.cur_text())?;
        if !self.at(Token::Identifier) {
            return None;
        }
        let span = self.cur_span();
        self.bump();
        Some(TypeRef { kind, span })
    }

    // -- top level ---------------------------------------------------------

    /// Parse a whole script: global declarations and states, in any order the
    /// (possibly broken) input presents them.
    fn parse_script(&mut self) -> Script {
        let mut globals = Vec::new();
        let mut states = Vec::new();
        while self.peek_kind().is_some() {
            let before = self.pos;
            if self.at_keyword("default") || self.at_keyword("state") {
                states.push(self.parse_state_def());
            } else if let Some(item) = self.parse_global_item() {
                globals.push(item);
            }
            if self.pos == before {
                // Nothing consumed: force progress so recovery cannot loop.
                self.error_here("expected a global declaration or a state".to_owned());
                self.bump();
            }
        }
        Script {
            globals,
            states,
            span: 0..self.source.len(),
        }
    }

    /// Parse one top-level global declaration (a variable or a function), or
    /// `None` if the current token starts neither.
    fn parse_global_item(&mut self) -> Option<GlobalItem> {
        let start = self.cur_start();
        if let Some(ty) = self.parse_type_ref() {
            let name = self.expect_ident("a variable or function name");
            if self.at(Token::LParen) {
                let params = self.parse_params();
                let body = self.parse_block();
                let span = start..body.span.end;
                Some(GlobalItem::Function(FunctionDef {
                    ret: Some(ty),
                    name,
                    params,
                    body,
                    span,
                }))
            } else {
                let init = if self.eat(Token::Assign) {
                    Some(self.parse_expr())
                } else {
                    None
                };
                self.expect(Token::Semicolon, "`;` after the global variable");
                let span = start..self.prev_end();
                Some(GlobalItem::Variable(GlobalVar {
                    ty,
                    name,
                    init,
                    span,
                }))
            }
        } else if self.at(Token::Identifier) {
            // A `void` function has no return type: `name(params) { body }`.
            let name = self.make_ident();
            let params = self.parse_params();
            let body = self.parse_block();
            let span = start..body.span.end;
            Some(GlobalItem::Function(FunctionDef {
                ret: None,
                name,
                params,
                body,
                span,
            }))
        } else {
            None
        }
    }

    /// Parse a state block: `default { … }` or `state name { … }`.
    fn parse_state_def(&mut self) -> StateDef {
        let start = self.cur_start();
        let name = if self.at_keyword("default") {
            let span = self.cur_span();
            self.bump();
            StateName::Default(span)
        } else {
            // `state` keyword.
            self.bump();
            StateName::Named(self.expect_ident("a state name"))
        };
        self.expect(Token::LBrace, "`{` to open the state body");
        let mut events = Vec::new();
        while !self.at(Token::RBrace) && self.peek_kind().is_some() {
            let before = self.pos;
            events.push(self.parse_event_handler());
            if self.pos == before {
                self.error_here("expected an event handler".to_owned());
                self.bump();
            }
        }
        self.expect(Token::RBrace, "`}` to close the state body");
        let span = start..self.prev_end();
        StateDef { name, events, span }
    }

    /// Parse an event handler: `name(params) { body }`.
    fn parse_event_handler(&mut self) -> EventHandler {
        let start = self.cur_start();
        let name = self.expect_ident("an event name");
        let params = self.parse_params();
        let body = self.parse_block();
        let span = start..body.span.end;
        EventHandler {
            name,
            params,
            body,
            span,
        }
    }

    /// Parse a parenthesised, comma-separated list of typed parameters:
    /// `(type name, type name, …)`.
    fn parse_params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        self.expect(Token::LParen, "`(` to open the parameter list");
        if !self.at(Token::RParen) && self.peek_kind().is_some() {
            loop {
                let before = self.pos;
                let start = self.cur_start();
                let ty = self.parse_type_ref().unwrap_or_else(|| {
                    self.error_here("expected a parameter type".to_owned());
                    let at = self.cur_start();
                    TypeRef {
                        kind: TypeName::Integer,
                        span: at..at,
                    }
                });
                let name = self.expect_ident("a parameter name");
                let span = start..self.prev_end();
                params.push(Param { ty, name, span });
                if !self.eat(Token::Comma) {
                    break;
                }
                if self.pos == before {
                    // No progress on a malformed parameter: bail out.
                    break;
                }
            }
        }
        self.expect(Token::RParen, "`)` to close the parameter list");
        params
    }

    // -- statements --------------------------------------------------------

    /// Parse a brace-delimited block of statements.
    fn parse_block(&mut self) -> Block {
        let start = self.cur_start();
        self.expect(Token::LBrace, "`{` to open a block");
        let mut statements = Vec::new();
        while !self.at(Token::RBrace) && self.peek_kind().is_some() {
            let before = self.pos;
            statements.push(self.parse_statement());
            if self.pos == before {
                self.error_here("expected a statement".to_owned());
                let span = self.cur_span();
                self.bump();
                statements.push(Stmt::Error(span));
            }
        }
        self.expect(Token::RBrace, "`}` to close a block");
        let span = start..self.prev_end();
        Block { statements, span }
    }

    /// Parse a single statement, dispatching on the leading token.
    fn parse_statement(&mut self) -> Stmt {
        match self.peek_kind() {
            Some(Token::LBrace) => Stmt::Block(self.parse_block()),
            Some(Token::Semicolon) => {
                let span = self.cur_span();
                self.bump();
                Stmt::Empty(span)
            }
            Some(Token::At) => self.parse_label(),
            Some(Token::Identifier) => match self.cur_text() {
                "if" => self.parse_if(),
                "while" => self.parse_while(),
                "do" => self.parse_do_while(),
                "for" => self.parse_for(),
                "return" => self.parse_return(),
                "jump" => self.parse_jump(),
                "state" => self.parse_state_change(),
                word if TypeName::from_keyword(word).is_some() => self.parse_local(),
                _ => self.parse_expr_statement(),
            },
            _ => self.parse_expr_statement(),
        }
    }

    /// Parse a jump-label definition: `@label;`.
    fn parse_label(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `@`
        let name = self.expect_ident("a label name");
        self.expect(Token::Semicolon, "`;` after the label");
        let span = start..self.prev_end();
        Stmt::Label { name, span }
    }

    /// Parse an `if` statement with an optional `else` branch.
    fn parse_if(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `if`
        self.expect(Token::LParen, "`(` after `if`");
        let cond = self.parse_expr();
        self.expect(Token::RParen, "`)` after the `if` condition");
        let then_branch = Box::new(self.parse_statement());
        let else_branch = if self.at_keyword("else") {
            self.bump();
            Some(Box::new(self.parse_statement()))
        } else {
            None
        };
        let span = start..self.prev_end();
        Stmt::If {
            cond,
            then_branch,
            else_branch,
            span,
        }
    }

    /// Parse a `while` loop.
    fn parse_while(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `while`
        self.expect(Token::LParen, "`(` after `while`");
        let cond = self.parse_expr();
        self.expect(Token::RParen, "`)` after the `while` condition");
        let body = Box::new(self.parse_statement());
        let span = start..self.prev_end();
        Stmt::While { cond, body, span }
    }

    /// Parse a `do`/`while` loop.
    fn parse_do_while(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `do`
        let body = Box::new(self.parse_statement());
        if !self.eat_keyword("while") {
            self.error_here("expected `while` after the `do` body".to_owned());
        }
        self.expect(Token::LParen, "`(` after `while`");
        let cond = self.parse_expr();
        self.expect(Token::RParen, "`)` after the `while` condition");
        self.expect(Token::Semicolon, "`;` after `do`/`while`");
        let span = start..self.prev_end();
        Stmt::DoWhile { body, cond, span }
    }

    /// Parse a `for` loop.
    fn parse_for(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `for`
        self.expect(Token::LParen, "`(` after `for`");
        let init = self.parse_expr_list(Token::Semicolon);
        self.expect(Token::Semicolon, "`;` after the `for` initialiser");
        let cond = if self.at(Token::Semicolon) {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(Token::Semicolon, "`;` after the `for` condition");
        let incr = self.parse_expr_list(Token::RParen);
        self.expect(Token::RParen, "`)` to close the `for` clauses");
        let body = Box::new(self.parse_statement());
        let span = start..self.prev_end();
        Stmt::For {
            init,
            cond,
            incr,
            body,
            span,
        }
    }

    /// Parse a `return` statement, with an optional value.
    fn parse_return(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `return`
        let value = if self.at(Token::Semicolon) {
            None
        } else {
            Some(self.parse_expr())
        };
        self.expect(Token::Semicolon, "`;` after `return`");
        let span = start..self.prev_end();
        Stmt::Return { value, span }
    }

    /// Parse a `jump` statement: `jump label;`.
    fn parse_jump(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `jump`
        let label = self.expect_ident("a label name");
        self.expect(Token::Semicolon, "`;` after `jump`");
        let span = start..self.prev_end();
        Stmt::Jump { label, span }
    }

    /// Parse a state-change statement: `state name;` or `state default;`.
    fn parse_state_change(&mut self) -> Stmt {
        let start = self.cur_start();
        self.bump(); // `state`
        let target = if self.at_keyword("default") {
            let span = self.cur_span();
            self.bump();
            StateName::Default(span)
        } else {
            StateName::Named(self.expect_ident("a state name"))
        };
        self.expect(Token::Semicolon, "`;` after the state change");
        let span = start..self.prev_end();
        Stmt::StateChange { target, span }
    }

    /// Parse a local variable declaration: `type name;` or `type name = init;`.
    fn parse_local(&mut self) -> Stmt {
        let start = self.cur_start();
        let ty = self.parse_type_ref().unwrap_or_else(|| {
            let at = self.cur_start();
            TypeRef {
                kind: TypeName::Integer,
                span: at..at,
            }
        });
        let name = self.expect_ident("a variable name");
        let init = if self.eat(Token::Assign) {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(Token::Semicolon, "`;` after the declaration");
        let span = start..self.prev_end();
        Stmt::Local {
            ty,
            name,
            init,
            span,
        }
    }

    /// Parse an expression used as a statement: `expr;`.
    fn parse_expr_statement(&mut self) -> Stmt {
        let start = self.cur_start();
        let expr = self.parse_expr();
        self.expect(Token::Semicolon, "`;` after the expression");
        let span = start..self.prev_end();
        Stmt::Expr { expr, span }
    }

    /// If the current token is an identifier keyword `keyword`, consume it and
    /// return `true`.
    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.at_keyword(keyword) {
            self.bump();
            true
        } else {
            false
        }
    }

    // -- expressions -------------------------------------------------------

    /// Parse a full expression (assignment level and below).
    fn parse_expr(&mut self) -> Expr {
        self.parse_expr_bp(0, false)
    }

    /// Precedence-climbing expression parse.
    ///
    /// `min_bp` is the minimum left binding power an infix operator must have to
    /// be taken here. `in_angle` suppresses the `<`/`>`-spelled operators so a
    /// vector/rotation constructor's closing `>` wins over a comparison.
    fn parse_expr_bp(&mut self, min_bp: u8, in_angle: bool) -> Expr {
        let mut lhs = self.parse_prefix(in_angle);
        while let Some(tok) = self.peek_kind() {
            if in_angle && is_angle_conflict(tok) {
                break;
            }
            let Some((lbp, rbp, op)) = infix_binding_power(tok) else {
                break;
            };
            if lbp < min_bp {
                break;
            }
            self.bump(); // operator
            let rhs = self.parse_expr_bp(rbp, in_angle);
            let span = lhs.span().start..rhs.span().end;
            lhs = match op {
                InfixOp::Binary(op) => Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                },
                InfixOp::Assign(op) => Expr::Assign {
                    op,
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                    span,
                },
            };
        }
        lhs
    }

    /// Parse a prefix-unary chain, a cast, or a primary followed by any postfix
    /// `++`/`--`.
    fn parse_prefix(&mut self, in_angle: bool) -> Expr {
        let start = self.cur_start();
        if let Some(op) = self.peek_kind().and_then(prefix_op) {
            self.bump();
            let operand = self.parse_prefix(in_angle);
            let span = start..operand.span().end;
            return Expr::Prefix {
                op,
                operand: Box::new(operand),
                span,
            };
        }
        let primary = self.parse_primary(in_angle);
        self.parse_postfix(primary)
    }

    /// Wrap `expr` in any trailing postfix `++`/`--` operators.
    fn parse_postfix(&mut self, expr: Expr) -> Expr {
        let mut expr = expr;
        loop {
            let op = match self.peek_kind() {
                Some(Token::PlusPlus) => PostfixOp::PostInc,
                Some(Token::MinusMinus) => PostfixOp::PostDec,
                _ => break,
            };
            let end = self.cur_end();
            self.bump();
            let span = expr.span().start..end;
            expr = Expr::Postfix {
                op,
                operand: Box::new(expr),
                span,
            };
        }
        expr
    }

    /// Parse a primary expression: a literal, a name (variable / call /
    /// member), a parenthesised expression or cast, a vector/rotation
    /// constructor, or a list.
    fn parse_primary(&mut self, in_angle: bool) -> Expr {
        let start = self.cur_start();
        match self.peek_kind() {
            Some(Token::IntegerLiteral) => {
                let raw = self.cur_text().to_owned();
                let span = self.cur_span();
                self.bump();
                Expr::Integer { raw, span }
            }
            Some(Token::FloatLiteral) => {
                let raw = self.cur_text().to_owned();
                let span = self.cur_span();
                self.bump();
                Expr::Float { raw, span }
            }
            Some(Token::StringLiteral) => {
                let raw = self.cur_text().to_owned();
                let span = self.cur_span();
                self.bump();
                Expr::Str { raw, span }
            }
            Some(Token::LParen) => self.parse_paren_or_cast(in_angle),
            Some(Token::Less) => self.parse_vector_or_rotation(),
            Some(Token::LBracket) => self.parse_list(),
            Some(Token::Identifier) => {
                let word = self.cur_text();
                if is_reserved_word(word) {
                    self.error_here(format!("expected an expression, found keyword `{word}`"));
                    Expr::Error(start..start)
                } else {
                    self.parse_name_expr()
                }
            }
            other => {
                let stop = other
                    .is_none_or(|tok| is_expr_stop(tok) || (in_angle && is_angle_conflict(tok)));
                if stop {
                    self.error_here("expected an expression".to_owned());
                    Expr::Error(start..start)
                } else {
                    // Consume one stray token so the parser always progresses.
                    let span = self.cur_span();
                    self.error_at(span.clone(), "expected an expression".to_owned());
                    self.bump();
                    Expr::Error(span)
                }
            }
        }
    }

    /// Parse a name used as an expression: a variable, a call `name(args)` or a
    /// member access `name.component`.
    fn parse_name_expr(&mut self) -> Expr {
        let name = self.make_ident();
        match self.peek_kind() {
            Some(Token::LParen) => {
                self.bump(); // `(`
                let args = self.parse_arg_list();
                let end = self.expect_close(Token::RParen, "`)` to close the call");
                let span = name.span.start..end;
                Expr::Call {
                    callee: name,
                    args,
                    span,
                }
            }
            Some(Token::Dot) => {
                self.bump(); // `.`
                let component = self.expect_ident("a component name");
                let span = name.span.start..component.span.end;
                Expr::Member {
                    base: name,
                    component,
                    span,
                }
            }
            _ => Expr::Variable(name),
        }
    }

    /// Parse a comma-separated argument list up to (but not consuming) the
    /// closing `)`.
    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        if !self.at(Token::RParen) && self.peek_kind().is_some() {
            loop {
                let before = self.pos;
                args.push(self.parse_expr_bp(0, false));
                if !self.eat(Token::Comma) {
                    break;
                }
                if self.pos == before {
                    break;
                }
            }
        }
        args
    }

    /// Parse a parenthesised expression, or a `(type)operand` cast if the
    /// parentheses wrap a bare type keyword.
    fn parse_paren_or_cast(&mut self, in_angle: bool) -> Expr {
        let start = self.cur_start();
        self.bump(); // `(`
        if self.at(Token::Identifier)
            && self.peek_kind_at(1) == Some(Token::RParen)
            && let Some(kind) = TypeName::from_keyword(self.cur_text())
        {
            let ty_span = self.cur_span();
            self.bump(); // type keyword
            self.bump(); // `)`
            let operand = self.parse_prefix(in_angle);
            let span = start..operand.span().end;
            return Expr::Cast {
                ty: TypeRef {
                    kind,
                    span: ty_span,
                },
                operand: Box::new(operand),
                span,
            };
        }
        let inner = self.parse_expr_bp(0, false);
        let end = self.expect_close(Token::RParen, "`)` to close the group");
        Expr::Paren {
            inner: Box::new(inner),
            span: start..end,
        }
    }

    /// Parse a vector `<x, y, z>` or rotation `<x, y, z, s>` constructor. The
    /// leading `<` is at the cursor.
    fn parse_vector_or_rotation(&mut self) -> Expr {
        let start = self.cur_start();
        self.bump(); // `<`
        let x = self.parse_expr_bp(0, true);
        self.expect(Token::Comma, "`,` between vector components");
        let y = self.parse_expr_bp(0, true);
        self.expect(Token::Comma, "`,` between vector components");
        let z = self.parse_expr_bp(0, true);
        if self.eat(Token::Comma) {
            let s = self.parse_expr_bp(0, true);
            let end = self.expect_close(Token::Greater, "`>` to close the rotation");
            Expr::Rotation {
                x: Box::new(x),
                y: Box::new(y),
                z: Box::new(z),
                s: Box::new(s),
                span: start..end,
            }
        } else {
            let end = self.expect_close(Token::Greater, "`>` to close the vector");
            Expr::Vector {
                x: Box::new(x),
                y: Box::new(y),
                z: Box::new(z),
                span: start..end,
            }
        }
    }

    /// Parse a list constructor `[a, b, c]`. The leading `[` is at the cursor.
    fn parse_list(&mut self) -> Expr {
        let start = self.cur_start();
        self.bump(); // `[`
        let mut elements = Vec::new();
        if !self.at(Token::RBracket) && self.peek_kind().is_some() {
            loop {
                let before = self.pos;
                elements.push(self.parse_expr_bp(0, false));
                if !self.eat(Token::Comma) {
                    break;
                }
                if self.pos == before {
                    break;
                }
            }
        }
        let end = self.expect_close(Token::RBracket, "`]` to close the list");
        Expr::List {
            elements,
            span: start..end,
        }
    }

    /// Consume a closing delimiter `kind`, returning the end offset. If it is
    /// missing, record an error and return the previous token's end.
    fn expect_close(&mut self, kind: Token, what: &str) -> usize {
        if self.at(kind) {
            let end = self.cur_end();
            self.bump();
            end
        } else {
            self.error_here(format!("expected {what}"));
            self.prev_end()
        }
    }

    /// Parse a comma-separated expression list (a `for` clause) up to — but not
    /// consuming — `terminator` or the closing `)`. An empty clause yields an
    /// empty vector.
    fn parse_expr_list(&mut self, terminator: Token) -> Vec<Expr> {
        let mut list = Vec::new();
        if self.at(terminator) || self.at(Token::RParen) || self.peek_kind().is_none() {
            return list;
        }
        loop {
            let before = self.pos;
            list.push(self.parse_expr());
            if !self.eat(Token::Comma) {
                break;
            }
            if self.pos == before {
                break;
            }
        }
        list
    }
}

/// The prefix-unary operator a token denotes, if any.
const fn prefix_op(token: Token) -> Option<PrefixOp> {
    match token {
        Token::Minus => Some(PrefixOp::Neg),
        Token::Bang => Some(PrefixOp::Not),
        Token::Tilde => Some(PrefixOp::BitNot),
        Token::PlusPlus => Some(PrefixOp::PreInc),
        Token::MinusMinus => Some(PrefixOp::PreDec),
        _ => None,
    }
}

/// The `(left, right)` binding powers and operator of an infix token, or `None`
/// if the token is not an infix operator.
///
/// Higher numbers bind tighter. The right power is lower than the left for
/// left-associative operators and higher for the right-associative assignment
/// operators. The levels mirror Linden Lab's LSL grammar — note that `&&` and
/// `||` share one level.
const fn infix_binding_power(token: Token) -> Option<(u8, u8, InfixOp)> {
    let result = match token {
        Token::Assign => (2, 1, InfixOp::Assign(AssignOp::Assign)),
        Token::PlusAssign => (2, 1, InfixOp::Assign(AssignOp::AddAssign)),
        Token::MinusAssign => (2, 1, InfixOp::Assign(AssignOp::SubAssign)),
        Token::StarAssign => (2, 1, InfixOp::Assign(AssignOp::MulAssign)),
        Token::SlashAssign => (2, 1, InfixOp::Assign(AssignOp::DivAssign)),
        Token::PercentAssign => (2, 1, InfixOp::Assign(AssignOp::ModAssign)),
        Token::OrOr => (3, 4, InfixOp::Binary(BinaryOp::Or)),
        Token::AndAnd => (3, 4, InfixOp::Binary(BinaryOp::And)),
        Token::Pipe => (5, 6, InfixOp::Binary(BinaryOp::BitOr)),
        Token::Caret => (7, 8, InfixOp::Binary(BinaryOp::BitXor)),
        Token::Amp => (9, 10, InfixOp::Binary(BinaryOp::BitAnd)),
        Token::EqEq => (11, 12, InfixOp::Binary(BinaryOp::Eq)),
        Token::NotEq => (11, 12, InfixOp::Binary(BinaryOp::Ne)),
        Token::Less => (13, 14, InfixOp::Binary(BinaryOp::Lt)),
        Token::LessEq => (13, 14, InfixOp::Binary(BinaryOp::Le)),
        Token::Greater => (13, 14, InfixOp::Binary(BinaryOp::Gt)),
        Token::GreaterEq => (13, 14, InfixOp::Binary(BinaryOp::Ge)),
        Token::ShiftLeft => (15, 16, InfixOp::Binary(BinaryOp::Shl)),
        Token::ShiftRight => (15, 16, InfixOp::Binary(BinaryOp::Shr)),
        Token::Plus => (17, 18, InfixOp::Binary(BinaryOp::Add)),
        Token::Minus => (17, 18, InfixOp::Binary(BinaryOp::Sub)),
        Token::Star => (19, 20, InfixOp::Binary(BinaryOp::Mul)),
        Token::Slash => (19, 20, InfixOp::Binary(BinaryOp::Div)),
        Token::Percent => (19, 20, InfixOp::Binary(BinaryOp::Mod)),
        _ => return None,
    };
    Some(result)
}

/// Whether a token is spelled with `<` or `>` and therefore collides with the
/// angle brackets of a vector/rotation constructor.
const fn is_angle_conflict(token: Token) -> bool {
    matches!(
        token,
        Token::Less
            | Token::LessEq
            | Token::Greater
            | Token::GreaterEq
            | Token::ShiftLeft
            | Token::ShiftRight
    )
}

/// Whether a token terminates an expression (a delimiter the primary parser
/// must not consume so the enclosing construct can see it).
const fn is_expr_stop(token: Token) -> bool {
    matches!(
        token,
        Token::Semicolon | Token::RParen | Token::RBrace | Token::RBracket | Token::Comma
    )
}

/// Whether an identifier word is a reserved LSL keyword that cannot appear as a
/// value in operand position (a type keyword or a control keyword).
fn is_reserved_word(word: &str) -> bool {
    TypeName::from_keyword(word).is_some()
        || matches!(
            word,
            "default"
                | "state"
                | "event"
                | "jump"
                | "return"
                | "if"
                | "else"
                | "for"
                | "do"
                | "while"
        )
}
