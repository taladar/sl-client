//! Library tranche: script permissions, money and payment
//! (`server-lsl-lib-money-permissions`) — asking an avatar for permissions
//! and reading what was granted, paying money out of the owner's account, and
//! setting what a payer is offered.
//!
//! Each function checks what it is given as the region's grid does (book
//! chapter `gridspec/lsl.md`, § Asking for permissions and § Script
//! permissions, money and payment, measured by `avatar/permissions` and the
//! `money/*` probes on aditi and both OpenSim engines) and hands the host a
//! finished request; the economy is the host's. Second Life's words are
//! aditi's; OpenSim's are `LSL_Api.cs`'s.

use crate::cast::until_nul;
use crate::library::dotnet;
use crate::library::{BuiltinId, CallError, Exception, Key};
use crate::parse;
use crate::value::{Element, NULL_KEY};
use crate::vm::comms::is_somebody;
use crate::vm::permissions::{
    PERMISSION_CONTROL_CAMERA, PERMISSION_DEBIT, SEATED_IMPLICIT, WORN_IMPLICIT,
};
use crate::vm::{PAY_DEFAULT, PAY_HIDE, PayPrice, Payment, Question, ScriptCtx, TransactionResult};

/// What Second Life says asking another avatar than the owner for
/// `PERMISSION_DEBIT`, putting no question (`money/debit`).
const NON_OWNER_DEBIT: &str =
    "Script asking non-owner for run time permissions only owner can grant";

/// What Second Life says asking for `PERMISSION_CONTROL_CAMERA` from an
/// object the avatar neither wears nor sits on, putting no question
/// (`avatar/permissions`).
const CAMERA_UNSUPPORTED: &str =
    "Camera control currently only supported for attachments and objects on which you are sitting.";

/// Whether two keys name the same thing, as a key comparison does.
const fn same_key(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// What `agent` is granted without a question by the script's object: the
/// avatar-control permissions when it wears the object or sits on it.
fn implicit(ctx: &mut ScriptCtx<'_>, agent: &str) -> i32 {
    let caller = ctx.caller();
    if ctx
        .host()
        .worn_by(caller)
        .is_some_and(|wearer| same_key(&wearer, agent))
    {
        WORN_IMPLICIT
    } else if ctx.host().seated_on(caller, agent) {
        SEATED_IMPLICIT
    } else {
        0
    }
}

/// `llRequestPermissions`: ask `agent` for the permissions `mask`. What the
/// object is granted unasked — the avatar-control permissions of its wearer
/// or of an avatar sitting on it — is held at once, with
/// `run_time_permissions`; anything else is a question put to the avatar,
/// whose answer comes back through the engine.
///
/// Second Life puts nothing for a mask of 0 and keeps what is held; it
/// refuses, in words on `DEBUG_CHANNEL` and with no question, to ask another
/// avatar than the owner for `PERMISSION_DEBIT` and to ask for
/// `PERMISSION_CONTROL_CAMERA` of an avatar that neither wears nor sits on the
/// object. OpenSim asks for anything; a mask of 0 or `NULL_KEY` releases what
/// is held with `run_time_permissions(0)`, and so does a question for an
/// avatar not in the region. A string that is no key is nothing there.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_request_permissions(
    ctx: &mut ScriptCtx<'_>,
    agent: Key,
    mask: i32,
) -> Result<(), CallError> {
    let opensim = ctx.flavour().is_opensim();
    if opensim {
        if !dotnet::is_uuid(&agent.0) {
            return Ok(());
        }
        if mask == 0 || !is_somebody(&agent.0) {
            ctx.grant_permissions(None, 0);
            return Ok(());
        }
    } else {
        if mask == 0 {
            return Ok(());
        }
        let caller = ctx.caller();
        let owner = ctx.host().owner(caller);
        if mask & PERMISSION_DEBIT != 0 && !same_key(&owner, &agent.0) {
            ctx.refuse(NON_OWNER_DEBIT.to_owned());
            return Ok(());
        }
        if mask & PERMISSION_CONTROL_CAMERA != 0 && implicit(ctx, &agent.0) == 0 {
            ctx.refuse(CAMERA_UNSUPPORTED.to_owned());
            return Ok(());
        }
    }
    if mask & !implicit(ctx, &agent.0) == 0 {
        ctx.grant_permissions(Some(agent.0), mask);
        return Ok(());
    }
    let asked = ctx.ask_permissions(Question {
        agent: agent.0,
        mask,
    });
    if !asked && opensim {
        ctx.raise_permissions(0);
    }
    Ok(())
}

