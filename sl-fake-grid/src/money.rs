//! Money: the grid's ledger of balances, and what it answers a balance
//! request, a gift, a payment to an object and a purchase of an object set for
//! sale — held by the grid (a `Ledger` on its `GridCore`) and answered by a
//! responder (`run_money_responder`) on each session.
//!
//! What each live grid does was measured by the `money` conformance census on
//! aditi and the local OpenSim (2026-10-18, `book/src/gridspec/economy.md`),
//! and what a script's money does by the `money/*` probes (2026-10-26,
//! `book/src/gridspec/lsl.md` § Script permissions, money and payment). Every
//! divergence is a field of [`MoneyPolicy`] or the ledger's opening balance:
//!
//! | | Second Life | OpenSim (stock `BetaGridLikeMoneyModule`) |
//! | --- | --- | --- |
//! | a balance | kept per account, L$ 10 000 to start | none kept: every balance reads 0 |
//! | a gift, a payment to an object, a script's payment | moved, and both sides told by a balance reply with a `TransactionInfo` block | dropped without a word |
//! | a gift to nobody, or of more than the payer holds | a failed balance reply to the payer | dropped |
//! | a payment to an object | taken as type `5008` whatever type it named, judged by the prim's pay layout, its `money` event raised | dropped |
//! | pay buttons of a prim no script set | `-2`, `1, 5, 10, 20` | `-2` ×5 |
//! | a purchase | at any price; refused by alert at the wrong kind of sale or price | refused by a message box at any price but L$ 0; at L$ 0 refused by agent alert as not for sale, at the wrong price or kind |
//! | what a purchase tells | both sides a balance reply of type `5000`; an original's buyer an alert keyed `NowOwnObject` | nothing but the goods |
//! | a bought copy, the contents | the item created; the contents a new folder over the event queue | the item created; the contents as unasked descendents of the root and the new folder |
//! | a bought original's scripts | `changed(CHANGED_OWNER)` once | three times |
//! | buying one item of the contents | refused, alert keyed `InventoryNotForSale` | not handled |
//!
//! The session's own `SimSession` balance (what an upload states and charges)
//! is the ledger's, written into it before every capability request and the
//! request's charge read back out of it after (`crate::caps_endpoint`).
//!
//! A land pass is sold here too, since what it costs is the ledger's: on Second
//! Life its price is paid to the parcel's owner and the buyer told by a balance
//! reply of type `5006`, "You paid *owner* L$*n* for a land access pass.".
//!
//! Not modelled: money held by a group (a gift to a group, an object a group
//! owns paid or bought — the transfer is refused as one to nobody; a pass to a
//! group's parcel is paid to nobody); a parcel's purchase, which sells for
//! nothing on both flavours (unmeasured on Second Life, where aditi's land
//! helper refuses every purchase); a contents item set for sale (refused like
//! one that is not).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use sl_lsl_runtime::vm::{PAYMENT_STOPPED, PayPrice, PaymentAdmission};
use sl_proto::{
    AlertInfo, ImDialog, InstantMessage, InventoryFolder, InventoryItem, LandArea, MoneyBalance,
    MoneyTransaction, MoneyTransferOrder, ObjectBuyItem, RegionLocalObjectId, SaleType,
    ServerEvent, SimSession, TransactionFlags, TransactionId,
};
use sl_types::key::{AgentKey, InventoryFolderKey, ObjectKey, OwnerKey};
use sl_types::money::LindenAmount;
use sl_wire::Permissions;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::driver::SharedSim;
use crate::runtime::GridCore;
use crate::social::{legacy_name, tell, warn_on};
use crate::world::{RegionChange, SceneFixtures};

/// The transaction type of a purchase (`TRANS_OBJECT_SALE`).
pub(crate) const OBJECT_SALE: i32 = 5000;

/// The transaction type of a payment to an object (`TRANS_PAY_OBJECT`), which
/// Second Life writes over whatever type the payer named.
pub(crate) const PAY_OBJECT: i32 = 5008;

/// The transaction type of a script's payment (`TRANS_OBJECT_PAYS`).
pub(crate) const OBJECT_PAYS: i32 = 5009;

/// The transaction type of a land pass (`TRANS_LAND_PASS_SALE`).
pub(crate) const LAND_PASS_SALE: i32 = 5006;

/// `CHANGED_OWNER`.
const CHANGED_OWNER: i32 = 0x80;

/// What Second Life tells the payer of a transfer to an id that is nobody's.
pub const NOBODY_TEXT: &str =
    "There has been a problem transferring L$. This error has been logged and reported.";

/// What Second Life tells the payer of a transfer it does not hold.
pub const INSUFFICIENT_TEXT: &str = "Insufficient funds.";

/// What OpenSim's stock money module answers a purchase at any price but L$ 0
/// with, as a message box.
pub const OPENSIM_PRICED_PURCHASE_TEXT: &str =
    "Buying anything for a price other than zero is not implemented";

