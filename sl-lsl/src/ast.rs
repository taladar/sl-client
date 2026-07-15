//! The **abstract syntax tree** the [`crate::parser`] builds from the token
//! stream.
//!
//! The tree is **owned** (nodes carry `String` names and raw literal text, not
//! borrows of the source) so a consumer — the semantic pass, the language
//! server — can hold and index it without pinning the source buffer. Every node
//! carries a byte [`span`](Ident::span)-shaped `Range<usize>` into the original
//! source, which is what go-to-definition, find-references, folding, an outline
//! and brace matching all point at.
//!
//! The tree is **not** validated: the parser recognises LSL *syntax* and leaves
//! *meaning* to the semantic pass. In particular no identifier is resolved
//! here — `integer`, `default`, `llSay`, a user global and `TRUE` are all just
//! words the parser classifies structurally (a type keyword, a state header, a
//! call, a variable) without a symbol table. Whether a called name exists, an
//! assignment targets a real lvalue or a state is reachable is a later concern.
//!
//! Because the parser is **error-tolerant** (an editor re-parses broken code on
//! every keystroke), the tree can contain [`Expr::Error`] and [`Stmt::Error`]
//! placeholder nodes where the input did not parse; the surrounding tree still
//! stands so a half-typed statement does not discard the rest of the file.

use core::ops::Range;

/// One of LSL's seven value types, named by a type keyword (`integer`, `float`,
/// `string`, `key`, `vector`, `rotation` — with `quaternion` as a legacy
/// synonym for `rotation` — and `list`).
///
/// The lexer emits every type keyword as a plain [`crate::Token::Identifier`];
/// the parser classifies it here by its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeName {
    /// `integer` — a 32-bit signed integer.
    Integer,
    /// `float` — a 32-bit IEEE float.
    Float,
    /// `string` — a text string.
    String,
    /// `key` — a UUID reference.
    Key,
    /// `vector` — three floats `<x, y, z>`.
    Vector,
    /// `rotation` (a.k.a. the legacy `quaternion`) — four floats
    /// `<x, y, z, s>`.
    Rotation,
    /// `list` — a heterogeneous list.
    List,
}

impl TypeName {
    /// The [`TypeName`] a type keyword denotes, or `None` if the word is not a
    /// type keyword. `quaternion` maps to [`TypeName::Rotation`], its modern
    /// spelling.
    #[must_use]
    pub fn from_keyword(word: &str) -> Option<Self> {
        match word {
            "integer" => Some(Self::Integer),
            "float" => Some(Self::Float),
            "string" => Some(Self::String),
            "key" => Some(Self::Key),
            "vector" => Some(Self::Vector),
            "rotation" | "quaternion" => Some(Self::Rotation),
            "list" => Some(Self::List),
            _ => None,
        }
    }

    /// The canonical type keyword this [`TypeName`] denotes — the inverse of
    /// [`from_keyword`](Self::from_keyword). [`Rotation`](Self::Rotation) renders
    /// as its modern spelling `rotation`, never the legacy `quaternion`.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::String => "string",
            Self::Key => "key",
            Self::Vector => "vector",
            Self::Rotation => "rotation",
            Self::List => "list",
        }
    }
}

/// A use of a type keyword, with the byte span of the keyword itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeRef {
    /// Which of the seven types the keyword names.
    pub kind: TypeName,
    /// The byte range of the type keyword in the source.
    pub span: Range<usize>,
}

/// An identifier — a variable, function, event, state, parameter, label or
/// member name — with its byte span in the source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ident {
    /// The identifier text (owned).
    pub name: String,
    /// The byte range of the identifier in the source.
    pub span: Range<usize>,
}

/// A whole parsed LSL script: the global declarations, then the states.
///
/// LSL requires globals before states and `default` first among the states;
/// the error-tolerant parser does not *enforce* that order, so a malformed
/// script may carry items in an order the grid would reject — the semantic
/// pass, not the tree, speaks to legality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    /// The global variables and functions, in source order.
    pub globals: Vec<GlobalItem>,
    /// The states (`default` and any named `state` blocks), in source order.
    pub states: Vec<StateDef>,
    /// The byte range spanning the whole script.
    pub span: Range<usize>,
}

/// A top-level global declaration: either a variable or a function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobalItem {
    /// A global variable declaration.
    Variable(GlobalVar),
    /// A global function definition.
    Function(FunctionDef),
}

/// A global variable declaration: `type name;` or `type name = init;`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalVar {
    /// The declared type.
    pub ty: TypeRef,
    /// The variable name.
    pub name: Ident,
    /// The initialiser expression, if any. LSL requires a *constant* global
    /// initialiser; that constraint is a semantic-pass concern, not enforced
    /// here.
    pub init: Option<Expr>,
    /// The byte range of the whole declaration.
    pub span: Range<usize>,
}

/// A global function definition: `[return-type] name(params) { body }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// The return type, or `None` for a `void` function (no leading type).
    pub ret: Option<TypeRef>,
    /// The function name.
    pub name: Ident,
    /// The typed parameters, in order.
    pub params: Vec<Param>,
    /// The function body.
    pub body: Block,
    /// The byte range of the whole definition.
    pub span: Range<usize>,
}

