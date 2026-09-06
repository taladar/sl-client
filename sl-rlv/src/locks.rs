//! Locks — the bookkeeping behind "you cannot take that off".
//!
//! Most RLV restrictions are a yes/no the whole viewer asks about: `@fly=n` is
//! in force or it is not. The wear restrictions are not like that. `@detach=n`
//! locks *this object* on; `@remattach:chest=n` locks *one attachment point*;
//! `@addoutfit:gloves=n` locks *one wearable layer* against being worn on; and
//! `@detachallthis=n` locks *a folder and everything under it*. Asking "is this
//! in force" is useless — every wear and detach path has to ask "is **this**
//! locked", and the reference keeps three registries to answer that
//! (`rlvlocks.cpp`).
//!
//! Here those registries are **derived**, not maintained. Everything they hold
//! is already in [`RlvState`]: which object issued `@detach:chest=n` is exactly
//! what the held-command list records. [`RlvLocks::of`] reads that list and
//! produces the same three registries plus the folder locks, so there is no
//! second copy of the truth to drift — an object detaching drops its locks
//! because [`RlvState::clear_object`] dropped its commands, with nothing else
//! to remember.
//!
//! What cannot be derived is where things *are*: which attachments hang off a
//! point, which folder an item came from, and — for a bare `@detach=n`, the one
//! restriction that means "this object" — where the issuing object itself is
//! worn. The first two come from an [`RlvLockSource`] the consumer implements;
//! the last is cached on the state machine by
//! [`RlvState::set_object_attachment`], because `@detach=y` routinely arrives
//! after the object is already gone and there would be nothing left to ask.
//!
//! The predicates at the end — [`RlvLocks::can_attach`],
//! [`RlvLocks::can_detach`], [`RlvLocks::can_wear`], [`RlvLocks::can_remove`] —
//! are what every wear path must consult, and are also the honest
//! implementation of the four `can_*` methods on
//! [`RlvQuerySource`](crate::RlvQuerySource): a consumer that has locks should
//! answer those by asking here rather than by guessing.

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::behaviour::{RlvBehaviour, RlvEntry};
use crate::command::RlvParamKind;
use crate::query::{RlvAttachmentPoint, RlvWearableSlot};
use crate::state::{RlvHeldCommand, RlvState};

/// The folder-name flag that exempts an item from `@detach` and `@remoutfit`
/// (`RLV_FOLDER_FLAG_NOSTRIP`, `rlvdefines.h:98`).
pub const NOSTRIP_FLAG: &str = "nostrip";

/// Which way a lock points (`ERlvLockMask`, `rlvdefines.h:366`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RlvLockKind {
    /// Nothing new may go here (`RLV_LOCK_ADD`) — `@addattach`, `@addoutfit`,
    /// `@attachthis`.
    Add,
    /// What is here may not come off (`RLV_LOCK_REMOVE`) — `@detach`,
    /// `@remattach`, `@remoutfit`, `@detachthis`.
    Remove,
}

impl RlvLockKind {
    /// Both directions, for the places the reference passes `RLV_LOCK_ANY`.
    pub const ALL: &'static [Self] = &[Self::Add, Self::Remove];
}

/// What may still be done with a slot or point (`ERlvWearMask`,
/// `rlvdefines.h:374`).
///
/// The reference never produces replace-without-add: replacing implies adding,
/// so [`RlvWearMask::replace`] set with [`RlvWearMask::add`] clear cannot
/// happen and [`RlvWearMask::REPLACE`] carries both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvWearMask {
    /// Something may be worn here *in addition* to what is already on it.
    pub add: bool,
    /// Something may be worn here *replacing* what is already on it.
    pub replace: bool,
}

impl RlvWearMask {
    /// Nothing may be worn here at all (`RLV_WEAR_LOCKED`).
    pub const LOCKED: Self = Self {
        add: false,
        replace: false,
    };
    /// Something may be added, but nothing already here may be displaced
    /// (`RLV_WEAR_ADD`).
    pub const ADD: Self = Self {
        add: true,
        replace: false,
    };
    /// Anything goes (`RLV_WEAR`).
    pub const REPLACE: Self = Self {
        add: true,
        replace: true,
    };

    /// Whether nothing at all may be worn here.
    #[must_use]
    pub const fn is_locked(self) -> bool {
        !self.add && !self.replace
    }
}

/// Where a restricting object sits on the avatar.
///
/// Cached on [`RlvState`] by the consumer, mirroring the lookup the reference's
/// `RlvObject` does once when it first hears from the object
/// (`rlvhelper.cpp:1135`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvObjectAttachment {
    /// The root of the attachment the object belongs to. A `@detach=n` locks
    /// *this*, not the prim that spoke.
    pub root: Uuid,
    /// The point the attachment hangs from.
    pub point: RlvAttachmentPoint,
}

impl RlvObjectAttachment {
    /// An object worn at `point` as part of the attachment rooted at `root`.
    #[must_use]
    pub const fn new(root: Uuid, point: RlvAttachmentPoint) -> Self {
        Self { root, point }
    }
}

/// One attachment currently worn on the avatar.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvWornAttachment {
    /// The attachment's root object id.
    pub root: Uuid,
    /// The point it hangs from.
    pub point: RlvAttachmentPoint,
    /// The inventory item it was worn from, if it has one.
    pub item: Option<Uuid>,
    /// Whether it is a temporary attachment, which no folder lock reaches
    /// because it came from no folder (`rlvlocks.h:479`).
    pub temporary: bool,
}

impl RlvWornAttachment {
    /// An attachment worn from `item` at `point`.
    #[must_use]
    pub const fn new(root: Uuid, point: RlvAttachmentPoint, item: Option<Uuid>) -> Self {
        Self {
            root,
            point,
            item,
            temporary: false,
        }
    }

    /// This attachment, marked as temporary.
    #[must_use]
    pub const fn temporary(self) -> Self {
        Self {
            temporary: true,
            ..self
        }
    }
}

// ---------------------------------------------------------------- folder locks

/// What a folder lock names (`RlvFolderLocks::ELockSourceType`,
/// `rlvlocks.h:585`).
///
/// Only [`SharedPath`](Self::SharedPath) and [`RootFolder`](Self::RootFolder)
/// name a folder outright; the rest name *things worn*, and the folders they
/// lock are wherever those things came from — which is why resolving a source
/// is the consumer's job ([`RlvLockSource::lock_source_folders`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvFolderLockSource {
    /// `@detachthis=n` with no option: the folder the *issuing object* was worn
    /// from (`ST_ATTACHMENT`).
    Attachment(Uuid),
    /// `@detachthis:<point>=n`: the folders everything on that point came from
    /// (`ST_ATTACHMENTPOINT`).
    AttachmentPoint(RlvAttachmentPoint),
    /// `@detachthis:<layer>=n`: the folders everything on that layer came from
    /// (`ST_WEARABLETYPE`).
    WearableType(RlvWearableSlot),
    /// A `#RLV`-relative folder path (`ST_SHAREDPATH`). The empty path is the
    /// `#RLV` root itself, which is what `@sharedunwear=n` locks.
    SharedPath(String),
    /// The whole inventory (`ST_ROOTFOLDER`), which `@unsharedunwear=n` locks
    /// before punching `#RLV` back out of it.
    RootFolder,
}

/// Whether a folder lock forbids or permits (`ELockPermission`,
/// `rlvlocks.h:589`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvFolderLockPermission {
    /// The folder is locked (`PERM_DENY`).
    Deny,
    /// The folder is exempt from this object's own locks (`PERM_ALLOW`) — what
    /// `@detachthis_except` grants.
    Allow,
}

