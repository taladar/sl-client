//! What a balance reply that names a transaction tells the agent: "You paid
//! … L$1", "… paid you L$1", or a failure.
//!
//! The reference's `process_money_balance_reply` keeps the balance (the status
//! bar does that here) and, when the reply carries a description, raises one
//! notice per transaction id: the `SystemMessage` with the description for a
//! reply with no `TransactionInfo` block, nothing for a pure balance update
//! (no source, no destination), and otherwise `PaymentSent`,
//! `PaymentFailure` or `PaymentReceived` built from the block — the other
//! side's name, the amount and a reason from the transaction type
//! (`process_money_balance_reply_extended`). It waits for the name before it
//! shows the notice, and gives no reason for a payment from somebody muted.
//!
//! Second Life answers a gift, a payment to an object and a purchase this way;
//! OpenSim's stock money module sends a balance with no transaction at all
//! (`book/src/gridspec/economy.md`), which raises nothing.

use std::collections::VecDeque;

use bevy::prelude::*;

use sl_client_bevy::{
    Command, MoneyBalance, OwnerKey, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid,
};
use sl_viewer_kit::grid_currency::GridCurrency;

use crate::i18n::{TransArgs, Translator};
use crate::notifications::ShowNotification;
use crate::social::{GroupsModel, MuteModel};
use crate::world_api::AvatarState;

/// How many transaction ids are remembered for the duplicate check.
const MAX_LOOKBACK: usize = 30;

/// How many of the oldest are forgotten at once when there are more.
const POP_FRONT: usize = 12;

/// The transaction type of a gift (`TRANS_GIFT`).
const TRANS_GIFT: i32 = 5001;

/// Why money moved, as the notice says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A fixed phrase, by its Fluent key.
    Phrase(&'static str),
    /// For an item (`for [ITEM]`): an object sale.
    ForItem(String),
    /// A gift's own description, verbatim.
    Said(String),
}

/// A notice a balance reply raises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentNotice {
    /// A reply with no transaction block: its description, as it came.
    System(String),
    /// A payment: the template, the message's Fluent key, the other side
    /// (none for a fee paid to the system), the amount and the reason.
    Payment {
        /// `PaymentSent`, `PaymentFailure` or `PaymentReceived`.
        template: &'static str,
        /// The Fluent key of the message.
        message_key: &'static str,
        /// Whom the agent paid, or who paid the agent.
        other: Option<OwnerKey>,
        /// The amount.
        amount: i64,
        /// Why, if the notice says.
        reason: Option<Reason>,
    },
}

/// The reason the reference gives for a transaction of `transaction_type`
/// (`reason_from_transaction_type`); `None` for none.
fn reason_of(transaction_type: i32, item_description: &str) -> Option<Reason> {
    match transaction_type {
        5000 => Some(Reason::ForItem(item_description.to_owned())),
        5002 => Some(Reason::Phrase("payment-reason-land")),
        5006 => Some(Reason::Phrase("payment-reason-land-pass")),
        1002 => Some(Reason::Phrase("payment-reason-group-create")),
        1004 => Some(Reason::Phrase("payment-reason-group-join")),
        1101 => Some(Reason::Phrase("payment-reason-upload")),
        1103 => Some(Reason::Phrase("payment-reason-classified")),
        6001 => Some(Reason::Phrase("payment-reason-group-land-deed")),
        // The simulator says "Payment" when the payer wrote nothing.
        TRANS_GIFT if item_description != "Payment" && !item_description.is_empty() => {
            Some(Reason::Said(item_description.to_owned()))
        }
        _ => None,
    }
}

/// The notice `money` raises for the agent `own`, of whom the payer is muted
/// when `muted` says so; `None` for a reply that raises none.
#[must_use]
pub fn payment_notice(
    money: &MoneyBalance,
    own: Uuid,
    muted: impl Fn(Uuid) -> bool,
) -> Option<PaymentNotice> {
    if money.description.is_empty() {
        return None;
    }
    let Some(transaction) = &money.transaction else {
        return Some(PaymentNotice::System(money.description.clone()));
    };
    let (source, dest) = (transaction.source, transaction.dest);
    if source.uuid().is_nil() && dest.uuid().is_nil() {
        return None;
    }
    let reason = reason_of(transaction.transaction_type, &transaction.item_description);
    let gift = transaction.transaction_type == TRANS_GIFT;
    let amount = i64::try_from(transaction.amount.0).unwrap_or(i64::MAX);
    if source.uuid() == own {
        let named = !dest.uuid().is_nil();
        let message_key = match (money.success, reason.is_some(), named, gift) {
            (true, true, true, true) => "payment-you-paid-gift",
            (true, true, true, false) => "payment-you-paid",
            (true, true, false, _) => "payment-you-paid-no-name",
            (true, false, true, _) => "payment-you-paid-no-reason",
            (true, false, false, _) => "payment-you-paid-no-info",
            (false, true, true, true) => "payment-you-failed-gift",
            (false, true, true, false) => "payment-you-failed",
            (false, true, false, _) => "payment-you-failed-no-name",
            (false, false, true, _) => "payment-you-failed-no-reason",
            (false, false, false, _) => "payment-you-failed-no-info",
        };
        Some(PaymentNotice::Payment {
            template: if money.success {
                "PaymentSent"
            } else {
                "PaymentFailure"
            },
            message_key,
            other: named.then_some(dest),
            amount,
            reason,
        })
    } else {
        let reason = reason.filter(|_reason| !muted(source.uuid()));
        let message_key = match (reason.is_some(), gift) {
            (true, true) => "payment-paid-you-gift",
            (true, false) => "payment-paid-you",
            (false, _) => "payment-paid-you-no-reason",
        };
        Some(PaymentNotice::Payment {
            template: "PaymentReceived",
            message_key,
            other: Some(source),
            amount,
            reason,
        })
    }
}

