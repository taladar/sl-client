//! The watchdog that puts back what a lock says may not come off.
//!
//! A lock is only a rule; the simulator does not know about it. A user can
//! still right-click a locked attachment and detach it, and a wear can still
//! land on a point that `@addattach=n` closed. What makes the lock real is that
//! the viewer notices and undoes it — the reference's
//! `RlvAttachmentLockWatchdog` (`rlvlocks.cpp:492`).
//!
//! There are three things to undo, and they are why this is a state machine
//! rather than a predicate:
//!
//! - a **locked attachment came off**: remember the item and put it back on.
//!   Not immediately — the simulator has to save the attachment's asset back
//!   into inventory first, and re-attaching before that gets an old version.
//!   So the item waits, is re-attached when the save arrives, and is forced
//!   after [`ASSET_SAVE_TIMEOUT_SECONDS`] if it never does;
//! - a wear landed on an **add-locked point**: put that point back exactly as
//!   it was, which means knowing what was on it *before the wear was asked
//!   for* — hence [`RlvAttachmentWatchdog::on_wear_requested`], which is the
//!   only reason this machine has to be told about a wear at all;
//! - a **replace** landed on a point holding something locked: either detach
//!   just the new arrival, or — with `RLVaWearReplaceUnlocked` on — let it
//!   replace only the unlocked attachments and leave the locked ones be.
//!
//! Like the rest of the crate this decides and does not act:
//! [`RlvWatchdogAction`] says what to send, and the consumer sends it. Time
//! comes in as a plain seconds count on every call, so the whole machine is
//! testable without a clock.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::locks::{RlvLockSource, RlvLocks, RlvWornAttachment};
use crate::query::RlvAttachmentPoint;

/// How often the reference runs [`RlvAttachmentWatchdog::tick`] while anything
/// is pending (`RlvAttachmentLockWatchdogTimer`, `rlvlocks.h:211`).
pub const TICK_INTERVAL_SECONDS: f64 = 10.0;

/// How long a re-attach waits for the simulator to save the asset back into
/// inventory before giving up and attaching the old version anyway
/// (`rlvlocks.cpp:751`).
pub const ASSET_SAVE_TIMEOUT_SECONDS: f64 = 15.0;

/// How long before a re-attach that did not take is asked for again
/// (`rlvlocks.cpp:757`).
pub const REATTACH_RETRY_SECONDS: f64 = 30.0;

/// How long a wear request is remembered before it is assumed to have failed
/// (`rlvlocks.cpp:726`).
pub const WEAR_TIMEOUT_SECONDS: f64 = 60.0;

/// How a wear was asked for (`ERlvWearMask` narrowed to the two the user can
/// actually pick).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvWearAction {
    /// Wear it alongside whatever is already on the point.
    Add,
    /// Wear it *instead of* whatever is already on the point.
    Replace,
}

/// Something the consumer has to do to make a lock stick.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvWatchdogAction {
    /// Send `ObjectDetach` for these attachment roots. They are detached
    /// together, as one message, because the reference packs them into one.
    Detach(Vec<Uuid>),
    /// Attach this inventory item at this point, replacing nothing.
    Attach {
        /// The item to put back on.
        item: Uuid,
        /// Where it belongs.
        point: RlvAttachmentPoint,
    },
}

/// What the watchdog made of an attach or detach it was told about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvWatchdogOutcome {
    /// Whether the change is allowed to stand.
    ///
    /// This is what `@notify` reports as the `attached` / `detached` half of
    /// its line (`RlvBehaviourNotifyHandler::onDetach`, `rlvhelper.cpp:1936`),
    /// so a script hears "you tried and it did not take".
    pub allowed: bool,
    /// What to send to make that true.
    pub actions: Vec<RlvWatchdogAction>,
}

impl RlvWatchdogOutcome {
    /// Nothing to do, and the change stands.
    const fn allowed() -> Self {
        Self {
            allowed: true,
            actions: Vec::new(),
        }
    }
}

