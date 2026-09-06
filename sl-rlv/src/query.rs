//! `@get*` — the commands that ask a question and are answered on a channel.
//!
//! A query is a command whose param is a **number**: `@getoutfit=2222` means
//! "chat what I am wearing on channel 2222". The object is not asking for
//! anything to be forbidden, so [`RlvState::apply`] hands it straight back as
//! [`RlvOutcome::NotAStateChange`]; this module is where it is answered.
//!
//! The split between this crate and its consumer is drawn at *facts*, not at
//! formatting. Everything a script actually sees — the bit strings, the
//! comma-joined name lists, the `|32` wear digits, the reply channel rules and
//! the 1023-byte chat cap — is built here and is unit-testable to the letter.
//! What the crate cannot know (what is worn, what is in the shared `#RLV`
//! folder, where the camera is) it asks an [`RlvQuerySource`] for. Three
//! families need nothing at all and are answered from the state machine alone:
//! `@version*`, `@getstatus` / `@getstatusall`, and `@getcommand` — plus the
//! `@getcam_*` queries that read back a modifier some object set.
//!
//! Two reference details drive the shape of [`RlvState::answer`]:
//!
//! - a query that **fails** is still answered, with an empty string. A script
//!   that asked a question and got nothing back would wait forever, so the
//!   reference always replies unless the channel itself was unusable
//!   (`rlvhandler.cpp:3437`);
//! - the answer is *shouted*, and a shout is truncated at 1023 bytes like any
//!   other chat line. A long `@getinv` answer really is cut off — the splitting
//!   [`split_chat`] exists for `@redirchat`, not for queries
//!   (`RlvUtil::sendChatReply` vs `sendChatReplySplit`, `rlvcommon.cpp:726`).
//!
//! ## One deliberate divergence
//!
//! [`RlvAttachmentPoint::group`] answers what the point anatomically *is*.
//! Firestorm derives it from a hard-coded index table
//! (`rlvAttachGroupFromIndex`, `rlvhelper.cpp:1991`) that reads joint group `8`
//! as the HUD group — but since the extended attachment points (tail, wings,
//! jaw, …) were added, group `8` is *them* and the HUD points are group `9`,
//! which that table does not know. The reference therefore answers
//! `@getattachnames:hud` with the extended points and never with a HUD point.
//! We answer the question that was asked; see `attach_group_matches_the_point`
//! in this module's tests.

use uuid::Uuid;

use crate::behaviour::RlvBehaviour;
use crate::command::{RlvCommand, RlvParam, RlvParamKind};
use crate::modifier::RlvModifier;
use crate::state::{RlvOutcome, RlvState};
use crate::version::{version_impl_num_reply, version_num_reply, version_reply};

/// The channel the viewer reserves for its own debug output, and so will not
/// chat a reply on (`CHAT_CHANNEL_DEBUG`, `indra_constants.h:286`).
pub const CHAT_CHANNEL_DEBUG: i32 = i32::MAX;

/// The byte cap on one outgoing chat line (`MAX_MSG_STR_LEN`,
/// `lldbstrings.h:76`). A reply longer than this is truncated, not split.
pub const MAX_CHAT_BYTES: usize = 1023;

/// The shared-inventory root folder every path in a `#RLV` reply is relative to
/// (`RLV_ROOT_FOLDER`, `rlvdefines.h:80`).
pub const SHARED_ROOT_FOLDER: &str = "#RLV";

/// The default separator between the fields of a command option
/// (`RLV_OPTION_SEPARATOR`, `rlvdefines.h:86`).
pub const OPTION_SEPARATOR: char = ';';

/// The default separator `@getstatus` puts *in front of* every restriction it
/// reports.
pub const STATUS_SEPARATOR: &str = "/";

/// The name prefix that hides a shared folder from `@getinv`
/// (`RLV_FOLDER_PREFIX_HIDDEN`, `rlvdefines.h:95`).
pub const FOLDER_PREFIX_HIDDEN: char = '.';

/// The character a shared folder name may not contain and still be listed
/// (`RLV_FOLDER_INVALID_CHARS`, `rlvdefines.h:97`).
pub const FOLDER_INVALID_CHAR: char = '/';

// ---------------------------------------------------------------- attachments

/// The coarse body region an attachment point belongs to
/// (`ERlvAttachGroupType`, `rlvdefines.h:382`).
///
/// `@getattachnames:<group>` filters on it, and `@showselfhead` asks whether a
/// point is on the head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum RlvAttachGroup {
    /// The skull and the points on the face.
    Head,
    /// The torso, from neck to pelvis.
    Torso,
    /// Shoulders, arms and hands.
    Arms,
    /// Hips, legs and feet.
    Legs,
    /// The HUD surfaces, which are not on the avatar at all.
    Hud,
}

impl RlvAttachGroup {
    /// Every group, in the order the reference's name table declares them
    /// (`cstrAttachGroups`, `rlvhelper.cpp:1988`).
    pub const ALL: &'static [Self] = &[Self::Head, Self::Torso, Self::Arms, Self::Legs, Self::Hud];

    /// The keyword `@getattachnames:<group>` uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Torso => "torso",
            Self::Arms => "arms",
            Self::Legs => "legs",
            Self::Hud => "hud",
        }
    }

    /// The group `name` spells, if any (`rlvAttachGroupFromString`,
    /// `rlvhelper.cpp:2015`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|group| group.name() == name)
    }
}

/// One row of the attachment-point table.
struct AttachmentPointRow {
    /// The wire index of the point (`LLVOAvatar::mAttachmentPoints`' key).
    index: u8,
    /// The point's name, lower-cased as the lookup map stores it.
    name: &'static str,
    /// The body region it is on.
    group: RlvAttachGroup,
}

/// Every attachment point, in index order — the order `@getattach` reports its
/// bits in, because the reference walks a `std::map<S32, …>` keyed by index.
///
/// Names and indices are `avatar_lad.xml`'s `<attachment_point>` nodes,
/// lower-cased exactly as `RlvAttachPtLookup::initLookupTable`
/// (`rlvlocks.cpp:39`) lower-cases them.
const ATTACHMENT_POINTS: &[AttachmentPointRow] = &[
    row(1, "chest", RlvAttachGroup::Torso),
    row(2, "skull", RlvAttachGroup::Head),
    row(3, "left shoulder", RlvAttachGroup::Arms),
    row(4, "right shoulder", RlvAttachGroup::Arms),
    row(5, "left hand", RlvAttachGroup::Arms),
    row(6, "right hand", RlvAttachGroup::Arms),
    row(7, "left foot", RlvAttachGroup::Legs),
    row(8, "right foot", RlvAttachGroup::Legs),
    row(9, "spine", RlvAttachGroup::Torso),
    row(10, "pelvis", RlvAttachGroup::Torso),
    row(11, "mouth", RlvAttachGroup::Head),
    row(12, "chin", RlvAttachGroup::Head),
    row(13, "left ear", RlvAttachGroup::Head),
    row(14, "right ear", RlvAttachGroup::Head),
    row(15, "left eyeball", RlvAttachGroup::Head),
    row(16, "right eyeball", RlvAttachGroup::Head),
    row(17, "nose", RlvAttachGroup::Head),
    row(18, "r upper arm", RlvAttachGroup::Arms),
    row(19, "r forearm", RlvAttachGroup::Arms),
    row(20, "l upper arm", RlvAttachGroup::Arms),
    row(21, "l forearm", RlvAttachGroup::Arms),
    row(22, "right hip", RlvAttachGroup::Legs),
    row(23, "r upper leg", RlvAttachGroup::Legs),
    row(24, "r lower leg", RlvAttachGroup::Legs),
    row(25, "left hip", RlvAttachGroup::Legs),
    row(26, "l upper leg", RlvAttachGroup::Legs),
    row(27, "l lower leg", RlvAttachGroup::Legs),
    row(28, "stomach", RlvAttachGroup::Torso),
    row(29, "left pec", RlvAttachGroup::Torso),
    row(30, "right pec", RlvAttachGroup::Torso),
    row(31, "center 2", RlvAttachGroup::Hud),
    row(32, "top right", RlvAttachGroup::Hud),
    row(33, "top", RlvAttachGroup::Hud),
    row(34, "top left", RlvAttachGroup::Hud),
    row(35, "center", RlvAttachGroup::Hud),
    row(36, "bottom left", RlvAttachGroup::Hud),
    row(37, "bottom", RlvAttachGroup::Hud),
    row(38, "bottom right", RlvAttachGroup::Hud),
    row(39, "neck", RlvAttachGroup::Torso),
    row(40, "avatar center", RlvAttachGroup::Torso),
    row(41, "left ring finger", RlvAttachGroup::Arms),
    row(42, "right ring finger", RlvAttachGroup::Arms),
    row(43, "tail base", RlvAttachGroup::Torso),
    row(44, "tail tip", RlvAttachGroup::Torso),
    row(45, "left wing", RlvAttachGroup::Torso),
    row(46, "right wing", RlvAttachGroup::Torso),
    row(47, "jaw", RlvAttachGroup::Head),
    row(48, "alt left ear", RlvAttachGroup::Head),
    row(49, "alt right ear", RlvAttachGroup::Head),
    row(50, "alt left eye", RlvAttachGroup::Head),
    row(51, "alt right eye", RlvAttachGroup::Head),
    row(52, "tongue", RlvAttachGroup::Head),
    row(53, "groin", RlvAttachGroup::Torso),
    row(54, "left hind foot", RlvAttachGroup::Legs),
    row(55, "right hind foot", RlvAttachGroup::Legs),
];

