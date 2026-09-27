//! Calling a library function: typed implementations behind an erased
//! dispatch.
//!
//! Nobody writes a function against `&[Value]`. An implementation is an
//! ordinary Rust function — `fn ll_abs<C>(_ctx: &mut C, value: i32) ->
//! Result<i32, CallError>` — and [`registry!`](crate::registry) registers it
//! under its [`BuiltinId`]. The registration goes through the generated
//! [`Signature`] of that id, which states the argument tuple and return type
//! the table gives the function, so an implementation at the wrong arity or
//! with a wrong parameter type is a **compile error**, not a run-time
//! surprise. The erased side — popping arguments off the VM's stack as those
//! types — is written once, here, over tuples.
//!
//! The context is the calling script's [`ScriptCtx`](crate::vm::ScriptCtx):
//! `call` takes one, and each implementation either names it (the functions
//! that touch the world or the script's control flow) or is generic over it
//! (the pure functions, which ignore it and are tested with `&mut ()`).

use sl_lsl::ast::TypeName;
use sl_types::lsl::{Rotation, Vector};

use crate::error::ValueError;
use crate::library::generated::BuiltinId;
use crate::value::{Element, Value};

/// A `key` argument or return value: text, typed apart from `string` so a
/// signature says which one the reference means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key(pub String);

/// Why a library call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// The function has neither an implementation nor a stub.
    #[error("{0:?} is not implemented")]
    Missing(BuiltinId),
    /// The call passed the wrong number of arguments — a lowering bug, since
    /// the compiler checks arity against the same table.
    #[error("{id:?} takes {expected} arguments, got {found}")]
    Arity {
        /// The function called.
        id: BuiltinId,
        /// The arity the table states.
        expected: usize,
        /// The arity passed.
        found: usize,
    },
    /// An argument had the wrong type — a lowering bug, since the lowering
    /// makes every implicit conversion explicit.
    #[error("argument {index} of {id:?} must be {expected:?}, got {found:?}")]
    Argument {
        /// The function called.
        id: BuiltinId,
        /// The zero-based argument position.
        index: usize,
        /// The type the table states.
        expected: TypeName,
        /// The type passed.
        found: TypeName,
    },
    /// The function hit a run-time error in the value model (a `Math
    /// Error`).
    #[error(transparent)]
    Value(#[from] ValueError),
}

/// A Rust type that stands for one LSL argument type.
pub trait FromValue: Sized {
    /// The LSL type it stands for.
    const TYPE: TypeName;

    /// The value as this type, or [`None`] when it has another LSL type.
    fn from_value(value: Value) -> Option<Self>;
}

/// A Rust type an implementation may return: one LSL type, or `()` for a
/// function that returns nothing.
pub trait IntoReturn {
    /// The LSL return type, [`None`] for `()`.
    const TYPE: Option<TypeName>;

    /// The returned value, [`None`] for `()`.
    fn into_return(self) -> Option<Value>;
}

/// Implements [`FromValue`] and [`IntoReturn`] for the Rust type standing
/// for one LSL type.
macro_rules! lsl_type {
    ($rust:ty, $lsl:ident, $value:ident($binding:ident) => $from:expr, $to:expr) => {
        impl FromValue for $rust {
            const TYPE: TypeName = TypeName::$lsl;

            fn from_value(value: Value) -> Option<Self> {
                match value {
                    Value::$value($binding) => Some($from),
                    _ => None,
                }
            }
        }

        impl IntoReturn for $rust {
            const TYPE: Option<TypeName> = Some(TypeName::$lsl);

            fn into_return(self) -> Option<Value> {
                let $binding = self;
                Some(Value::$value($to))
            }
        }
    };
}

lsl_type!(i32, Integer, Integer(value) => value, value);
lsl_type!(f32, Float, Float(value) => value, value);
lsl_type!(String, String, String(text) => text, text);
lsl_type!(Key, Key, Key(text) => Key(text), text.0);
lsl_type!(Vector, Vector, Vector(vector) => vector, vector);
lsl_type!(Rotation, Rotation, Rotation(rotation) => rotation, rotation);
lsl_type!(Vec<Element>, List, List(elements) => elements, elements);