/// A single typed parameter of a function or event handler: `type name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The parameter type.
    pub ty: TypeRef,
    /// The parameter name.
    pub name: Ident,
    /// The byte range of the parameter.
    pub span: Range<usize>,
}

/// A state block: `default { events }` or `state name { events }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDef {
    /// The state's name (`default` or a user name).
    pub name: StateName,
    /// The event handlers defined in the state, in source order.
    pub events: Vec<EventHandler>,
    /// The byte range of the whole state block.
    pub span: Range<usize>,
}

/// The name of a state: the reserved `default` state or a user-named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateName {
    /// The reserved `default` state, with the byte span of the `default`
    /// keyword.
    Default(Range<usize>),
    /// A user-defined state, named after the `state` keyword.
    Named(Ident),
}

/// An event handler inside a state: `name(params) { body }`.
///
/// Structurally identical to a `void` function; the parser does not check the
/// name against the grid's event table (a semantic-pass concern).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventHandler {
    /// The event name.
    pub name: Ident,
    /// The typed parameters, in order.
    pub params: Vec<Param>,
    /// The handler body.
    pub body: Block,
    /// The byte range of the whole handler.
    pub span: Range<usize>,
}

/// A brace-delimited block of statements: `{ ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The statements in the block, in source order.
    pub statements: Vec<Stmt>,
    /// The byte range of the block including its braces.
    pub span: Range<usize>,
}