/// One attachment-point table row, so the table above reads as a table.
const fn row(index: u8, name: &'static str, group: RlvAttachGroup) -> AttachmentPointRow {
    AttachmentPointRow { index, name, group }
}

/// The alias the RLV API accepts for the `avatar center` point
/// (`rlvlocks.cpp:58`: "the RLV API randomly renames Avatar Center to Root").
const ATTACHMENT_POINT_ROOT_ALIAS: &str = "root";

/// An avatar attachment point, named the way an RLV command names it.
///
/// The seam to the rest of the workspace is the **wire index** — the same byte
/// `AttachmentPoint::to_code` produces — so this crate stays free of the
/// protocol crates while still speaking about the same points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RlvAttachmentPoint(u8);

impl RlvAttachmentPoint {
    /// The point's wire index (1-based; `0` is "the item's default point" and is
    /// not a point of its own, so it never appears here).
    #[must_use]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The row backing this point. Every value of this type came from the
    /// table, so the lookup cannot fail; it is written as a search rather than
    /// an index so that it cannot be the place a bad index panics.
    fn table_row(self) -> Option<&'static AttachmentPointRow> {
        ATTACHMENT_POINTS.iter().find(|row| row.index == self.0)
    }

    /// The point with this wire index, if it is one the avatar has.
    #[must_use]
    pub fn from_index(index: u8) -> Option<Self> {
        ATTACHMENT_POINTS
            .iter()
            .find(|row| row.index == index)
            .map(|row| Self(row.index))
    }

    /// The point `name` spells, matched case-insensitively, with `root` also
    /// naming `avatar center` (`RlvAttachPtLookup::getAttachPointIndex`,
    /// `rlvlocks.h:407`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        if name == ATTACHMENT_POINT_ROOT_ALIAS {
            return Self::from_name("avatar center");
        }
        ATTACHMENT_POINTS
            .iter()
            .find(|row| row.name == name)
            .map(|row| Self(row.index))
    }

    /// The point's name, as `@getattachnames` reports it.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.table_row().map_or("", |row| row.name)
    }

    /// The body region the point is on.
    ///
    /// This is the anatomical answer, which is *not* what Firestorm's index
    /// table produces for the HUD and extended points; see the module docs.
    #[must_use]
    pub fn group(self) -> RlvAttachGroup {
        self.table_row()
            .map_or(RlvAttachGroup::Torso, |row| row.group)
    }

    /// Every attachment point, in the index order `@getattach` reports.
    pub fn all() -> impl Iterator<Item = Self> {
        ATTACHMENT_POINTS.iter().map(|row| Self(row.index))
    }
}

// ------------------------------------------------------------------ wearables

/// One row of the wearable-slot table.
struct WearableSlotRow {
    /// The `LLWearableType::EType` wire code.
    code: u8,
    /// The slot's name, as `@getoutfit:<layer>` spells it.
    name: &'static str,
    /// Whether the slot is a body part (an avatar always wears exactly one).
    body_part: bool,
}

/// Every wearable slot, in `LLWearableType::EType` order — the order
/// `@getoutfitnames` walks (`llwearabletype.h:44`, `llwearabletype.cpp:35`).
const WEARABLE_SLOTS: &[WearableSlotRow] = &[
    slot(0, "shape", true),
    slot(1, "skin", true),
    slot(2, "hair", true),
    slot(3, "eyes", true),
    slot(4, "shirt", false),
    slot(5, "pants", false),
    slot(6, "shoes", false),
    slot(7, "socks", false),
    slot(8, "jacket", false),
    slot(9, "gloves", false),
    slot(10, "undershirt", false),
    slot(11, "underpants", false),
    slot(12, "skirt", false),
    slot(13, "alpha", false),
    slot(14, "tattoo", false),
    slot(15, "physics", false),
    slot(16, "universal", false),
];

/// One wearable-slot table row, so the table above reads as a table.
const fn slot(code: u8, name: &'static str, body_part: bool) -> WearableSlotRow {
    WearableSlotRow {
        code,
        name,
        body_part,
    }
}

/// The order `@getoutfit` reports its bits in (`wtRlvTypes`,
/// `rlvhandler.cpp:3931`).
///
/// It is neither alphabetical nor the wire order: it is the RLV 1.x order,
/// frozen because every script that reads the answer indexes into it.
const GETOUTFIT_ORDER: [u8; 17] = [
    9,  // gloves
    8,  // jacket
    5,  // pants
    4,  // shirt
    6,  // shoes
    12, // skirt
    7,  // socks
    11, // underpants
    10, // undershirt
    1,  // skin
    3,  // eyes
    2,  // hair
    0,  // shape
    13, // alpha
    14, // tattoo
    15, // physics
    16, // universal
];

/// A wearable slot, named the way an RLV command names it.
///
/// As with [`RlvAttachmentPoint`], the seam to the rest of the workspace is the
/// wire code — the byte `WearableType::to_code` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RlvWearableSlot(u8);

impl RlvWearableSlot {
    /// The slot's `LLWearableType::EType` wire code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self.0
    }

    /// The row backing this slot; see [`RlvAttachmentPoint::table_row`].
    fn table_row(self) -> Option<&'static WearableSlotRow> {
        WEARABLE_SLOTS.iter().find(|row| row.code == self.0)
    }

    /// The slot with this wire code, if it is one this viewer wears.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        WEARABLE_SLOTS
            .iter()
            .find(|row| row.code == code)
            .map(|row| Self(row.code))
    }

    /// The slot `name` spells, matched case-insensitively
    /// (`LLWearableType::typeNameToType`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        WEARABLE_SLOTS
            .iter()
            .find(|row| row.name == name)
            .map(|row| Self(row.code))
    }

    /// The slot's name, as `@getoutfitnames` reports it.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.table_row().map_or("", |row| row.name)
    }

    /// Whether this is a body part rather than a clothing layer.
    ///
    /// `@getoutfit` never hides a body part, even with `RLVaHideLockedLayers`
    /// on: an avatar always wears one, so hiding it would be a lie rather than
    /// a secret.
    #[must_use]
    pub fn is_body_part(self) -> bool {
        self.table_row().is_some_and(|row| row.body_part)
    }

    /// Every slot, in wire order — the order `@getoutfitnames` walks.
    pub fn all() -> impl Iterator<Item = Self> {
        WEARABLE_SLOTS.iter().map(|row| Self(row.code))
    }

    /// Every slot, in the frozen order `@getoutfit` reports its bits in.
    pub fn getoutfit_order() -> impl Iterator<Item = Self> {
        GETOUTFIT_ORDER.into_iter().map(Self)
    }
}

// -------------------------------------------------------------------- queries

/// Which side of "worn" a `*names` query asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvNamesQuery {
    /// `@getattachnames` / `@getoutfitnames` — everything currently worn.
    Worn,
    /// `@getaddattachnames` / `@getaddoutfitnames` — everything that could be
    /// worn on, locks considered.
    Addable,
    /// `@getremattachnames` / `@getremoutfitnames` — everything worn that could
    /// be taken off again.
    Removable,
}

/// What `@getpath` was asked about (`RlvCommandOptionGetPath`,
/// `rlvhelper.cpp:1025`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[expect(
    variant_size_differences,
    reason = "an attachment is named by its id and the other targets by a one-byte table index; boxing the id to even them out would cost an allocation per query to save fifteen bytes"
)]
pub enum RlvPathTarget {
    /// No option: the object that issued the command.
    Issuer,
    /// A specific worn attachment, by object id.
    Attachment(Uuid),
    /// Everything worn on one attachment point.
    Point(RlvAttachmentPoint),
    /// Everything worn on one wearable slot.
    Slot(RlvWearableSlot),
}

/// Which version number `@versionnum` was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvVersionNum {
    /// No option — the RLV specification version.
    Specification,
    /// `:impl` — the RLVa implementation version.
    Implementation,
    /// Some other option. The reference does not fail on it, it answers with an
    /// empty string, so a script asking for a version this viewer does not know
    /// hears silence rather than an error.
    Unrecognised,
}