/// One attachment waiting to be put back (`RlvReattachInfo`, `rlvlocks.h:180`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reattach {
    /// The inventory item to attach.
    item: Uuid,
    /// Whether the simulator has saved the asset back into inventory.
    asset_saved: bool,
    /// When the attachment came off.
    detached_at: f64,
    /// When the last attach request went out; zero until one has.
    attached_at: f64,
}

/// One wear the user asked for, and what the locked points looked like then
/// (`RlvWearInfo`, `rlvlocks.h:190`).
#[derive(Debug, Clone, PartialEq)]
struct PendingWear {
    /// Whether the wear replaces or adds.
    action: RlvWearAction,
    /// When it was asked for.
    requested_at: f64,
    /// For each **add-locked** point, the items that were on it at the time.
    ///
    /// Only add-locked points are recorded: those are the ones whose contents
    /// have to be restored exactly, and remembering the rest would be so much
    /// bookkeeping nobody reads.
    points: BTreeMap<RlvAttachmentPoint, Vec<Uuid>>,
}

/// The re-attach watchdog.
///
/// Feed it every attach and detach the viewer sees, every wear the user asks
/// for, and a [`RlvAttachmentWatchdog::tick`] while
/// [`RlvAttachmentWatchdog::is_idle`] is false.
#[derive(Debug, Clone, Default)]
pub struct RlvAttachmentWatchdog {
    /// Items this watchdog asked to have detached, so their detach is not
    /// mistaken for the user fighting a lock.
    pending_detach: Vec<Uuid>,
    /// Attachments waiting to be put back, by the point they belong on.
    pending_attach: Vec<(RlvAttachmentPoint, Reattach)>,
    /// Wears asked for but not yet landed, by item.
    pending_wear: BTreeMap<Uuid, PendingWear>,
    /// Whether `RLVaWearReplaceUnlocked` is on: a replace onto a point holding
    /// something locked replaces only the *unlocked* attachments instead of
    /// being refused outright.
    wear_replace_unlocked: bool,
}

impl RlvAttachmentWatchdog {
    /// A watchdog with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `RLVaWearReplaceUnlocked`.
    ///
    /// Off (the default), a replace onto a point holding a locked attachment is
    /// refused and the new arrival is detached again. On, the new attachment
    /// stays and replaces only what was free to go.
    pub const fn set_wear_replace_unlocked(&mut self, enabled: bool) {
        self.wear_replace_unlocked = enabled;
    }

