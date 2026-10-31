//! Money a script moves and money paid to its prim: what the library hands
//! the host to pay (`llGiveMoney`, `llTransferLindenDollars`), what the host
//! answers, and the prim's pay layout (`llSetPayPrice`), which says what a
//! viewer's pay dialog offers and which payments Second Life takes.
//!
//! The economy itself — balances, the transaction log, the balance replies a
//! payment sends — is the host's. Book chapter `gridspec/lsl.md`, § Script
//! permissions, money and payment, measured by the `money/*` probes.

use crate::vm::flavour::EngineFlavour;

/// `PAY_HIDE`: the price field or a button is not shown.
pub const PAY_HIDE: i32 = -1;

/// `PAY_DEFAULT`: the price field or a button as the viewer shows it unset.
pub const PAY_DEFAULT: i32 = -2;

/// The amounts Second Life answers for the four buttons left at
/// `PAY_DEFAULT` (`money/pay-price`); OpenSim answers `PAY_DEFAULT` itself.
pub const SECOND_LIFE_DEFAULT_BUTTONS: [i32; 4] = [1, 5, 10, 20];

/// What Second Life alerts a payer whose payment it refuses because the
/// price and every button are hidden (`money/pay`, the two spaces aditi's).
pub const PAYMENT_STOPPED: &str =
    "Payment stopped:  the price paid does not match any of the pay buttons set for this object";

/// Money the script's object pays an avatar, from its owner's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    /// The key the script named as the payee, as it named it.
    pub to: String,
    /// How much, in L$; more than zero.
    pub amount: i32,
}

/// Why the economy did not move a [`Payment`] of `llTransferLindenDollars`,
/// as `transaction_result` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyRefusal {
    /// The payee has no account: no such avatar, a prim's key, `NULL_KEY` on
    /// OpenSim.
    NoSuchEntity,
    /// The owner holds less than the amount (Second Life; OpenSim's stock
    /// money module refuses nothing).
    InsufficientFunds,
}

impl MoneyRefusal {
    /// The `transaction_result` data that says it.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoSuchEntity => "LINDENDOLLAR_ENTITYDOESNOTEXIST",
            Self::InsufficientFunds => "LINDENDOLLAR_INSUFFICIENTFUNDS",
        }
    }
}

/// The `transaction_result(id, success, data)` a transfer raises in the
/// script that asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionResult {
    /// The handle `llTransferLindenDollars` returned.
    pub id: String,
    /// Whether the money moved.
    pub success: bool,
    /// The payee and amount, or why it did not move.
    pub data: String,
}

/// A prim's pay layout: the price field and the four buttons a viewer's pay
/// dialog shows, each an amount, [`PAY_HIDE`] or [`PAY_DEFAULT`]. It is the
/// prim's, kept by the host: a reset leaves it (`money/pay-price`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayPrice {
    /// The price field.
    pub price: i32,
    /// The four buttons.
    pub buttons: [i32; 4],
}

impl Default for PayPrice {
    /// A prim no script has set: every amount [`PAY_DEFAULT`].
    fn default() -> Self {
        Self {
            price: PAY_DEFAULT,
            buttons: [PAY_DEFAULT; 4],
        }
    }
}

/// What becomes of a payment to a prim under its [`PayPrice`] on Second Life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentAdmission {
    /// The money moves and every script of the prim with a `money` handler
    /// in its state hears it — taken even when none can.
    Taken,
    /// Dropped without a word: no event, no balance reply, no alert.
    Dropped,
    /// Refused with the payer alerted ([`PAYMENT_STOPPED`]).
    Refused,
}

impl PayPrice {
    /// What a viewer's `RequestPayPrice` is answered, or [`None`] for no
    /// answer. Second Life answers a button at [`PAY_DEFAULT`] with its
    /// default amount; OpenSim answers the layout as set, and nothing at all
    /// for a layout of zeros (`LLClientView.SendPayPrice`).
    #[must_use]
    pub fn reply(&self, flavour: EngineFlavour) -> Option<Self> {
        if flavour.is_opensim() {
            return (self.price != 0 || self.buttons.iter().any(|button| *button != 0))
                .then_some(*self);
        }
        let mut buttons = self.buttons;
        for (button, default) in buttons.iter_mut().zip(SECOND_LIFE_DEFAULT_BUTTONS) {
            if *button == PAY_DEFAULT {
                *button = default;
            }
        }
        Some(Self {
            price: self.price,
            buttons,
        })
    }

    /// Whether Second Life takes a payment of `amount` under this layout
    /// (`money/pay`). It checks the amount only when the price field is
    /// hidden: then it must be one of the buttons shown, or the payment is
    /// dropped — unless every button is hidden too, which is refused. A shown
    /// price takes any amount above zero; nothing takes zero.
    #[must_use]
    pub fn takes(&self, amount: i32) -> PaymentAdmission {
        if amount <= 0 {
            return PaymentAdmission::Dropped;
        }
        if self.price != PAY_HIDE {
            return PaymentAdmission::Taken;
        }
        let shown = self
            .reply(EngineFlavour::SecondLife)
            .unwrap_or(*self)
            .buttons;
        if shown.iter().all(|button| *button == PAY_HIDE) {
            PaymentAdmission::Refused
        } else if shown.contains(&amount) {
            PaymentAdmission::Taken
        } else {
            PaymentAdmission::Dropped
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn a_default_button_is_answered_with_its_amount_on_second_life_and_as_set_on_opensim() {
        let unset = PayPrice::default();
        assert_eq!(
            unset.reply(EngineFlavour::SecondLife),
            Some(PayPrice {
                price: PAY_DEFAULT,
                buttons: [1, 5, 10, 20],
            })
        );
        assert_eq!(unset.reply(EngineFlavour::YEngine), Some(unset));
        let zeros = PayPrice {
            price: 0,
            buttons: [0; 4],
        };
        assert_eq!(zeros.reply(EngineFlavour::SecondLife), Some(zeros));
        assert_eq!(zeros.reply(EngineFlavour::XEngine), None);
    }

    #[test]
    fn second_life_checks_the_amount_only_under_a_hidden_price() {
        let layout = |price, buttons| PayPrice { price, buttons };
        let cases = [
            (layout(5, [1, 2, 3, 4]), 7, PaymentAdmission::Taken),
            (
                layout(PAY_HIDE, [PAY_HIDE; 4]),
                1,
                PaymentAdmission::Refused,
            ),
            (layout(PAY_HIDE, [1, 2, 3, 4]), 7, PaymentAdmission::Dropped),
            (layout(PAY_HIDE, [1, 2, 3, 4]), 2, PaymentAdmission::Taken),
            (layout(5, [PAY_HIDE; 4]), 7, PaymentAdmission::Taken),
            (
                layout(PAY_HIDE, [PAY_DEFAULT; 4]),
                10,
                PaymentAdmission::Taken,
            ),
            (PayPrice::default(), 0, PaymentAdmission::Dropped),
        ];
        for (layout, amount, admission) in cases {
            assert_eq!(layout.takes(amount), admission, "{layout:?} {amount}");
        }
    }
}