/// A decoded query: which question, and the options it was asked with.
///
/// Produced by [`RlvQuery::classify`], which is where every option string is
/// parsed and validated — so an answering routine only ever sees options that
/// made sense.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RlvQuery {
    /// `@version` / `@versionnew` — the handshake. `legacy` is the `@version`
    /// spelling, which answers `RestrainedLife` rather than `RestrainedLove`.
    Version {
        /// Whether the legacy `@version` spelling was used.
        legacy: bool,
    },
    /// `@versionnum[:impl]` — the packed version number.
    VersionNum {
        /// Which number was asked for.
        which: RlvVersionNum,
    },
    /// `@getstatus[:<filter>[;<separator>]]` and its `all` sibling.
    GetStatus {
        /// Whether every object's restrictions are reported (`@getstatusall`)
        /// rather than only the issuer's.
        all: bool,
        /// Only restrictions whose text contains this are reported.
        filter: String,
        /// Put in front of every reported restriction.
        separator: String,
    },
    /// `@getcommand[:<filter>[;<type>[;<separator>]]]` — the dictionary.
    GetCommand {
        /// Only keywords containing this are reported.
        filter: String,
        /// Only commands of this kind, or every kind for `None`.
        kind: Option<RlvParamKind>,
        /// Joins the reported keywords.
        separator: String,
    },
    /// `@getoutfit[:<layer>]` — one bit per wearable slot.
    GetOutfit {
        /// One slot, or every slot for `None`.
        slot: Option<RlvWearableSlot>,
    },
    /// `@getoutfitnames` and its add / rem siblings — slot names.
    GetOutfitNames {
        /// Which side of "worn" was asked about.
        which: RlvNamesQuery,
    },
    /// `@getattach[:<point>]` — one bit per attachment point.
    GetAttach {
        /// One point, or every point for `None`.
        point: Option<RlvAttachmentPoint>,
    },
    /// `@getattachnames[:<group>]` and its add / rem siblings — point names.
    GetAttachNames {
        /// Which side of "worn" was asked about.
        which: RlvNamesQuery,
        /// Only points in this body region, or every point for `None`.
        group: Option<RlvAttachGroup>,
    },
    /// `@getinv[:<path>]` — the folders directly under a shared folder.
    GetInv {
        /// The `#RLV`-relative folder path; empty is the shared root itself.
        path: String,
    },
    /// `@getinvworn[:<path>]` — the same folders, with how much of each is worn.
    GetInvWorn {
        /// The `#RLV`-relative folder path; empty is the shared root itself.
        path: String,
    },
    /// `@findfolder:<criteria>` / `@findfolders:<criteria>`.
    FindFolder {
        /// The search criteria, `&&`-joined words as the reference matches them.
        criteria: String,
        /// Whether every match is reported (`@findfolders`) rather than the
        /// deepest one.
        all: bool,
    },
    /// `@getpath[:<option>]` / `@getpathnew[:<option>]`.
    GetPath {
        /// What the path was asked for.
        target: RlvPathTarget,
        /// Whether every matching folder is reported (`@getpathnew`) rather
        /// than the first.
        all: bool,
    },
    /// `@getsitid` — the object the avatar is sitting on.
    GetSitId,
    /// `@getgroup` — the active group's name.
    GetGroup,
    /// `@getheightoffset` — the avatar hover height, in centimetres.
    GetHeightOffset,
    /// `@getcam_avdist` — how far the camera is from the avatar right now.
    GetCamAvdist,
    /// `@getcam_fov` — the camera's field of view right now.
    GetCamFov,
    /// `@getcam_avdistmin` / `avdistmax` / `fovmin` / `fovmax` — a limit some
    /// object set, read back off the modifier slot rather than off the camera.
    GetCamLimit {
        /// The modifier slot holding the limit.
        modifier: RlvModifier,
    },
    /// `@getcam_textures` — the texture `@setcam_textures` is forcing.
    GetCamTextures,
}

impl RlvQuery {
    /// Decode `command` as a query.
    ///
    /// # Errors
    ///
    /// `Err` carries the reference's own failure code: [`RlvOutcome::FailedParam`]
    /// for a command that is not a query at all, [`RlvOutcome::FailedOption`]
    /// for one whose option does not parse. Both still get an empty reply — see
    /// [`RlvState::answer`].
    ///
    /// ```
    /// # use sl_rlv::{RlvCommand, RlvQuery, RlvWearableSlot};
    /// let cmd = RlvCommand::parse_field("getoutfit:gloves=2222")?;
    /// assert_eq!(
    ///     RlvQuery::classify(&cmd),
    ///     Ok(RlvQuery::GetOutfit { slot: RlvWearableSlot::from_name("gloves") })
    /// );
    /// # Ok::<(), sl_rlv::RlvParseError>(())
    /// ```
    pub fn classify(command: &RlvCommand) -> Result<Self, RlvOutcome> {
        if command.param.kind() != RlvParamKind::Reply {
            return Err(RlvOutcome::FailedParam);
        }
        let option = command.option.as_deref().unwrap_or("");
        match command.behaviour {
            RlvBehaviour::Version => Ok(Self::Version { legacy: true }),
            RlvBehaviour::Versionnew => Ok(Self::Version { legacy: false }),
            RlvBehaviour::Versionnum => Ok(Self::VersionNum {
                which: match option {
                    "" => RlvVersionNum::Specification,
                    "impl" => RlvVersionNum::Implementation,
                    _ => RlvVersionNum::Unrecognised,
                },
            }),
            RlvBehaviour::Getstatus | RlvBehaviour::Getstatusall => {
                let (filter, separator) = parse_status_option(option);
                Ok(Self::GetStatus {
                    all: command.behaviour == RlvBehaviour::Getstatusall,
                    filter,
                    separator,
                })
            }
            RlvBehaviour::Getcommand => parse_getcommand_option(option),
            RlvBehaviour::Getoutfit => Ok(Self::GetOutfit {
                slot: parse_optional(option, RlvWearableSlot::from_name)?,
            }),
            RlvBehaviour::Getoutfitnames
            | RlvBehaviour::Getaddoutfitnames
            | RlvBehaviour::Getremoutfitnames => {
                // All three are optionless in the reference, which fails rather
                // than ignoring an option it was not expecting.
                if !option.is_empty() {
                    return Err(RlvOutcome::FailedOption);
                }
                Ok(Self::GetOutfitNames {
                    which: match command.behaviour {
                        RlvBehaviour::Getaddoutfitnames => RlvNamesQuery::Addable,
                        RlvBehaviour::Getremoutfitnames => RlvNamesQuery::Removable,
                        _ => RlvNamesQuery::Worn,
                    },
                })
            }
            RlvBehaviour::Getattach => Ok(Self::GetAttach {
                point: parse_optional(option, RlvAttachmentPoint::from_name)?,
            }),
            RlvBehaviour::Getattachnames
            | RlvBehaviour::Getaddattachnames
            | RlvBehaviour::Getremattachnames => Ok(Self::GetAttachNames {
                which: match command.behaviour {
                    RlvBehaviour::Getaddattachnames => RlvNamesQuery::Addable,
                    RlvBehaviour::Getremattachnames => RlvNamesQuery::Removable,
                    _ => RlvNamesQuery::Worn,
                },
                // An unrecognised group is not an error here: the reference
                // reads it as RLV_ATTACHGROUP_INVALID, which matches every
                // point, so `@getattachnames:frobnicate` lists the lot.
                group: RlvAttachGroup::from_name(option),
            }),
            RlvBehaviour::Getinv => Ok(Self::GetInv {
                path: option.to_owned(),
            }),
            RlvBehaviour::Getinvworn => Ok(Self::GetInvWorn {
                path: option.to_owned(),
            }),
            RlvBehaviour::Findfolder | RlvBehaviour::Findfolders => {
                // RLV 1.16.1 answered a criteria-less @findfolder with the
                // first folder it happened to find; RLVa refuses instead.
                if option.is_empty() {
                    return Err(RlvOutcome::FailedOption);
                }
                Ok(Self::FindFolder {
                    criteria: option.to_owned(),
                    all: command.behaviour == RlvBehaviour::Findfolders,
                })
            }
            RlvBehaviour::Getpath | RlvBehaviour::Getpathnew => Ok(Self::GetPath {
                target: parse_path_target(option)?,
                all: command.behaviour == RlvBehaviour::Getpathnew,
            }),
            RlvBehaviour::Getsitid => Ok(Self::GetSitId),
            RlvBehaviour::Getgroup => Ok(Self::GetGroup),
            RlvBehaviour::Getheightoffset => optionless(option, Self::GetHeightOffset),
            RlvBehaviour::GetcamAvdist => optionless(option, Self::GetCamAvdist),
            RlvBehaviour::GetcamFov => optionless(option, Self::GetCamFov),
            RlvBehaviour::GetcamTextures => optionless(option, Self::GetCamTextures),
            RlvBehaviour::GetcamAvdistmin => cam_limit(option, RlvModifier::SetcamAvdistmin),
            RlvBehaviour::GetcamAvdistmax => cam_limit(option, RlvModifier::SetcamAvdistmax),
            RlvBehaviour::GetcamFovmin => cam_limit(option, RlvModifier::SetcamFovmin),
            RlvBehaviour::GetcamFovmax => cam_limit(option, RlvModifier::SetcamFovmax),
            _ => Err(RlvOutcome::FailedParam),
        }
    }
}

/// A query that takes no option at all, and fails if given one.
fn optionless(option: &str, query: RlvQuery) -> Result<RlvQuery, RlvOutcome> {
    if option.is_empty() {
        Ok(query)
    } else {
        Err(RlvOutcome::FailedOption)
    }
}

/// One of the `@getcam_*` limit queries, which read back a modifier slot.
fn cam_limit(option: &str, modifier: RlvModifier) -> Result<RlvQuery, RlvOutcome> {
    optionless(option, RlvQuery::GetCamLimit { modifier })
}

/// An option that may be absent, and must otherwise name something `lookup`
/// knows. An option present but unrecognised is a failure, not an absent one —
/// `@getoutfit:frobnicate` must not be answered as `@getoutfit`.
fn parse_optional<T>(
    option: &str,
    lookup: impl Fn(&str) -> Option<T>,
) -> Result<Option<T>, RlvOutcome> {
    if option.is_empty() {
        return Ok(None);
    }
    lookup(option).map(Some).ok_or(RlvOutcome::FailedOption)
}

/// `@getstatus[:<filter>[;<separator>]]` — both halves optional, the separator
/// defaulting to `/` even when the option ends with a bare `;`
/// (`rlvParseGetStatusOption`, `rlvhandler.cpp:118`).
fn parse_status_option(option: &str) -> (String, String) {
    // The reference tokenises on every `;` and reads only the first two, so a
    // third field is ignored rather than becoming part of the separator.
    let mut fields = option.split(OPTION_SEPARATOR);
    let filter = fields.next().unwrap_or("");
    let separator = match fields.next() {
        Some(separator) if !separator.is_empty() => separator,
        _ => STATUS_SEPARATOR,
    };
    (filter.to_owned(), separator.to_owned())
}