/// `llGetPermissions`: the permissions held.
///
/// # Errors
///
/// None; the signature is the library's.
pub const fn ll_get_permissions(ctx: &mut ScriptCtx<'_>) -> Result<i32, CallError> {
    Ok(ctx.permissions().mask())
}

/// `llGetPermissionsKey`: who granted the permissions held, or `NULL_KEY`.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_get_permissions_key(ctx: &mut ScriptCtx<'_>) -> Result<Key, CallError> {
    Ok(Key(ctx
        .permissions()
        .granter()
        .unwrap_or(NULL_KEY)
        .to_owned()))
}

/// `llGiveMoney`: pay `destination` `amount` L$ from the owner's account;
/// always 0, whatever becomes of it, and no event follows.
///
/// Second Life refuses, on `DEBUG_CHANNEL`, a script that does not hold
/// `PERMISSION_DEBIT` and an amount that is not positive. OpenSim says
/// nothing of a script that holds no permissions at all, refuses one that
/// holds others but not `PERMISSION_DEBIT` and a destination that is no key,
/// and leaves the amount to its money module.
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_give_money(
    ctx: &mut ScriptCtx<'_>,
    destination: Key,
    amount: i32,
) -> Result<i32, CallError> {
    let held = ctx.permissions().holds(PERMISSION_DEBIT);
    if ctx.flavour().is_opensim() {
        if ctx.permissions().granter().is_none() {
            return Ok(0);
        }
        if !held {
            ctx.refuse("llGiveMoney: No permissions to give money".to_owned());
            return Ok(0);
        }
        if !dotnet::is_uuid(&destination.0) {
            ctx.refuse("llGiveMoney: Bad key in llGiveMoney".to_owned());
            return Ok(0);
        }
    } else {
        if !held {
            ctx.refuse(
                "Script trying to debit L$ from owner but PERMISSION_DEBIT permission not set!"
                    .to_owned(),
            );
            return Ok(0);
        }
        if amount <= 0 {
            ctx.refuse("Invalid amount for llGiveMoney().".to_owned());
            return Ok(0);
        }
    }
    let caller = ctx.caller();
    ctx.host().give_money(
        caller,
        &Payment {
            to: destination.0,
            amount,
        },
    );
    Ok(0)
}

/// A fresh key for a transfer's handle, from the region's seeded source: a
/// version 4 UUID, as each grid mints one.
fn fresh_key(ctx: &mut ScriptCtx<'_>) -> String {
    let words = [
        ctx.random_bits(),
        ctx.random_bits(),
        ctx.random_bits(),
        ctx.random_bits(),
    ];
    let [a, b, c, d] = words;
    let version = (b & 0xffff_0fff) | 0x0000_4000;
    let variant = (c & 0x3fff_ffff) | 0x8000_0000;
    format!(
        "{a:08x}-{:04x}-{:04x}-{:04x}-{:04x}{d:08x}",
        version >> 16,
        version & 0xffff,
        variant >> 16,
        variant & 0xffff,
    )
}

/// `llTransferLindenDollars`: pay `destination` `amount` L$ from the owner's
/// account; a handle, always, which the `transaction_result` that follows
/// names — `TRUE` with the payee and the amount, or `FALSE` with why not.
///
/// Refused before anything moves: a script without `PERMISSION_DEBIT`
/// (`MISSING_PERMISSION_DEBIT`), an amount that is not positive
/// (`INVALID_AMOUNT`, which OpenSim checks first), on Second Life `NULL_KEY`
/// (`INVALID_DESTINATION`) and on OpenSim a string that is no key
/// (`INVALID_AGENT`). Then the economy answers ([`MoneyRefusal`]). Second
/// Life's payee and amount are `"<payee>, <amount>"`, OpenSim's
/// `"<payee>,<amount>"`. Which of Second Life's refusals comes first when two
/// apply is not measured.
///
/// [`MoneyRefusal`]: crate::vm::MoneyRefusal
///
/// # Errors
///
/// None; the signature is the library's.
pub fn ll_transfer_linden_dollars(
    ctx: &mut ScriptCtx<'_>,
    destination: Key,
    amount: i32,
) -> Result<Key, CallError> {
    let handle = fresh_key(ctx);
    let opensim = ctx.flavour().is_opensim();
    let permitted = ctx.permissions().holds(PERMISSION_DEBIT);
    let refused = if opensim && amount <= 0 {
        Some("INVALID_AMOUNT")
    } else if !permitted {
        Some("MISSING_PERMISSION_DEBIT")
    } else if amount <= 0 {
        Some("INVALID_AMOUNT")
    } else if opensim && !dotnet::is_uuid(&destination.0) {
        Some("INVALID_AGENT")
    } else if !opensim && destination.0 == NULL_KEY {
        Some("INVALID_DESTINATION")
    } else {
        None
    };
    let (success, data) = if let Some(refused) = refused {
        (false, refused.to_owned())
    } else {
        let caller = ctx.caller();
        let payment = Payment {
            to: destination.0,
            amount,
        };
        match ctx.host().transfer_money(caller, &payment) {
            Ok(()) if opensim => (true, format!("{},{amount}", payment.to)),
            Ok(()) => (true, format!("{}, {amount}", payment.to)),
            Err(refusal) => (false, refusal.code().to_owned()),
        }
    };
    ctx.raise_transaction(TransactionResult {
        id: handle.clone(),
        success,
        data,
    });
    Ok(Key(handle))
}