    /// Whether nothing is pending, so the consumer can stop ticking
    /// (`RlvAttachmentLockWatchdog::onTimer`'s return value,
    /// `rlvlocks.cpp:766`).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.pending_detach.is_empty()
            && self.pending_attach.is_empty()
            && self.pending_wear.is_empty()
    }

    /// The user asked to wear `item`.
    ///
    /// Nothing is decided here — where the item will land is not known until
    /// the simulator says so. What is recorded is the state of every add-locked
    /// point *now*, so that if the wear lands on one of them
    /// [`RlvAttachmentWatchdog::on_attach`] can put it back exactly as it was.
    /// With no attachment-point lock in force there is nothing to protect and
    /// nothing is recorded.
    pub fn on_wear_requested(
        &mut self,
        item: Uuid,
        action: RlvWearAction,
        now: f64,
        locks: &RlvLocks,
        source: &impl RlvLockSource,
    ) {
        if !locks.has_point_lock(crate::locks::RlvLockKind::Add)
            && !locks.has_point_lock(crate::locks::RlvLockKind::Remove)
        {
            return;
        }
        let mut points = BTreeMap::new();
        for point in RlvAttachmentPoint::all() {
            if !locks.is_point_locked(point, crate::locks::RlvLockKind::Add) {
                continue;
            }
            let worn = source
                .attachments_at(point)
                .into_iter()
                .filter_map(|attachment| attachment.item)
                // An attachment already on its way off is not part of the
                // state we are preserving.
                .filter(|item| !self.pending_detach.contains(item))
                .collect();
            points.insert(point, worn);
        }
        self.pending_wear.insert(
            item,
            PendingWear {
                action,
                requested_at: now,
                points,
            },
        );
    }

    /// An attachment appeared on the avatar.
    pub fn on_attach(
        &mut self,
        attachment: &RlvWornAttachment,
        now: f64,
        locks: &RlvLocks,
        source: &impl RlvLockSource,
    ) -> RlvWatchdogOutcome {
        let Some(item) = attachment.item else {
            return RlvWatchdogOutcome::allowed();
        };
        let point = attachment.point;

        // Was this point waiting for something to come back?
        if self.pending_attach.iter().any(|&(at, _)| at == point) {
            if let Some(index) = self
                .pending_attach
                .iter()
                .position(|&(at, ref pending)| at == point && pending.item == item)
            {
                self.pending_attach.remove(index);
                return RlvWatchdogOutcome::allowed();
            }
            // Something *else* arrived on a point we are still restoring, so it
            // goes straight back off.
            return RlvWatchdogOutcome {
                allowed: false,
                actions: vec![self.detach(core::slice::from_ref(attachment))],
            };
        }

        let Some(wear) = self.pending_wear.remove(&item) else {
            return RlvWatchdogOutcome::allowed();
        };
        match wear.points.get(&point) {
            Some(previous) => self.restore_add_locked_point(attachment, previous, now, source),
            None if wear.action == RlvWearAction::Replace => {
                self.finish_replace(attachment, locks, source)
            }
            None => RlvWatchdogOutcome::allowed(),
        }
    }

    /// A wear landed on an add-locked point: put the point back to `previous`.
    ///
    /// If the item was already there before the wear then nothing was gained
    /// and nothing is undone — the user re-wore what they were already wearing.
    fn restore_add_locked_point(
        &mut self,
        attachment: &RlvWornAttachment,
        previous: &[Uuid],
        now: f64,
        source: &impl RlvLockSource,
    ) -> RlvWatchdogOutcome {
        if attachment.item.is_some_and(|item| previous.contains(&item)) {
            return RlvWatchdogOutcome::allowed();
        }
        let point = attachment.point;
        let worn = source.attachments_at(point);

        if previous.is_empty() {
            // The point was empty, so everything on it now is unwanted.
            return RlvWatchdogOutcome {
                allowed: false,
                actions: vec![self.detach(&worn)],
            };
        }

        // Detach whatever was not there before; whatever *was* there and is
        // gone now has to come back.
        let mut missing: Vec<Uuid> = previous.to_vec();
        let mut unwanted = Vec::new();
        for attachment in worn {
            match attachment
                .item
                .and_then(|item| missing.iter().position(|&was| was == item))
            {
                Some(index) => {
                    missing.remove(index);
                }
                None => unwanted.push(attachment),
            }
        }
        let mut actions = Vec::new();
        if !unwanted.is_empty() {
            actions.push(self.detach(&unwanted));
        }
        for item in missing {
            self.queue_reattach(point, item, now);
        }
        RlvWatchdogOutcome {
            allowed: false,
            actions,
        }
    }

    /// A replace landed: work out whether it may actually displace what is on
    /// the point (`rlvlocks.cpp:619`).
    fn finish_replace(
        &mut self,
        attachment: &RlvWornAttachment,
        locks: &RlvLocks,
        source: &impl RlvLockSource,
    ) -> RlvWatchdogOutcome {
        let mut keep = Vec::new();
        let mut allowed = true;
        for worn in source.attachments_at(attachment.point) {
            if worn.root == attachment.root || !locks.is_attachment_locked(&worn, None, source) {
                continue;
            }
            if self.wear_replace_unlocked {
                keep.push(worn.root);
            } else {
                allowed = false;
                break;
            }
        }

        if !allowed {
            // Refused: the new arrival goes back off and everything stays.
            return RlvWatchdogOutcome {
                allowed: false,
                actions: vec![self.detach(core::slice::from_ref(attachment))],
            };
        }

        keep.push(attachment.root);
        let displaced: Vec<RlvWornAttachment> = source
            .attachments_at(attachment.point)
            .into_iter()
            .filter(|worn| !keep.contains(&worn.root))
            .collect();
        let actions = if displaced.is_empty() {
            Vec::new()
        } else {
            vec![self.detach(&displaced)]
        };
        RlvWatchdogOutcome { allowed, actions }
    }

    /// An attachment came off the avatar.
    ///
    /// A detach this watchdog asked for is simply ticked off. One it did not
    /// ask for, of something a lock holds on, is queued to go straight back —
    /// which is what makes `@detach=n` mean anything at all.
    pub fn on_detach(
        &mut self,
        attachment: &RlvWornAttachment,
        now: f64,
        locks: &RlvLocks,
        source: &impl RlvLockSource,
    ) -> RlvWatchdogOutcome {
        let Some(item) = attachment.item else {
            return RlvWatchdogOutcome::allowed();
        };
        if let Some(index) = self
            .pending_detach
            .iter()
            .position(|&pending| pending == item)
        {
            self.pending_detach.remove(index);
            return RlvWatchdogOutcome::allowed();
        }
        if !locks.is_attachment_locked(attachment, None, source) {
            return RlvWatchdogOutcome::allowed();
        }
        self.queue_reattach(attachment.point, item, now);
        RlvWatchdogOutcome {
            allowed: false,
            actions: Vec::new(),
        }
    }

    /// The simulator saved `item`'s asset back into inventory, so a re-attach
    /// waiting on it can go out now (`onSavedAssetIntoInventory`,
    /// `rlvlocks.cpp:706`).
    ///
    /// The reference does *not* mark the entry as saved here, only stamps the
    /// attempt — so a re-attach that this triggers is still eligible for the
    /// [`ASSET_SAVE_TIMEOUT_SECONDS`] forced attempt later. That is reproduced
    /// rather than tidied: an extra attach request is harmless, and a script
    /// timing the sequence sees what it has always seen.
    pub fn on_asset_saved(&mut self, item: Uuid, now: f64) -> Vec<RlvWatchdogAction> {
        let mut actions = Vec::new();
        for &mut (point, ref mut pending) in &mut self.pending_attach {
            if pending.asset_saved || pending.item != item {
                continue;
            }
            pending.attached_at = now;
            actions.push(RlvWatchdogAction::Attach { item, point });
        }
        actions
    }

    /// Time passed: retry what is owed and forget what has gone stale.
    ///
    /// `in_inventory` says whether an item is still there to be attached; one
    /// that has been deleted is dropped rather than retried forever.
    pub fn tick(
        &mut self,
        now: f64,
        in_inventory: impl Fn(Uuid) -> bool,
    ) -> Vec<RlvWatchdogAction> {
        self.pending_wear
            .retain(|_, wear| wear.requested_at + WEAR_TIMEOUT_SECONDS >= now);

        let mut actions = Vec::new();
        self.pending_attach
            .retain(|&(_, pending)| in_inventory(pending.item));
        for &mut (point, ref mut pending) in &mut self.pending_attach {
            let force =
                !pending.asset_saved && pending.detached_at + ASSET_SAVE_TIMEOUT_SECONDS < now;
            let retry = pending.asset_saved && pending.attached_at + REATTACH_RETRY_SECONDS < now;
            if force {
                // Give up on the save and put back what we have.
                pending.asset_saved = true;
            } else if !retry {
                continue;
            }
            pending.attached_at = now;
            actions.push(RlvWatchdogAction::Attach {
                item: pending.item,
                point,
            });
        }
        actions
    }

    /// Queue `item` to go back on `point`, unless it already is queued.
    fn queue_reattach(&mut self, point: RlvAttachmentPoint, item: Uuid, now: f64) {
        if self
            .pending_attach
            .iter()
            .any(|&(at, ref pending)| at == point && pending.item == item)
        {
            return;
        }
        self.pending_attach.push((
            point,
            Reattach {
                item,
                asset_saved: false,
                detached_at: now,
                attached_at: 0.0,
            },
        ));
    }

    /// Ask for `attachments` to be detached, remembering that we asked so their
    /// detach is not read as the user fighting a lock.
    fn detach(&mut self, attachments: &[RlvWornAttachment]) -> RlvWatchdogAction {
        for attachment in attachments {
            if let Some(item) = attachment.item
                && !self.pending_detach.contains(&item)
            {
                self.pending_detach.push(item);
            }
        }
        RlvWatchdogAction::Detach(
            attachments
                .iter()
                .map(|attachment| attachment.root)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        ASSET_SAVE_TIMEOUT_SECONDS, REATTACH_RETRY_SECONDS, RlvAttachmentWatchdog,
        RlvWatchdogAction, RlvWearAction, WEAR_TIMEOUT_SECONDS,
    };
    use crate::locks::{RlvLockSource, RlvWornAttachment};
    use crate::{RlvAttachmentPoint, RlvFolderLockSource, RlvState, RlvWearableSlot};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// The object standing in for a collar.
    const COLLAR: Uuid = Uuid::from_u128(1);

    /// What is worn, and nothing else — the folder half of the lock source is
    /// unused here because these tests lock points, not folders.
    #[derive(Debug, Default)]
    struct Worn {
        /// Attachments by point index.
        attachments: BTreeMap<u8, Vec<RlvWornAttachment>>,
    }

    impl Worn {
        /// Wear `attachment` on the point it names.
        fn wearing(mut self, attachment: RlvWornAttachment) -> Self {
            self.attachments
                .entry(attachment.point.index())
                .or_default()
                .push(attachment);
            self
        }
    }

    impl RlvLockSource for Worn {
        fn attachments_at(&self, point: RlvAttachmentPoint) -> Vec<RlvWornAttachment> {
            self.attachments
                .get(&point.index())
                .cloned()
                .unwrap_or_default()
        }
        fn wearables_on(&self, _slot: RlvWearableSlot) -> Vec<Uuid> {
            Vec::new()
        }
        fn can_add_wearable(&self, _slot: RlvWearableSlot) -> bool {
            true
        }
        fn inventory_root(&self) -> Uuid {
            Uuid::from_u128(0x100)
        }
        fn folder_parent(&self, _folder: Uuid) -> Option<Uuid> {
            None
        }
        fn folder_name(&self, _folder: Uuid) -> Option<String> {
            None
        }
        fn item_name(&self, _item: Uuid) -> Option<String> {
            None
        }
        fn item_folders(&self, _item: Uuid) -> Vec<Uuid> {
            Vec::new()
        }
        fn lock_source_folders(&self, _source: &RlvFolderLockSource) -> Vec<Uuid> {
            Vec::new()
        }
    }

    /// A state holding `line` from the collar.
    fn state_with(line: &str) -> Result<RlvState, TestError> {
        let mut state = RlvState::new();
        let commands = crate::parse_chat_line(line).ok_or("not an rlv line")?;
        for command in &commands {
            state.apply(COLLAR, command.as_ref().map_err(ToString::to_string)?);
        }
        Ok(state)
    }

    /// The point named, for tests.
    fn point(name: &str) -> Result<RlvAttachmentPoint, TestError> {
        Ok(RlvAttachmentPoint::from_name(name).ok_or("no such point")?)
    }

    #[test]
    fn a_locked_attachment_taken_off_goes_back_on() -> Result<(), TestError> {
        let (root, item) = (Uuid::from_u128(0x10), Uuid::from_u128(0x11));
        let chest = point("chest")?;
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        let outcome = watchdog.on_detach(&worn, 100.0, &locks, &source);
        assert!(
            !outcome.allowed,
            "a locked attachment may not just come off"
        );
        assert!(
            outcome.actions.is_empty(),
            "the put-back waits for the tick"
        );
        assert!(!watchdog.is_idle());

        // Nothing happens until the asset has had time to be saved.
        assert_eq!(watchdog.tick(105.0, |_| true), Vec::new());
        let forced = watchdog.tick(100.0 + ASSET_SAVE_TIMEOUT_SECONDS + 1.0, |_| true);
        assert_eq!(
            forced,
            vec![RlvWatchdogAction::Attach { item, point: chest }]
        );

        // Once it lands, the watchdog is done with it.
        let back = RlvWornAttachment::new(root, chest, Some(item));
        let outcome = watchdog.on_attach(&back, 120.0, &locks, &source);
        assert!(outcome.allowed);
        assert!(outcome.actions.is_empty());
        assert!(watchdog.is_idle());
        Ok(())
    }

    #[test]
    fn a_saved_asset_puts_it_back_sooner() -> Result<(), TestError> {
        let (root, item) = (Uuid::from_u128(0x10), Uuid::from_u128(0x11));
        let chest = point("chest")?;
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_detach(&worn, 100.0, &locks, &source);
        assert_eq!(
            watchdog.on_asset_saved(item, 101.0),
            vec![RlvWatchdogAction::Attach { item, point: chest }]
        );
        // A different item's save is not ours.
        assert_eq!(
            watchdog.on_asset_saved(Uuid::from_u128(0x99), 102.0),
            Vec::new()
        );
        Ok(())
    }

    #[test]
    fn a_reattach_is_retried_but_only_after_a_while() -> Result<(), TestError> {
        let (root, item) = (Uuid::from_u128(0x10), Uuid::from_u128(0x11));
        let chest = point("chest")?;
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_detach(&worn, 0.0, &locks, &source);
        let first = watchdog.tick(ASSET_SAVE_TIMEOUT_SECONDS + 1.0, |_| true);
        assert_eq!(first.len(), 1);
        // Too soon to try again.
        assert_eq!(
            watchdog.tick(ASSET_SAVE_TIMEOUT_SECONDS + 2.0, |_| true),
            Vec::new()
        );
        let retried = watchdog.tick(
            ASSET_SAVE_TIMEOUT_SECONDS + REATTACH_RETRY_SECONDS + 2.0,
            |_| true,
        );
        assert_eq!(retried.len(), 1, "still gone, so ask again");

        // An item that is no longer in inventory is given up on.
        assert_eq!(watchdog.tick(1000.0, |_| false), Vec::new());
        assert!(watchdog.is_idle());
        Ok(())
    }

    #[test]
    fn a_detach_the_watchdog_asked_for_is_not_undone() -> Result<(), TestError> {
        let (root, item) = (Uuid::from_u128(0x10), Uuid::from_u128(0x11));
        let chest = point("chest")?;
        // Nothing is locked, but the point was add-locked when the wear was
        // asked for, so the arrival is undone and its detach is ours.
        let state = state_with("@addattach:chest=n")?;
        let locks = state.locks();
        let arriving = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(arriving.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(item, RlvWearAction::Replace, 0.0, &locks, &Worn::default());
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &source);
        assert!(!outcome.allowed, "the point was closed to new attachments");
        assert_eq!(outcome.actions, vec![RlvWatchdogAction::Detach(vec![root])]);

        // ... and when that detach lands it is ticked off, not fought.
        let outcome = watchdog.on_detach(&arriving, 2.0, &locks, &source);
        assert!(outcome.allowed);
        assert!(watchdog.is_idle());
        Ok(())
    }

    #[test]
    fn a_wear_onto_an_add_locked_point_restores_what_was_there() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (old_root, old_item) = (Uuid::from_u128(0x20), Uuid::from_u128(0x21));
        let (new_root, new_item) = (Uuid::from_u128(0x30), Uuid::from_u128(0x31));
        let state = state_with("@addattach:chest=n")?;
        let locks = state.locks();

        let before =
            Worn::default().wearing(RlvWornAttachment::new(old_root, chest, Some(old_item)));
        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(new_item, RlvWearAction::Replace, 0.0, &locks, &before);

        // The replace landed: the old one is gone and the new one is on.
        let arriving = RlvWornAttachment::new(new_root, chest, Some(new_item));
        let after = Worn::default().wearing(arriving.clone());
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &after);
        assert!(!outcome.allowed);
        assert_eq!(
            outcome.actions,
            vec![RlvWatchdogAction::Detach(vec![new_root])],
            "the new arrival goes back off"
        );
        // ... and the one it displaced is queued to come back.
        let restored = watchdog.tick(ASSET_SAVE_TIMEOUT_SECONDS + 2.0, |_| true);
        assert_eq!(
            restored,
            vec![RlvWatchdogAction::Attach {
                item: old_item,
                point: chest
            }]
        );
        Ok(())
    }

    #[test]
    fn re_wearing_what_was_already_there_is_left_alone() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (root, item) = (Uuid::from_u128(0x20), Uuid::from_u128(0x21));
        let state = state_with("@addattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(item, RlvWearAction::Replace, 0.0, &locks, &source);
        let outcome = watchdog.on_attach(&worn, 1.0, &locks, &source);
        assert!(outcome.allowed, "nothing was gained, so nothing is undone");
        assert!(outcome.actions.is_empty());
        Ok(())
    }

    #[test]
    fn a_replace_over_a_locked_attachment_is_refused() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (locked_root, locked_item) = (Uuid::from_u128(0x20), Uuid::from_u128(0x21));
        let (new_root, new_item) = (Uuid::from_u128(0x30), Uuid::from_u128(0x31));
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();

        let before = Worn::default().wearing(RlvWornAttachment::new(
            locked_root,
            chest,
            Some(locked_item),
        ));
        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(new_item, RlvWearAction::Replace, 0.0, &locks, &before);

        let arriving = RlvWornAttachment::new(new_root, chest, Some(new_item));
        let after = Worn::default()
            .wearing(RlvWornAttachment::new(
                locked_root,
                chest,
                Some(locked_item),
            ))
            .wearing(arriving.clone());
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &after);
        assert!(!outcome.allowed);
        assert_eq!(
            outcome.actions,
            vec![RlvWatchdogAction::Detach(vec![new_root])]
        );
        Ok(())
    }

    #[test]
    fn wear_replace_unlocked_keeps_the_locked_one_and_the_new_one() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (locked_root, locked_item) = (Uuid::from_u128(0x20), Uuid::from_u128(0x21));
        let (free_root, free_item) = (Uuid::from_u128(0x40), Uuid::from_u128(0x41));
        let (new_root, new_item) = (Uuid::from_u128(0x30), Uuid::from_u128(0x31));
        // Only the one attachment is locked on, by name.
        let mut state = RlvState::new();
        let commands = crate::parse_chat_line("@detach=n").ok_or("not an rlv line")?;
        state.apply(
            locked_root,
            commands
                .first()
                .ok_or("no command")?
                .as_ref()
                .map_err(ToString::to_string)?,
        );
        state.set_object_attachment(
            locked_root,
            Some(crate::RlvObjectAttachment::new(locked_root, chest)),
        );
        let locks = state.locks();

        let arriving = RlvWornAttachment::new(new_root, chest, Some(new_item));
        let after = Worn::default()
            .wearing(RlvWornAttachment::new(
                locked_root,
                chest,
                Some(locked_item),
            ))
            .wearing(RlvWornAttachment::new(free_root, chest, Some(free_item)))
            .wearing(arriving.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.set_wear_replace_unlocked(true);
        watchdog.on_wear_requested(new_item, RlvWearAction::Replace, 0.0, &locks, &after);
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &after);
        assert!(outcome.allowed, "the replace was allowed to stand");
        assert_eq!(
            outcome.actions,
            vec![RlvWatchdogAction::Detach(vec![free_root])],
            "only the unlocked one is displaced"
        );
        Ok(())
    }

    #[test]
    fn a_replace_onto_a_free_point_displaces_what_is_there() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (old_root, old_item) = (Uuid::from_u128(0x20), Uuid::from_u128(0x21));
        let (new_root, new_item) = (Uuid::from_u128(0x30), Uuid::from_u128(0x31));
        // Something is locked somewhere, or the watchdog would not be watching.
        let state = state_with("@remattach:spine=n")?;
        let locks = state.locks();

        let arriving = RlvWornAttachment::new(new_root, chest, Some(new_item));
        let after = Worn::default()
            .wearing(RlvWornAttachment::new(old_root, chest, Some(old_item)))
            .wearing(arriving.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(new_item, RlvWearAction::Replace, 0.0, &locks, &after);
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &after);
        assert!(outcome.allowed);
        assert_eq!(
            outcome.actions,
            vec![RlvWatchdogAction::Detach(vec![old_root])]
        );
        Ok(())
    }

    #[test]
    fn an_add_is_left_alone_on_a_point_with_nothing_locked() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (new_root, new_item) = (Uuid::from_u128(0x30), Uuid::from_u128(0x31));
        let state = state_with("@remattach:spine=n")?;
        let locks = state.locks();
        let arriving = RlvWornAttachment::new(new_root, chest, Some(new_item));
        let after = Worn::default().wearing(arriving.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(new_item, RlvWearAction::Add, 0.0, &locks, &after);
        let outcome = watchdog.on_attach(&arriving, 1.0, &locks, &after);
        assert!(outcome.allowed);
        assert!(outcome.actions.is_empty());
        Ok(())
    }

    #[test]
    fn nothing_locked_means_nothing_remembered() -> Result<(), TestError> {
        let state = RlvState::new();
        let locks = state.locks();
        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(
            Uuid::from_u128(0x31),
            RlvWearAction::Replace,
            0.0,
            &locks,
            &Worn::default(),
        );
        assert!(watchdog.is_idle());
        Ok(())
    }

    #[test]
    fn a_wear_that_never_lands_is_forgotten() -> Result<(), TestError> {
        let state = state_with("@addattach:chest=n")?;
        let locks = state.locks();
        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_wear_requested(
            Uuid::from_u128(0x31),
            RlvWearAction::Replace,
            0.0,
            &locks,
            &Worn::default(),
        );
        assert!(!watchdog.is_idle());
        assert_eq!(
            watchdog.tick(WEAR_TIMEOUT_SECONDS - 1.0, |_| true),
            Vec::new()
        );
        assert!(!watchdog.is_idle());
        assert_eq!(
            watchdog.tick(WEAR_TIMEOUT_SECONDS + 1.0, |_| true),
            Vec::new()
        );
        assert!(watchdog.is_idle(), "a wear that never landed is dropped");
        Ok(())
    }

    #[test]
    fn something_else_arriving_on_a_restoring_point_goes_back_off() -> Result<(), TestError> {
        let chest = point("chest")?;
        let (root, item) = (Uuid::from_u128(0x10), Uuid::from_u128(0x11));
        let (other_root, other_item) = (Uuid::from_u128(0x50), Uuid::from_u128(0x51));
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(root, chest, Some(item));
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        watchdog.on_detach(&worn, 0.0, &locks, &source);

        let intruder = RlvWornAttachment::new(other_root, chest, Some(other_item));
        let outcome = watchdog.on_attach(&intruder, 1.0, &locks, &source);
        assert!(!outcome.allowed);
        assert_eq!(
            outcome.actions,
            vec![RlvWatchdogAction::Detach(vec![other_root])]
        );
        assert!(!watchdog.is_idle(), "the real one is still owed");
        Ok(())
    }

    #[test]
    fn a_temporary_attachment_has_no_item_and_is_left_alone() -> Result<(), TestError> {
        let chest = point("chest")?;
        let state = state_with("@remattach:chest=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(Uuid::from_u128(0x60), chest, None);
        let source = Worn::default().wearing(worn.clone());

        let mut watchdog = RlvAttachmentWatchdog::new();
        let outcome = watchdog.on_detach(&worn, 0.0, &locks, &source);
        assert!(outcome.allowed, "nothing to put back without an item");
        assert!(watchdog.is_idle());
        Ok(())
    }
}