/// `@getcommand[:<filter>[;<type>[;<separator>]]]`
/// (`RlvReplyHandler<RLV_BHVR_GETCOMMAND>`, `rlvhandler.cpp:3770`).
fn parse_getcommand_option(option: &str) -> Result<RlvQuery, RlvOutcome> {
    let mut fields = option.split(OPTION_SEPARATOR);
    let filter = fields.next().unwrap_or("").to_owned();
    let kind = match fields.next() {
        None | Some("" | "any") => None,
        Some("add") => Some(RlvParamKind::AddRem),
        Some("force") => Some(RlvParamKind::Force),
        Some("reply") => Some(RlvParamKind::Reply),
        Some(_) => return Err(RlvOutcome::FailedOption),
    };
    let separator = fields
        .next()
        .map_or_else(|| OPTION_SEPARATOR.to_string(), ToOwned::to_owned);
    Ok(RlvQuery::GetCommand {
        filter,
        kind,
        separator,
    })
}

/// `@getpath[:<option>]` — a wearable slot, an attachment point, an attachment
/// by id, or nothing (the issuing object).
fn parse_path_target(option: &str) -> Result<RlvPathTarget, RlvOutcome> {
    if option.is_empty() {
        return Ok(RlvPathTarget::Issuer);
    }
    if let Some(slot) = RlvWearableSlot::from_name(option) {
        return Ok(RlvPathTarget::Slot(slot));
    }
    if let Some(point) = RlvAttachmentPoint::from_name(option) {
        return Ok(RlvPathTarget::Point(point));
    }
    // The reference insists on the 36-character hyphenated form, as it does
    // everywhere else an option names an id.
    if option.len() == 36
        && let Ok(id) = Uuid::parse_str(option)
    {
        return Ok(RlvPathTarget::Attachment(id));
    }
    Err(RlvOutcome::FailedOption)
}

// --------------------------------------------------------------------- source

/// How much of one shared folder is worn (`rlv_wear_info`,
/// `rlvhandler.cpp:3844`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RlvFolderWearCounts {
    /// Wearable items directly in the folder that are worn.
    pub worn: u32,
    /// Wearable items directly in the folder, worn or not.
    pub total: u32,
    /// Worn wearable items in folders *below* this one.
    pub child_worn: u32,
    /// Wearable items in folders below this one, worn or not.
    pub child_total: u32,
}

impl RlvFolderWearCounts {
    /// The two digits `@getinvworn` reports this folder as: how much of the
    /// folder itself is worn, then how much of everything under it.
    ///
    /// `0` nothing to wear, `1` nothing worn, `2` some worn, `3` all worn.
    #[must_use]
    pub const fn digits(self) -> (u8, u8) {
        (
            wear_digit(self.worn, self.total),
            wear_digit(self.child_worn, self.child_total),
        )
    }
}

/// One digit of a `@getinvworn` answer.
const fn wear_digit(worn: u32, total: u32) -> u8 {
    if total == 0 {
        0
    } else if worn == 0 {
        1
    } else if worn == total {
        3
    } else {
        2
    }
}

/// One folder below the one `@getinvworn` asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvFolderWearChild {
    /// The folder's name, as it is reported.
    pub name: String,
    /// How much of it is worn.
    pub counts: RlvFolderWearCounts,
}

impl RlvFolderWearChild {
    /// A child folder with these counts.
    #[must_use]
    pub const fn new(name: String, counts: RlvFolderWearCounts) -> Self {
        Self { name, counts }
    }
}

/// What `@getinvworn` found: the queried folder's own items, and every folder
/// below it.
///
/// The queried folder's *child* counts are not carried, because they are the
/// sum of the children's and this crate adds them up — one place to be wrong is
/// better than two that can disagree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct RlvFolderWear {
    /// Wearable items directly in the queried folder that are worn.
    pub worn: u32,
    /// Wearable items directly in the queried folder, worn or not.
    pub total: u32,
    /// Every folder below the queried one, in the order it is reported.
    pub children: Vec<RlvFolderWearChild>,
}

/// The facts a query needs that the state machine cannot know.
///
/// The consumer answers each from whatever it has — a Bevy scene mirror, a
/// headless session, a test fixture. Every method answers about *this agent*
/// right now; nothing here mutates, and a query is expected to be cheap.
///
/// The four `can_*` questions are the lock model's, and a consumer that has
/// built an [`RlvLocks`](crate::RlvLocks) should answer them by asking it —
/// [`RlvLocks::can_attach`](crate::RlvLocks::can_attach),
/// [`can_detach`](crate::RlvLocks::can_detach),
/// [`can_wear`](crate::RlvLocks::can_wear) and
/// [`can_remove`](crate::RlvLocks::can_remove) — rather than by guessing. They
/// are asked separately from "is it worn" because `@getaddattachnames` wants
/// the points that are free and `@getremattachnames` the ones that would let
/// go.
pub trait RlvQuerySource {
    /// The agent's own id — an object issuing a query *as* the agent (the RLV
    /// debug console does) is allowed to reply on channel `0`.
    fn agent(&self) -> Uuid;

    /// How many objects are attached at `point`.
    fn attachment_count(&self, point: RlvAttachmentPoint) -> u32;

    /// Whether something could be attached at `point`
    /// (`RlvAttachmentLocks::canAttach`).
    fn can_attach(&self, point: RlvAttachmentPoint) -> bool;

    /// Whether something attached at `point` could be taken off, ignoring any
    /// lock held by `except` (`RlvForceWear::isForceDetachable`).
    fn can_detach(&self, point: RlvAttachmentPoint, except: Option<Uuid>) -> bool;

    /// How many wearables are worn on `slot`.
    fn wearable_count(&self, slot: RlvWearableSlot) -> u32;

    /// Whether something could be worn on `slot` (`RlvWearableLocks::canWear`).
    fn can_wear(&self, slot: RlvWearableSlot) -> bool;

    /// Whether something worn on `slot` could be taken off, ignoring any lock
    /// held by `except` (`RlvForceWear::isForceRemovable`).
    fn can_remove(&self, slot: RlvWearableSlot, except: Option<Uuid>) -> bool;

    /// Whether `RLVaHideLockedAttachments` is on, which makes `@getattach`
    /// report a locked-on attachment as *not* worn.
    fn hide_locked_attachments(&self) -> bool;

    /// Whether `RLVaHideLockedLayers` is on, which makes `@getoutfit` report a
    /// locked-on clothing layer as *not* worn. Body parts are never hidden.
    fn hide_locked_layers(&self) -> bool;

    /// The root object the avatar is sitting on, if it is sitting.
    fn sit_target(&self) -> Option<Uuid>;

    /// The active group's name, if any is active.
    fn active_group(&self) -> Option<String>;

    /// The avatar's hover offset in metres, or `None` if the avatar is not
    /// ready to be asked.
    fn hover_height(&self) -> Option<f32>;

    /// How far the camera is from the avatar right now, in metres.
    fn camera_avatar_distance(&self) -> Option<f32>;

    /// The camera's current field of view, in radians.
    fn camera_field_of_view(&self) -> Option<f32>;

    /// Whether a `#RLV` shared root folder exists at all. Without one the
    /// inventory queries fail differently — the object is told the viewer has
    /// nothing shared rather than that its path was wrong.
    fn has_shared_root(&self) -> bool;

    /// The folders directly under the shared folder at `path` (empty for the
    /// shared root), in inventory order, or `None` if there is no such folder.
    ///
    /// Hidden and invalid names are filtered by this crate, not here.
    ///
    /// Every path reaching this trait is **lower-case**: the reference
    /// lower-cases the whole owner-say line before parsing it
    /// (`llviewermessage.cpp:3146`), so folder names have to be matched
    /// case-insensitively — the names answered with keep their real casing,
    /// because that is what is chatted back.
    fn shared_folder_children(&self, path: &str) -> Option<Vec<String>>;

    /// What is worn under the shared folder at `path`, or `None` if there is no
    /// such folder.
    fn shared_folder_wear(&self, path: &str) -> Option<RlvFolderWear>;

    /// Every shared folder matching `criteria`, as `#RLV`-relative paths.
    fn find_shared_folders(&self, criteria: &str) -> Vec<String>;

    /// The `#RLV`-relative paths of the folders holding what `target` names,
    /// with `issuer` standing in for [`RlvPathTarget::Issuer`].
    fn shared_paths_of(&self, target: RlvPathTarget, issuer: Uuid) -> Vec<String>;
}

// --------------------------------------------------------------------- answer

/// One line to chat back.
///
/// A `channel` of `0` is not a chat channel: it is the RLV debug console, which
/// only an object running as the agent itself can reach
/// (`RlvFloaterConsole::addCommandReply`, `rlvhandler.cpp:3441`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvReply {
    /// The channel the query named.
    pub channel: i32,
    /// The answer, already truncated to [`MAX_CHAT_BYTES`].
    pub message: String,
}

/// What answering a query produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvAnswer {
    /// The line to chat, or `None` when the channel was not one a reply may go
    /// on — the one case where a script is left waiting, because there is
    /// nowhere to answer it.
    pub reply: Option<RlvReply>,
    /// How the query went. A failure still carries an empty [`RlvReply`].
    pub outcome: RlvOutcome,
}

/// Whether `channel` is one a reply may be chatted on
/// (`RlvUtil::isValidReplyChannel`, `rlvcommon.h:340`).
///
/// `loopback` is for a command the agent issued to itself, which may use channel
/// `0` to mean the debug console.
#[must_use]
pub const fn is_valid_reply_channel(channel: i32, loopback: bool) -> bool {
    let floor = if loopback { -1 } else { 0 };
    channel > floor && channel != CHAT_CHANNEL_DEBUG
}

/// Truncate `text` to [`MAX_CHAT_BYTES`] without splitting a character, as a
/// shout is truncated (`utf8str_truncate`, `llstring.cpp:540`).
#[must_use]
pub fn truncate_chat(text: &str) -> &str {
    if text.len() <= MAX_CHAT_BYTES {
        return text;
    }
    let mut end = MAX_CHAT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or("")
}