/// How far down a folder lock reaches (`ELockScope`, `rlvlocks.h:590`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvFolderLockScope {
    /// The named folder only — the plain `@detachthis` spelling.
    Node,
    /// The named folder and everything under it — the `@detachallthis`
    /// spelling, which the dictionary marks
    /// [`RlvBehaviourFlags::SUBTREE`](crate::RlvBehaviourFlags::SUBTREE).
    Subtree,
}

/// One folder lock (`folderlock_descr_t`, `rlvlocks.h:598`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvFolderLock {
    /// The object holding it.
    pub object: Uuid,
    /// Whether it blocks wearing or removing.
    pub kind: RlvLockKind,
    /// What it names.
    pub source: RlvFolderLockSource,
    /// Whether it forbids or exempts.
    pub permission: RlvFolderLockPermission,
    /// How far down it reaches.
    pub scope: RlvFolderLockScope,
}

// ------------------------------------------------------------- point/type locks

/// One attachment-point lock (`RlvAttachmentLocks::m_AttachPtAdd` /
/// `m_AttachPtRem`, `rlvlocks.h:127`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvAttachmentPointLock {
    /// The point, or `None` for the bare `@addattach=n` / `@remattach=n` that
    /// locks every point at once.
    pub point: Option<RlvAttachmentPoint>,
    /// Which way it points.
    pub kind: RlvLockKind,
    /// The object holding it.
    pub object: Uuid,
}

/// One attachment lock: a specific worn attachment that may not come off
/// (`RlvAttachmentLocks::m_AttachObjRem`, `rlvlocks.h:129`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvAttachmentLock {
    /// The root of the locked attachment.
    pub attachment: Uuid,
    /// The object holding it — which is the locked attachment itself, because
    /// only a bare `@detach=n` produces one of these.
    pub object: Uuid,
}

/// One wearable-layer lock (`RlvWearableLocks::m_WearableTypeAdd` /
/// `m_WearableTypeRem`, `rlvlocks.h:274`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RlvWearableTypeLock {
    /// The layer, or `None` for the bare `@addoutfit=n` / `@remoutfit=n` that
    /// locks every layer at once.
    pub slot: Option<RlvWearableSlot>,
    /// Which way it points.
    pub kind: RlvLockKind,
    /// The object holding it.
    pub object: Uuid,
}

// --------------------------------------------------------------------- source

/// The facts the lock model needs that the state machine cannot know: what is
/// worn, and what the inventory looks like.
///
/// Everything here is a plain lookup about *right now*; nothing mutates.
pub trait RlvLockSource {
    /// The attachments worn on `point`.
    fn attachments_at(&self, point: RlvAttachmentPoint) -> Vec<RlvWornAttachment>;

    /// The wearables worn on `slot`, by inventory item id, in wear order.
    fn wearables_on(&self, slot: RlvWearableSlot) -> Vec<Uuid>;

    /// Whether `slot` has room for another wearable — the five-per-layer cap
    /// (`LLAgentWearables::canAddWearable`), which is not an RLV lock but does
    /// decide whether a locked layer can still be added to.
    fn can_add_wearable(&self, slot: RlvWearableSlot) -> bool;

    /// The agent's inventory root folder, where the walk up the tree stops.
    fn inventory_root(&self) -> Uuid;

    /// The parent of `folder`, or `None` if it has none or is unknown.
    fn folder_parent(&self, folder: Uuid) -> Option<Uuid>;

    /// The name of `folder`, which decides whether it is a *folded* folder and
    /// whether it carries the `nostrip` flag.
    fn folder_name(&self, folder: Uuid) -> Option<String>;

    /// The name of `item`, for the `nostrip` test.
    fn item_name(&self, item: Uuid) -> Option<String>;

    /// Every folder that holds `item` **or a link to it** under `#RLV`.
    ///
    /// The reference checks the links too (`RlvFolderLocks::getLockedItems`,
    /// `rlvlocks.cpp:1050`): an outfit folder under `#RLV` that links to a
    /// worn item locks that item, even though the item itself lives elsewhere.
    fn item_folders(&self, item: Uuid) -> Vec<Uuid>;

    /// The folders one lock source names
    /// (`RlvFolderLocks::getLockedFolders`, `rlvlocks.cpp:982`).
    ///
    /// A source naming nothing — a path with no such folder, an object that is
    /// not worn — resolves to no folders and therefore locks nothing.
    fn lock_source_folders(&self, source: &RlvFolderLockSource) -> Vec<Uuid>;
}

// ---------------------------------------------------------------------- locks

/// The lock registries, derived from a [`RlvState`].
///
/// Build one with [`RlvLocks::of`] after the state changes; it is a snapshot,
/// and it is cheap — the input is the tens of commands the worn objects are
/// holding, not the inventory.
#[derive(Debug, Clone, Default)]
pub struct RlvLocks {
    /// Attachment points that may not be attached to or detached from.
    points: Vec<RlvAttachmentPointLock>,
    /// Specific attachments that may not come off.
    attachments: Vec<RlvAttachmentLock>,
    /// Wearable layers that may not be worn on or taken off.
    slots: Vec<RlvWearableTypeLock>,
    /// Folder locks, in the order they were issued.
    folders: Vec<RlvFolderLock>,
}