/// The alert key Second Life refuses a purchase of one item of the contents
/// with, and its text.
const INVENTORY_NOT_FOR_SALE: (&str, &str) = ("InventoryNotForSale", "Inventory is not for sale.");

/// The alert key Second Life tells the buyer of an original with.
const NOW_OWN_OBJECT: &str = "NowOwnObject";

/// How a grid runs its money where the ledger alone does not say
/// ([`ImitatedGrid::money_policy`](crate::ImitatedGrid::money_policy)).
#[expect(
    clippy::module_name_repetitions,
    reason = "read where it is used, `ImitatedGrid::money_policy`, beside the other policies"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoneyPolicy {
    /// How a purchase is judged and told.
    pub purchases: Purchases,
    /// How many times a bought original's scripts hear `changed(CHANGED_OWNER)`.
    pub owner_changed_raised: u8,
    /// Whether buying one item of the contents is refused with an alert keyed
    /// `InventoryNotForSale`, rather than not answered.
    pub refuses_contents_item: bool,
}

impl Default for MoneyPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.money_policy()
    }
}

/// How a purchase is judged and told ([`MoneyPolicy::purchases`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purchases {
    /// At any price; the kind of sale checked before the price, each refused
    /// by an alert ("Sale of *name* cancelled.  …"), nothing said of an object
    /// that is not for sale; the money moved and both sides told: Second
    /// Life.
    Charged,
    /// Refused at any price but L$ 0 by a message box from nobody; at L$ 0 an
    /// object not for sale, the wrong price and the wrong kind (in that order)
    /// refused by agent alert; nothing moved or told: OpenSim's stock money
    /// module.
    FreeOnly,
}

/// Why the ledger did not move a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The grid keeps no balances: nothing moves, and nobody is told.
    NoLedger,
    /// The payer holds less than the amount.
    InsufficientFunds,
}

/// Every account's L$ balance, where the grid keeps them
/// ([`ImitatedGrid::upload_funds`](crate::ImitatedGrid::upload_funds) is what
/// an account starts with).
///
/// One per grid, for as long as it runs: a balance outlives the session that
/// changed it, so an avatar that logs out and in again holds what it held.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ledger {
    /// What an account holds before anything moves it; `None` for a grid
    /// that keeps no balances.
    opening: Option<i64>,
    /// What each account holds once something moved it.
    balances: BTreeMap<AgentKey, i64>,
}

impl Ledger {
    /// A ledger whose accounts start with `opening`, or one that keeps no
    /// balances.
    pub(crate) const fn new(opening: Option<i64>) -> Self {
        Self {
            opening,
            balances: BTreeMap::new(),
        }
    }

    /// What `agent` holds, where balances are kept.
    pub(crate) fn balance(&self, agent: AgentKey) -> Option<i64> {
        self.opening
            .map(|opening| self.balances.get(&agent).copied().unwrap_or(opening))
    }

    /// What `agent` holds as a reply states it: never below zero, and zero
    /// where no balance is kept.
    pub(crate) fn stated(&self, agent: AgentKey) -> LindenAmount {
        LindenAmount(
            self.balance(agent)
                .and_then(|balance| u64::try_from(balance).ok())
                .unwrap_or(0),
        )
    }

    /// Takes `amount` from `agent` — a fee the grid charges, an upload or a
    /// founding — where balances are kept.
    pub(crate) fn charge(&mut self, agent: AgentKey, amount: i64) {
        if let Some(balance) = self.balance(agent) {
            let _was = self.balances.insert(agent, balance.saturating_sub(amount));
        }
    }

    /// Moves `amount` from `from` to `to`: an account, or a group, which
    /// keeps no balance here, so what is paid to one goes nowhere.
    pub(crate) fn pay(&mut self, from: AgentKey, to: OwnerKey, amount: i64) -> Result<(), Refusal> {
        match to {
            OwnerKey::Agent(to) => self.transfer(from, to, amount),
            OwnerKey::Group(_group) => {
                let held = self.balance(from).ok_or(Refusal::NoLedger)?;
                if held < amount {
                    return Err(Refusal::InsufficientFunds);
                }
                self.charge(from, amount);
                Ok(())
            }
        }
    }

    /// Moves `amount` from `from` to `to` (one account, when they are the
    /// same).
    pub(crate) fn transfer(
        &mut self,
        from: AgentKey,
        to: AgentKey,
        amount: i64,
    ) -> Result<(), Refusal> {
        let held = self.balance(from).ok_or(Refusal::NoLedger)?;
        if held < amount {
            return Err(Refusal::InsufficientFunds);
        }
        self.charge(from, amount);
        self.charge(to, amount.saturating_neg());
        Ok(())
    }
}

/// One side of a transaction: who is told, and in what words.
struct Told {
    /// The agent told.
    agent: AgentKey,
    /// Its balance after the transaction.
    balance: LindenAmount,
    /// Whether the transaction succeeded.
    success: bool,
    /// The reply's description.
    description: String,
}