/// Split `text` into chat-sized lines, breaking at `separator` where one is
/// close enough (`utf8str_split`, `llstring.cpp:602`).
///
/// This is what `@redirchat` sends long chat with; a **query** is answered with
/// a single truncated line, because that is what `RlvUtil::sendChatReply` does.
#[must_use]
pub fn split_chat(text: &str, separator: char) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.len() <= MAX_CHAT_BYTES {
            lines.push(rest.to_owned());
            break;
        }
        let head = truncate_chat(rest);
        // Prefer the last separator in the window, exactly as the reference
        // walks back from `maxlen` — which is one byte *past* the longest line,
        // so a separator sitting exactly there is a clean break too. With none,
        // or one at the very front, the window is taken whole.
        let window = rest.get(..=MAX_CHAT_BYTES).unwrap_or(head);
        let take = window.rfind(separator).unwrap_or(head.len());
        let take = if take == 0 { head.len() } else { take };
        let (line, tail) = rest.split_at_checked(take).unwrap_or((rest, ""));
        lines.push(line.to_owned());
        rest = tail.strip_prefix(separator).unwrap_or(tail);
    }
    lines
}

/// Answer one query, as `RlvUtil::sendChatReply` would — the body of
/// [`RlvState::answer`], which is where it is documented.
pub(crate) fn answer(
    state: &RlvState,
    issuer: Uuid,
    command: &RlvCommand,
    source: &impl RlvQuerySource,
) -> RlvAnswer {
    let RlvParam::Reply { channel } = command.param else {
        return RlvAnswer {
            reply: None,
            outcome: RlvOutcome::FailedParam,
        };
    };
    if !is_valid_reply_channel(channel, issuer == source.agent()) {
        return RlvAnswer {
            reply: None,
            outcome: RlvOutcome::FailedParam,
        };
    }

    let (message, outcome) = match RlvQuery::classify(command) {
        Ok(query) => answer_query(state, issuer, &query, source),
        Err(outcome) => (String::new(), outcome),
    };
    RlvAnswer {
        reply: Some(RlvReply {
            channel,
            message: truncate_chat(&message).to_owned(),
        }),
        outcome,
    }
}

/// The text one already-decoded query answers with — the body of
/// [`RlvState::answer_query`], which is where it is documented.
pub(crate) fn answer_query(
    state: &RlvState,
    issuer: Uuid,
    query: &RlvQuery,
    source: &impl RlvQuerySource,
) -> (String, RlvOutcome) {
    let ok = |text: String| (text, RlvOutcome::Success);
    match *query {
        RlvQuery::Version { legacy } => ok(version_reply(legacy, false)),
        RlvQuery::VersionNum { which } => ok(match which {
            RlvVersionNum::Specification => version_num_reply(false),
            RlvVersionNum::Implementation => version_impl_num_reply(),
            RlvVersionNum::Unrecognised => String::new(),
        }),
        RlvQuery::GetStatus {
            all,
            ref filter,
            ref separator,
        } => ok(if all {
            state.status_string_all(filter, separator)
        } else {
            state.status_string(issuer, filter, separator)
        }),
        RlvQuery::GetCommand {
            ref filter,
            kind,
            ref separator,
        } => ok(state.known_commands(filter, kind).join(separator)),
        RlvQuery::GetOutfit { slot } => ok(get_outfit(issuer, slot, source)),
        RlvQuery::GetOutfitNames { which } => ok(outfit_names(which, source)),
        RlvQuery::GetAttach { point } => ok(get_attach(issuer, point, source)),
        RlvQuery::GetAttachNames { which, group } => ok(attach_names(which, group, source)),
        RlvQuery::GetInv { ref path } => match source.shared_folder_children(path) {
            Some(children) => ok(children
                .into_iter()
                .filter(|name| is_listable_folder(name))
                .collect::<Vec<_>>()
                .join(",")),
            None => (String::new(), no_such_folder(source)),
        },
        RlvQuery::GetInvWorn { ref path } => match source.shared_folder_wear(path) {
            Some(wear) => ok(format_inv_worn(&wear)),
            None => (String::new(), no_such_folder(source)),
        },
        RlvQuery::FindFolder { ref criteria, all } => {
            let found = source.find_shared_folders(criteria);
            ok(if all {
                found.join(",")
            } else {
                // "In depth": whoever has the most '/' wins, and a child of the
                // shared root (no '/' at all) still beats nothing.
                found
                    .into_iter()
                    .max_by_key(|path| path.matches(FOLDER_INVALID_CHAR).count())
                    .unwrap_or_default()
            })
        }
        RlvQuery::GetPath { target, all } => {
            let found = source.shared_paths_of(target, issuer);
            ok(if all {
                found.join(",")
            } else {
                found.into_iter().next().unwrap_or_default()
            })
        }
        RlvQuery::GetSitId => ok(source
            .sit_target()
            .unwrap_or(Uuid::nil())
            .as_hyphenated()
            .to_string()),
        // RLV-1.16.1 answers "none" rather than an empty string, so a script can
        // tell "no group" from "the viewer did not understand".
        RlvQuery::GetGroup => ok(source.active_group().unwrap_or_else(|| "none".to_owned())),
        RlvQuery::GetHeightOffset => match source.hover_height() {
            Some(metres) => ok(format!("{:.2}", metres * 100.0)),
            None => (String::new(), RlvOutcome::Failed),
        },
        RlvQuery::GetCamAvdist => match source.camera_avatar_distance() {
            Some(distance) => ok(format!("{distance:.3}")),
            None => (String::new(), RlvOutcome::Failed),
        },
        RlvQuery::GetCamFov => match source.camera_field_of_view() {
            Some(fov) => ok(format!("{fov:.3}")),
            None => (String::new(), RlvOutcome::Failed),
        },
        RlvQuery::GetCamLimit { modifier } => ok(modifier_reply(state, modifier)),
        RlvQuery::GetCamTextures => ok(
            match state
                .modifiers()
                .value(RlvModifier::SetcamTexture)
                .as_uuid()
            {
                Some(texture) if state.modifiers().has_value(RlvModifier::SetcamTexture) => {
                    texture.as_hyphenated().to_string()
                }
                _ => String::new(),
            },
        ),
    }
}

/// A `@getcam_*` limit: the value some object set, or nothing at all.
///
/// An unset limit answers with an empty string rather than the default the
/// modifier would fall back to, so a script can tell "nobody has limited this"
/// from "somebody has limited it to the default".
fn modifier_reply(state: &RlvState, modifier: RlvModifier) -> String {
    match state.modifiers().value(modifier).as_float() {
        Some(value) if state.modifiers().has_value(modifier) => format!("{value:.3}"),
        _ => String::new(),
    }
}

/// Why an inventory query found no folder: a wrong path, or no `#RLV` folder to
/// look in at all.
fn no_such_folder(source: &impl RlvQuerySource) -> RlvOutcome {
    if source.has_shared_root() {
        RlvOutcome::FailedOption
    } else {
        RlvOutcome::FailedNoSharedRoot
    }
}

/// `@getoutfit[:<layer>]` — one `0` or `1` per slot, in the frozen RLV order
/// (`RlvHandler::onGetOutfit`, `rlvhandler.cpp:3923`).
fn get_outfit(issuer: Uuid, only: Option<RlvWearableSlot>, source: &impl RlvQuerySource) -> String {
    RlvWearableSlot::getoutfit_order()
        .filter(|slot| only.is_none_or(|wanted| wanted == *slot))
        .map(|slot| {
            let worn = source.wearable_count(slot) > 0
                && (!source.hide_locked_layers()
                    || slot.is_body_part()
                    || source.can_remove(slot, Some(issuer)));
            if worn { '1' } else { '0' }
        })
        .collect()
}

/// `@getoutfitnames` and siblings — the slot names, comma-joined
/// (`RlvHandler::onGetOutfitNames`, `rlvhandler.cpp:3959`).
fn outfit_names(which: RlvNamesQuery, source: &impl RlvQuerySource) -> String {
    RlvWearableSlot::all()
        .filter(|&slot| match which {
            RlvNamesQuery::Worn => source.wearable_count(slot) > 0,
            RlvNamesQuery::Addable => source.can_wear(slot),
            RlvNamesQuery::Removable => source.can_remove(slot, None),
        })
        .map(RlvWearableSlot::name)
        .collect::<Vec<_>>()
        .join(",")
}

/// `@getattach[:<point>]` — one `0` or `1` per point, in index order
/// (`RlvHandler::onGetAttach`, `rlvhandler.cpp:3616`).
///
/// Asking about every point prefixes a `0` for the nonexistent point `0`, which
/// is the historical quirk that makes the answer 1-indexed.
fn get_attach(
    issuer: Uuid,
    only: Option<RlvAttachmentPoint>,
    source: &impl RlvQuerySource,
) -> String {
    let mut reply = String::new();
    if only.is_none() {
        reply.push('0');
    }
    for point in
        RlvAttachmentPoint::all().filter(|point| only.is_none_or(|wanted| wanted == *point))
    {
        let worn = source.attachment_count(point) > 0
            && (!source.hide_locked_attachments() || source.can_detach(point, Some(issuer)));
        reply.push(if worn { '1' } else { '0' });
    }
    reply
}