/// OpenSim's padding of `llSetPayPrice`'s button list: one of fewer than four
/// elements gains [`PAY_HIDE`]s up to five. OpenSim pads the very list the
/// script passed, so the compiler pads a list variable passed to it the same
/// way after the call (`money/pay-price`: `[1, 2]` reads `[1, 2, -1, -1, -1]`
/// after).
#[must_use]
pub(crate) fn pad_pay_buttons(mut buttons: Vec<Element>) -> Vec<Element> {
    if buttons.len() < 4 {
        buttons.resize(5, Element::Integer(PAY_HIDE));
    }
    buttons
}

/// OpenSim's `GetLSLIntegerItem` of a pay button: an integer as it is, a
/// string or a key as the cast reads it, and anything else a throw.
fn opensim_button(button: &Element) -> Result<i32, Exception> {
    match button {
        Element::Integer(integer) => Ok(*integer),
        Element::String(text) | Element::Key(text) => Ok(parse::integer(&until_nul(text.clone()))),
        Element::Float(_) => Err(Exception::InvalidCast),
        Element::Vector(_) => Err(Exception::VectorNotInteger),
        Element::Rotation(_) => Err(Exception::RotationNotInteger),
    }
}

/// `llSetPayPrice`: offer a payer `price` in the price field and `buttons` on
/// the four buttons, each an amount, `PAY_HIDE` or `PAY_DEFAULT`. No grid
/// checks an amount's sign or size, and a fifth button on is ignored.
///
/// Second Life hides a missing button and one that is not an integer, and
/// leaves every button at its default for an empty list. OpenSim pads a
/// short list with `PAY_HIDE`, reads a string button as `(integer)` does and
/// throws an `InvalidCastException` at any other button that is not an
/// integer, the layout left as it was; a script in a child prim sets nothing
/// there.
///
/// # Errors
///
/// [`CallError::Exception`] on OpenSim for a float, vector or rotation
/// button.
pub fn ll_set_pay_price(
    ctx: &mut ScriptCtx<'_>,
    price: i32,
    buttons: Vec<Element>,
) -> Result<(), CallError> {
    let caller = ctx.caller();
    let mut amounts = [PAY_HIDE; 4];
    if ctx.flavour().is_opensim() {
        if ctx.host().links(caller).this > 1 {
            return Ok(());
        }
        let padded = pad_pay_buttons(buttons);
        for (amount, button) in amounts.iter_mut().zip(&padded) {
            *amount = opensim_button(button).map_err(|exception| CallError::Exception {
                id: BuiltinId::LlSetPayPrice,
                exception,
            })?;
        }
    } else if buttons.is_empty() {
        amounts = [PAY_DEFAULT; 4];
    } else {
        for (amount, button) in amounts.iter_mut().zip(&buttons) {
            if let Element::Integer(integer) = button {
                *amount = *integer;
            }
        }
    }
    ctx.host().set_pay_price(
        caller,
        PayPrice {
            price,
            buttons: amounts,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_short_button_list_is_padded_to_five_and_a_long_one_left() {
        let integers = |values: &[i32]| -> Vec<Element> {
            values.iter().copied().map(Element::Integer).collect()
        };
        assert_eq!(
            pad_pay_buttons(integers(&[1, 2])),
            integers(&[1, 2, -1, -1, -1])
        );
        assert_eq!(pad_pay_buttons(Vec::new()), integers(&[-1; 5]));
        assert_eq!(
            pad_pay_buttons(integers(&[1, 2, 3, 4])),
            integers(&[1, 2, 3, 4])
        );
    }

    #[test]
    fn opensim_reads_a_string_button_and_throws_at_a_float() {
        assert_eq!(opensim_button(&Element::String("5".to_owned())), Ok(5));
        assert_eq!(
            opensim_button(&Element::Float(2.5)),
            Err(Exception::InvalidCast)
        );
    }
}