/// The transaction ids already told, and the payment notices waiting on a
/// name.
#[derive(Resource, Debug, Default)]
pub struct PaymentNotices {
    /// The transaction ids of the replies already told, oldest first.
    recent: VecDeque<Uuid>,
    /// The notices waiting on the other side's name, and whether the name
    /// was asked for.
    waiting: Vec<(PaymentNotice, bool)>,
}

impl PaymentNotices {
    /// Whether `transaction` was told already; remembers it if not, as the
    /// reference does.
    fn seen(&mut self, transaction: Uuid) -> bool {
        if self.recent.contains(&transaction) {
            return true;
        }
        if self.recent.len() > MAX_LOOKBACK {
            self.recent.drain(..POP_FRONT);
        }
        self.recent.push_back(transaction);
        false
    }
}

/// Queues the notice of every balance reply that raises one.
pub fn ingest_payments(
    mut events: MessageReader<SlEvent>,
    mut notices: ResMut<PaymentNotices>,
    mut show: MessageWriter<ShowNotification>,
    identity: Option<Res<SlIdentity>>,
    mutes: Option<Res<MuteModel>>,
) {
    let own = identity
        .and_then(|identity| identity.agent_id)
        .map_or_else(Uuid::nil, |agent| agent.uuid());
    for event in events.read() {
        let SlSessionEvent::MoneyBalance(money) = &event.0 else {
            continue;
        };
        if money.description.is_empty() || notices.seen(money.transaction_id) {
            continue;
        }
        let muted = |id: Uuid| mutes.as_ref().is_some_and(|mutes| mutes.is_muted(id));
        match payment_notice(money, own, muted) {
            Some(PaymentNotice::System(message)) => {
                show.write(ShowNotification::new("SystemMessage").arg("MESSAGE", message));
            }
            Some(notice) => notices.waiting.push((notice, false)),
            None => {}
        }
    }
}