/// `@getattachnames` and siblings — the point names, comma-joined
/// (`RlvHandler::onGetAttachNames`, `rlvhandler.cpp:3650`).
fn attach_names(
    which: RlvNamesQuery,
    group: Option<RlvAttachGroup>,
    source: &impl RlvQuerySource,
) -> String {
    RlvAttachmentPoint::all()
        .filter(|point| group.is_none_or(|wanted| wanted == point.group()))
        .filter(|&point| match which {
            RlvNamesQuery::Worn => source.attachment_count(point) > 0,
            RlvNamesQuery::Addable => source.can_attach(point),
            RlvNamesQuery::Removable => source.can_detach(point, None),
        })
        .map(RlvAttachmentPoint::name)
        .collect::<Vec<_>>()
        .join(",")
}

/// Whether a shared folder's name may be listed by `@getinv`: not hidden, and
/// with nothing in it that would break a path (`RlvHandler::onGetInv`,
/// `rlvhandler.cpp:3825`).
fn is_listable_folder(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(FOLDER_PREFIX_HIDDEN)
        && !name.contains(FOLDER_INVALID_CHAR)
}

/// `@getinvworn` — `|<own><children>` for the queried folder, then
/// `,<name>|<own><children>` per folder below it
/// (`RlvHandler::onGetInvWorn`, `rlvhandler.cpp:3848`).
fn format_inv_worn(wear: &RlvFolderWear) -> String {
    let mut root = RlvFolderWearCounts {
        worn: wear.worn,
        total: wear.total,
        child_worn: 0,
        child_total: 0,
    };
    let mut children = String::new();
    for child in &wear.children {
        root.child_worn = root
            .child_worn
            .saturating_add(child.counts.worn)
            .saturating_add(child.counts.child_worn);
        root.child_total = root
            .child_total
            .saturating_add(child.counts.total)
            .saturating_add(child.counts.child_total);
        let (own, below) = child.counts.digits();
        children.push(',');
        children.push_str(&child.name);
        children.push('|');
        children.push_str(&own.to_string());
        children.push_str(&below.to_string());
    }
    let (own, below) = root.digits();
    format!("|{own}{below}{children}")
}

// ------------------------------------------------------------------ IM queries

/// A query an avatar sent by **instant message** rather than an object by chat
/// (`RlvHandler::processIMQuery`, `rlvhandler.cpp:652`).
///
/// This is a courtesy channel between people, not the scripted API: someone
/// wondering why you will not answer can ask what you are under. It is gated by
/// `RLVaEnableIMQuery`, and the two list forms need the user's consent before
/// anything is sent, so the consumer decides — this only says what was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RlvImQuery {
    /// `@stopim` — "close this conversation". Answered even with IM queries
    /// off, because the sender needs to know the session is going away.
    StopIm,
    /// `@version` — the same handshake an object asks for, by IM.
    Version,
    /// `@list` — everything in force. Needs the user's consent.
    List,
    /// `@except` — the exceptions in force. Needs the user's consent.
    Except,
}

impl RlvImQuery {
    /// The query `message` is, if it is one.
    ///
    /// The reference compares the whole message, so a line that merely *starts*
    /// with `@version` is ordinary IM text and stays visible.
    #[must_use]
    pub const fn classify(message: &str) -> Option<Self> {
        match message.as_bytes() {
            b"@stopim" => Some(Self::StopIm),
            b"@version" => Some(Self::Version),
            b"@list" => Some(Self::List),
            b"@except" => Some(Self::Except),
            _ => None,
        }
    }

    /// Whether answering this needs the user's say-so first.
    #[must_use]
    pub const fn needs_consent(self) -> bool {
        matches!(self, Self::List | Self::Except)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        MAX_CHAT_BYTES, RlvAttachGroup, RlvAttachmentPoint, RlvFolderWear, RlvFolderWearChild,
        RlvFolderWearCounts, RlvImQuery, RlvNamesQuery, RlvPathTarget, RlvQuery, RlvQuerySource,
        RlvVersionNum, RlvWearableSlot, is_valid_reply_channel, split_chat, truncate_chat,
    };
    use crate::{
        RlvCommand, RlvModifier, RlvOutcome, RlvParamKind, RlvState, parse_chat_line, version_reply,
    };
    use uuid::Uuid;

    /// A boxed error so tests can use `?` instead of the disallowed `unwrap`.
    type TestError = Box<dyn core::error::Error>;

    /// The agent this fixture speaks for.
    const AGENT: Uuid = Uuid::from_u128(0x0a);

    /// A source with a fixed, hand-set outfit — enough to pin every format.
    #[derive(Debug, Default)]
    struct Fixture {
        /// Attachment points with something on them.
        attached: Vec<u8>,
        /// Wearable slots with something on them.
        worn: Vec<u8>,
        /// Points and slots that are locked on (nothing may be removed).
        locked: bool,
        /// Whether the hide-locked settings are on.
        hide_locked: bool,
        /// The object being sat on.
        sitting_on: Option<Uuid>,
        /// The active group's name.
        group: Option<String>,
        /// Whether a `#RLV` folder exists.
        shared_root: bool,
        /// The folders under the queried path.
        children: Option<Vec<String>>,
        /// What is worn under the queried path.
        wear: Option<RlvFolderWear>,
        /// What a folder search finds.
        found: Vec<String>,
    }

    impl RlvQuerySource for Fixture {
        fn agent(&self) -> Uuid {
            AGENT
        }
        fn attachment_count(&self, point: RlvAttachmentPoint) -> u32 {
            u32::from(self.attached.contains(&point.index()))
        }
        fn can_attach(&self, point: RlvAttachmentPoint) -> bool {
            !self.locked && !self.attached.contains(&point.index())
        }
        fn can_detach(&self, point: RlvAttachmentPoint, _except: Option<Uuid>) -> bool {
            !self.locked && self.attached.contains(&point.index())
        }
        fn wearable_count(&self, slot: RlvWearableSlot) -> u32 {
            u32::from(self.worn.contains(&slot.code()))
        }
        fn can_wear(&self, slot: RlvWearableSlot) -> bool {
            !self.locked && !self.worn.contains(&slot.code())
        }
        fn can_remove(&self, slot: RlvWearableSlot, _except: Option<Uuid>) -> bool {
            !self.locked && self.worn.contains(&slot.code())
        }
        fn hide_locked_attachments(&self) -> bool {
            self.hide_locked
        }
        fn hide_locked_layers(&self) -> bool {
            self.hide_locked
        }
        fn sit_target(&self) -> Option<Uuid> {
            self.sitting_on
        }
        fn active_group(&self) -> Option<String> {
            self.group.clone()
        }
        fn hover_height(&self) -> Option<f32> {
            Some(0.125)
        }
        fn camera_avatar_distance(&self) -> Option<f32> {
            Some(3.5)
        }
        fn camera_field_of_view(&self) -> Option<f32> {
            Some(1.0)
        }
        fn has_shared_root(&self) -> bool {
            self.shared_root
        }
        fn shared_folder_children(&self, _path: &str) -> Option<Vec<String>> {
            self.children.clone()
        }
        fn shared_folder_wear(&self, _path: &str) -> Option<RlvFolderWear> {
            self.wear.clone()
        }
        fn find_shared_folders(&self, _criteria: &str) -> Vec<String> {
            self.found.clone()
        }
        fn shared_paths_of(&self, _target: RlvPathTarget, _issuer: Uuid) -> Vec<String> {
            self.found.clone()
        }
    }

    /// The object standing in for a collar in these tests.
    const COLLAR: Uuid = Uuid::from_u128(1);

    /// The first command of a chat line, owned so it outlives the parse.
    fn first_command(line: &str) -> Result<RlvCommand, TestError> {
        let commands = parse_chat_line(line).ok_or("not an rlv line")?;
        let command = commands.into_iter().next().ok_or("no command")?;
        Ok(command.map_err(|error| error.to_string())?)
    }

    /// Answer `line` against `state` and `source`, as the collar.
    fn answer(
        state: &RlvState,
        source: &Fixture,
        line: &str,
    ) -> Result<(String, RlvOutcome), TestError> {
        answer_from(state, source, COLLAR, line)
    }

    /// Answer `line` as `issuer`.
    fn answer_from(
        state: &RlvState,
        source: &Fixture,
        issuer: Uuid,
        line: &str,
    ) -> Result<(String, RlvOutcome), TestError> {
        let answered = state.answer(issuer, &first_command(line)?, source);
        let message = answered
            .reply
            .map(|reply| reply.message)
            .ok_or("no reply")?;
        Ok((message, answered.outcome))
    }