impl IntoReturn for () {
    const TYPE: Option<TypeName> = None;

    fn into_return(self) -> Option<Value> {
        None
    }
}

/// The next argument as `T`, or the error naming its position.
fn next_argument<T: FromValue>(
    id: BuiltinId,
    index: usize,
    args: &mut impl Iterator<Item = Value>,
) -> Result<T, CallError> {
    let value = args.next().ok_or_else(|| CallError::Arity {
        id,
        expected: index.saturating_add(1),
        found: index,
    })?;
    let found = value.type_name();
    T::from_value(value).ok_or(CallError::Argument {
        id,
        index,
        expected: T::TYPE,
        found,
    })
}

/// A tuple of argument types, taken off an argument list in order.
pub trait FromArgs: Sized {
    /// The arguments as this tuple.
    ///
    /// # Errors
    ///
    /// [`CallError::Arity`] or [`CallError::Argument`] when `args` does not
    /// match.
    fn from_args(id: BuiltinId, args: Vec<Value>) -> Result<Self, CallError>;
}

/// A typed implementation callable with the argument tuple `Args`.
pub trait Handler<C, Args, R> {
    /// Calls the implementation.
    ///
    /// # Errors
    ///
    /// Whatever the implementation returns.
    fn call(self, ctx: &mut C, args: Args) -> Result<R, CallError>;
}

/// Implements [`FromArgs`] and [`Handler`] for one arity.
macro_rules! arity {
    ($count:expr; $($index:tt $arg:ident),*) => {
        impl<$($arg: FromValue),*> FromArgs for ($($arg,)*) {
            #[allow(
                unused_mut,
                unused_variables,
                clippy::allow_attributes,
                reason = "the zero-arity instance reads nothing"
            )]
            fn from_args(id: BuiltinId, args: Vec<Value>) -> Result<Self, CallError> {
                let found = args.len();
                if found != $count {
                    return Err(CallError::Arity {
                        id,
                        expected: $count,
                        found,
                    });
                }
                let mut values = args.into_iter();
                Ok(($(next_argument::<$arg>(id, $index, &mut values)?,)*))
            }
        }

        impl<C, R, F, $($arg),*> Handler<C, ($($arg,)*), R> for F
        where
            F: FnOnce(&mut C $(, $arg)*) -> Result<R, CallError>,
        {
            #[allow(
                non_snake_case,
                clippy::allow_attributes,
                reason = "the tuple fields are named after their type parameters"
            )]
            fn call(self, ctx: &mut C, ($($arg,)*): ($($arg,)*)) -> Result<R, CallError> {
                self(ctx $(, $arg)*)
            }
        }
    };
}

arity!(0;);
arity!(1; 0 A0);
arity!(2; 0 A0, 1 A1);
arity!(3; 0 A0, 1 A1, 2 A2);
arity!(4; 0 A0, 1 A1, 2 A2, 3 A3);
arity!(5; 0 A0, 1 A1, 2 A2, 3 A3, 4 A4);
arity!(6; 0 A0, 1 A1, 2 A2, 3 A3, 4 A4, 5 A5);
arity!(7; 0 A0, 1 A1, 2 A2, 3 A3, 4 A4, 5 A5, 6 A6);
arity!(8; 0 A0, 1 A1, 2 A2, 3 A3, 4 A4, 5 A5, 6 A6, 7 A7);
arity!(9; 0 A0, 1 A1, 2 A2, 3 A3, 4 A4, 5 A5, 6 A6, 7 A7, 8 A8);

/// The argument and return types the table states for one function — one
/// generated implementation per [`BuiltinId`], in `generated::signatures`.
pub trait Signature {
    /// The argument tuple.
    type Args: FromArgs;
    /// The return type (`()` for none).
    type Ret: IntoReturn;
    /// The function.
    const ID: BuiltinId;
}

/// What a call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Called {
    /// The returned value; [`None`] for a function that returns nothing.
    pub value: Option<Value>,
    /// Whether a stub answered: the value is the type's default, and the
    /// caller should say so once rather than pretend the call happened.
    pub stubbed: bool,
}