/// A transaction both sides are told of.
#[derive(Clone)]
struct Transaction {
    /// Its id.
    id: Uuid,
    /// Its type.
    kind: i32,
    /// Who paid.
    source: AgentKey,
    /// Who was paid, or the id the payer named.
    dest: OwnerKey,
    /// How much.
    amount: u64,
    /// The item description.
    item: String,
}

impl Transaction {
    /// The balance reply that tells `told` of it.
    fn reply(&self, told: Told) -> MoneyBalance {
        MoneyBalance {
            agent_id: told.agent,
            transaction_id: self.id,
            success: told.success,
            balance: told.balance,
            square_meters_credit: LandArea(0),
            square_meters_committed: LandArea(0),
            description: told.description,
            transaction: Some(MoneyTransaction {
                transaction_type: self.kind,
                source: OwnerKey::Agent(self.source),
                dest: self.dest,
                amount: LindenAmount(self.amount),
                item_description: self.item.clone(),
            }),
        }
    }
}

/// The per-session responder for money: answers its client's balance
/// requests, transfers and purchases, until the session closes or the grid
/// shuts down.
pub(crate) async fn run_money_responder(core: Arc<GridCore>, shared: SharedSim) {
    let agent = shared.state.lock().await.avatar.agent_id;
    let mut events = shared.subscribe_events();
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
            break;
        }
        let received = tokio::select! {
            received = events.recv() => received,
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    break;
                }
                continue;
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
        };
        match received {
            Ok(event) => answer(&core, &shared, agent, event).await,
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("money responder missed {missed} events");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Answers one thing the client did.
async fn answer(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, event: ServerEvent) {
    match event {
        ServerEvent::ClientMessage(message) => {
            if let sl_proto::AnyMessage::MoneyBalanceRequest(request) = *message {
                answer_balance(core, shared, agent, request.money_data.transaction_id).await;
            }
        }
        ServerEvent::MoneyTransferRequested(order) => {
            transfer(core, shared, agent, &order).await;
        }
        ServerEvent::BuyObject {
            category_id,
            objects,
            ..
        } => {
            let folder = InventoryFolderKey::from(category_id);
            for item in objects {
                buy(core, shared, agent, folder, &item).await;
            }
        }
        ServerEvent::BuyParcelPass { local_id } => buy_pass(core, shared, agent, local_id).await,
        ServerEvent::BuyObjectInventory { .. } if core.money_policy.refuses_contents_item => {
            let now = shared.now();
            let (key, text) = INVENTORY_NOT_FOR_SALE;
            shared
                .with_sim(|sim| keyed_alert(sim, text, key, String::new(), now))
                .await;
        }
        _other => {}
    }
}

/// Answers a `MoneyBalanceRequest` with the account's balance, or zero where
/// the grid keeps none.
async fn answer_balance(core: &GridCore, shared: &SharedSim, agent: AgentKey, transaction: Uuid) {
    let balance = MoneyBalance {
        agent_id: agent,
        transaction_id: transaction,
        success: true,
        balance: core.ledger.lock().stated(agent),
        square_meters_credit: LandArea(0),
        square_meters_committed: LandArea(0),
        description: String::new(),
        transaction: None,
    };
    let now = shared.now();
    shared
        .with_sim(|sim| warn_on(sim.send_money_balance_reply(&balance, now), "a balance"))
        .await;
}

/// Who a transfer pays.
enum Payee {
    /// An account.
    Agent(AgentKey),
    /// The owner of a prim of the payer's region, which is to hear it.
    Object {
        /// The prim's owner.
        owner: AgentKey,
        /// The prim.
        prim: ObjectKey,
    },
}

/// A `MoneyTransferRequest`: a gift to an account, or a payment to a prim of
/// the payer's region.
async fn transfer(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    order: &MoneyTransferOrder,
) {
    if core.ledger.lock().balance(agent).is_none() {
        // OpenSim's stock module drops every transfer.
        return;
    }
    if order.source_id != agent.uuid() || order.amount <= 0 {
        return;
    }
    let amount = i64::from(order.amount);
    let payee = if order.flags.contains(TransactionFlags::DEST_GROUP) {
        None
    } else {
        payee_of(core, shared, ObjectKey::from(order.dest_id), order.amount).await
    };
    let payee = match payee {
        Some(Ok(payee)) => payee,
        // A payment the prim's layout drops, or one it refuses with the alert.
        Some(Err(admission)) => {
            if admission == PaymentAdmission::Refused {
                let now = shared.now();
                shared
                    .with_sim(|sim| {
                        warn_on(
                            sim.send_alert_message(PAYMENT_STOPPED, &[], &[], now),
                            "a refused payment",
                        );
                    })
                    .await;
            }
            return;
        }
        None => {
            let failed = Transaction {
                id: core.minter.uuid(),
                kind: order.kind.to_i32(),
                source: agent,
                dest: OwnerKey::Agent(AgentKey::from(order.dest_id)),
                amount: u64::from(order.amount.unsigned_abs()),
                item: order.description.clone(),
            };
            tell_failure(core, &failed, NOBODY_TEXT).await;
            return;
        }
    };
    let (owner, prim, kind) = match payee {
        Payee::Agent(payee) => (payee, None, order.kind.to_i32()),
        Payee::Object { owner, prim } => (owner, Some(prim), PAY_OBJECT),
    };
    let transaction = Transaction {
        id: core.minter.uuid(),
        kind,
        source: agent,
        dest: OwnerKey::Agent(owner),
        amount: u64::from(order.amount.unsigned_abs()),
        item: order.description.clone(),
    };
    let moved = core.ledger.lock().transfer(agent, owner, amount);
    match moved {
        Ok(()) => {}
        Err(Refusal::InsufficientFunds) => {
            tell_failure(core, &transaction, INSUFFICIENT_TEXT).await;
            return;
        }
        Err(Refusal::NoLedger) => return,
    }
    let n = transaction.amount;
    tell_both(
        core,
        &transaction,
        format!("You paid {} L${n}.", legacy_name(core, owner)),
        format!("{} paid you L${n}.", legacy_name(core, agent)),
    )
    .await;
    if let Some(prim) = prim {
        let world = Arc::clone(&shared.state.lock().await.world);
        let mut world = world.lock();
        let payer = agent.uuid().to_string();
        for id in world.script_instances.scripts_in(prim) {
            if let Err(error) = world
                .script_instances
                .engine_mut()
                .paid(id, &payer, order.amount)
            {
                tracing::warn!(?id, %error, "raising money failed");
            }
        }
    }
}

/// Who `dest` is: an account, or a prim of the payer's region and whether its
/// layout takes `amount` — `None` for nobody.
async fn payee_of(
    core: &GridCore,
    shared: &SharedSim,
    dest: ObjectKey,
    amount: i32,
) -> Option<Result<Payee, PaymentAdmission>> {
    let world = Arc::clone(&shared.state.lock().await.world);
    let found = {
        let world = world.lock();
        world.local_id_of(dest).and_then(|local_id| {
            let owner = world.properties_of(local_id)?.owner;
            let layout = world
                .script_instances
                .pay_price(dest)
                .unwrap_or_else(PayPrice::default);
            Some((owner, layout))
        })
    };
    if let Some((owner, layout)) = found {
        // An object a group owns is paid to nobody this grid keeps money for.
        let OwnerKey::Agent(owner) = owner else {
            return None;
        };
        return Some(match layout.takes(amount) {
            PaymentAdmission::Taken => Ok(Payee::Object { owner, prim: dest }),
            other => Err(other),
        });
    }
    let agent = AgentKey::from(dest.uuid());
    core.account_by_agent(agent)
        .map(|_account| Ok(Payee::Agent(agent)))
}

/// Tells the payer of `transaction` that it failed, in `text`.
async fn tell_failure(core: &GridCore, transaction: &Transaction, text: &str) {
    let payer = transaction.source;
    let reply = transaction.reply(Told {
        agent: payer,
        balance: core.ledger.lock().stated(payer),
        success: false,
        description: text.to_owned(),
    });
    let _told = tell(core, payer, |state, now| {
        warn_on(
            state.sim.send_money_balance_reply(&reply, now),
            "a failed transfer",
        );
    })
    .await;
}

/// Tells both sides of `transaction` what it did: the payee first, then the
/// payer — one account twice when it paid itself.
async fn tell_both(
    core: &GridCore,
    transaction: &Transaction,
    payer_text: String,
    payee_text: String,
) {
    let payee = AgentKey::from(transaction.dest.uuid());
    let (payee_reply, payer_reply) = {
        let ledger = core.ledger.lock();
        (
            transaction.reply(Told {
                agent: payee,
                balance: ledger.stated(payee),
                success: true,
                description: payee_text,
            }),
            transaction.reply(Told {
                agent: transaction.source,
                balance: ledger.stated(transaction.source),
                success: true,
                description: payer_text,
            }),
        )
    };
    let _told = tell(core, payee, |state, now| {
        warn_on(
            state.sim.send_money_balance_reply(&payee_reply, now),
            "a payment",
        );
    })
    .await;
    let _told = tell(core, transaction.source, |state, now| {
        warn_on(
            state.sim.send_money_balance_reply(&payer_reply, now),
            "a payment",
        );
    })
    .await;
}

/// A pass to the parcel `local_id`: sold as the parcel's rules say
/// ([`crate::land_sales::sell_pass`]), a timed entry on its allow list
/// replacing the buyer's own; where the grid keeps balances the pass price
/// paid to the parcel's owner first, the buyer told by a balance reply of type
/// `5006` (`book/src/gridspec/land.md` § Transactions). A pass the buyer
/// cannot pay for is not sold (unmeasured: the buyer is told as for any
/// transfer it does not hold).
async fn buy_pass(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    local_id: sl_proto::RegionLocalParcelId,
) {
    let sold = shared
        .with_state(|state| {
            let now = shared.now();
            let mut world = state.world.lock();
            let parcel = world.parcel_by_local_id(local_id).cloned()?;
            let allowed = world
                .access_list(local_id, sl_proto::ParcelAccessScope::Access)
                .to_vec();
            let answer = crate::land_sales::sell_pass(
                &parcel,
                &allowed,
                agent,
                world.wall.unix_seconds_i32(),
                core.parcel_policy,
            );
            let (entry, told) = match answer {
                crate::land_sales::PassAnswer::Sold { entry, told } => (entry, told),
                crate::land_sales::PassAnswer::Refused(told) => {
                    drop(world);
                    if let Some(told) = told {
                        warn_on(
                            state.sim.send_agent_alert_message(agent, false, &told, now),
                            "a refused pass",
                        );
                    }
                    return None;
                }
            };
            let price = parcel.pass_price.0;
            let transaction = Transaction {
                id: core.minter.uuid(),
                kind: LAND_PASS_SALE,
                source: agent,
                dest: parcel.owner,
                amount: price,
                item: String::new(),
            };
            let paid = core.ledger.lock().pay(
                agent,
                parcel.owner,
                i64::try_from(price).unwrap_or(i64::MAX),
            );
            if paid == Err(Refusal::InsufficientFunds) {
                drop(world);
                let reply = transaction.reply(Told {
                    agent,
                    balance: core.ledger.lock().stated(agent),
                    success: false,
                    description: INSUFFICIENT_TEXT.to_owned(),
                });
                warn_on(
                    state.sim.send_money_balance_reply(&reply, now),
                    "a refused pass",
                );
                return None;
            }
            let held = world.access_list_mut(local_id, sl_proto::ParcelAccessScope::Access);
            held.retain(|kept| kept.id != entry.id);
            held.push(entry);
            drop(world);
            if let Some(told) = told {
                warn_on(
                    state.sim.send_alert_message(told, &[], &[], now),
                    "a sold pass",
                );
            }
            (paid == Ok(())).then_some(transaction)
        })
        .await;
    let Some(transaction) = sold else {
        return;
    };
    let owner = match transaction.dest {
        OwnerKey::Agent(owner) => legacy_name(core, owner),
        OwnerKey::Group(group) => core
            .groups
            .lock()
            .name_of(group)
            .map_or_else(String::new, ToOwned::to_owned),
    };
    let reply = transaction.reply(Told {
        agent,
        balance: core.ledger.lock().stated(agent),
        success: true,
        description: format!(
            "You paid {owner} L${} for a land access pass.",
            transaction.amount
        ),
    });
    let _told = tell(core, agent, |state, now| {
        warn_on(
            state.sim.send_money_balance_reply(&reply, now),
            "a sold pass",
        );
    })
    .await;
}

/// What a script's payment did ([`pay_from_object`]).
#[derive(Debug)]
pub(crate) enum ObjectPayment {
    /// Moved: tell both sides with these replies, the payee's first.
    Moved(Box<(MoneyBalance, MoneyBalance)>),
    /// Refused: tell the owner with this failed reply.
    Refused(Box<MoneyBalance>, sl_lsl_runtime::vm::MoneyRefusal),
    /// The grid keeps no balances: nothing moved, nobody is told.
    Unkept,
}

/// A script's payment (`llGiveMoney`, `llTransferLindenDollars`): `owner`, the
/// prim's owner, pays `payee` `amount` L$, the prim's name `item` as the
/// item — or, for a payee that is no account, nothing moves and the owner is
/// told of the failure (`book/src/gridspec/lsl.md` § Giving money).
pub(crate) fn pay_from_object(
    core: &GridCore,
    owner: AgentKey,
    payee: Uuid,
    amount: i32,
    item: &str,
) -> ObjectPayment {
    let transaction = Transaction {
        id: core.minter.uuid(),
        kind: OBJECT_PAYS,
        source: owner,
        dest: OwnerKey::Agent(AgentKey::from(payee)),
        amount: u64::from(amount.unsigned_abs()),
        item: item.to_owned(),
    };
    let payee_agent = AgentKey::from(payee);
    let known = core.account_by_agent(payee_agent).is_some();
    let failed = |text: &str, balance: LindenAmount| {
        transaction.reply(Told {
            agent: owner,
            balance,
            success: false,
            description: text.to_owned(),
        })
    };
    let moved = {
        let mut ledger = core.ledger.lock();
        if ledger.balance(owner).is_none() {
            Err(Refusal::NoLedger)
        } else if known {
            ledger.transfer(owner, payee_agent, i64::from(amount))
        } else {
            Ok(())
        }
    };
    let stated = |agent: AgentKey| core.ledger.lock().stated(agent);
    match moved {
        Ok(()) if !known => {
            return ObjectPayment::Refused(
                Box::new(failed(NOBODY_TEXT, stated(owner))),
                sl_lsl_runtime::vm::MoneyRefusal::NoSuchEntity,
            );
        }
        Ok(()) => {}
        Err(Refusal::InsufficientFunds) => {
            return ObjectPayment::Refused(
                Box::new(failed(INSUFFICIENT_TEXT, stated(owner))),
                sl_lsl_runtime::vm::MoneyRefusal::InsufficientFunds,
            );
        }
        Err(Refusal::NoLedger) => return ObjectPayment::Unkept,
    }
    let n = transaction.amount;
    let payee_reply = transaction.reply(Told {
        agent: payee_agent,
        balance: stated(payee_agent),
        success: true,
        description: format!("{} paid you L${n}.", legacy_name(core, owner)),
    });
    let owner_reply = transaction.reply(Told {
        agent: owner,
        balance: stated(owner),
        success: true,
        description: format!("You paid {} L${n}.", legacy_name(core, payee_agent)),
    });
    ObjectPayment::Moved(Box::new((payee_reply, owner_reply)))
}

/// Why a purchase was refused, and how the buyer is told.
enum Refused {
    /// Nothing is said.
    Silently,
    /// A plain alert.
    Alert(String),
    /// An agent alert.
    AgentAlert(String),
    /// A message box from nobody.
    MessageBox(&'static str),
}

/// What a purchase is of, read off the object while the region is held.
struct Ware {
    /// The linkset's root.
    root: RegionLocalObjectId,
    /// Its name.
    name: String,
    /// Who sells it.
    seller: AgentKey,
    /// How it is sold.
    sale_type: SaleType,
}

/// Judges a purchase of `item` against the object it names, as `policy` does.
fn judge(world: &SceneFixtures, item: &ObjectBuyItem, policy: Purchases) -> Result<Ware, Refused> {
    let offered = item.sale_price.0;
    if policy == Purchases::FreeOnly && offered != 0 {
        return Err(Refused::MessageBox(OPENSIM_PRICED_PURCHASE_TEXT));
    }
    let root = world.root_of(item.local_id);
    if policy == Purchases::FreeOnly && root != item.local_id {
        // OpenSim ignores a purchase of a child prim without a word.
        return Err(Refused::Silently);
    }
    let properties = world.properties_of(root).ok_or(Refused::Silently)?;
    let name = properties.name.clone();
    let sale_type = SaleType::from_code(properties.sale_type);
    let price = properties.sale_price.as_ref().map_or(0, |price| price.0);
    let wrong_type = sale_type != item.sale_type;
    let wrong_price = price != offered;
    match policy {
        Purchases::Charged => {
            if sale_type == SaleType::NotForSale {
                return Err(Refused::Silently);
            }
            if wrong_type {
                return Err(Refused::Alert(format!(
                    "Sale of {name} cancelled.  Sell type (copy,original,contents) does not match \
                     expected."
                )));
            }
            if wrong_price {
                return Err(Refused::Alert(format!(
                    "Sale of {name} cancelled.  Price does not match expected."
                )));
            }
        }
        Purchases::FreeOnly => {
            if sale_type == SaleType::NotForSale {
                return Err(Refused::AgentAlert(format!(
                    "/Object {name} is not for sale"
                )));
            }
            if wrong_price {
                return Err(Refused::AgentAlert(format!(
                    "/Object {name} price does not match selected price"
                )));
            }
            if wrong_type {
                return Err(Refused::AgentAlert(format!(
                    "/Object {name} sell type does not match selected type"
                )));
            }
        }
    }
    // An object a group owns is sold by nobody this grid keeps money for.
    let OwnerKey::Agent(seller) = properties.owner else {
        return Err(Refused::Silently);
    };
    Ok(Ware {
        root,
        name,
        seller,
        sale_type,
    })
}

/// Tells the buyer why its purchase was refused.
fn tell_refusal(sim: &mut SimSession, agent: AgentKey, refused: &Refused, now: Instant) {
    match refused {
        Refused::Silently => {}
        Refused::Alert(text) => warn_on(
            sim.send_alert_message(text, &[], &[], now),
            "a refused purchase",
        ),
        Refused::AgentAlert(text) => warn_on(
            sim.send_agent_alert_message(agent, false, text, now),
            "a refused purchase",
        ),
        Refused::MessageBox(text) => {
            let message = InstantMessage {
                from_agent_id: AgentKey::from(Uuid::nil()),
                from_agent_name: String::new(),
                to_agent_id: agent,
                dialog: ImDialog::MessageBox,
                from_group: false,
                region_id: None,
                position: sl_types::map::RegionCoordinates::new(0.0, 0.0, 0.0),
                offline: false,
                timestamp: None,
                id: Uuid::nil(),
                parent_estate_id: 0,
                message: (*text).to_owned(),
                binary_bucket: Vec::new(),
            };
            warn_on(
                sim.send_instant_message(&message, now),
                "a refused purchase",
            );
        }
    }
}

/// Sends a keyed alert: `text`, keyed `key` with `extra` as its parameters.
fn keyed_alert(sim: &mut SimSession, text: &str, key: &str, extra: String, now: Instant) {
    let info = [AlertInfo {
        message: key.to_owned(),
        extra_params: extra,
    }];
    warn_on(sim.send_alert_message(text, &info, &[], now), "an alert");
}

/// `masks` once `next` is applied for a new owner: the base and owner masks
/// cut to the next owner's, nothing for the group, and everyone's cut too.
fn for_next_owner(masks: &mut sl_wire::Permissions5) {
    let next = masks.next_owner;
    let cut = |mask: Permissions| Permissions::from_bits(mask.bits() & next.bits());
    masks.base = cut(masks.base);
    masks.owner = cut(masks.owner);
    masks.group = Permissions::NONE;
    masks.everyone = cut(masks.everyone);
}

/// A purchase of one object: judged, paid for where the grid charges, and
/// handed over as its kind of sale says.
async fn buy(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    folder: InventoryFolderKey,
    item: &ObjectBuyItem,
) {
    let policy = core.money_policy;
    let world = Arc::clone(&shared.state.lock().await.world);
    let judged = judge(&world.lock(), item, policy.purchases);
    let ware = match judged {
        Ok(ware) => ware,
        Err(refused) => {
            let now = shared.now();
            shared
                .with_sim(|sim| tell_refusal(sim, agent, &refused, now))
                .await;
            return;
        }
    };
    let price = item.sale_price.0;
    let charged = policy.purchases == Purchases::Charged;
    let transaction = Transaction {
        id: core.minter.uuid(),
        kind: OBJECT_SALE,
        source: agent,
        dest: OwnerKey::Agent(ware.seller),
        amount: price,
        item: ware.name.clone(),
    };
    if charged {
        let moved = core.ledger.lock().transfer(
            agent,
            ware.seller,
            i64::try_from(price).unwrap_or(i64::MAX),
        );
        match moved {
            Ok(()) | Err(Refusal::NoLedger) => {}
            Err(Refusal::InsufficientFunds) => {
                tell_failure(core, &transaction, INSUFFICIENT_TEXT).await;
                return;
            }
        }
    }
    match ware.sale_type {
        SaleType::Copy => sell_copy(core, shared, agent, folder, &ware).await,
        SaleType::Contents => sell_contents(core, shared, agent, folder, &ware).await,
        SaleType::Original => sell_original(core, shared, agent, &ware).await,
        _not_for_sale => {}
    }
    if charged {
        let seller_text = if price == 0 {
            String::new()
        } else {
            format!(
                "{} paid you L${price} for {}.",
                legacy_name(core, agent),
                ware.name
            )
        };
        tell_both(
            core,
            &transaction,
            format!(
                "You paid {} L${price} for {}.",
                legacy_name(core, ware.seller),
                ware.name
            ),
            seller_text,
        )
        .await;
    }
}

/// Hands the buyer a copy of the object: an item in `folder` with the next
/// owner's masks, backed by the linkset as it stands.
async fn sell_copy(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    folder: InventoryFolderKey,
    ware: &Ware,
) {
    let minter = core.minter.clone();
    let mint = move || minter.uuid();
    shared
        .with_state(|state| {
            let now = shared.now();
            let filed = crate::world::file_linkset_copy(
                &mut state.world.lock(),
                &state.assets,
                ware.root,
                (folder, agent),
                state.object_assets,
                &mint,
                now,
            );
            let Some(mut item) = filed else {
                return;
            };
            item.last_owner_id = ware.seller.uuid();
            for_next_owner(&mut item.permissions);
            warn_on(
                state.sim.send_inventory_item_created(
                    std::slice::from_ref(&item),
                    TransactionId::from(Uuid::nil()),
                    true,
                    now,
                ),
                "a bought copy",
            );
            state.sim.agent_inventory_mut().insert_item(item);
        })
        .await;
}

/// Hands the buyer the object's contents: a new folder in `folder` named
/// after the object, holding a copy of each item of the root with the next
/// owner's masks.
async fn sell_contents(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    folder: InventoryFolderKey,
    ware: &Ware,
) {
    let new_folder = InventoryFolder {
        folder_id: InventoryFolderKey::from(core.minter.uuid()),
        parent_id: Some(folder),
        name: ware.name.clone(),
        folder_type: -1,
        version: 1,
    };
    let minter = core.minter.clone();
    let mint = move || minter.uuid();
    let purchases = core.money_policy.purchases;
    shared
        .with_state(|state| {
            let now = shared.now();
            let held: Vec<_> = state
                .world
                .lock()
                .objects
                .contents(ware.root)
                .map(|contents| contents.items.clone())
                .unwrap_or_default();
            let items: Vec<InventoryItem> = held
                .iter()
                .map(|held| {
                    let mut item =
                        crate::task_contents::agent_item_from(held, new_folder.folder_id, &mint);
                    item.owner = OwnerKey::Agent(agent);
                    item.last_owner_id = ware.seller.uuid();
                    for_next_owner(&mut item.permissions);
                    item
                })
                .collect();
            let inventory = state.sim.agent_inventory_mut();
            inventory.insert_folder(new_folder.clone());
            for item in items {
                inventory.insert_item(item);
            }
            match purchases {
                Purchases::Charged => warn_on(
                    state.sim.enqueue_bulk_update_inventory(
                        TransactionId::from(Uuid::nil()),
                        std::slice::from_ref(&new_folder),
                        &[],
                    ),
                    "bought contents",
                ),
                Purchases::FreeOnly => {
                    let root = state
                        .sim
                        .agent_inventory()
                        .folders()
                        .find(|folder| folder.parent_id.is_none())
                        .map(|folder| folder.folder_id);
                    for shown in root.into_iter().chain([new_folder.folder_id]) {
                        warn_on(
                            state
                                .sim
                                .send_inventory_descendents(shown, agent, 0, true, true, now)
                                .map(|_known| ()),
                            "bought contents",
                        );
                    }
                }
            }
        })
        .await;
}

/// Hands the buyer the object itself: its owner, every prim's next owner's
/// masks applied and no longer for sale, the seller its last owner — told to
/// the region, and to the buyer by alert where the grid says so.
async fn sell_original(core: &GridCore, shared: &SharedSim, agent: AgentKey, ware: &Ware) {
    let policy = core.money_policy;
    shared
        .with_region(|world, sim, now| {
            let prims: Vec<RegionLocalObjectId> = std::iter::once(ware.root)
                .chain(crate::link_sets::children_in_order(world, ware.root))
                .collect();
            let mut changes = Vec::new();
            for local_id in &prims {
                crate::object_edits::edit_properties(world, *local_id, None, |properties| {
                    properties.owner = OwnerKey::Agent(agent);
                    properties.last_owner_id = ware.seller.uuid();
                    properties.sale_type = SaleType::NotForSale.to_code();
                    properties.sale_price = Some(LindenAmount(0));
                    for_next_owner(&mut properties.permissions);
                });
                if let Some(mut object) = world.object_mut(*local_id) {
                    object.owner_id = agent.uuid();
                }
                if let Some(object) = world.object_by_local_id(*local_id) {
                    crate::object_edits::push_object(sim, &object, now);
                    changes.push(RegionChange::Updated(Box::new(object)));
                }
            }
            if let Some(record) = world.record_of(ware.root) {
                crate::object_edits::push_properties(sim, &record, now);
            }
            for _raised in 0..policy.owner_changed_raised {
                crate::scripts::raise_changed(world, ware.root, CHANGED_OWNER);
            }
            if policy.purchases == Purchases::Charged {
                let name = &ware.name;
                keyed_alert(
                    sim,
                    &format!("You are now the owner of object {name}"),
                    NOW_OWN_OBJECT,
                    format!(
                        "<? LLSD/XML ?>\n<llsd><map><key>OBJECT_NAME</key><string>{}</string></map></llsd>\n",
                        xml_escaped(name)
                    ),
                    now,
                );
            }
            ((), changes)
        })
        .await;
}

/// `text` with the five XML metacharacters escaped.
fn xml_escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::AgentKey;
    use uuid::Uuid;

    use super::{Ledger, Refusal};

    /// Two accounts.
    fn accounts() -> (AgentKey, AgentKey) {
        (
            AgentKey::from(Uuid::from_u128(1)),
            AgentKey::from(Uuid::from_u128(2)),
        )
    }

    #[test]
    fn a_transfer_moves_what_the_payer_holds() {
        let (a, b) = accounts();
        let mut ledger = Ledger::new(Some(10));
        assert_eq!(ledger.transfer(a, b, 4), Ok(()));
        assert_eq!((ledger.balance(a), ledger.balance(b)), (Some(6), Some(14)));
    }

    #[test]
    fn a_transfer_of_more_than_is_held_moves_nothing() {
        let (a, b) = accounts();
        let mut ledger = Ledger::new(Some(10));
        assert_eq!(ledger.transfer(a, b, 11), Err(Refusal::InsufficientFunds));
        assert_eq!((ledger.balance(a), ledger.balance(b)), (Some(10), Some(10)));
    }

    #[test]
    fn a_transfer_to_oneself_changes_nothing() {
        let (a, _b) = accounts();
        let mut ledger = Ledger::new(Some(10));
        assert_eq!(ledger.transfer(a, a, 3), Ok(()));
        assert_eq!(ledger.balance(a), Some(10));
    }

    #[test]
    fn a_grid_without_balances_moves_nothing() {
        let (a, b) = accounts();
        let mut ledger = Ledger::new(None);
        assert_eq!(ledger.transfer(a, b, 1), Err(Refusal::NoLedger));
        assert_eq!(ledger.balance(a), None);
        assert_eq!(ledger.stated(a).0, 0);
    }

    #[test]
    fn a_charge_is_taken_from_the_balance() {
        let (a, _b) = accounts();
        let mut ledger = Ledger::new(Some(10));
        ledger.charge(a, 3);
        assert_eq!(ledger.balance(a), Some(7));
    }
}