/// Shows each waiting payment notice once the other side's name is known,
/// asking for the name once.
pub fn show_payments(
    mut notices: ResMut<PaymentNotices>,
    mut show: MessageWriter<ShowNotification>,
    mut commands: MessageWriter<SlCommand>,
    translator: Translator,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    currency: Option<Res<GridCurrency>>,
) {
    if notices.waiting.is_empty() {
        return;
    }
    let symbol = currency
        .as_ref()
        .map_or(sl_viewer_kit::grid_currency::LINDEN_SYMBOL, |currency| {
            currency.symbol()
        });
    let waiting = std::mem::take(&mut notices.waiting);
    for (notice, asked) in waiting {
        let PaymentNotice::Payment {
            template,
            message_key,
            other,
            amount,
            reason,
        } = &notice
        else {
            continue;
        };
        let name = match other {
            None => Some(String::new()),
            Some(OwnerKey::Agent(agent)) => avatars.shown_name_of(*agent).map(str::to_owned),
            Some(OwnerKey::Group(group)) => groups.group_name(*group).map(str::to_owned),
        };
        let Some(name) = name else {
            if !asked {
                commands.write(SlCommand(match other {
                    Some(OwnerKey::Group(group)) => Command::RequestGroupNames(vec![*group]),
                    Some(OwnerKey::Agent(agent)) => Command::RequestAvatarNames(vec![*agent]),
                    None => continue,
                }));
            }
            notices.waiting.push((notice, true));
            continue;
        };
        let reason = match reason {
            None => String::new(),
            Some(Reason::Phrase(key)) => translator.get(key),
            Some(Reason::ForItem(item)) => translator.format(
                "payment-reason-for-item",
                &TransArgs::new().text("item", item),
            ),
            Some(Reason::Said(said)) => said.clone(),
        };
        let message = translator.format(
            message_key,
            &TransArgs::new()
                .text("name", &name)
                .text("amount", &translator.currency(symbol, *amount))
                .text("reason", &reason),
        );
        show.write(ShowNotification::new(template).arg("MESSAGE", message));
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, GroupKey, LandArea, LindenAmount, MoneyBalance, MoneyTransaction, OwnerKey, Uuid,
    };

    use super::{PaymentNotice, PaymentNotices, Reason, payment_notice};

    /// The agent.
    const OWN: u128 = 0xa;

    /// Somebody else.
    const OTHER: u128 = 0xb;

    /// A reply with `description`, naming a transaction of `kind` from
    /// `source` to `dest` when `transaction` says so.
    fn reply(
        success: bool,
        description: &str,
        transaction: Option<(i32, OwnerKey, OwnerKey, &str)>,
    ) -> MoneyBalance {
        MoneyBalance {
            agent_id: AgentKey::from(Uuid::from_u128(OWN)),
            transaction_id: Uuid::from_u128(0x7),
            success,
            balance: LindenAmount(100),
            square_meters_credit: LandArea(0),
            square_meters_committed: LandArea(0),
            description: description.to_owned(),
            transaction: transaction.map(|(kind, source, dest, item)| MoneyTransaction {
                transaction_type: kind,
                source,
                dest,
                amount: LindenAmount(1),
                item_description: item.to_owned(),
            }),
        }
    }

    /// An agent's key as an owner.
    fn agent(id: u128) -> OwnerKey {
        OwnerKey::Agent(AgentKey::from(Uuid::from_u128(id)))
    }

    /// A group's key as an owner.
    fn group(id: u128) -> OwnerKey {
        OwnerKey::Group(GroupKey::from(Uuid::from_u128(id)))
    }

    /// Nobody.
    fn nobody() -> OwnerKey {
        OwnerKey::Agent(AgentKey::from(Uuid::nil()))
    }

    /// A reply with no description raises nothing; one with no transaction
    /// block raises its description as it came.
    #[test]
    fn a_bare_reply_raises_its_description() {
        assert_eq!(
            payment_notice(&reply(true, "", None), Uuid::from_u128(OWN), |_| false),
            None
        );
        assert_eq!(
            payment_notice(&reply(true, "Paid", None), Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::System("Paid".to_owned()))
        );
    }

    /// A pure balance update names no side and raises nothing.
    #[test]
    fn a_balance_update_raises_nothing() {
        let money = reply(true, "x", Some((5001, nobody(), nobody(), "")));
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            None
        );
    }

    /// A gift the agent made, with the simulator's "Payment" for no
    /// description, says whom and how much and no reason.
    #[test]
    fn a_gift_sent_names_the_payee() {
        let money = reply(true, "x", Some((5001, agent(OWN), agent(OTHER), "Payment")));
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::Payment {
                template: "PaymentSent",
                message_key: "payment-you-paid-no-reason",
                other: Some(agent(OTHER)),
                amount: 1,
                reason: None,
            })
        );
    }

    /// A failed purchase of an object says what it was for.
    #[test]
    fn a_failed_purchase_says_what_for() {
        let money = reply(false, "x", Some((5000, agent(OWN), agent(OTHER), "Cube")));
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::Payment {
                template: "PaymentFailure",
                message_key: "payment-you-failed",
                other: Some(agent(OTHER)),
                amount: 1,
                reason: Some(Reason::ForItem("Cube".to_owned())),
            })
        );
    }

    /// A fee to the system names nobody.
    #[test]
    fn a_fee_names_nobody() {
        let money = reply(true, "x", Some((1101, agent(OWN), nobody(), "")));
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::Payment {
                template: "PaymentSent",
                message_key: "payment-you-paid-no-name",
                other: None,
                amount: 1,
                reason: Some(Reason::Phrase("payment-reason-upload")),
            })
        );
    }

    /// A gift received keeps its description, unless the payer is muted;
    /// a group's payment names the group.
    #[test]
    fn a_gift_received_says_why_unless_muted() {
        let money = reply(true, "x", Some((5001, agent(OTHER), agent(OWN), "thanks")));
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::Payment {
                template: "PaymentReceived",
                message_key: "payment-paid-you-gift",
                other: Some(agent(OTHER)),
                amount: 1,
                reason: Some(Reason::Said("thanks".to_owned())),
            })
        );
        assert_eq!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| true),
            Some(PaymentNotice::Payment {
                template: "PaymentReceived",
                message_key: "payment-paid-you-no-reason",
                other: Some(agent(OTHER)),
                amount: 1,
                reason: None,
            })
        );
        let money = reply(true, "x", Some((5006, group(0xc), agent(OWN), "")));
        assert!(matches!(
            payment_notice(&money, Uuid::from_u128(OWN), |_| false),
            Some(PaymentNotice::Payment { other: Some(other), .. }) if other == group(0xc)
        ));
    }

    /// A transaction is told once.
    #[test]
    fn a_transaction_is_told_once() {
        let mut notices = PaymentNotices::default();
        assert!(!notices.seen(Uuid::from_u128(1)));
        assert!(notices.seen(Uuid::from_u128(1)));
        for id in 2..=40 {
            let _seen = notices.seen(Uuid::from_u128(id));
        }
        assert!(notices.recent.len() <= 31, "the lookback is bounded");
    }
}