/// Calls `handler` as the function `S`, taking its arguments off `args` as
/// the types `S` states. This is where a registered implementation is held
/// to the table: `handler` must accept exactly `S::Args` and return
/// `S::Ret`.
///
/// # Errors
///
/// A [`CallError`] from the argument conversion or the implementation.
pub fn invoke<S, C, F>(ctx: &mut C, args: Vec<Value>, handler: F) -> Result<Called, CallError>
where
    S: Signature,
    F: Handler<C, S::Args, S::Ret>,
{
    let args = S::Args::from_args(S::ID, args)?;
    let value = handler.call(ctx, args)?.into_return();
    Ok(Called {
        value,
        stubbed: false,
    })
}

/// Answers a call to a function declared a stub: the arguments are still
/// checked against the table, and the return value is the type's default.
///
/// # Errors
///
/// [`CallError::Arity`] or [`CallError::Argument`] when `args` does not
/// match the table.
pub fn stub(id: BuiltinId, args: &[Value]) -> Result<Called, CallError> {
    let descriptor = id.descriptor();
    if args.len() != descriptor.args.len() {
        return Err(CallError::Arity {
            id,
            expected: descriptor.args.len(),
            found: args.len(),
        });
    }
    for (index, (value, parameter)) in args.iter().zip(descriptor.args).enumerate() {
        if value.type_name() != parameter.ty {
            return Err(CallError::Argument {
                id,
                index,
                expected: parameter.ty,
                found: value.type_name(),
            });
        }
    }
    Ok(Called {
        value: descriptor.ret.map(Value::default_of),
        stubbed: true,
    })
}

/// How far the library covers one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    /// Not registered: calling it is [`CallError::Missing`].
    Missing,
    /// Registered as a stub: it answers with the type's default.
    Stubbed,
    /// Registered with an implementation.
    Implemented,
}

/// Declares a library registry: which functions are implemented (and by
/// which Rust function) and which are deliberate stubs. It expands to
/// `call`, which dispatches a [`BuiltinId`] to its implementation through
/// the generated [`Signature`] (so a mismatched implementation does not
/// compile), and `status`, which the coverage test reads.
#[macro_export]
macro_rules! registry {
    (
        implemented { $($id:ident => $function:path),* $(,)? }
        stubbed { $($stub:ident),* $(,)? }
    ) => {
        /// Calls the library function `id` with `args`.
        ///
        /// # Errors
        ///
        /// [`CallError::Missing`]($crate::library::CallError::Missing) for
        /// a function neither implemented nor stubbed, or whatever the
        /// argument conversion or the implementation reports.
        pub fn call(
            id: $crate::library::BuiltinId,
            ctx: &mut $crate::vm::ScriptCtx<'_>,
            args: ::std::vec::Vec<$crate::value::Value>,
        ) -> ::core::result::Result<$crate::library::Called, $crate::library::CallError> {
            match id {
                $(
                    $crate::library::BuiltinId::$id => $crate::library::invoke::<
                        $crate::library::signatures::$id,
                        $crate::vm::ScriptCtx<'_>,
                        _,
                    >(ctx, args, $function),
                )*
                $(
                    $crate::library::BuiltinId::$stub => $crate::library::stub(id, &args),
                )*
                #[allow(
                    unreachable_patterns,
                    clippy::allow_attributes,
                    reason = "once every function is registered nothing is missing"
                )]
                _ => ::core::result::Result::Err($crate::library::CallError::Missing(id)),
            }
        }

        /// Whether `id` is implemented, stubbed or missing.
        #[must_use]
        pub const fn status(id: $crate::library::BuiltinId) -> $crate::library::Status {
            match id {
                $($crate::library::BuiltinId::$id => $crate::library::Status::Implemented,)*
                $($crate::library::BuiltinId::$stub => $crate::library::Status::Stubbed,)*
                #[allow(
                    unreachable_patterns,
                    clippy::allow_attributes,
                    reason = "once every function is registered nothing is missing"
                )]
                _ => $crate::library::Status::Missing,
            }
        }
    };
}
