//! OpenSim's lists as the shared objects they are there.
//!
//! On Second Life a list is a value: assigning one, passing it or returning
//! it copies it, and nothing a script can do to one list shows in another.
//! On OpenSim an LSL list is a .NET object (`LSL_Types.list`, a class) that
//! both engines hand on by reference — an assignment, an argument, a return
//! and a `(list)` cast all hand on the same object — and the object can
//! change in place (`vm/list-identity`, both engines, 2026-11-01):
//!
//! - `osListSortInPlace` sorts the array the object holds;
//! - `llSetPayPrice` pads a short button list, `llDialog` adds `"Ok"` to an
//!   empty one, and `llJsonSetValue`, and for JSON that is no array or object
//!   `llJsonGetValue` and `llJsonValueType`, add a `0` to empty specifiers
//!   (`LSL_Types.list.Add`, which gives the object a longer array);
//! - XEngine compiles `list + string`, `list + integer`, `list + float` and
//!   `list + key` to an operator that appends to its left operand and hands
//!   it back (`LSL_Types.list.ExtendAndAdd`), where YEngine builds a new list;
//! - `llList2List` hands back the list it was given when both indices are
//!   still negative after the length is added and the start is past the end
//!   (`GetSublist`'s `return this`), `llListInsertList` the list inserted
//!   into an empty list at an index of 0 or less, and `llListReplaceList` the
//!   list put in from 0 over the whole of a list.
//!
//! So a change shows through every variable holding the object. The value
//! model copies, so the VM keeps an identity beside each list it holds on
//! OpenSim — an [`Operand::Shared`] on the operand stack, an entry of a
//! frame's or the instance's object slots beside a local or a global — and a
//! change in place is written to everything holding that identity
//! ([`rewrite`]). A list nothing else holds — a literal, a library result, a
//! concatenation — carries no identity until something could hold it twice:
//! loading it from a variable or duplicating it on the stack mints one.
//!
//! No API builds a second list object over another's array, so one identity
//! per object serves both kinds of change. A change in place is not a store:
//! YEngine charges a list to each variable when it is stored there
//! (`HeapTrackerList.Save`), and is not told of a change in place, so
//! neither is the memory accounting here.

use sl_lsl::ast::{BinaryOp, TypeName};

use crate::operand::Operand;
use crate::value::{Element, Value};
use crate::vm::flavour::EngineFlavour;

/// The identity of one of OpenSim's list objects, unique within the instance
/// that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListId(pub(crate) u64);

/// An instance's list objects: whether its lists are objects at all, the
/// identity to mint next, and which list each global holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListObjects {
    /// Whether a list is a shared object in the flavour — OpenSim's, both
    /// engines — rather than a value.
    shared: bool,
    /// The identity [`Self::mint`] hands out next.
    pub(crate) next: u64,
    /// The object each global holds, for a list global that has been loaded
    /// or stored; [`None`] for every other global.
    pub(crate) globals: Vec<Option<ListId>>,
}

impl ListObjects {
    /// No objects yet, `globals` globals holding none, lists shared as
    /// `flavour` shares them.
    pub(crate) fn new(flavour: EngineFlavour, globals: usize) -> Self {
        Self {
            shared: flavour.is_opensim(),
            next: 0,
            globals: vec![None; globals],
        }
    }

    /// A new identity, or [`None`] where lists are values.
    pub(crate) const fn mint(&mut self) -> Option<ListId> {
        if !self.shared {
            return None;
        }
        let id = ListId(self.next);
        self.next = self.next.wrapping_add(1);
        Some(id)
    }

    /// The object the variable holding `value` holds, minting one for a list
    /// variable that holds none yet: what a load hands on.
    pub(crate) const fn of_slot(
        &mut self,
        slot: &mut Option<ListId>,
        value: &Value,
    ) -> Option<ListId> {
        if slot.is_none() && matches!(value, Value::List(_)) {
            *slot = self.mint();
        }
        *slot
    }

    /// Give a plain list on the stack an identity, as it is about to be held
    /// twice.
    pub(crate) fn share(&mut self, operand: &mut Operand) {
        if let Operand::Value(Value::List(elements)) = operand
            && let Some(id) = self.mint()
        {
            *operand = Operand::Shared(core::mem::take(elements), id);
        }
    }
}

/// `value` as the operand that hands on `object`: a list with an identity
/// shared, anything else as it is.
pub(crate) fn operand(value: Value, object: Option<ListId>) -> Operand {
    match (value, object) {
        (Value::List(elements), Some(id)) => Operand::Shared(elements, id),
        (value, _) => Operand::Value(value),
    }
}

/// `operand` as a value and the object it hands on, a double rounded: what a
/// store, an argument and a return take.
pub(crate) fn split(operand: Operand) -> (Value, Option<ListId>) {
    match operand {
        Operand::Shared(elements, id) => (Value::List(elements), Some(id)),
        other => (other.round(), None),
    }
}

/// The object `left op right` changes in place and hands back, if it does:
/// XEngine's `list + string`, `+ integer`, `+ float` and `+ key` (the key
/// appended as a string, as the value model already appends it there) of a
/// list that has an identity. Of a list nothing else holds the change is
/// unobservable, and the result is an ordinary new list.
pub(crate) fn appends_in_place(
    flavour: EngineFlavour,
    op: BinaryOp,
    left: &Operand,
    right: &Operand,
) -> Option<ListId> {
    if flavour != EngineFlavour::XEngine || op != BinaryOp::Add {
        return None;
    }
    let appended = match right {
        Operand::Double(_) => true,
        Operand::Value(value) => matches!(
            value.type_name(),
            TypeName::String | TypeName::Integer | TypeName::Float | TypeName::Key
        ),
        Operand::Shared(..) => false,
    };
    match left {
        Operand::Shared(_, id) if appended => Some(*id),
        Operand::Shared(..) | Operand::Value(_) | Operand::Double(_) => None,
    }
}

/// Write `elements` to every global, local and operand holding `id`: the
/// object changed in place.
pub(crate) fn rewrite<'frame>(
    id: ListId,
    elements: &[Element],
    globals: (&mut [Value], &[Option<ListId>]),
    frames: impl Iterator<Item = (&'frame mut [Value], &'frame [Option<ListId>])>,
    stack: &mut [Operand],
) {
    let (values, objects) = globals;
    let write = |values: &mut [Value], objects: &[Option<ListId>]| {
        for (value, object) in values.iter_mut().zip(objects) {
            if *object == Some(id) {
                *value = Value::List(elements.to_vec());
            }
        }
    };
    write(values, objects);
    for (locals, objects) in frames {
        write(locals, objects);
    }
    for operand in stack {
        if let Operand::Shared(held, object) = operand
            && *object == id
        {
            *held = elements.to_vec();
        }
    }
}