impl RlvLocks {
    /// Read the locks out of `state`.
    ///
    /// ```
    /// # use sl_rlv::{parse_chat_line, RlvAttachmentPoint, RlvLockKind, RlvLocks, RlvState};
    /// # use uuid::Uuid;
    /// # fn main() -> Result<(), Box<dyn core::error::Error>> {
    /// let collar = Uuid::from_u128(1);
    /// let mut state = RlvState::new();
    /// let cmds = parse_chat_line("@remattach:chest=n").ok_or("not rlv")?;
    /// state.apply(collar, cmds.first().ok_or("no command")?.as_ref().map_err(ToString::to_string)?);
    ///
    /// let locks = RlvLocks::of(&state);
    /// let chest = RlvAttachmentPoint::from_name("chest").ok_or("no chest")?;
    /// assert!(locks.is_point_locked(chest, RlvLockKind::Remove));
    /// assert!(!locks.is_point_locked(chest, RlvLockKind::Add));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn of(state: &RlvState) -> Self {
        let mut locks = Self::default();
        for object in state.restricting_objects() {
            for held in state.restrictions_of(object) {
                locks.add(state, object, held);
            }
        }
        locks
    }

    /// Fold one held restriction into the registries.
    fn add(&mut self, state: &RlvState, object: Uuid, held: &RlvHeldCommand) {
        let option = held.option.as_deref();
        match held.behaviour {
            // `@detach=n` locks the issuing object's own attachment on;
            // `@detach:<point>=n` locks a point both ways
            // (`rlvhandler.cpp:1947`).
            RlvBehaviour::Detach => match option {
                None => {
                    if let Some(attachment) = state.object_attachment(object) {
                        self.attachments.push(RlvAttachmentLock {
                            attachment: attachment.root,
                            object,
                        });
                    }
                }
                Some(name) => {
                    if let Some(point) = RlvAttachmentPoint::from_name(name) {
                        for &kind in RlvLockKind::ALL {
                            self.points.push(RlvAttachmentPointLock {
                                point: Some(point),
                                kind,
                                object,
                            });
                        }
                    }
                }
            },
            RlvBehaviour::Addattach | RlvBehaviour::Remattach => {
                let kind = if held.behaviour == RlvBehaviour::Addattach {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                // A bare command locks every point; the reference expands it
                // over the avatar's points, which is the same thing said the
                // long way (`rlvhandler.cpp:1911`).
                let point = match option {
                    None => None,
                    Some(name) => match RlvAttachmentPoint::from_name(name) {
                        Some(point) => Some(point),
                        // An option naming no point is a failed command, and
                        // `apply` will not have stored it — but a keyword that
                        // reaches here anyway must not silently widen into
                        // "every point".
                        None => return,
                    },
                };
                self.points.push(RlvAttachmentPointLock {
                    point,
                    kind,
                    object,
                });
            }
            RlvBehaviour::Addoutfit | RlvBehaviour::Remoutfit => {
                let kind = if held.behaviour == RlvBehaviour::Addoutfit {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                let slot = match option {
                    None => None,
                    Some(name) => match RlvWearableSlot::from_name(name) {
                        Some(slot) => Some(slot),
                        None => return,
                    },
                };
                self.slots.push(RlvWearableTypeLock { slot, kind, object });
            }
            RlvBehaviour::Attachthis | RlvBehaviour::Detachthis => {
                let kind = if held.behaviour == RlvBehaviour::Attachthis {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                let Some(source) = folder_lock_source(object, option) else {
                    return;
                };
                self.folders.push(RlvFolderLock {
                    object,
                    kind,
                    source,
                    permission: RlvFolderLockPermission::Deny,
                    scope: keyword_scope(&held.keyword),
                });
            }
            RlvBehaviour::AttachthisExcept | RlvBehaviour::DetachthisExcept => {
                let kind = if held.behaviour == RlvBehaviour::AttachthisExcept {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                // The exception form only ever names a shared folder path
                // (`RlvHandler::onAddRemFolderLockException`,
                // `rlvhandler.cpp:2032`).
                let Some(path) = option else { return };
                self.folders.push(RlvFolderLock {
                    object,
                    kind,
                    source: RlvFolderLockSource::SharedPath(path.to_owned()),
                    permission: RlvFolderLockPermission::Allow,
                    scope: keyword_scope(&held.keyword),
                });
            }
            // `@sharedwear` / `@sharedunwear` lock the whole `#RLV` tree.
            RlvBehaviour::Sharedwear | RlvBehaviour::Sharedunwear => {
                let kind = if held.behaviour == RlvBehaviour::Sharedwear {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                self.folders.push(RlvFolderLock {
                    object,
                    kind,
                    source: RlvFolderLockSource::SharedPath(String::new()),
                    permission: RlvFolderLockPermission::Deny,
                    scope: RlvFolderLockScope::Subtree,
                });
            }
            // `@unsharedwear` / `@unsharedunwear` lock *everything* and then
            // punch `#RLV` back out of it, which is how "only shared items"
            // is expressed (`rlvhandler.cpp:1660`).
            RlvBehaviour::Unsharedwear | RlvBehaviour::Unsharedunwear => {
                let kind = if held.behaviour == RlvBehaviour::Unsharedwear {
                    RlvLockKind::Add
                } else {
                    RlvLockKind::Remove
                };
                self.folders.push(RlvFolderLock {
                    object,
                    kind,
                    source: RlvFolderLockSource::RootFolder,
                    permission: RlvFolderLockPermission::Deny,
                    scope: RlvFolderLockScope::Subtree,
                });
                self.folders.push(RlvFolderLock {
                    object,
                    kind,
                    source: RlvFolderLockSource::SharedPath(String::new()),
                    permission: RlvFolderLockPermission::Allow,
                    scope: RlvFolderLockScope::Subtree,
                });
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------- registries

    /// Every attachment-point lock, in issue order.
    pub fn point_locks(&self) -> impl Iterator<Item = RlvAttachmentPointLock> {
        self.points.iter().copied()
    }

    /// Every locked-on attachment, in issue order.
    pub fn attachment_locks(&self) -> impl Iterator<Item = RlvAttachmentLock> {
        self.attachments.iter().copied()
    }

    /// Every wearable-layer lock, in issue order.
    pub fn wearable_type_locks(&self) -> impl Iterator<Item = RlvWearableTypeLock> {
        self.slots.iter().copied()
    }

    /// Every folder lock, in issue order.
    pub fn folder_locks(&self) -> impl Iterator<Item = &RlvFolderLock> {
        self.folders.iter()
    }

    /// Whether anything at all is locked, which is the cheap check before doing
    /// any of the work below.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.points.is_empty()
            && self.attachments.is_empty()
            && self.slots.is_empty()
            && self.folders.is_empty()
    }

    // ------------------------------------------------------------- predicates

    /// Whether any attachment point is locked this way
    /// (`hasLockedAttachmentPoint`, `rlvlocks.h:457`).
    ///
    /// A locked *attachment* counts as a remove-locked point, because the point
    /// it is on has something on it that will not come off.
    #[must_use]
    pub fn has_point_lock(&self, kind: RlvLockKind) -> bool {
        self.points.iter().any(|lock| lock.kind == kind)
            || (kind == RlvLockKind::Remove && !self.attachments.is_empty())
    }

    /// Whether any wearable layer is locked this way
    /// (`hasLockedWearableType`, `rlvlocks.h:535`).
    #[must_use]
    pub fn has_wearable_type_lock(&self, kind: RlvLockKind) -> bool {
        self.slots.iter().any(|lock| lock.kind == kind)
    }

    /// Whether any folder is locked this way.
    #[must_use]
    pub fn has_folder_lock(&self, kind: RlvLockKind) -> bool {
        self.folders
            .iter()
            .any(|lock| lock.kind == kind && lock.permission == RlvFolderLockPermission::Deny)
    }

    /// Whether `point` is locked this way (`isLockedAttachmentPoint`,
    /// `rlvlocks.h:484`).
    #[must_use]
    pub fn is_point_locked(&self, point: RlvAttachmentPoint, kind: RlvLockKind) -> bool {
        self.is_point_locked_except(point, kind, None)
    }

    /// Whether `point` is locked this way by anything other than `except`
    /// (`isLockedAttachmentPointExcept`, `rlvlocks.cpp:299`).
    #[must_use]
    pub fn is_point_locked_except(
        &self,
        point: RlvAttachmentPoint,
        kind: RlvLockKind,
        except: Option<Uuid>,
    ) -> bool {
        self.points.iter().any(|lock| {
            lock.kind == kind
                && lock.point.is_none_or(|locked| locked == point)
                && except != Some(lock.object)
        })
    }

    /// Whether `slot` is locked this way (`isLockedWearableType`,
    /// `rlvlocks.h:552`).
    #[must_use]
    pub fn is_wearable_type_locked(&self, slot: RlvWearableSlot, kind: RlvLockKind) -> bool {
        self.is_wearable_type_locked_except(slot, kind, None)
    }

    /// Whether `slot` is locked this way by anything other than `except`
    /// (`isLockedWearableTypeExcept`, `rlvlocks.cpp:866`).
    #[must_use]
    pub fn is_wearable_type_locked_except(
        &self,
        slot: RlvWearableSlot,
        kind: RlvLockKind,
        except: Option<Uuid>,
    ) -> bool {
        self.slots.iter().any(|lock| {
            lock.kind == kind
                && lock.slot.is_none_or(|locked| locked == slot)
                && except != Some(lock.object)
        })
    }

    /// Whether one worn attachment may not come off
    /// (`isLockedAttachment`, `rlvlocks.h:466`), ignoring any lock held by
    /// `except`.
    ///
    /// Three ways to be locked, and a temporary attachment can only be the
    /// first two: it came from no inventory folder, so no folder lock reaches
    /// it.
    #[must_use]
    pub fn is_attachment_locked(
        &self,
        attachment: &RlvWornAttachment,
        except: Option<Uuid>,
        source: &impl RlvLockSource,
    ) -> bool {
        let named = self
            .attachments
            .iter()
            .any(|lock| lock.attachment == attachment.root && except != Some(lock.object));
        named
            || self.is_point_locked_except(attachment.point, RlvLockKind::Remove, except)
            || (!attachment.temporary
                && attachment.item.is_some_and(|item| {
                    self.is_item_folder_locked(item, RlvLockKind::Remove, source)
                }))
    }

    /// Whether one worn wearable may not come off (`isLockedWearable`,
    /// `rlvlocks.h:546`), ignoring any lock held by `except`.
    #[must_use]
    pub fn is_wearable_locked(
        &self,
        slot: RlvWearableSlot,
        item: Uuid,
        except: Option<Uuid>,
        source: &impl RlvLockSource,
    ) -> bool {
        self.is_wearable_type_locked_except(slot, RlvLockKind::Remove, except)
            || self.is_item_folder_locked(item, RlvLockKind::Remove, source)
    }

    /// Whether any folder holding `item` — or a `#RLV` link to it — is locked
    /// this way.
    #[must_use]
    pub fn is_item_folder_locked(
        &self,
        item: Uuid,
        kind: RlvLockKind,
        source: &impl RlvLockSource,
    ) -> bool {
        if !self.has_folder_lock(kind) {
            return false;
        }
        source
            .item_folders(item)
            .into_iter()
            .any(|folder| self.is_folder_locked(folder, kind, source))
    }

    /// Whether `folder` is locked this way (`RlvFolderLocks::isLockedFolder`,
    /// `rlvlocks.cpp:1149`).
    ///
    /// The walk goes up the tree from the folder to the inventory root. On the
    /// way, a `PERM_DENY` lock locks it outright, while a `PERM_ALLOW` lock
    /// from some object makes every *later* lock from that same object stop
    /// counting — which is how `@detachthis_except` exempts a folder from its
    /// own object's `@detachallthis`. A node-scoped lock only counts on the
    /// folder that was asked about; a subtree-scoped one counts all the way
    /// down.
    ///
    /// Unlike the reference this takes no lock-source mask: the only call that
    /// passes one asks whether the *whole inventory* lock alone is doing the
    /// locking, and the reference's own mask test compares a lock **type**
    /// against a **source type** (`rlvlocks.cpp:1181`) — the two enumerations
    /// share no meaning, so that filter does not do what its name says. It is
    /// left out rather than reproduced.
    #[must_use]
    pub fn is_folder_locked(
        &self,
        folder: Uuid,
        kind: RlvLockKind,
        source: &impl RlvLockSource,
    ) -> bool {
        if !self.has_folder_lock(kind) {
            return false;
        }
        let Some(folder) = folded_parent(folder, source) else {
            return false;
        };
        let root = source.inventory_root();

        // Resolve every lock to the folders it names. A lock naming the
        // inventory root does not go in the map: it is a whole-inventory lock,
        // and only a subtree-scoped one means anything there.
        let mut root_locked = false;
        let mut resolved: Vec<(Uuid, &RlvFolderLock)> = Vec::new();
        for lock in &self.folders {
            if lock.kind != kind {
                continue;
            }
            for locked in source.lock_source_folders(&lock.source) {
                if locked == root {
                    if lock.scope == RlvFolderLockScope::Subtree
                        && lock.permission == RlvFolderLockPermission::Deny
                    {
                        root_locked = true;
                    }
                } else {
                    resolved.push((locked, lock));
                }
            }
        }

        let mut exempted: BTreeSet<Uuid> = BTreeSet::new();
        let mut current = folder;
        loop {
            if current == root {
                break;
            }
            for &(locked, lock) in &resolved {
                if locked != current
                    || (lock.scope == RlvFolderLockScope::Node && current != folder)
                    || exempted.contains(&lock.object)
                {
                    continue;
                }
                match lock.permission {
                    RlvFolderLockPermission::Deny => return true,
                    RlvFolderLockPermission::Allow => {
                        exempted.insert(lock.object);
                    }
                }
            }
            match source.folder_parent(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }

        // Nothing said no on the way up, so the folder is locked only if the
        // whole inventory is and nobody exempted it.
        root_locked && exempted.is_empty()
    }

    // --------------------------------------------------------------- can-wear

    /// What may still be attached to `point` (`RlvAttachmentLocks::canAttach`,
    /// `rlvlocks.h:437`).
    ///
    /// An add-locked point takes nothing. An unlocked point takes anything —
    /// unless something already on it will not come off, in which case a new
    /// attachment may be *added* but cannot *replace* what is there.
    #[must_use]
    pub fn can_attach(
        &self,
        point: RlvAttachmentPoint,
        source: &impl RlvLockSource,
    ) -> RlvWearMask {
        if self.is_point_locked(point, RlvLockKind::Add) {
            return RlvWearMask::LOCKED;
        }
        if self.can_detach_all(point, source) {
            RlvWearMask::REPLACE
        } else {
            RlvWearMask::ADD
        }
    }

    /// Whether anything at all may still be attached, anywhere
    /// (`RlvAttachmentLocks::canAttach()`, `rlvlocks.cpp:217`).
    #[must_use]
    pub fn can_attach_anywhere(&self) -> bool {
        RlvAttachmentPoint::all().any(|point| !self.is_point_locked(point, RlvLockKind::Add))
    }

    /// What may still be worn on `slot` (`RlvWearableLocks::canWear`,
    /// `rlvlocks.h:521`).
    ///
    /// An add-locked layer takes nothing. A layer with something locked on it
    /// takes an addition only while the layer still has room — the five-per-
    /// layer cap is not an RLV lock, but it is what decides whether "add" is
    /// still an option once "replace" is off the table.
    #[must_use]
    pub fn can_wear(&self, slot: RlvWearableSlot, source: &impl RlvLockSource) -> RlvWearMask {
        if self.is_wearable_type_locked(slot, RlvLockKind::Add) {
            return RlvWearMask::LOCKED;
        }
        if !self.has_locked_wearable(slot, source) {
            return RlvWearMask::REPLACE;
        }
        if source.can_add_wearable(slot) {
            RlvWearMask::ADD
        } else {
            RlvWearMask::LOCKED
        }
    }

    /// Whether at least one attachment on `point` may come off, ignoring locks
    /// held by `except` (`RlvAttachmentLocks::canDetach`, `rlvlocks.cpp:232`,
    /// and `RlvForceWear::isForceDetachable`, `rlvhelper.cpp:1430`).
    ///
    /// A point with nothing on it answers `false`: there is nothing to detach,
    /// which is not the same as being free to detach.
    #[must_use]
    pub fn can_detach(
        &self,
        point: RlvAttachmentPoint,
        except: Option<Uuid>,
        source: &impl RlvLockSource,
    ) -> bool {
        source
            .attachments_at(point)
            .iter()
            .any(|attachment| self.can_detach_attachment(attachment, except, source))
    }

    /// Whether *every* attachment on `point` may come off — the `fDetachAll`
    /// arm of `canDetach`, which is what decides whether a new attachment can
    /// replace what is there.
    ///
    /// An empty point answers `true`: nothing is in the way.
    #[must_use]
    pub fn can_detach_all(&self, point: RlvAttachmentPoint, source: &impl RlvLockSource) -> bool {
        source
            .attachments_at(point)
            .iter()
            .all(|attachment| !self.is_attachment_locked(attachment, None, source))
    }

    /// Whether one worn attachment may come off
    /// (`RlvForceWear::isForceDetachable`, `rlvhelper.cpp:1401`): not locked,
    /// and not marked `nostrip`.
    #[must_use]
    pub fn can_detach_attachment(
        &self,
        attachment: &RlvWornAttachment,
        except: Option<Uuid>,
        source: &impl RlvLockSource,
    ) -> bool {
        !self.is_attachment_locked(attachment, except, source)
            && attachment
                .item
                .is_none_or(|item| is_strippable(item, source))
    }

    /// Whether at least one wearable on `slot` may come off, ignoring locks
    /// held by `except` (`RlvWearableLocks::canRemove`, `rlvlocks.cpp:836`, and
    /// `RlvForceWear::isForceRemovable`, `rlvhelper.cpp:1503`).
    ///
    /// A body part never comes off — an avatar always wears exactly one of each
    /// — so only clothing layers can answer `true` here.
    #[must_use]
    pub fn can_remove(
        &self,
        slot: RlvWearableSlot,
        except: Option<Uuid>,
        source: &impl RlvLockSource,
    ) -> bool {
        if slot.is_body_part() {
            return false;
        }
        source.wearables_on(slot).into_iter().any(|item| {
            !self.is_wearable_locked(slot, item, except, source) && is_strippable(item, source)
        })
    }

    /// Whether anything worn on `slot` will not come off
    /// (`RlvWearableLocks::hasLockedWearable`, `rlvlocks.cpp:846`).
    #[must_use]
    pub fn has_locked_wearable(&self, slot: RlvWearableSlot, source: &impl RlvLockSource) -> bool {
        source
            .wearables_on(slot)
            .into_iter()
            .any(|item| self.is_wearable_locked(slot, item, None, source))
    }

    /// Whether anything on `point` will not come off
    /// (`RlvAttachmentLocks::hasLockedAttachment`, `rlvlocks.cpp:257`).
    #[must_use]
    pub fn has_locked_attachment(
        &self,
        point: RlvAttachmentPoint,
        source: &impl RlvLockSource,
    ) -> bool {
        source
            .attachments_at(point)
            .iter()
            .any(|attachment| self.is_attachment_locked(attachment, None, source))
    }

    /// Whether any HUD attachment will not come off
    /// (`RlvAttachmentLocks::hasLockedHUD`, `rlvlocks.h:75`) — the thing that
    /// decides whether the viewer may let the user hide their HUDs.
    #[must_use]
    pub fn has_locked_hud(&self, source: &impl RlvLockSource) -> bool {
        RlvAttachmentPoint::all()
            .filter(|point| point.group() == crate::query::RlvAttachGroup::Hud)
            .any(|point| self.has_locked_attachment(point, source))
    }
}

// ------------------------------------------------------------------- helpers

/// How far a folder-lock spelling reaches: `@detachallthis` covers the subtree,
/// `@detachthis` only the folder.
fn keyword_scope(keyword: &str) -> RlvFolderLockScope {
    if RlvEntry::lookup(keyword, RlvParamKind::AddRem).is_some_and(|entry| entry.flags.is_subtree())
    {
        RlvFolderLockScope::Subtree
    } else {
        RlvFolderLockScope::Node
    }
}

/// What `@attachthis[:<option>]=n` names (`RlvHandler::onAddRemFolderLock`,
/// `rlvhandler.cpp:1988`).
///
/// The reference reads the option through its generic parser, which tries a
/// wearable layer first, then an attachment point, then a UUID, then a shared
/// folder path (`rlvhelper.cpp:943`) — and then refuses anything that came back
/// a UUID. So does this; a path naming no folder is not refused here but simply
/// resolves to no folders, because whether a folder exists is not something
/// this crate can know.
fn folder_lock_source(object: Uuid, option: Option<&str>) -> Option<RlvFolderLockSource> {
    let Some(option) = option else {
        return Some(RlvFolderLockSource::Attachment(object));
    };
    if let Some(slot) = RlvWearableSlot::from_name(option) {
        return Some(RlvFolderLockSource::WearableType(slot));
    }
    if let Some(point) = RlvAttachmentPoint::from_name(option) {
        return Some(RlvFolderLockSource::AttachmentPoint(point));
    }
    if option.len() == 36 && Uuid::parse_str(option).is_ok() {
        return None;
    }
    Some(RlvFolderLockSource::SharedPath(option.to_owned()))
}

/// The nearest ancestor of `folder` that is not a *folded* folder
/// (`RlvInventory::getFoldedParent`, `rlvinventory.h:292`).
///
/// A folded folder is a naming convention, not a container: `.(chest)` says
/// "wear my contents on the chest" and is not a folder in its own right for
/// locking purposes, so a lock on its parent locks it.
fn folded_parent(folder: Uuid, source: &impl RlvLockSource) -> Option<Uuid> {
    let mut current = folder;
    // Bounded by the inventory depth; the guard is against a parent cycle in a
    // consumer's tree, which would otherwise hang the viewer.
    for _ in 0..MAX_FOLDER_DEPTH {
        let name = source.folder_name(current)?;
        if !is_folded_folder_name(&name) {
            return Some(current);
        }
        current = source.folder_parent(current)?;
    }
    None
}

/// How far up an inventory tree any walk here will go before giving up.
///
/// Second Life's own inventory is far shallower than this; the cap exists so a
/// consumer whose tree has a cycle gets a wrong answer rather than a hang.
const MAX_FOLDER_DEPTH: usize = 128;

/// Whether a folder name is a *folded* folder: `.(<attachment point>)` or
/// `.(nostrip)` (`RlvInventory::isFoldedFolder`, `rlvinventory.h:307`).
#[must_use]
pub fn is_folded_folder_name(name: &str) -> bool {
    let Some(inner) = name
        .strip_prefix(".(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    inner == NOSTRIP_FLAG || RlvAttachmentPoint::from_name(inner.trim()).is_some()
}

/// Whether `item` may be stripped by `@detach` or `@remoutfit`
/// (`RlvForceWear::isStrippable`, `rlvhelper.cpp:1546`).
///
/// The `nostrip` flag is a naming convention: an item — or any folder above it,
/// up to the inventory root — whose name contains `nostrip` anywhere is exempt
/// from being taken off by a command. It is how a creator says "this is part of
/// the avatar", and it is not an RLV restriction at all: nobody issued it and
/// nothing lifts it.
#[must_use]
pub fn is_strippable(item: Uuid, source: &impl RlvLockSource) -> bool {
    if source
        .item_name(item)
        .is_some_and(|name| name.contains(NOSTRIP_FLAG))
    {
        return false;
    }
    let root = source.inventory_root();
    let mut current = source.item_folders(item).into_iter().next();
    for _ in 0..MAX_FOLDER_DEPTH {
        let Some(folder) = current else { break };
        if folder == root {
            break;
        }
        if source
            .folder_name(folder)
            .is_some_and(|name| name.contains(NOSTRIP_FLAG))
        {
            return false;
        }
        current = source.folder_parent(folder);
    }
    true
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        RlvFolderLockPermission, RlvFolderLockScope, RlvFolderLockSource, RlvLockKind,
        RlvLockSource, RlvObjectAttachment, RlvWearMask, RlvWornAttachment, is_folded_folder_name,
        is_strippable,
    };
    use crate::{RlvAttachmentPoint, RlvState, RlvWearableSlot, parse_chat_line};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// The object standing in for a collar.
    const COLLAR: Uuid = Uuid::from_u128(1);
    /// A second restricting object.
    const CUFFS: Uuid = Uuid::from_u128(2);
    /// The agent's inventory root.
    const INVENTORY_ROOT: Uuid = Uuid::from_u128(0x100);

    /// A hand-built inventory and outfit.
    #[derive(Debug, Default)]
    struct Fixture {
        /// Attachments, by point index.
        attachments: BTreeMap<u8, Vec<RlvWornAttachment>>,
        /// Worn wearable items, by slot code.
        wearables: BTreeMap<u8, Vec<Uuid>>,
        /// Layers with no room left.
        full: Vec<u8>,
        /// Folder parents.
        parents: BTreeMap<Uuid, Uuid>,
        /// Folder names.
        folder_names: BTreeMap<Uuid, String>,
        /// Item names.
        item_names: BTreeMap<Uuid, String>,
        /// The folders holding each item (including `#RLV` links).
        item_folders: BTreeMap<Uuid, Vec<Uuid>>,
        /// The folder each `#RLV` path names.
        paths: BTreeMap<String, Uuid>,
    }

    impl Fixture {
        /// Add a folder with `name` under `parent`.
        fn folder(mut self, folder: Uuid, parent: Uuid, name: &str) -> Self {
            self.parents.insert(folder, parent);
            self.folder_names.insert(folder, name.to_owned());
            self
        }

        /// Name `folder` as the `#RLV`-relative `path`.
        fn path(mut self, path: &str, folder: Uuid) -> Self {
            self.paths.insert(path.to_owned(), folder);
            self
        }

        /// Put `item` in `folder`, named `name`.
        fn item(mut self, item: Uuid, folder: Uuid, name: &str) -> Self {
            self.item_names.insert(item, name.to_owned());
            self.item_folders.insert(item, vec![folder]);
            self
        }

        /// Wear `attachment` on `point`.
        fn worn(mut self, point: &str, attachment: RlvWornAttachment) -> Self {
            if let Some(point) = RlvAttachmentPoint::from_name(point) {
                self.attachments
                    .entry(point.index())
                    .or_default()
                    .push(attachment);
            }
            self
        }

        /// Wear `item` on the layer named `slot`.
        fn wearing(mut self, slot: &str, item: Uuid) -> Self {
            if let Some(slot) = RlvWearableSlot::from_name(slot) {
                self.wearables.entry(slot.code()).or_default().push(item);
            }
            self
        }
    }

    impl RlvLockSource for Fixture {
        fn attachments_at(&self, point: RlvAttachmentPoint) -> Vec<RlvWornAttachment> {
            self.attachments
                .get(&point.index())
                .cloned()
                .unwrap_or_default()
        }
        fn wearables_on(&self, slot: RlvWearableSlot) -> Vec<Uuid> {
            self.wearables
                .get(&slot.code())
                .cloned()
                .unwrap_or_default()
        }
        fn can_add_wearable(&self, slot: RlvWearableSlot) -> bool {
            !self.full.contains(&slot.code())
        }
        fn inventory_root(&self) -> Uuid {
            INVENTORY_ROOT
        }
        fn folder_parent(&self, folder: Uuid) -> Option<Uuid> {
            self.parents.get(&folder).copied()
        }
        fn folder_name(&self, folder: Uuid) -> Option<String> {
            self.folder_names.get(&folder).cloned()
        }
        fn item_name(&self, item: Uuid) -> Option<String> {
            self.item_names.get(&item).cloned()
        }
        fn item_folders(&self, item: Uuid) -> Vec<Uuid> {
            self.item_folders.get(&item).cloned().unwrap_or_default()
        }
        fn lock_source_folders(&self, source: &RlvFolderLockSource) -> Vec<Uuid> {
            match *source {
                RlvFolderLockSource::RootFolder => vec![INVENTORY_ROOT],
                RlvFolderLockSource::SharedPath(ref path) => {
                    self.paths.get(path).copied().into_iter().collect()
                }
                // The fixture resolves the worn-thing sources through the items
                // actually worn, as the reference's inventory lookup does.
                RlvFolderLockSource::Attachment(object) => self
                    .attachments
                    .values()
                    .flatten()
                    .find(|worn| worn.root == object)
                    .and_then(|worn| worn.item)
                    .map(|item| self.item_folders(item))
                    .unwrap_or_default(),
                RlvFolderLockSource::AttachmentPoint(point) => self
                    .attachments_at(point)
                    .into_iter()
                    .filter_map(|worn| worn.item)
                    .flat_map(|item| self.item_folders(item))
                    .collect(),
                RlvFolderLockSource::WearableType(slot) => self
                    .wearables_on(slot)
                    .into_iter()
                    .flat_map(|item| self.item_folders(item))
                    .collect(),
            }
        }
    }

    /// Apply `line` as `object`.
    fn apply(state: &mut RlvState, object: Uuid, line: &str) -> Result<(), TestError> {
        let commands = parse_chat_line(line).ok_or("not an rlv line")?;
        for command in &commands {
            let command = command.as_ref().map_err(ToString::to_string)?;
            state.apply(object, command);
        }
        Ok(())
    }

    /// The point named, for tests.
    fn point(name: &str) -> Result<RlvAttachmentPoint, TestError> {
        Ok(RlvAttachmentPoint::from_name(name).ok_or("no such point")?)
    }

    /// The slot named, for tests.
    fn slot(name: &str) -> Result<RlvWearableSlot, TestError> {
        Ok(RlvWearableSlot::from_name(name).ok_or("no such slot")?)
    }

    #[test]
    fn a_bare_lock_covers_every_point_and_a_named_one_covers_one() -> Result<(), TestError> {
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@remattach:chest=n")?;
        apply(&mut state, CUFFS, "@addattach=n")?;
        let locks = state.locks();

        let chest = point("chest")?;
        let spine = point("spine")?;
        assert!(locks.is_point_locked(chest, RlvLockKind::Remove));
        assert!(!locks.is_point_locked(spine, RlvLockKind::Remove));
        // The bare `@addattach=n` covers every point.
        assert!(locks.is_point_locked(chest, RlvLockKind::Add));
        assert!(locks.is_point_locked(spine, RlvLockKind::Add));
        assert!(!locks.can_attach_anywhere());

        // ... and the `except` form ignores one object's word.
        assert!(!locks.is_point_locked_except(spine, RlvLockKind::Add, Some(CUFFS)));
        assert!(locks.is_point_locked_except(chest, RlvLockKind::Remove, Some(CUFFS)));
        Ok(())
    }

    #[test]
    fn detach_with_a_point_locks_it_both_ways() -> Result<(), TestError> {
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detach:chest=n")?;
        let locks = state.locks();
        let chest = point("chest")?;
        assert!(locks.is_point_locked(chest, RlvLockKind::Add));
        assert!(locks.is_point_locked(chest, RlvLockKind::Remove));
        Ok(())
    }

    #[test]
    fn a_bare_detach_locks_the_object_that_said_it() -> Result<(), TestError> {
        let collar_root = Uuid::from_u128(0x10);
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detach=n")?;

        // Before the consumer says where the collar is, there is nothing to
        // lock — which is the reference's "hasn't rezzed yet" case.
        assert_eq!(state.locks().attachment_locks().count(), 0);

        state.set_object_attachment(
            COLLAR,
            Some(RlvObjectAttachment::new(collar_root, point("neck")?)),
        );
        let locks = state.locks();
        assert_eq!(locks.attachment_locks().count(), 1);

        let source = Fixture::default().worn(
            "neck",
            RlvWornAttachment::new(collar_root, point("neck")?, None),
        );
        let worn = RlvWornAttachment::new(collar_root, point("neck")?, None);
        assert!(locks.is_attachment_locked(&worn, None, &source));
        // The object that put the lock there can still ask past it.
        assert!(!locks.is_attachment_locked(&worn, Some(COLLAR), &source));
        assert!(!locks.can_detach(point("neck")?, None, &source));
        assert!(locks.can_detach(point("neck")?, Some(COLLAR), &source));

        // And letting go drops the lock with nothing else to remember.
        state.clear_object(COLLAR);
        assert_eq!(state.locks().attachment_locks().count(), 0);
        Ok(())
    }

    #[test]
    fn a_locked_attachment_blocks_replacing_but_not_adding() -> Result<(), TestError> {
        let boot = Uuid::from_u128(0x20);
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@remattach:right foot=n")?;
        let locks = state.locks();

        let foot = point("right foot")?;
        let source =
            Fixture::default().worn("right foot", RlvWornAttachment::new(boot, foot, None));
        assert_eq!(locks.can_attach(foot, &source), RlvWearMask::ADD);
        assert!(!locks.can_detach(foot, None, &source));

        // An empty point is free either way.
        let empty = Fixture::default();
        assert_eq!(
            locks.can_attach(point("spine")?, &empty),
            RlvWearMask::REPLACE
        );
        Ok(())
    }

    #[test]
    fn wearable_layers_lock_like_attachment_points() -> Result<(), TestError> {
        let gloves = Uuid::from_u128(0x30);
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@remoutfit:gloves=n")?;
        let locks = state.locks();

        let gloves_slot = slot("gloves")?;
        let source = Fixture::default().wearing("gloves", gloves);
        assert!(locks.is_wearable_type_locked(gloves_slot, RlvLockKind::Remove));
        assert!(!locks.can_remove(gloves_slot, None, &source));
        assert!(locks.can_remove(gloves_slot, Some(COLLAR), &source));
        // A remove lock still lets a layer be added to, but not replaced.
        assert_eq!(locks.can_wear(gloves_slot, &source), RlvWearMask::ADD);

        // A full layer with something locked on it takes nothing at all.
        let full = Fixture {
            full: vec![gloves_slot.code()],
            ..Fixture::default()
        }
        .wearing("gloves", gloves);
        assert_eq!(locks.can_wear(gloves_slot, &full), RlvWearMask::LOCKED);
        Ok(())
    }

    #[test]
    fn a_body_part_never_comes_off() -> Result<(), TestError> {
        let shape = Uuid::from_u128(0x31);
        let state = RlvState::new();
        let locks = state.locks();
        let source = Fixture::default().wearing("shape", shape);
        assert!(!locks.can_remove(slot("shape")?, None, &source));
        Ok(())
    }

    #[test]
    fn addoutfit_without_a_layer_locks_every_layer() -> Result<(), TestError> {
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@addoutfit=n")?;
        let locks = state.locks();
        for slot in RlvWearableSlot::all() {
            assert!(
                locks.is_wearable_type_locked(slot, RlvLockKind::Add),
                "{} is not add-locked",
                slot.name()
            );
        }
        assert!(!locks.has_wearable_type_lock(RlvLockKind::Remove));
        Ok(())
    }

    #[test]
    fn a_folder_lock_reaches_only_as_far_as_its_spelling() -> Result<(), TestError> {
        let (clothes, boots, winter) = (
            Uuid::from_u128(0x40),
            Uuid::from_u128(0x41),
            Uuid::from_u128(0x42),
        );
        let source = Fixture::default()
            .folder(clothes, INVENTORY_ROOT, "Clothes")
            .folder(boots, clothes, "Boots")
            .folder(winter, boots, "Winter")
            .path("clothes", clothes)
            .path("clothes/boots", boots);

        let mut node = RlvState::new();
        apply(&mut node, COLLAR, "@detachthis:clothes/boots=n")?;
        let node_locks = node.locks();
        assert!(node_locks.is_folder_locked(boots, RlvLockKind::Remove, &source));
        assert!(
            !node_locks.is_folder_locked(winter, RlvLockKind::Remove, &source),
            "@detachthis is the folder alone"
        );
        assert!(!node_locks.is_folder_locked(boots, RlvLockKind::Add, &source));

        let mut subtree = RlvState::new();
        apply(&mut subtree, COLLAR, "@detachallthis:clothes/boots=n")?;
        let subtree_locks = subtree.locks();
        assert!(subtree_locks.is_folder_locked(boots, RlvLockKind::Remove, &source));
        assert!(
            subtree_locks.is_folder_locked(winter, RlvLockKind::Remove, &source),
            "@detachallthis reaches down"
        );
        assert!(!subtree_locks.is_folder_locked(clothes, RlvLockKind::Remove, &source));
        Ok(())
    }

    #[test]
    fn an_except_exempts_only_its_own_objects_locks() -> Result<(), TestError> {
        let (clothes, boots) = (Uuid::from_u128(0x40), Uuid::from_u128(0x41));
        let source = Fixture::default()
            .folder(clothes, INVENTORY_ROOT, "Clothes")
            .folder(boots, clothes, "Boots")
            .path("clothes", clothes)
            .path("clothes/boots", boots);

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detachallthis:clothes=n")?;
        apply(&mut state, COLLAR, "@detachallthis_except:clothes/boots=n")?;
        let locks = state.locks();
        assert!(locks.is_folder_locked(clothes, RlvLockKind::Remove, &source));
        assert!(
            !locks.is_folder_locked(boots, RlvLockKind::Remove, &source),
            "the collar exempted its own lock"
        );

        // A second object's lock is not exempted by the first one's exception.
        apply(&mut state, CUFFS, "@detachallthis:clothes=n")?;
        assert!(
            state
                .locks()
                .is_folder_locked(boots, RlvLockKind::Remove, &source),
            "the cuffs never agreed to that exception"
        );
        Ok(())
    }

    #[test]
    fn unsharedunwear_locks_everything_but_the_shared_tree() -> Result<(), TestError> {
        let (shared, outfit, elsewhere) = (
            Uuid::from_u128(0x50),
            Uuid::from_u128(0x51),
            Uuid::from_u128(0x52),
        );
        let source = Fixture::default()
            .folder(shared, INVENTORY_ROOT, "#RLV")
            .folder(outfit, shared, "Outfit")
            .folder(elsewhere, INVENTORY_ROOT, "My Stuff")
            .path("", shared);

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@unsharedunwear=n")?;
        let locks = state.locks();
        assert!(locks.is_folder_locked(elsewhere, RlvLockKind::Remove, &source));
        assert!(
            !locks.is_folder_locked(outfit, RlvLockKind::Remove, &source),
            "#RLV is punched back out of the whole-inventory lock"
        );
        assert!(!locks.is_folder_locked(elsewhere, RlvLockKind::Add, &source));

        // And the wearing half is the same shape the other way round.
        let mut wear = RlvState::new();
        apply(&mut wear, COLLAR, "@unsharedwear=n")?;
        assert!(
            wear.locks()
                .is_folder_locked(elsewhere, RlvLockKind::Add, &source)
        );
        Ok(())
    }

    #[test]
    fn sharedunwear_locks_the_shared_tree_and_nothing_else() -> Result<(), TestError> {
        let (shared, outfit, elsewhere) = (
            Uuid::from_u128(0x50),
            Uuid::from_u128(0x51),
            Uuid::from_u128(0x52),
        );
        let source = Fixture::default()
            .folder(shared, INVENTORY_ROOT, "#RLV")
            .folder(outfit, shared, "Outfit")
            .folder(elsewhere, INVENTORY_ROOT, "My Stuff")
            .path("", shared);

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@sharedunwear=n")?;
        let locks = state.locks();
        assert!(locks.is_folder_locked(outfit, RlvLockKind::Remove, &source));
        assert!(!locks.is_folder_locked(elsewhere, RlvLockKind::Remove, &source));
        Ok(())
    }

    #[test]
    fn a_folder_lock_reaches_the_item_worn_from_it() -> Result<(), TestError> {
        let (clothes, boots) = (Uuid::from_u128(0x40), Uuid::from_u128(0x41));
        let boot_item = Uuid::from_u128(0x60);
        let boot_root = Uuid::from_u128(0x61);
        let source = Fixture::default()
            .folder(clothes, INVENTORY_ROOT, "Clothes")
            .folder(boots, clothes, "Boots")
            .path("clothes/boots", boots)
            .item(boot_item, boots, "Left Boot")
            .worn(
                "left foot",
                RlvWornAttachment::new(boot_root, point("left foot")?, Some(boot_item)),
            );

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detachthis:clothes/boots=n")?;
        let locks = state.locks();
        let worn = RlvWornAttachment::new(boot_root, point("left foot")?, Some(boot_item));
        assert!(locks.is_attachment_locked(&worn, None, &source));
        assert!(!locks.can_detach(point("left foot")?, None, &source));

        // A temporary attachment came from no folder, so no folder lock has it.
        let temporary = worn.clone().temporary();
        assert!(!locks.is_attachment_locked(&temporary, None, &source));
        Ok(())
    }

    #[test]
    fn a_bare_detachthis_names_the_folder_the_object_came_from() -> Result<(), TestError> {
        let (clothes, boots) = (Uuid::from_u128(0x40), Uuid::from_u128(0x41));
        let collar_item = Uuid::from_u128(0x62);
        let source = Fixture::default()
            .folder(clothes, INVENTORY_ROOT, "Clothes")
            .folder(boots, clothes, "Boots")
            .item(collar_item, boots, "Collar")
            .worn(
                "neck",
                RlvWornAttachment::new(COLLAR, point("neck")?, Some(collar_item)),
            );

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detachthis=n")?;
        let locks = state.locks();
        assert_eq!(
            locks.folder_locks().next().map(|lock| lock.source.clone()),
            Some(RlvFolderLockSource::Attachment(COLLAR))
        );
        assert!(locks.is_folder_locked(boots, RlvLockKind::Remove, &source));
        Ok(())
    }

    #[test]
    fn a_folder_lock_option_is_read_as_a_layer_then_a_point_then_a_path() -> Result<(), TestError> {
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detachthis:gloves=n")?;
        apply(&mut state, CUFFS, "@detachthis:chest=n")?;
        apply(
            &mut state,
            Uuid::from_u128(3),
            "@detachthis:clothes/boots=n",
        )?;
        let sources: Vec<RlvFolderLockSource> = state
            .locks()
            .folder_locks()
            .map(|lock| lock.source.clone())
            .collect();
        assert_eq!(
            sources,
            [
                RlvFolderLockSource::WearableType(slot("gloves")?),
                RlvFolderLockSource::AttachmentPoint(point("chest")?),
                RlvFolderLockSource::SharedPath("clothes/boots".to_owned()),
            ]
        );
        Ok(())
    }

    #[test]
    fn scope_and_permission_come_off_the_spelling() -> Result<(), TestError> {
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@attachallthis:clothes=n")?;
        apply(&mut state, CUFFS, "@attachthis_except:clothes=n")?;
        let locks = state.locks();
        let described: Vec<(RlvLockKind, RlvFolderLockPermission, RlvFolderLockScope)> = locks
            .folder_locks()
            .map(|lock| (lock.kind, lock.permission, lock.scope))
            .collect();
        assert_eq!(
            described,
            [
                (
                    RlvLockKind::Add,
                    RlvFolderLockPermission::Deny,
                    RlvFolderLockScope::Subtree
                ),
                (
                    RlvLockKind::Add,
                    RlvFolderLockPermission::Allow,
                    RlvFolderLockScope::Node
                ),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_folded_folder_is_locked_by_its_parent() -> Result<(), TestError> {
        let (outfit, folded) = (Uuid::from_u128(0x70), Uuid::from_u128(0x71));
        let source = Fixture::default()
            .folder(outfit, INVENTORY_ROOT, "Outfit")
            .folder(folded, outfit, ".(chest)")
            .path("outfit", outfit);

        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@detachthis:outfit=n")?;
        assert!(
            state
                .locks()
                .is_folder_locked(folded, RlvLockKind::Remove, &source),
            "a `.(chest)` folder is its parent for locking"
        );
        Ok(())
    }

    #[test]
    fn folded_folder_names() {
        assert!(is_folded_folder_name(".(chest)"));
        assert!(is_folded_folder_name(".(nostrip)"));
        assert!(is_folded_folder_name(".( left hand )"));
        assert!(!is_folded_folder_name(".(frobnicate)"));
        assert!(!is_folded_folder_name("chest"));
        assert!(!is_folded_folder_name(".hidden"));
        assert!(!is_folded_folder_name(""));
    }

    #[test]
    fn nostrip_exempts_an_item_from_being_taken_off() -> Result<(), TestError> {
        let outfit = Uuid::from_u128(0x80);
        let locked_folder = Uuid::from_u128(0x81);
        let (plain, flagged, in_flagged_folder) = (
            Uuid::from_u128(0x90),
            Uuid::from_u128(0x91),
            Uuid::from_u128(0x92),
        );
        let source = Fixture::default()
            .folder(outfit, INVENTORY_ROOT, "Outfit")
            .folder(locked_folder, INVENTORY_ROOT, "Skin (nostrip)")
            .item(plain, outfit, "Shirt")
            .item(flagged, outfit, "Collar (nostrip)")
            .item(in_flagged_folder, locked_folder, "Eyes");

        assert!(is_strippable(plain, &source));
        assert!(!is_strippable(flagged, &source));
        assert!(!is_strippable(in_flagged_folder, &source));

        // ... and it stops a detach even with nothing locked.
        let locks = RlvState::new().locks();
        let source = source.worn(
            "chest",
            RlvWornAttachment::new(Uuid::from_u128(0x93), point("chest")?, Some(flagged)),
        );
        assert!(!locks.can_detach(point("chest")?, None, &source));
        Ok(())
    }

    #[test]
    fn nothing_held_is_nothing_locked() -> Result<(), TestError> {
        let state = RlvState::new();
        let locks = state.locks();
        assert!(locks.is_empty());
        assert!(!locks.has_point_lock(RlvLockKind::Add));
        assert!(!locks.has_point_lock(RlvLockKind::Remove));
        assert!(locks.can_attach_anywhere());

        let source = Fixture::default();
        assert!(!locks.has_locked_hud(&source));
        assert!(locks.can_detach_all(point("chest")?, &source));
        Ok(())
    }

    #[test]
    fn a_locked_hud_is_noticed() -> Result<(), TestError> {
        let hud = Uuid::from_u128(0xa0);
        let mut state = RlvState::new();
        apply(&mut state, COLLAR, "@remattach:center=n")?;
        let source = Fixture::default().worn(
            "center",
            RlvWornAttachment::new(hud, point("center")?, None),
        );
        assert!(state.locks().has_locked_hud(&source));

        let elsewhere =
            Fixture::default().worn("chest", RlvWornAttachment::new(hud, point("chest")?, None));
        assert!(!state.locks().has_locked_hud(&elsewhere));
        Ok(())
    }
}