/// A statement inside a function, event handler or nested block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// An empty statement: a bare `;`.
    Empty(Range<usize>),
    /// A nested block: `{ ... }`.
    Block(Block),
    /// A local variable declaration: `type name;` or `type name = init;`.
    Local {
        /// The declared type.
        ty: TypeRef,
        /// The variable name.
        name: Ident,
        /// The initialiser expression, if any.
        init: Option<Expr>,
        /// The byte range of the whole declaration (excluding the `;`).
        span: Range<usize>,
    },
    /// An expression used as a statement: `expr;`.
    Expr {
        /// The expression evaluated for its effect.
        expr: Expr,
        /// The byte range of the statement.
        span: Range<usize>,
    },
    /// An `if` statement, with an optional `else` branch.
    If {
        /// The condition expression.
        cond: Expr,
        /// The `then` branch.
        then_branch: Box<Self>,
        /// The `else` branch, if present.
        else_branch: Option<Box<Self>>,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A `while` loop: `while (cond) body`.
    While {
        /// The loop condition.
        cond: Expr,
        /// The loop body.
        body: Box<Self>,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A `do`/`while` loop: `do body while (cond);`.
    DoWhile {
        /// The loop body.
        body: Box<Self>,
        /// The loop condition.
        cond: Expr,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A `for` loop: `for (init; cond; incr) body`.
    For {
        /// The comma-separated initialiser expressions (may be empty).
        init: Vec<Expr>,
        /// The loop condition, if any (an empty middle clause is `None`).
        cond: Option<Expr>,
        /// The comma-separated increment expressions (may be empty).
        incr: Vec<Expr>,
        /// The loop body.
        body: Box<Self>,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A `return` statement, with an optional value.
    Return {
        /// The returned expression, if any.
        value: Option<Expr>,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A `jump` to a label: `jump label;`.
    Jump {
        /// The target label name.
        label: Ident,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A jump-label definition: `@label;`.
    Label {
        /// The label name.
        name: Ident,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A state change: `state name;` or `state default;`.
    StateChange {
        /// The target state.
        target: StateName,
        /// The byte range of the whole statement.
        span: Range<usize>,
    },
    /// A placeholder for input that did not parse as a statement, so the rest
    /// of the block still parses.
    Error(Range<usize>),
}

impl Stmt {
    /// The byte range this statement spans in the source.
    #[must_use]
    pub const fn span(&self) -> Range<usize> {
        match self {
            Self::Block(block) => block.span.start..block.span.end,
            Self::Empty(span)
            | Self::Error(span)
            | Self::Local { span, .. }
            | Self::Expr { span, .. }
            | Self::If { span, .. }
            | Self::While { span, .. }
            | Self::DoWhile { span, .. }
            | Self::For { span, .. }
            | Self::Return { span, .. }
            | Self::Jump { span, .. }
            | Self::Label { span, .. }
            | Self::StateChange { span, .. } => span.start..span.end,
        }
    }
}

/// A prefix (leading) unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefixOp {
    /// `-` — arithmetic negation.
    Neg,
    /// `!` — logical not.
    Not,
    /// `~` — bitwise complement.
    BitNot,
    /// `++` — pre-increment.
    PreInc,
    /// `--` — pre-decrement.
    PreDec,
}

/// A postfix (trailing) unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PostfixOp {
    /// `++` — post-increment.
    PostInc,
    /// `--` — post-decrement.
    PostDec,
}

/// A binary operator (everything that combines two operands except assignment,
/// which is [`AssignOp`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `&&` — logical and. In LSL `&&` and `||` share one precedence level and
    /// associate left-to-right (a well-known departure from C).
    And,
    /// `||` — logical or. See [`BinaryOp::And`] for the shared-precedence note.
    Or,
}

/// An assignment operator (`=` and the compound forms), the loosest-binding,
/// right-associative operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssignOp {
    /// `=`
    Assign,
    /// `+=`
    AddAssign,
    /// `-=`
    SubAssign,
    /// `*=`
    MulAssign,
    /// `/=`
    DivAssign,
    /// `%=`
    ModAssign,
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// An integer literal, keeping its raw text (decimal or `0x` hex,
    /// unparsed).
    Integer {
        /// The literal's raw source text.
        raw: String,
        /// The byte range of the literal.
        span: Range<usize>,
    },
    /// A float literal, keeping its raw text (unparsed).
    Float {
        /// The literal's raw source text.
        raw: String,
        /// The byte range of the literal.
        span: Range<usize>,
    },
    /// A string literal, keeping its raw text *including the quotes and any
    /// escape sequences* — unescaping is left to the consumer.
    Str {
        /// The literal's raw source text, quotes included.
        raw: String,
        /// The byte range of the literal.
        span: Range<usize>,
    },
    /// A variable reference (a bare identifier).
    Variable(Ident),
    /// Component access on a vector or rotation: `base.component`.
    Member {
        /// The accessed variable.
        base: Ident,
        /// The component name (`x`, `y`, `z` or `s`).
        component: Ident,
        /// The byte range of the whole access.
        span: Range<usize>,
    },
    /// A function call: `callee(args)`.
    Call {
        /// The called name.
        callee: Ident,
        /// The argument expressions, in order.
        args: Vec<Self>,
        /// The byte range of the whole call.
        span: Range<usize>,
    },
    /// A list constructor: `[a, b, c]`.
    List {
        /// The element expressions, in order.
        elements: Vec<Self>,
        /// The byte range including the brackets.
        span: Range<usize>,
    },
    /// A vector constructor: `<x, y, z>`.
    Vector {
        /// The `x` component.
        x: Box<Self>,
        /// The `y` component.
        y: Box<Self>,
        /// The `z` component.
        z: Box<Self>,
        /// The byte range including the angle brackets.
        span: Range<usize>,
    },
    /// A rotation constructor: `<x, y, z, s>`.
    Rotation {
        /// The `x` component.
        x: Box<Self>,
        /// The `y` component.
        y: Box<Self>,
        /// The `z` component.
        z: Box<Self>,
        /// The `s` (scalar) component.
        s: Box<Self>,
        /// The byte range including the angle brackets.
        span: Range<usize>,
    },
    /// A prefix unary operation: `op operand`.
    Prefix {
        /// The operator.
        op: PrefixOp,
        /// The operand.
        operand: Box<Self>,
        /// The byte range of the whole expression.
        span: Range<usize>,
    },
    /// A postfix unary operation: `operand op`.
    Postfix {
        /// The operator.
        op: PostfixOp,
        /// The operand.
        operand: Box<Self>,
        /// The byte range of the whole expression.
        span: Range<usize>,
    },
    /// A binary operation: `lhs op rhs`.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        lhs: Box<Self>,
        /// The right operand.
        rhs: Box<Self>,
        /// The byte range of the whole expression.
        span: Range<usize>,
    },
    /// An assignment: `target op value`.
    Assign {
        /// The assignment operator.
        op: AssignOp,
        /// The assignment target (an lvalue in valid LSL; not checked here).
        target: Box<Self>,
        /// The assigned value.
        value: Box<Self>,
        /// The byte range of the whole expression.
        span: Range<usize>,
    },
    /// A typecast: `(type)operand`.
    Cast {
        /// The target type.
        ty: TypeRef,
        /// The cast operand.
        operand: Box<Self>,
        /// The byte range of the whole expression.
        span: Range<usize>,
    },
    /// A parenthesised expression: `(inner)`. Kept as its own node so the tree
    /// reproduces the source faithfully (useful for formatting and precisely
    /// spanned diagnostics).
    Paren {
        /// The inner expression.
        inner: Box<Self>,
        /// The byte range including the parentheses.
        span: Range<usize>,
    },
    /// A placeholder for input that did not parse as an expression.
    Error(Range<usize>),
}

impl Expr {
    /// The byte range this expression spans in the source.
    #[must_use]
    pub const fn span(&self) -> Range<usize> {
        match self {
            Self::Variable(ident) => ident.span.start..ident.span.end,
            Self::Error(span)
            | Self::Integer { span, .. }
            | Self::Float { span, .. }
            | Self::Str { span, .. }
            | Self::Member { span, .. }
            | Self::Call { span, .. }
            | Self::List { span, .. }
            | Self::Vector { span, .. }
            | Self::Rotation { span, .. }
            | Self::Prefix { span, .. }
            | Self::Postfix { span, .. }
            | Self::Binary { span, .. }
            | Self::Assign { span, .. }
            | Self::Cast { span, .. }
            | Self::Paren { span, .. } => span.start..span.end,
        }
    }
}