    #[test]
    fn version_handshake_is_answered_from_the_crate_alone() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture::default();
        assert_eq!(
            answer(&state, &source, "@version=2222")?,
            (version_reply(true, false), RlvOutcome::Success)
        );
        assert_eq!(
            answer(&state, &source, "@versionnew=2222")?,
            (version_reply(false, false), RlvOutcome::Success)
        );
        assert_eq!(
            answer(&state, &source, "@versionnum=2222")?,
            ("3040300".to_owned(), RlvOutcome::Success)
        );
        assert_eq!(
            answer(&state, &source, "@versionnum:impl=2222")?,
            ("2040213".to_owned(), RlvOutcome::Success)
        );
        // An option it does not know is silence, not a failure.
        assert_eq!(
            answer(&state, &source, "@versionnum:frobnicate=2222")?,
            (String::new(), RlvOutcome::Success)
        );
        Ok(())
    }

    #[test]
    fn getstatus_reports_the_issuer_and_getstatusall_everyone() -> Result<(), TestError> {
        let cuffs = Uuid::from_u128(2);
        let mut state = RlvState::new();
        for (object, line) in [
            (COLLAR, "@detach=n"),
            (COLLAR, "@tplm=n"),
            (cuffs, "@fly=n"),
        ] {
            state.apply(object, &first_command(line)?);
        }
        let source = Fixture::default();

        assert_eq!(
            answer(&state, &source, "@getstatus=2222")?.0,
            "/detach/tplm"
        );
        // The filter matches the reported text, and the separator is the second
        // half of the option.
        assert_eq!(answer(&state, &source, "@getstatus:tp=2222")?.0, "/tplm");
        assert_eq!(
            answer(&state, &source, "@getstatus:;|=2222")?.0,
            "|detach|tplm"
        );
        // A bare `;` leaves the default separator in place, and a third field
        // is ignored rather than joining the separator.
        assert_eq!(answer(&state, &source, "@getstatus:tp;=2222")?.0, "/tplm");
        assert_eq!(
            answer(&state, &source, "@getstatus:;|;x=2222")?.0,
            "|detach|tplm"
        );
        // An object holding nothing gets an empty string, not a bare separator.
        assert_eq!(
            answer_from(&state, &source, Uuid::from_u128(9), "@getstatus=2222")?.0,
            ""
        );
        assert_eq!(
            answer(&state, &source, "@getstatusall=2222")?.0,
            "/detach/tplm/fly"
        );
        Ok(())
    }

    #[test]
    fn getcommand_reads_the_dictionary() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture::default();

        let (message, outcome) = answer(&state, &source, "@getcommand:tplure;add=2222")?;
        assert_eq!(outcome, RlvOutcome::Success);
        assert_eq!(message, "tplure;tplure_sec");

        // The third field is the separator.
        assert_eq!(
            answer(&state, &source, "@getcommand:tplure;add;+=2222")?.0,
            "tplure+tplure_sec"
        );
        // An empty type field is "any kind".
        assert_eq!(
            answer(&state, &source, "@getcommand:getstatusall;=2222")?.0,
            "getstatusall"
        );
        // An unknown type field is the one option failure here.
        assert_eq!(
            answer(&state, &source, "@getcommand:tplure;frobnicate=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn getoutfit_is_a_bit_per_slot_in_the_frozen_order() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            // gloves (9) and shape (0): first and thirteenth bit.
            worn: vec![9, 0],
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &source, "@getoutfit=2222")?.0,
            "10000000000010000"
        );
        assert_eq!(answer(&state, &source, "@getoutfit:gloves=2222")?.0, "1");
        assert_eq!(answer(&state, &source, "@getoutfit:skirt=2222")?.0, "0");
        // A layer name it does not know fails, and still replies.
        assert_eq!(
            answer(&state, &source, "@getoutfit:frobnicate=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn getoutfit_hides_a_locked_layer_but_never_a_body_part() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            worn: vec![9, 0],
            locked: true,
            hide_locked: true,
            ..Fixture::default()
        };
        // The gloves are locked on and so reported as not worn; the shape is a
        // body part and is reported honestly.
        assert_eq!(
            answer(&state, &source, "@getoutfit=2222")?.0,
            "00000000000010000"
        );
        Ok(())
    }

    #[test]
    fn getattach_is_one_indexed_by_a_leading_zero() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            // chest (1) and skull (2).
            attached: vec![1, 2],
            ..Fixture::default()
        };
        let (message, outcome) = answer(&state, &source, "@getattach=2222")?;
        assert_eq!(outcome, RlvOutcome::Success);
        assert_eq!(
            message.len(),
            56,
            "one bit per point, plus the leading zero"
        );
        assert!(message.starts_with("0110"), "{message}");

        // One point is one bit, with no leading zero to index past.
        assert_eq!(answer(&state, &source, "@getattach:chest=2222")?.0, "1");
        assert_eq!(answer(&state, &source, "@getattach:spine=2222")?.0, "0");
        // The RLV API's alias for the avatar-centre point.
        assert_eq!(answer(&state, &source, "@getattach:root=2222")?.0, "0");
        assert_eq!(
            answer(&state, &source, "@getattach:frobnicate=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn attach_and_outfit_names_answer_all_three_sides() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            attached: vec![1, 35],
            worn: vec![9],
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &source, "@getattachnames=2222")?.0,
            "chest,center"
        );
        assert_eq!(
            answer(&state, &source, "@getattachnames:hud=2222")?.0,
            "center"
        );
        assert_eq!(
            answer(&state, &source, "@getremattachnames=2222")?.0,
            "chest,center"
        );
        // Everything not already worn can be attached to.
        let addable = answer(&state, &source, "@getaddattachnames:hud=2222")?.0;
        assert!(!addable.contains("center,"), "{addable}");
        assert!(addable.contains("top left"), "{addable}");

        assert_eq!(answer(&state, &source, "@getoutfitnames=2222")?.0, "gloves");
        assert_eq!(
            answer(&state, &source, "@getremoutfitnames=2222")?.0,
            "gloves"
        );
        // These three take no option at all.
        assert_eq!(
            answer(&state, &source, "@getoutfitnames:shirt=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn getinv_hides_the_folders_a_path_cannot_name() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            shared_root: true,
            children: Some(vec![
                "Boots".to_owned(),
                ".hidden".to_owned(),
                "with/slash".to_owned(),
                String::new(),
                "Hat".to_owned(),
            ]),
            ..Fixture::default()
        };
        assert_eq!(answer(&state, &source, "@getinv=2222")?.0, "Boots,Hat");

        // No such folder, but there is a shared root: the path was wrong.
        let no_folder = Fixture {
            shared_root: true,
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &no_folder, "@getinv:nope=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        // No shared root at all: a different answer, so a script can tell.
        assert_eq!(
            answer(&state, &Fixture::default(), "@getinv=2222")?,
            (String::new(), RlvOutcome::FailedNoSharedRoot)
        );
        Ok(())
    }

    #[test]
    fn getinvworn_sums_its_children_into_the_root_digits() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            shared_root: true,
            wear: Some(RlvFolderWear {
                worn: 1,
                total: 2,
                children: vec![
                    RlvFolderWearChild::new(
                        "Boots".to_owned(),
                        RlvFolderWearCounts {
                            worn: 2,
                            total: 2,
                            child_worn: 0,
                            child_total: 0,
                        },
                    ),
                    RlvFolderWearChild::new(
                        "Hat".to_owned(),
                        RlvFolderWearCounts {
                            worn: 0,
                            total: 1,
                            child_worn: 0,
                            child_total: 3,
                        },
                    ),
                ],
            }),
            ..Fixture::default()
        };
        // Root: 1 of 2 worn => 2; below it 2 of 6 worn => 2.
        // Boots: all worn => 3, nothing below => 0.
        // Hat: none of 1 worn => 1, none of 3 below worn => 1.
        assert_eq!(
            answer(&state, &source, "@getinvworn=2222")?.0,
            "|22,Boots|30,Hat|11"
        );
        Ok(())
    }

    #[test]
    fn findfolder_picks_the_deepest_and_findfolders_lists_them() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            shared_root: true,
            found: vec![
                "Clothes".to_owned(),
                "Clothes/Boots/Winter".to_owned(),
                "Clothes/Boots".to_owned(),
            ],
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &source, "@findfolder:boots=2222")?.0,
            "Clothes/Boots/Winter"
        );
        assert_eq!(
            answer(&state, &source, "@findfolders:boots=2222")?.0,
            "Clothes,Clothes/Boots/Winter,Clothes/Boots"
        );
        // No criteria is the one failure.
        assert_eq!(
            answer(&state, &source, "@findfolder=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn getpath_takes_a_slot_a_point_an_id_or_the_issuer() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            shared_root: true,
            found: vec!["Clothes/Boots".to_owned(), "Clothes/Socks".to_owned()],
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &source, "@getpath=2222")?.0,
            "Clothes/Boots",
            "the first, not the lot"
        );
        assert_eq!(
            answer(&state, &source, "@getpathnew=2222")?.0,
            "Clothes/Boots,Clothes/Socks"
        );

        for option in ["gloves", "chest", "a3f2c1d4-0000-4000-8000-000000000000"] {
            let command = RlvCommand::parse_field(&format!("getpath:{option}=2222"))?;
            assert!(
                matches!(RlvQuery::classify(&command), Ok(RlvQuery::GetPath { .. })),
                "`{option}` is not a @getpath target"
            );
        }
        assert_eq!(
            answer(&state, &source, "@getpath:frobnicate=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn the_small_agent_queries() -> Result<(), TestError> {
        let state = RlvState::new();
        let seat = Uuid::from_u128(0xbeef);
        let source = Fixture {
            sitting_on: Some(seat),
            group: Some("The Test Group".to_owned()),
            ..Fixture::default()
        };
        assert_eq!(
            answer(&state, &source, "@getsitid=2222")?.0,
            seat.as_hyphenated().to_string()
        );
        assert_eq!(
            answer(&state, &source, "@getgroup=2222")?.0,
            "The Test Group"
        );
        assert_eq!(answer(&state, &source, "@getheightoffset=2222")?.0, "12.50");
        assert_eq!(answer(&state, &source, "@getcam_avdist=2222")?.0, "3.500");
        assert_eq!(answer(&state, &source, "@getcam_fov=2222")?.0, "1.000");

        // Not sitting is the nil id, not an empty string — RLV-1.16.1's answer.
        let standing = Fixture::default();
        assert_eq!(
            answer(&state, &standing, "@getsitid=2222")?.0,
            Uuid::nil().as_hyphenated().to_string()
        );
        assert_eq!(answer(&state, &standing, "@getgroup=2222")?.0, "none");
        Ok(())
    }

    #[test]
    fn getcam_limits_read_back_what_an_object_set() -> Result<(), TestError> {
        let mut state = RlvState::new();
        let source = Fixture::default();

        // Nothing set: an empty answer, not a zero.
        assert_eq!(answer(&state, &source, "@getcam_fovmin=2222")?.0, "");

        state.apply(COLLAR, &first_command("@setcam_fovmin:0.5=n")?);
        assert_eq!(answer(&state, &source, "@getcam_fovmin=2222")?.0, "0.500");
        assert_eq!(
            RlvQuery::classify(&RlvCommand::parse_field("getcam_fovmin=2222")?),
            Ok(RlvQuery::GetCamLimit {
                modifier: RlvModifier::SetcamFovmin
            })
        );
        // These take no option.
        assert_eq!(
            answer(&state, &source, "@getcam_fovmin:2=2222")?,
            (String::new(), RlvOutcome::FailedOption)
        );
        Ok(())
    }

    #[test]
    fn a_bad_channel_leaves_the_script_waiting_and_a_bad_query_does_not() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture::default();

        // Channel 0 is the debug console, and only the agent itself may use it.
        let command = first_command("@version=0")?;
        assert_eq!(state.answer(COLLAR, &command, &source).reply, None);
        let loopback = state.answer(AGENT, &command, &source);
        assert_eq!(loopback.outcome, RlvOutcome::Success);
        assert_eq!(loopback.reply.map(|reply| reply.channel), Some(0));

        for line in ["@version=-1", &format!("@version={}", i32::MAX)] {
            let answered = state.answer(AGENT, &first_command(line)?, &source);
            assert_eq!(answered.reply, None, "`{line}` should have no reply");
            assert_eq!(answered.outcome, RlvOutcome::FailedParam);
        }

        // A command that is not a query at all.
        let answered = state.answer(COLLAR, &first_command("@fly=n")?, &source);
        assert_eq!(answered.reply, None);
        assert_eq!(answered.outcome, RlvOutcome::FailedParam);
        Ok(())
    }

    #[test]
    fn every_reply_row_classifies() -> Result<(), TestError> {
        use crate::RlvEntry;

        for entry in RlvEntry::ALL {
            if entry.kind != RlvParamKind::Reply {
                continue;
            }
            let command = RlvCommand::parse_field(&format!("{}=2222", entry.keyword))?;
            let classified = RlvQuery::classify(&command);
            assert!(
                classified.is_ok() || classified == Err(RlvOutcome::FailedOption),
                "`{}` is a query nothing answers: {classified:?}",
                entry.keyword
            );
        }
        Ok(())
    }

    #[test]
    fn attach_group_matches_the_point() {
        // The divergence the module docs call out: Firestorm reads joint group
        // 8 as the HUD group, which since the extended points were added is
        // *them* — so its `@getattachnames:hud` never names a HUD surface.
        let hud = RlvAttachmentPoint::from_name("center").map(RlvAttachmentPoint::group);
        assert_eq!(hud, Some(RlvAttachGroup::Hud));
        let tail = RlvAttachmentPoint::from_name("tail base").map(RlvAttachmentPoint::group);
        assert_eq!(tail, Some(RlvAttachGroup::Torso));
        let jaw = RlvAttachmentPoint::from_name("jaw").map(RlvAttachmentPoint::group);
        assert_eq!(jaw, Some(RlvAttachGroup::Head));

        // Every HUD surface, and only those, is in the HUD group.
        let hud_points: Vec<&str> = RlvAttachmentPoint::all()
            .filter(|point| point.group() == RlvAttachGroup::Hud)
            .map(RlvAttachmentPoint::name)
            .collect();
        assert_eq!(
            hud_points,
            [
                "center 2",
                "top right",
                "top",
                "top left",
                "center",
                "bottom left",
                "bottom",
                "bottom right"
            ]
        );
    }

    #[test]
    fn the_tables_look_themselves_up() {
        for point in RlvAttachmentPoint::all() {
            assert_eq!(RlvAttachmentPoint::from_name(point.name()), Some(point));
            assert_eq!(RlvAttachmentPoint::from_index(point.index()), Some(point));
            assert!(RlvAttachGroup::ALL.contains(&point.group()));
        }
        assert_eq!(RlvAttachmentPoint::all().count(), 55);
        assert_eq!(RlvAttachmentPoint::from_index(0), None);

        for slot in RlvWearableSlot::all() {
            assert_eq!(RlvWearableSlot::from_name(slot.name()), Some(slot));
            assert_eq!(RlvWearableSlot::from_code(slot.code()), Some(slot));
        }
        assert_eq!(RlvWearableSlot::all().count(), 17);
        assert_eq!(RlvWearableSlot::getoutfit_order().count(), 17);
        // The frozen order is a permutation of the wire order, not a subset.
        let mut ordered: Vec<u8> = RlvWearableSlot::getoutfit_order()
            .map(RlvWearableSlot::code)
            .collect();
        ordered.sort_unstable();
        let wire: Vec<u8> = RlvWearableSlot::all().map(RlvWearableSlot::code).collect();
        assert_eq!(ordered, wire);

        for group in RlvAttachGroup::ALL {
            assert_eq!(RlvAttachGroup::from_name(group.name()), Some(*group));
        }
        assert_eq!(RlvAttachGroup::from_name("frobnicate"), None);
    }

    #[test]
    fn names_are_matched_case_insensitively() {
        assert_eq!(
            RlvAttachmentPoint::from_name("LEFT Shoulder"),
            RlvAttachmentPoint::from_name("left shoulder")
        );
        assert_eq!(
            RlvWearableSlot::from_name("UnderShirt"),
            RlvWearableSlot::from_name("undershirt")
        );
        assert_eq!(
            RlvAttachmentPoint::from_name("ROOT"),
            RlvAttachmentPoint::from_name("avatar center")
        );
    }

    #[test]
    fn a_long_answer_is_truncated_not_split() -> Result<(), TestError> {
        let state = RlvState::new();
        let source = Fixture {
            shared_root: true,
            children: Some(
                (0..500)
                    .map(|index| format!("Folder{index:04}"))
                    .collect::<Vec<_>>(),
            ),
            ..Fixture::default()
        };
        let (message, outcome) = answer(&state, &source, "@getinv=2222")?;
        assert_eq!(outcome, RlvOutcome::Success);
        assert_eq!(message.len(), MAX_CHAT_BYTES);
        Ok(())
    }

    #[test]
    fn truncation_keeps_characters_whole() {
        let long = "é".repeat(MAX_CHAT_BYTES);
        let cut = truncate_chat(&long);
        assert!(cut.len() <= MAX_CHAT_BYTES);
        // 1023 is odd and 'é' is two bytes, so the last one has to be dropped.
        assert_eq!(cut.chars().count(), 511);
        assert_eq!(truncate_chat("short"), "short");
    }

    #[test]
    fn splitting_breaks_at_the_separator() {
        let word = "x".repeat(100);
        let long = vec![word.as_str(); 30].join(" ");
        let lines = split_chat(&long, ' ');
        assert!(lines.len() > 1, "{} lines", lines.len());
        for line in &lines {
            assert!(line.len() <= MAX_CHAT_BYTES, "{}", line.len());
            assert!(!line.starts_with(' '), "a line kept its separator");
        }
        assert_eq!(lines.join(" "), long, "splitting lost something");

        // Nothing to break on: the window is taken whole.
        let unbroken = "y".repeat(MAX_CHAT_BYTES.saturating_mul(2).saturating_add(5));
        let lines = split_chat(&unbroken, ' ');
        assert_eq!(lines.len(), 3);
        assert_eq!(lines.concat(), unbroken);

        assert_eq!(split_chat("", ' '), Vec::<String>::new());
    }

    #[test]
    fn reply_channels() {
        assert!(is_valid_reply_channel(2222, false));
        assert!(!is_valid_reply_channel(0, false));
        assert!(is_valid_reply_channel(0, true));
        assert!(!is_valid_reply_channel(-1, true));
        assert!(!is_valid_reply_channel(i32::MAX, true));
    }

    #[test]
    fn im_queries_are_whole_messages() {
        assert_eq!(RlvImQuery::classify("@version"), Some(RlvImQuery::Version));
        assert_eq!(RlvImQuery::classify("@stopim"), Some(RlvImQuery::StopIm));
        assert_eq!(RlvImQuery::classify("@list"), Some(RlvImQuery::List));
        assert_eq!(RlvImQuery::classify("@except"), Some(RlvImQuery::Except));
        assert_eq!(RlvImQuery::classify("@version please"), None);
        assert_eq!(RlvImQuery::classify("hello"), None);
        assert!(RlvImQuery::List.needs_consent());
        assert!(!RlvImQuery::Version.needs_consent());
    }

    #[test]
    fn classification_carries_the_options_through() -> Result<(), TestError> {
        assert_eq!(
            RlvQuery::classify(&RlvCommand::parse_field("versionnum:impl=1")?),
            Ok(RlvQuery::VersionNum {
                which: RlvVersionNum::Implementation
            })
        );
        assert_eq!(
            RlvQuery::classify(&RlvCommand::parse_field("getattachnames:legs=1")?),
            Ok(RlvQuery::GetAttachNames {
                which: RlvNamesQuery::Worn,
                group: Some(RlvAttachGroup::Legs),
            })
        );
        // An unknown group matches everything rather than failing.
        assert_eq!(
            RlvQuery::classify(&RlvCommand::parse_field("getattachnames:frobnicate=1")?),
            Ok(RlvQuery::GetAttachNames {
                which: RlvNamesQuery::Worn,
                group: None,
            })
        );
        assert_eq!(
            RlvQuery::classify(&RlvCommand::parse_field("getinv:Clothes/Boots=1")?),
            Ok(RlvQuery::GetInv {
                // The whole line arrives lower-cased, paths and all.
                path: "clothes/boots".to_owned()
            })
        );
        Ok(())
    }
}
