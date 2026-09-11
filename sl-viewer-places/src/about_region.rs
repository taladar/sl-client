//! The **Region / Estate** ("About Region") floater
//! (`viewer-region-options-debug` / `-general` / `-terrain` / `-estate`): the
//! region-and-estate information surface. It presents the reference viewer's
//! `llfloaterregioninfo` as tabs — **Region**, **Debug**, **Terrain**,
//! **Estate**, **Covenant**, **Access**, plus placeholder **Environment** and
//! **Experiences** tabs (their write paths — `ExtEnvironment` PUT and the
//! experience service — are their own roadmap items).
//!
//! # One window per region
//!
//! The floater opens on the region the agent is standing in, and that region is
//! its **subject** from then on: the window is keyed by the region's id, so
//! crossing into a new region and opening Region / Estate again gives a second
//! window rather than repointing the first. Everything a window knows lives on
//! its root entity as components, and closing it ends that instance.
//!
//! The reference keeps `LLFloaterRegionInfo` a singleton, because it only ever
//! describes where you are. Keying it is a deliberate divergence, recorded in
//! `viewer-keyed-floater-audit`: comparing two regions' settings, or reading a
//! covenant after stepping across the border, is a real thing to want.
//!
//! **A window for a region the agent has left is frozen and read-only.** Every
//! reply this floater reads — `RegionInfo`, the estate `getinfo`, the covenant,
//! the access lists — is about the *current* region and names no region of its
//! own, and every write goes out on the current circuit. So a window whose
//! region is no longer current keeps the last snapshot it had, takes no further
//! replies, and hides its write controls: showing one region's settings while a
//! save would land on another is the one outcome worth ruling out. Walking back
//! into the region wakes it again.
//!
//! # Build once, update in place (no despawn)
//!
//! Every tab's structure is spawned **once**, when the window opens, and never
//! torn down while it lives.
//! Replies update values *in place*: value labels via `set_value_node`,
//! checkbox glyphs via `set_check_visual`, the maturity combo by writing its
//! [`ComboSelection`](crate::ui_combo), edit fields by seeding
//! `EditableText::editor_mut().set_text` on a fresh region, and the four estate
//! access lists (managers, allowed residents, allowed groups, banned residents)
//! through the **table widget** ([`crate::ui_table`]) — a bounded, scrolling
//! viewport that pools and binds its rows, never despawning them. Churn is the
//! root cause of the Bevy despawn panics that plagued the profile floaters
//! (`never-hide-errors`), so this floater — like About Land — refuses it.
//!
//! # Editing and disabled controls
//!
//! The editable region settings mutate a single [`RegionInfoUpdate`] draft,
//! seeded from the live region each time its data changes; the **Apply** button
//! commits it with [`Command::SetRegionInfo`]. The estate access **Add** /
//! **Remove** buttons mutate a list with [`Command::UpdateEstateAccess`], and the
//! Debug tab's restart controls send [`Command::RestartRegion`]. When the agent
//! is not an estate manager every editable control carries
//! [`bevy::ui::InteractionDisabled`] — the widgets grey out and refuse input —
//! and the write buttons hide. Controls with no protocol write path (the Debug
//! `setregiondebug` toggles, the terrain `setregionterrain` fields) are shown as
//! **permanently disabled** controls reflecting the grid's value, not as prose.
//!
//! Reference (Firestorm, read-only): `llfloaterregioninfo.cpp`,
//! `panel_region_*.xml`; the `EstateOwnerMessage` `setregioninfo` /
//! `estateaccessdelta` / `restart` methods.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use sl_client_bevy::{
    AgentKey, Asset, AssetKey, AssetType, Command, EstateAccessDelta, EstateAccessKind,
    EstateCovenant, EstateFlags, EstateInfo, EstateInfoUpdate, GroupKey, Maturity, OwnerKey,
    ProductType, RegionDebugUpdate, RegionFlags, RegionIdentity, RegionInfoUpdate, RegionName,
    RegionTerrainUpdate, SlCommand, SlCurrentRegion, SlEvent, SlRegionIdentity, SlRegionLimits,
    SlSessionEvent, TextureKey, Uuid,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters, host_floater,
};
use crate::i18n::{Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::ui::{column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_name_link::{NameLink, NameLinkSpec, NameTarget, set_name_link, spawn_name_link};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableSelectionMode, TableSpec,
    set_table_cell, spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::ui_texture_picker::{TextureSwatchValue, spawn_texture_swatch};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::AvatarState;
use crate::world_api::GroupsModel;
use crate::world_api::TexturePicked;
use crate::world_api::{AvatarPicked, OpenAvatarPicker};

/// The floater's body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A read value's text colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dim label / secondary text colour.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A checked toggle's tick colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A disabled control's text colour (matching the disabled text field / combo).
const DISABLED_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// A list background.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The glyph for a checked toggle.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The glyph for an unchecked toggle.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The bounded height of each estate access list, in logical pixels — the widget
/// scrolls beyond it rather than growing the tab.
const LIST_HEIGHT: f32 = 130.0;

/// One list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 22.0;

/// The avatar-picker requester tag for adding an estate manager.
const PICK_MANAGER: &str = "about-region-manager";

/// The avatar-picker requester tag for adding an allowed resident.
const PICK_ALLOWED: &str = "about-region-allowed";

/// The avatar-picker requester tag for adding a banned resident.
const PICK_BANNED: &str = "about-region-banned";

/// The avatar-picker requester tag for teleporting one resident home.
const PICK_TELEPORT: &str = "about-region-teleport";

/// The avatar-picker requester tag for kicking a resident from the estate.
const PICK_KICK: &str = "about-region-kick";

/// The estate-manager list table (name + per-row remove).
const MANAGERS_TABLE: TableSpec = access_table("about-region-managers");

/// The allowed-residents list table.
const ALLOWED_TABLE: TableSpec = access_table("about-region-allowed");

/// The allowed-groups list table.
const ALLOWED_GROUPS_TABLE: TableSpec = access_table("about-region-allowed-groups");

/// The banned-residents list table.
const BANNED_TABLE: TableSpec = access_table("about-region-banned");

/// The shared two-column layout (name, remove) of an estate access list table,
/// parameterised by element.
const fn access_table(element: &'static str) -> TableSpec {
    TableSpec {
        element,
        selection: TableSelectionMode::None,
        columns: &[
            TableColumn {
                header_key: "about-region-access-name",
                token: "name",
                kind: TableColumnKind::Text,
                width: TableColumnWidth::Flex(1.0),
                align: TableAlign::Start,
                sortable: false,
            },
            TableColumn {
                header_key: "about-region-access-remove",
                token: "remove",
                kind: TableColumnKind::Custom,
                width: TableColumnWidth::Fixed { default: 70.0 },
                align: TableAlign::End,
                sortable: false,
            },
        ],
        default_sort: &[],
        builtin_sort: false,
        row_height: ROW_HEIGHT,
        font_size: FONT_SIZE,
        header_color: DIM_LABEL_COLOR,
        cell_color: LABEL_COLOR,
        column_gap: 6.0,
        row_padding: 4.0,
        sort_setting: None,
        widths_setting: None,
    }
}

/// The maturity combo's option keys, indexed to match [`maturity_from_index`].
const MATURITY_KEYS: &[&str] = &[
    "about-region-rating-pg",
    "about-region-rating-mature",
    "about-region-rating-adult",
];

// ---------------------------------------------------------------------------
// Open request.
// ---------------------------------------------------------------------------

/// A request to open the Region / Estate floater on the agent's current region.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct OpenAboutRegion;

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// One window's data model — **one per region** (see the module header), on the
/// window's root entity.
#[derive(Component, Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a distinct floater flag (requested / seeded / manage rights)"
)]
struct AboutRegionState {
    /// The region this window is about — its subject, fixed at open.
    region: Uuid,
    /// The region record this window last saw, kept as a **snapshot**: a window
    /// whose region the agent has left goes on showing what it had rather than
    /// the region the agent walked into.
    identity: Option<RegionIdentity>,
    /// Whether [`region`](Self::region) is still the region the agent is in.
    /// A window that is not takes no replies and offers no writes — see the
    /// module header.
    is_current: bool,
    /// Whether the agent may manage the estate (owner or manager) **and** this
    /// window is current; gates editing.
    can_manage: bool,
    /// Which picker tag this window has a resident pick outstanding for.
    ///
    /// The avatar picker echoes a `&'static str` rather than an entity, so a
    /// pick cannot name its window. It does not have to: the picker is one
    /// window per tag, so at most one Region / Estate window can be waiting on
    /// a tag, and this is that window's claim on it.
    pending_pick: Option<&'static str>,
    /// The editable region-settings draft, seeded from the live region.
    draft: RegionInfoUpdate,
    /// The editable region-debug draft (disable scripts / collisions / physics).
    debug_draft: RegionDebugUpdate,
    /// The editable region-terrain draft (water / limits / textures / elevation).
    terrain_draft: RegionTerrainUpdate,
    /// The editable estate-flags draft (access / limit / voice / teleport bits).
    estate_draft: EstateFlags,
    /// Whether the region drafts have been seeded from region data since the
    /// last change; a fresh region (or a `RegionInfo` reply) reseeds them.
    draft_seeded: bool,
    /// Whether the estate-flags draft has been seeded from the estate reply.
    estate_seeded: bool,
    /// The estate configuration (name / owner / abuse email), from `getinfo`.
    estate: Option<EstateInfo>,
    /// The estate covenant summary, from `EstateCovenantReply`.
    covenant: Option<EstateCovenant>,
    /// The decoded covenant notecard text, once fetched.
    covenant_text: Option<String>,
    /// The covenant notecard asset id awaiting fetch.
    covenant_pending: Option<Uuid>,
    /// The estate managers.
    managers: Vec<Uuid>,
    /// The allowed residents.
    allowed: Vec<Uuid>,
    /// The allowed groups.
    allowed_groups: Vec<Uuid>,
    /// The banned residents.
    banned: Vec<Uuid>,
    /// A monotonically-increasing revision bumped when [`Self::managers`] changes.
    managers_revision: u64,
    /// A revision bumped when [`Self::allowed`] changes.
    allowed_revision: u64,
    /// A revision bumped when [`Self::allowed_groups`] changes.
    allowed_groups_revision: u64,
    /// A revision bumped when [`Self::banned`] changes.
    banned_revision: u64,
}

impl AboutRegionState {
    /// Clear the estate access lists (on a fresh `getinfo`), bumping revisions so
    /// the views rebind to the empty lists before the reply chunks arrive.
    fn clear_access(&mut self) {
        self.managers.clear();
        self.allowed.clear();
        self.allowed_groups.clear();
        self.banned.clear();
        self.managers_revision = self.managers_revision.wrapping_add(1);
        self.allowed_revision = self.allowed_revision.wrapping_add(1);
        self.allowed_groups_revision = self.allowed_groups_revision.wrapping_add(1);
        self.banned_revision = self.banned_revision.wrapping_add(1);
    }

    /// The list for an access-list kind, with its revision counter.
    const fn list_mut(&mut self, list: AccessList) -> (&mut Vec<Uuid>, &mut u64) {
        match list {
            AccessList::Managers => (&mut self.managers, &mut self.managers_revision),
            AccessList::Allowed => (&mut self.allowed, &mut self.allowed_revision),
            AccessList::AllowedGroups => {
                (&mut self.allowed_groups, &mut self.allowed_groups_revision)
            }
            AccessList::Banned => (&mut self.banned, &mut self.banned_revision),
        }
    }
}

/// One window's per-tab dirty flags: a value refresh runs only when its data
/// changed.
#[derive(Component, Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent dirty flag per tab / refresh pass"
)]
struct AboutRegionDirty {
    /// The Region tab's values need refreshing.
    region_values: bool,
    /// The Debug tab's values need refreshing.
    debug_values: bool,
    /// The Terrain tab's values need refreshing.
    terrain_values: bool,
    /// The Estate tab's values need refreshing.
    estate_values: bool,
    /// The Covenant tab's values need refreshing.
    covenant_values: bool,
    /// The checkbox glyphs / control-enable states need refreshing.
    controls: bool,
    /// The edit fields / maturity combo need reseeding from the draft.
    seed_fields: bool,
}

impl AboutRegionDirty {
    /// Mark every panel dirty (a fresh open / a region change).
    const fn mark_all(&mut self) {
        self.region_values = true;
        self.debug_values = true;
        self.terrain_values = true;
        self.estate_values = true;
        self.covenant_values = true;
        self.controls = true;
        self.seed_fields = true;
    }
}

/// One resolved access-list row.
#[derive(Debug, Clone)]
struct AccessRowData {
    /// The resolved display name (or `(id)` fallback).
    name: String,
    /// The agent / group id (for the Remove command).
    id: Uuid,
}

/// The estate-managers list view model.
#[derive(Component, Debug, Default)]
struct ManagersView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The allowed-residents list view model.
#[derive(Component, Debug, Default)]
struct AllowedView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The allowed-groups list view model.
#[derive(Component, Debug, Default)]
struct AllowedGroupsView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The banned-residents list view model.
#[derive(Component, Debug, Default)]
struct BannedView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

// ---------------------------------------------------------------------------
// Handles.
// ---------------------------------------------------------------------------

/// The Region tab's live value / field handles.
#[derive(Debug, Default)]
struct RegionHandles {
    /// The region-name value node.
    name: Option<Entity>,
    /// The region-type (product) value node.
    region_type: Option<Entity>,
    /// The region-owner name value node.
    owner: Option<Entity>,
    /// The grid-position value node.
    grid_position: Option<Entity>,
    /// The maturity combo.
    maturity_combo: Option<Entity>,
    /// The agent-limit edit field.
    agent_limit_field: Option<Entity>,
    /// The object-bonus edit field.
    object_bonus_field: Option<Entity>,
}

/// The Debug tab's handles.
#[derive(Debug, Default)]
struct DebugHandles {
    /// The region-name value node.
    name: Option<Entity>,
    /// The restart-delay (seconds) edit field.
    restart_field: Option<Entity>,
}

/// The Terrain tab's editable field / swatch handles.
#[derive(Debug, Default)]
struct TerrainHandles {
    /// The region-name value node.
    name: Option<Entity>,
    /// The water-height edit field.
    water_field: Option<Entity>,
    /// The terrain-raise-limit edit field.
    raise_field: Option<Entity>,
    /// The terrain-lower-limit edit field.
    lower_field: Option<Entity>,
    /// The four detail-texture swatch value nodes (lowest to highest ground).
    textures: [Option<Entity>; 4],
    /// The four per-corner blend-start edit fields (slot order 00, 01, 10, 11).
    start_fields: [Option<Entity>; 4],
    /// The four per-corner blend-range edit fields (slot order 00, 01, 10, 11).
    range_fields: [Option<Entity>; 4],
}

/// The Estate tab's handles.
#[derive(Debug, Default)]
struct EstateHandles {
    /// The estate-name value node.
    name: Option<Entity>,
    /// The estate-owner name value node.
    owner: Option<Entity>,
    /// The abuse-email value node.
    abuse_email: Option<Entity>,
    /// The estate-message compose field.
    message_field: Option<Entity>,
}

/// The Covenant tab's handles (read-only).
#[derive(Debug, Default)]
struct CovenantHandles {
    /// The estate-name value node.
    estate: Option<Entity>,
    /// The estate-owner name value node.
    estate_owner: Option<Entity>,
    /// The covenant-body value node.
    text: Option<Entity>,
    /// The last-modified timestamp value node.
    timestamp: Option<Entity>,
    /// The region-name value node.
    region: Option<Entity>,
    /// The region-type value node.
    region_type: Option<Entity>,
    /// The region-rating value node.
    region_rating: Option<Entity>,
    /// The resale-clause value node.
    resale: Option<Entity>,
    /// The subdivide-clause value node.
    subdivide: Option<Entity>,
}

/// The Access tab's table handles (one viewport + root per list).
#[derive(Debug, Default)]
struct AccessHandles {
    /// The estate-managers viewport (carries [`VirtualList`]).
    managers_viewport: Option<Entity>,
    /// The estate-managers table root.
    managers_table: Option<Entity>,
    /// The allowed-residents viewport.
    allowed_viewport: Option<Entity>,
    /// The allowed-residents table root.
    allowed_table: Option<Entity>,
    /// The allowed-groups viewport.
    allowed_groups_viewport: Option<Entity>,
    /// The allowed-groups table root.
    allowed_groups_table: Option<Entity>,
    /// The banned-residents viewport.
    banned_viewport: Option<Entity>,
    /// The banned-residents table root.
    banned_table: Option<Entity>,
}

/// One window's live entity handles.
#[derive(Component, Debug)]
struct AboutRegionUi {
    /// The floater's title text node — rewritten with the region's name, so two
    /// windows are tellable apart in the title bar and the window list.
    title_text: Entity,
    /// The Region tab's handles.
    region: RegionHandles,
    /// The Debug tab's handles.
    debug: DebugHandles,
    /// The Terrain tab's handles.
    terrain: TerrainHandles,
    /// The Estate tab's handles.
    estate: EstateHandles,
    /// The Covenant tab's handles.
    covenant: CovenantHandles,
    /// The Access tab's handles.
    access: AccessHandles,
}

// ---------------------------------------------------------------------------
// Components.
// ---------------------------------------------------------------------------

/// A checkbox on the Region / Debug tabs.
#[derive(Component, Debug, Clone, Copy)]
struct AboutRegionCheck {
    /// What the checkbox reflects.
    kind: CheckKind,
    /// The check-glyph text node.
    glyph: Entity,
    /// The label text node (greyed with the glyph when disabled).
    label: Entity,
}

/// Which region / debug / estate setting a checkbox drives. Every kind is
/// editable and backed by one of the three drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckKind {
    /// Block terraforming (→ region draft).
    BlockTerraform,
    /// Block flying (→ region draft).
    BlockFly,
    /// Allow damage / combat (→ region draft).
    AllowDamage,
    /// Restrict pushing (→ region draft).
    RestrictPush,
    /// Allow land resell (→ region draft).
    AllowLandResell,
    /// Allow parcel join / divide (→ region draft).
    AllowLandJoinDivide,
    /// Disable scripts region-wide (→ debug draft).
    DisableScripts,
    /// Disable collisions region-wide (→ debug draft).
    DisableCollisions,
    /// Disable physics region-wide (→ debug draft).
    DisablePhysics,
    /// Estate is publicly visible / anyone may visit (→ estate draft).
    EstatePublicAccess,
    /// Allow direct teleport (→ estate draft).
    EstateAllowDirectTeleport,
    /// Require payment info on file — deny anonymous (→ estate draft).
    EstateRequirePayment,
    /// Require age verification (→ estate draft).
    EstateRequireAgeVerified,
    /// Allow voice chat (→ estate draft).
    EstateAllowVoice,
    /// Parcel owners may set stricter access (→ estate draft).
    EstateParcelOverride,
    /// Deny scripted agents / bots (→ estate draft).
    EstateDenyBots,
}

impl CheckKind {
    /// The estate flag bit this checkbox drives, for the estate kinds.
    const fn estate_bit(self) -> Option<EstateFlags> {
        match self {
            Self::EstatePublicAccess => Some(EstateFlags::EXTERNALLY_VISIBLE),
            Self::EstateAllowDirectTeleport => Some(EstateFlags::ALLOW_DIRECT_TELEPORT),
            Self::EstateRequirePayment => Some(EstateFlags::DENY_ANONYMOUS),
            Self::EstateRequireAgeVerified => Some(EstateFlags::DENY_AGEUNVERIFIED),
            Self::EstateAllowVoice => Some(EstateFlags::ALLOW_VOICE),
            Self::EstateParcelOverride => Some(EstateFlags::ALLOW_ACCESS_OVERRIDE),
            Self::EstateDenyBots => Some(EstateFlags::DENY_BOTS),
            _other => None,
        }
    }

    /// Flip the draft field this checkbox drives.
    const fn toggle(self, state: &mut AboutRegionState) {
        match self {
            Self::BlockTerraform => state.draft.block_terraform = !state.draft.block_terraform,
            Self::BlockFly => state.draft.block_fly = !state.draft.block_fly,
            Self::AllowDamage => state.draft.allow_damage = !state.draft.allow_damage,
            Self::RestrictPush => {
                state.draft.restrict_pushobject = !state.draft.restrict_pushobject;
            }
            Self::AllowLandResell => {
                state.draft.allow_land_resell = !state.draft.allow_land_resell;
            }
            Self::AllowLandJoinDivide => {
                state.draft.allow_parcel_changes = !state.draft.allow_parcel_changes;
            }
            Self::DisableScripts => {
                state.debug_draft.disable_scripts = !state.debug_draft.disable_scripts;
            }
            Self::DisableCollisions => {
                state.debug_draft.disable_collisions = !state.debug_draft.disable_collisions;
            }
            Self::DisablePhysics => {
                state.debug_draft.disable_physics = !state.debug_draft.disable_physics;
            }
            _estate => {
                if let Some(bit) = self.estate_bit() {
                    let on = state.estate_draft.contains(bit);
                    state.estate_draft = state.estate_draft.with(bit, !on);
                }
            }
        }
    }

    /// The checkbox's on-state, read from the draft it drives.
    fn checked(self, state: &AboutRegionState) -> bool {
        match self {
            Self::BlockTerraform => state.draft.block_terraform,
            Self::BlockFly => state.draft.block_fly,
            Self::AllowDamage => state.draft.allow_damage,
            Self::RestrictPush => state.draft.restrict_pushobject,
            Self::AllowLandResell => state.draft.allow_land_resell,
            Self::AllowLandJoinDivide => state.draft.allow_parcel_changes,
            Self::DisableScripts => state.debug_draft.disable_scripts,
            Self::DisableCollisions => state.debug_draft.disable_collisions,
            Self::DisablePhysics => state.debug_draft.disable_physics,
            _estate => self
                .estate_bit()
                .is_some_and(|bit| state.estate_draft.contains(bit)),
        }
    }
}

/// A control whose interactivity follows the agent's estate rights: `Manager` is
/// A marker on every editable control (checkbox, edit field, combo, texture
/// swatch): its [`InteractionDisabled`] follows the agent's estate rights.
#[derive(Component, Debug, Clone, Copy)]
struct EditGate;

/// A marker on every write button (Apply / Add / Restart / …), so their
/// visibility follows the agent's estate rights in one pass.
#[derive(Component, Debug, Clone, Copy)]
struct WriteButton;

/// A per-row access Remove button: which list it targets and the pooled table
/// row it sits in (so a press resolves the current entry via the table view).
#[derive(Component, Debug, Clone, Copy)]
struct RemoveAccessButton {
    /// Which list the row belongs to.
    list: AccessList,
    /// The pooled [`VirtualRow`] this button sits in.
    row: Entity,
}

/// Which estate access list a control targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessList {
    /// The estate-managers list.
    Managers,
    /// The allowed-residents list.
    Allowed,
    /// The allowed-groups list.
    AllowedGroups,
    /// The banned-residents list.
    Banned,
}

impl AccessList {
    /// The `estateaccessdelta` for adding to this list.
    const fn add_delta(self) -> EstateAccessDelta {
        match self {
            Self::Managers => EstateAccessDelta::ManagerAdd,
            Self::Allowed => EstateAccessDelta::AllowedAgentAdd,
            Self::AllowedGroups => EstateAccessDelta::AllowedGroupAdd,
            Self::Banned => EstateAccessDelta::BannedAgentAdd,
        }
    }

    /// The `estateaccessdelta` for removing from this list.
    const fn remove_delta(self) -> EstateAccessDelta {
        match self {
            Self::Managers => EstateAccessDelta::ManagerRemove,
            Self::Allowed => EstateAccessDelta::AllowedAgentRemove,
            Self::AllowedGroups => EstateAccessDelta::AllowedGroupRemove,
            Self::Banned => EstateAccessDelta::BannedAgentRemove,
        }
    }

    /// Whether this list holds groups (rather than agents).
    const fn is_group(self) -> bool {
        matches!(self, Self::AllowedGroups)
    }

    /// The command target for a `target` id on this list.
    fn target(self, target: Uuid) -> OwnerKey {
        if self.is_group() {
            OwnerKey::Group(GroupKey::from(target))
        } else {
            OwnerKey::Agent(AgentKey::from(target))
        }
    }
}

/// A press-dispatch tag on the floater's action buttons.
#[derive(Component, Debug, Clone, Copy)]
enum AboutRegionAction {
    /// Commit the region-settings draft via [`Command::SetRegionInfo`].
    Apply,
    /// Commit the region-debug draft via [`Command::SetRegionDebug`].
    ApplyDebug,
    /// Commit the region-terrain draft via [`Command::SetRegionTerrain`].
    ApplyTerrain,
    /// Commit the estate-flags draft via [`Command::SetEstateInfo`].
    ApplyEstate,
    /// Open the avatar picker to teleport one resident home.
    TeleportHomeOne,
    /// Teleport every resident in the region home.
    TeleportHomeAll,
    /// Restart the region after the entered delay.
    Restart,
    /// Cancel a pending region restart.
    CancelRestart,
    /// Send the composed estate message.
    SendEstateMessage,
    /// Open the avatar picker to kick a resident from the estate.
    KickEstate,
    /// Open the avatar picker to add an estate manager.
    AddManager,
    /// Open the avatar picker to add an allowed resident.
    AddAllowed,
    /// Open the avatar picker to add a banned resident.
    AddBanned,
}

/// A marker on a terrain texture-swatch button carrying which detail slot it
/// edits, so a texture pick routes back to the right terrain-draft slot.
#[derive(Component, Debug, Clone, Copy)]
struct TerrainSwatch {
    /// The detail-texture slot index (0–3).
    slot: usize,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the Region / Estate floater into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct AboutRegionPlugin;

impl Plugin for AboutRegionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenAboutRegion>()
            .add_systems(
                Update,
                // After the manager's command pass — see `FloaterSystems`: the
                // menu click that opens this window also raises the window it
                // was clicked in, and the later raise wins.
                open_about_region
                    .after(FloaterSystems::Commands)
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    ingest_about_region_events,
                    refresh_on_region,
                    refresh_on_names,
                    seed_edit_fields,
                    update_control_enable,
                    update_region_tab,
                    update_debug_tab,
                    update_terrain_tab,
                    update_estate_tab,
                    update_covenant_tab,
                    sync_managers_view,
                    sync_allowed_view,
                    sync_allowed_groups_view,
                    sync_banned_view,
                    apply_combo_edits,
                    apply_avatar_picks,
                    apply_texture_edits,
                )
                    .chain()
                    .after(open_about_region)
                    .before(layout_virtual_lists)
                    .run_if(any_with_component::<AboutRegionState>),
            )
            .add_systems(
                Update,
                (populate_access_rows, bind_access_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(any_with_component::<AboutRegionState>),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The Region / Estate floater's stable [`crate::floater::Floater::id`], the
/// key [`open_about_region`] looks the panel up by.
const ABOUT_REGION_FLOATER_ID: &str = "about-region";

/// The about region floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn about_region_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: ABOUT_REGION_FLOATER_ID,
        title: "Region / Estate".to_owned(),
        position: Vec2::new(400.0, 80.0),
        default_size: Some(Vec2::new(500.0, 500.0)),
        min_size: Some(Vec2::new(430.0, 340.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Build one window's content: the tab container and every tab, returning the
/// handles the update passes write through.
///
/// Called once per window, as it is spawned — a keyed instance's content is
/// built into the window it belongs to, not deferred to a first open that no
/// longer exists ([`KeyedFloaters`]).
fn build_region_content(commands: &mut Commands, handle: FloaterHandle) -> AboutRegionUi {
    let labels: Vec<String> = [
        "about-region-tab-region",
        "about-region-tab-debug",
        "about-region-tab-terrain",
        "about-region-tab-estate",
        "about-region-tab-covenant",
        "about-region-tab-access",
        "about-region-tab-environment",
        "about-region-tab-experiences",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        commands,
        handle.content,
        &TabSpec {
            element: "about-region-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 1,
            font_size: FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    let panel = |index: usize| tabs.panels.get(index).copied().unwrap_or(handle.content);

    let region = build_region_tab(commands, panel(0));
    let debug = build_debug_tab(commands, panel(1));
    let terrain = build_terrain_tab(commands, panel(2));
    let estate = build_estate_tab(commands, panel(3));
    let covenant = build_covenant_tab(commands, panel(4));
    let access = build_access_tab(commands, panel(5));
    build_placeholder_tab(commands, panel(6), "about-region-env-unimplemented");
    build_placeholder_tab(commands, panel(7), "about-region-experiences-unimplemented");

    AboutRegionUi {
        title_text: handle.title_text,
        region,
        debug,
        terrain,
        estate,
        covenant,
        access,
    }
}

// ---------------------------------------------------------------------------
// Structure builders.
// ---------------------------------------------------------------------------

/// Build the Region tab: the read-only identity, the editable settings, and the
/// estate-manager actions.
fn build_region_tab(commands: &mut Commands, panel: Entity) -> RegionHandles {
    let mut handles = RegionHandles::default();
    let name_row = spawn_labeled_row(commands, panel, "about-region-region");
    handles.name = Some(spawn_value_node(commands, name_row));
    let type_row = spawn_labeled_row(commands, panel, "about-region-type");
    handles.region_type = Some(spawn_value_node(commands, type_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-region-owner");
    handles.owner = Some(spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new("about-region-loading", "about-region-none"),
    ));
    let grid_row = spawn_labeled_row(commands, panel, "about-region-grid-position");
    handles.grid_position = Some(spawn_value_node(commands, grid_row));

    spawn_check(
        commands,
        panel,
        "about-region-block-terraform",
        CheckKind::BlockTerraform,
    );
    spawn_check(
        commands,
        panel,
        "about-region-block-fly",
        CheckKind::BlockFly,
    );
    spawn_check(
        commands,
        panel,
        "about-region-allow-damage",
        CheckKind::AllowDamage,
    );
    spawn_check(
        commands,
        panel,
        "about-region-restrict-push",
        CheckKind::RestrictPush,
    );
    spawn_check(
        commands,
        panel,
        "about-region-allow-resell",
        CheckKind::AllowLandResell,
    );
    spawn_check(
        commands,
        panel,
        "about-region-allow-join-divide",
        CheckKind::AllowLandJoinDivide,
    );

    let limit_row = spawn_labeled_row(commands, panel, "about-region-agent-limit");
    handles.agent_limit_field = Some(spawn_edit_field(
        commands,
        limit_row,
        "about-region-agent-limit-field",
        TextInputKind::NonNegativeInteger,
        6.0,
        2,
        5,
    ));
    let bonus_row = spawn_labeled_row(commands, panel, "about-region-object-bonus");
    handles.object_bonus_field = Some(spawn_edit_field(
        commands,
        bonus_row,
        "about-region-object-bonus-field",
        TextInputKind::Float,
        6.0,
        3,
        6,
    ));
    let maturity_row = spawn_labeled_row(commands, panel, "about-region-maturity");
    handles.maturity_combo = Some(spawn_maturity_combo(commands, maturity_row, 4));

    spawn_apply_button(commands, panel, 5);
    let actions = spawn_row(commands, panel);
    spawn_action_button(
        commands,
        actions,
        "about-region-teleport-home-one",
        AboutRegionAction::TeleportHomeOne,
        6,
        true,
    );
    spawn_action_button(
        commands,
        actions,
        "about-region-teleport-home-all",
        AboutRegionAction::TeleportHomeAll,
        7,
        true,
    );
    handles
}

/// Build the Debug tab: the editable script/collision/physics toggles and the
/// region-restart controls.
fn build_debug_tab(commands: &mut Commands, panel: Entity) -> DebugHandles {
    let mut handles = DebugHandles::default();
    let name_row = spawn_labeled_row(commands, panel, "about-region-region");
    handles.name = Some(spawn_value_node(commands, name_row));
    spawn_check(
        commands,
        panel,
        "about-region-disable-scripts",
        CheckKind::DisableScripts,
    );
    spawn_check(
        commands,
        panel,
        "about-region-disable-collisions",
        CheckKind::DisableCollisions,
    );
    spawn_check(
        commands,
        panel,
        "about-region-disable-physics",
        CheckKind::DisablePhysics,
    );
    spawn_row_action_button(
        commands,
        panel,
        "about-region-apply",
        AboutRegionAction::ApplyDebug,
        1,
    );

    let restart_row = spawn_labeled_row(commands, panel, "about-region-restart-delay");
    handles.restart_field = Some(spawn_edit_field(
        commands,
        restart_row,
        "about-region-restart-field",
        TextInputKind::NonNegativeInteger,
        6.0,
        2,
        5,
    ));
    let actions = spawn_row(commands, panel);
    spawn_action_button(
        commands,
        actions,
        "about-region-restart",
        AboutRegionAction::Restart,
        3,
        true,
    );
    spawn_action_button(
        commands,
        actions,
        "about-region-cancel-restart",
        AboutRegionAction::CancelRestart,
        4,
        true,
    );
    handles
}

/// Build the Terrain tab: the editable water/limit fields, the four detail
/// texture swatches, and the per-corner elevation fields.
fn build_terrain_tab(commands: &mut Commands, panel: Entity) -> TerrainHandles {
    let mut handles = TerrainHandles::default();
    let name_row = spawn_labeled_row(commands, panel, "about-region-region");
    handles.name = Some(spawn_value_node(commands, name_row));
    let water_row = spawn_labeled_row(commands, panel, "about-region-water-height");
    handles.water_field = Some(spawn_terrain_field(
        commands,
        water_row,
        "about-region-water-field",
        2,
    ));
    let raise_row = spawn_labeled_row(commands, panel, "about-region-terrain-raise");
    handles.raise_field = Some(spawn_terrain_field(
        commands,
        raise_row,
        "about-region-raise-field",
        3,
    ));
    let lower_row = spawn_labeled_row(commands, panel, "about-region-terrain-lower");
    handles.lower_field = Some(spawn_terrain_field(
        commands,
        lower_row,
        "about-region-lower-field",
        4,
    ));

    spawn_section_label(commands, panel, "about-region-terrain-textures");
    for (index, key) in [
        "about-region-terrain-tex-1",
        "about-region-terrain-tex-2",
        "about-region-terrain-tex-3",
        "about-region-terrain-tex-4",
    ]
    .into_iter()
    .enumerate()
    {
        let row_entity = spawn_labeled_row(commands, panel, key);
        if let Some(slot) = handles.textures.get_mut(index) {
            *slot = Some(spawn_detail_swatch(commands, row_entity, index));
        }
    }

    spawn_section_label(commands, panel, "about-region-terrain-elevation");
    for (index, keys) in [
        ("about-region-corner-sw-low", "about-region-corner-sw-high"),
        ("about-region-corner-se-low", "about-region-corner-se-high"),
        ("about-region-corner-nw-low", "about-region-corner-nw-high"),
        ("about-region-corner-ne-low", "about-region-corner-ne-high"),
    ]
    .into_iter()
    .enumerate()
    {
        let (low_key, high_key) = keys;
        let row_entity = spawn_row(commands, panel);
        spawn_key_label(commands, row_entity, low_key, DIM_LABEL_COLOR);
        let low = spawn_terrain_field(commands, row_entity, "about-region-corner-low", 6);
        spawn_key_label(commands, row_entity, high_key, DIM_LABEL_COLOR);
        let high = spawn_terrain_field(commands, row_entity, "about-region-corner-high", 6);
        if let Some(slot) = handles.start_fields.get_mut(index) {
            *slot = Some(low);
        }
        if let Some(slot) = handles.range_fields.get_mut(index) {
            *slot = Some(high);
        }
    }
    spawn_row_action_button(
        commands,
        panel,
        "about-region-apply",
        AboutRegionAction::ApplyTerrain,
        7,
    );
    handles
}

/// Build the Estate tab: the read-only estate identity plus the estate-message
/// and kick actions.
fn build_estate_tab(commands: &mut Commands, panel: Entity) -> EstateHandles {
    let mut handles = EstateHandles::default();
    let name_row = spawn_labeled_row(commands, panel, "about-region-estate");
    handles.name = Some(spawn_value_node(commands, name_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-region-estate-owner");
    handles.owner = Some(spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new("about-region-loading", "about-region-none"),
    ));
    let email_row = spawn_labeled_row(commands, panel, "about-region-abuse-email");
    handles.abuse_email = Some(spawn_value_node(commands, email_row));
    spawn_note(commands, panel, "about-region-estate-note");

    spawn_check(
        commands,
        panel,
        "about-region-estate-public",
        CheckKind::EstatePublicAccess,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-direct-tp",
        CheckKind::EstateAllowDirectTeleport,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-payment",
        CheckKind::EstateRequirePayment,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-age",
        CheckKind::EstateRequireAgeVerified,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-bots",
        CheckKind::EstateDenyBots,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-voice",
        CheckKind::EstateAllowVoice,
    );
    spawn_check(
        commands,
        panel,
        "about-region-estate-override",
        CheckKind::EstateParcelOverride,
    );
    spawn_row_action_button(
        commands,
        panel,
        "about-region-apply-estate",
        AboutRegionAction::ApplyEstate,
        2,
    );

    spawn_section_label(commands, panel, "about-region-estate-message");
    handles.message_field = Some(spawn_edit_field(
        commands,
        panel,
        "about-region-estate-message-field",
        TextInputKind::Line,
        36.0,
        2,
        255,
    ));
    let actions = spawn_row(commands, panel);
    spawn_action_button(
        commands,
        actions,
        "about-region-send-estate-message",
        AboutRegionAction::SendEstateMessage,
        3,
        true,
    );
    spawn_action_button(
        commands,
        actions,
        "about-region-kick-estate",
        AboutRegionAction::KickEstate,
        4,
        true,
    );
    handles
}

/// Build the Covenant tab (read-only).
fn build_covenant_tab(commands: &mut Commands, panel: Entity) -> CovenantHandles {
    let mut handles = CovenantHandles::default();
    let estate_row = spawn_labeled_row(commands, panel, "about-region-estate");
    handles.estate = Some(spawn_value_node(commands, estate_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-region-estate-owner");
    handles.estate_owner = Some(spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new("about-region-loading", "about-region-none"),
    ));
    handles.text = Some(spawn_value_block(commands, panel));
    let ts_row = spawn_labeled_row(commands, panel, "about-region-last-modified");
    handles.timestamp = Some(spawn_value_node(commands, ts_row));
    let region_row = spawn_labeled_row(commands, panel, "about-region-region");
    handles.region = Some(spawn_value_node(commands, region_row));
    let type_row = spawn_labeled_row(commands, panel, "about-region-type");
    handles.region_type = Some(spawn_value_node(commands, type_row));
    let rating_row = spawn_labeled_row(commands, panel, "about-region-maturity");
    handles.region_rating = Some(spawn_value_node(commands, rating_row));
    let resale_row = spawn_labeled_row(commands, panel, "about-region-resale");
    handles.resale = Some(spawn_value_node(commands, resale_row));
    let subdivide_row = spawn_labeled_row(commands, panel, "about-region-subdivide");
    handles.subdivide = Some(spawn_value_node(commands, subdivide_row));
    handles
}

/// Build the Access tab: the four estate access-list tables with add / remove.
fn build_access_tab(commands: &mut Commands, panel: Entity) -> AccessHandles {
    let mut handles = AccessHandles::default();

    spawn_section_label(commands, panel, "about-region-managers");
    let managers = spawn_bounded_table(commands, panel, &MANAGERS_TABLE);
    handles.managers_viewport = Some(managers.viewport);
    handles.managers_table = Some(managers.root);
    spawn_row_action_button(
        commands,
        panel,
        "about-region-add-manager",
        AboutRegionAction::AddManager,
        2,
    );

    spawn_section_label(commands, panel, "about-region-allowed");
    let allowed = spawn_bounded_table(commands, panel, &ALLOWED_TABLE);
    handles.allowed_viewport = Some(allowed.viewport);
    handles.allowed_table = Some(allowed.root);
    spawn_row_action_button(
        commands,
        panel,
        "about-region-add-allowed",
        AboutRegionAction::AddAllowed,
        3,
    );

    spawn_section_label(commands, panel, "about-region-allowed-groups");
    let groups = spawn_bounded_table(commands, panel, &ALLOWED_GROUPS_TABLE);
    handles.allowed_groups_viewport = Some(groups.viewport);
    handles.allowed_groups_table = Some(groups.root);
    spawn_note(commands, panel, "about-region-allowed-groups-note");

    spawn_section_label(commands, panel, "about-region-banned");
    let banned = spawn_bounded_table(commands, panel, &BANNED_TABLE);
    handles.banned_viewport = Some(banned.viewport);
    handles.banned_table = Some(banned.root);
    spawn_row_action_button(
        commands,
        panel,
        "about-region-add-banned",
        AboutRegionAction::AddBanned,
        4,
    );

    handles
}

/// Build a placeholder tab that just states the feature is not yet implemented.
fn build_placeholder_tab(commands: &mut Commands, panel: Entity, key: &'static str) {
    spawn_note(commands, panel, key);
}

/// The root + viewport handles a table hosts.
struct BoundedTable {
    /// The table root.
    root: Entity,
    /// The virtual-list viewport (carries [`VirtualList`]).
    viewport: Entity,
}

/// Spawn a table bounded to [`LIST_HEIGHT`] under `parent`.
fn spawn_bounded_table(
    commands: &mut Commands,
    parent: Entity,
    spec: &'static TableSpec,
) -> BoundedTable {
    let wrapper = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(LIST_HEIGHT),
                ..default()
            },
            BackgroundColor(LIST_BACKGROUND),
            ChildOf(parent),
        ))
        .id();
    let table = spawn_table(commands, wrapper, spec);
    BoundedTable {
        root: table.root,
        viewport: table.viewport,
    }
}

// ---------------------------------------------------------------------------
// Open.
// ---------------------------------------------------------------------------

/// The [`FloaterKey`] of the window describing the region `identity`.
///
/// The region's own id where the grid sent one, and its handle otherwise (an
/// OpenSim region can answer a handshake before its `RegionInfo2` block is
/// known) — either way, two regions are two subjects.
fn region_key(identity: &RegionIdentity) -> FloaterKey {
    if identity.region_id.is_nil() {
        FloaterKey::subject(&format!("handle/{}", identity.region_handle.get()))
    } else {
        FloaterKey::subject(&identity.region_id)
    }
}

/// Open a window on the region the agent is in — that region's window if it is
/// already up — and request fresh region / estate data.
fn open_about_region(
    mut requests: MessageReader<OpenAboutRegion>,
    mut windows: KeyedFloaters,
    mut regions_state: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut spawner: Commands,
    mut commands: MessageWriter<SlCommand>,
) {
    if requests.read().last().is_none() {
        return;
    }
    let Some(identity) = regions.iter().next().map(|region| region.0.clone()) else {
        // Nothing to open *on*: this floater's every reply is about the region
        // the agent is in, and there is no such region yet.
        warn!("About Region asked for with no current region");
        return;
    };
    let opened = windows.open(about_region_floater_spec(), region_key(&identity));
    // A fresh open re-asks the grid, whether the window is new or was already
    // up: the estate data is a snapshot, and re-opening is how a resident asks
    // for a newer one.
    commands.write(SlCommand(Command::RequestRegionInfo));
    commands.write(SlCommand(Command::RequestEstateInfo));
    commands.write(SlCommand(Command::RequestEstateCovenant));
    match opened {
        KeyedFloaterOpen::Spawned(handle) => {
            let ui = build_region_content(&mut spawner, handle);
            spawner
                .entity(handle.title_text)
                .insert(Translated::new("about-region-title"));
            // Seeded here rather than after the insert: the components only
            // reach the world when this frame's commands flush, so a window
            // spawned now is not queryable yet.
            let mut state = AboutRegionState {
                region: identity.region_id,
                is_current: true,
                ..AboutRegionState::default()
            };
            let mut dirty = AboutRegionDirty::default();
            restart_region_open(&mut state, &mut dirty);
            spawner.entity(handle.root).insert((
                state,
                dirty,
                ManagersView::default(),
                AllowedView::default(),
                AllowedGroupsView::default(),
                BannedView::default(),
                ui,
            ));
        }
        KeyedFloaterOpen::Existing(window) => {
            if let Ok((mut state, mut dirty)) = regions_state.get_mut(window) {
                restart_region_open(&mut state, &mut dirty);
            }
        }
    }
}

/// Reset a window's drafts and access lists for a fresh open, so the replies
/// the open asks for reseed everything.
fn restart_region_open(state: &mut AboutRegionState, dirty: &mut AboutRegionDirty) {
    state.draft_seeded = false;
    state.estate_seeded = false;
    state.clear_access();
    dirty.mark_all();
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold estate info / covenant / access-list / covenant-asset replies into state.
/// Fold estate info / covenant / access-list / covenant-asset replies into the
/// window whose region the agent is in.
///
/// Only that window: every reply here is about the current region and names no
/// region of its own, so a window the agent has walked out of keeps the
/// snapshot it had (see the module header).
fn ingest_about_region_events(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    mut commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for (mut state, mut dirty) in &mut windows {
        if !state.is_current {
            continue;
        }
        for event in &frame {
            match &event.0 {
                SlSessionEvent::EstateInfo(info) => {
                    if !info.estate_owner.is_nil() {
                        request_name(AgentKey::from(info.estate_owner), &mut commands);
                    }
                    // Seed the estate-flags draft from the estate's current flags on
                    // the first reply after an open, preserving bits the UI omits.
                    if !state.estate_seeded {
                        state.estate_draft = EstateFlags::from_bits(info.estate_flags);
                        state.estate_seeded = true;
                        dirty.controls = true;
                    }
                    state.estate = Some((**info).clone());
                    dirty.estate_values = true;
                }
                SlSessionEvent::EstateCovenant(covenant) => {
                    if let Some(id) = covenant.covenant_id {
                        state.covenant_pending = Some(id);
                        commands.write(SlCommand(Command::FetchAsset {
                            asset_id: AssetKey::from(id),
                            asset_type: AssetType::Notecard,
                            byte_range: None,
                        }));
                    } else {
                        state.covenant_text = None;
                        state.covenant_pending = None;
                    }
                    if !covenant.estate_owner_id.is_nil() {
                        request_name(AgentKey::from(covenant.estate_owner_id), &mut commands);
                    }
                    state.covenant = Some(covenant.clone());
                    dirty.covenant_values = true;
                }
                SlSessionEvent::EstateAccessList { kind, members, .. } => {
                    ingest_access_list(&mut state, *kind, members, &mut commands);
                }
                SlSessionEvent::AssetReceived(asset)
                    if state.covenant_pending == Some(asset.id) =>
                {
                    state.covenant_pending = None;
                    state.covenant_text = Some(decode_covenant(asset));
                    dirty.covenant_values = true;
                }
                _other => {}
            }
        }
    }
}

/// Fold one estate access-list reply chunk into its list, resolving names.
fn ingest_access_list(
    state: &mut AboutRegionState,
    kind: EstateAccessKind,
    members: &[Uuid],
    commands: &mut MessageWriter<SlCommand>,
) {
    let list = match kind {
        EstateAccessKind::Managers => AccessList::Managers,
        EstateAccessKind::AllowedAgents => AccessList::Allowed,
        EstateAccessKind::AllowedGroups => AccessList::AllowedGroups,
        EstateAccessKind::BannedAgents => AccessList::Banned,
        _other => return,
    };
    let is_group = list.is_group();
    {
        let (target, revision) = state.list_mut(list);
        for id in members {
            if !target.contains(id) {
                target.push(*id);
            }
        }
        *revision = revision.wrapping_add(1);
    }
    if is_group {
        let groups: Vec<GroupKey> = members.iter().map(|id| GroupKey::from(*id)).collect();
        if !groups.is_empty() {
            commands.write(SlCommand(Command::RequestGroupNames(groups)));
        }
    } else {
        let agents: Vec<AgentKey> = members.iter().map(|id| AgentKey::from(*id)).collect();
        if !agents.is_empty() {
            commands.write(SlCommand(Command::RequestAvatarNames(agents)));
        }
    }
}

/// Decode the covenant notecard asset (empty on error).
fn decode_covenant(asset: &Asset) -> String {
    match sl_notecard::Notecard::decode(&asset.data) {
        Ok(notecard) => notecard.text,
        Err(error) => {
            warn!("failed to decode covenant notecard {}: {error}", asset.id);
            String::new()
        }
    }
}

/// Request a single agent's display name.
fn request_name(agent: AgentKey, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
}

// ---------------------------------------------------------------------------
// Region-change refresh + draft seeding.
// ---------------------------------------------------------------------------

/// Keep each window's snapshot, rights and drafts in step with the region it is
/// about — and freeze the ones the agent has left.
///
/// A window is **current** while its region is the one the agent is in. That
/// window takes the live record (a `RegionHandshake`, a `RegionInfo` reply, a
/// teleport) and reseeds its drafts from it; every other window keeps the last
/// snapshot it had and loses its write rights, because every write this floater
/// makes goes out on the current circuit and would land on the wrong region.
#[expect(
    clippy::type_complexity,
    reason = "the region query needs the identity plus the optional limits with change detection"
)]
fn refresh_on_region(
    mut windows: Query<(&mut AboutRegionState, &mut AboutRegionDirty)>,
    regions: Query<(Ref<SlRegionIdentity>, Option<Ref<SlRegionLimits>>), With<SlCurrentRegion>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let current = regions.iter().next();
    let current_region = current
        .as_ref()
        .map(|(identity, _limits)| identity.0.region_id);
    for (mut state, mut dirty) in &mut windows {
        let is_current = current_region == Some(state.region);
        if state.is_current != is_current {
            state.is_current = is_current;
            if !is_current {
                state.can_manage = false;
            }
            dirty.mark_all();
        }
        if !is_current {
            continue;
        }
        let Some((identity, limits)) = current.as_ref() else {
            continue;
        };
        let changed =
            identity.is_changed() || limits.as_ref().is_some_and(|limits| limits.is_changed());
        if !changed && state.draft_seeded {
            continue;
        }
        state.identity = Some(identity.0.clone());
        state.can_manage = identity.0.is_estate_manager;
        state.draft = seed_draft(identity, limits.as_deref());
        state.debug_draft = seed_debug_draft(identity, limits.as_deref());
        state.terrain_draft = seed_terrain_draft(identity, limits.as_deref());
        state.draft_seeded = true;
        if !identity.0.sim_owner.is_nil() {
            request_name(AgentKey::from(identity.0.sim_owner), &mut commands);
        }
        dirty.mark_all();
    }
}

/// The region flags to seed a draft from, newest source first.
///
/// `SlRegionIdentity` is written **only** by a `RegionHandshake`, which arrives
/// once on entry. `SlRegionLimits` is written by every `RegionInfo` — including
/// the one a simulator pushes to the whole region when another estate manager
/// saves — and it carries the same `RegionFlags` bitfield. Seeding the flag
/// checkboxes from the identity therefore re-read a record frozen at entry, and
/// since `SetRegionInfo` sends the **whole** form, Apply re-asserted those stale
/// flags and reverted the other manager's change
/// ([[viewer-floaters-never-reread-after-a-push]]).
fn region_flags_for_draft(
    identity: &SlRegionIdentity,
    limits: Option<&SlRegionLimits>,
) -> RegionFlags {
    RegionFlags::from_bits(freshest_region_flags(
        identity.0.region_flags,
        limits.map(|limits| limits.0.region_flags),
    ))
}

/// The precedence [`region_flags_for_draft`] applies, without the components
/// wrapped around it: the `RegionInfo`'s bits when there are any, else the
/// handshake's.
const fn freshest_region_flags(handshake: u32, region_info: Option<u32>) -> u32 {
    match region_info {
        Some(bits) => bits,
        None => handshake,
    }
}

/// Build the region-settings draft from the live region identity and limits.
fn seed_draft(identity: &SlRegionIdentity, limits: Option<&SlRegionLimits>) -> RegionInfoUpdate {
    let flags = region_flags_for_draft(identity, limits);
    RegionInfoUpdate {
        block_terraform: flags.contains(RegionFlags::BLOCK_TERRAFORM),
        block_fly: flags.contains(RegionFlags::BLOCK_FLY),
        allow_damage: flags.contains(RegionFlags::ALLOW_DAMAGE),
        allow_land_resell: !flags.contains(RegionFlags::BLOCK_LAND_RESELL),
        restrict_pushobject: flags.contains(RegionFlags::RESTRICT_PUSHOBJECT),
        allow_parcel_changes: flags.contains(RegionFlags::ALLOW_PARCEL_CHANGES),
        agent_limit: limits.map_or(40, |limits| {
            i32::try_from(limits.0.max_agents).unwrap_or(40)
        }),
        object_bonus: limits.map_or(1.0, |limits| limits.0.object_bonus_factor),
        // Same reason as the flags: the handshake's maturity is the one it had
        // at entry, and a `RegionInfo` carries the current one.
        maturity: limits.map_or(identity.0.maturity, |limits| limits.0.maturity),
    }
}

/// Build the region-debug draft from the live region flags.
fn seed_debug_draft(
    identity: &SlRegionIdentity,
    limits: Option<&SlRegionLimits>,
) -> RegionDebugUpdate {
    let flags = region_flags_for_draft(identity, limits);
    RegionDebugUpdate {
        disable_scripts: flags.contains(RegionFlags::SKIP_SCRIPTS),
        disable_collisions: flags.contains(RegionFlags::SKIP_COLLISIONS),
        disable_physics: flags.contains(RegionFlags::SKIP_PHYSICS),
    }
}

/// Build the region-terrain draft from the live region terrain composition and
/// limits.
fn seed_terrain_draft(
    identity: &SlRegionIdentity,
    limits: Option<&SlRegionLimits>,
) -> RegionTerrainUpdate {
    let terrain = identity.0.terrain;
    let sun_hour = limits.map_or(0.0, |limits| limits.0.sun_hour.max(0.0));
    RegionTerrainUpdate {
        water_height: limits.map_or(identity.0.water_height, |limits| limits.0.water_height),
        terrain_raise_limit: limits.map_or(4.0, |limits| limits.0.terrain_raise_limit),
        terrain_lower_limit: limits.map_or(-4.0, |limits| limits.0.terrain_lower_limit),
        use_estate_sun: limits.is_none_or(|limits| limits.0.use_estate_sun),
        fixed_sun: false,
        sun_hour,
        // Nil detail slots render as the standard Linden textures, so show and
        // round-trip those rather than a bare nil id.
        detail_textures: terrain.detail_textures_or_default(),
        start_heights: terrain.start_heights,
        height_ranges: terrain.height_ranges,
    }
}

/// Mark the estate / covenant values dirty when a name cache changes (so a newly
/// resolved owner / manager name lands in place).
/// Mark each window's estate / covenant values dirty when a name cache changes
/// (so a newly resolved owner / manager name lands in place).
fn refresh_on_names(
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut windows: Query<&mut AboutRegionDirty>,
) {
    if !avatars.is_changed() && !groups.is_changed() {
        return;
    }
    for mut dirty in &mut windows {
        dirty.region_values = true;
        dirty.estate_values = true;
        dirty.covenant_values = true;
    }
}

// ---------------------------------------------------------------------------
// Seed edit fields / combo.
// ---------------------------------------------------------------------------

/// Seed the region and terrain edit fields, the maturity combo, and the terrain
/// texture-swatch labels from the drafts on a fresh region.
/// Seed each window's region and terrain edit fields, maturity combo and
/// terrain texture-swatch labels from its drafts.
fn seed_edit_fields(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    mut fields: Query<&mut EditableText>,
    mut combos: Query<&mut ComboSelection>,
    mut swatches: Query<&mut TextureSwatchValue>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.seed_fields {
            continue;
        }
        dirty.seed_fields = false;
        set_field_text(
            &mut fields,
            ui.region.agent_limit_field,
            &state.draft.agent_limit.to_string(),
        );
        set_field_text(
            &mut fields,
            ui.region.object_bonus_field,
            &format!("{:.2}", state.draft.object_bonus),
        );
        set_combo(
            &mut combos,
            ui.region.maturity_combo,
            maturity_index(state.draft.maturity),
        );
        // Terrain fields + swatch labels.
        let terrain = &state.terrain_draft;
        set_field_text(
            &mut fields,
            ui.terrain.water_field,
            &format!("{:.2}", terrain.water_height),
        );
        set_field_text(
            &mut fields,
            ui.terrain.raise_field,
            &format!("{:.2}", terrain.terrain_raise_limit),
        );
        set_field_text(
            &mut fields,
            ui.terrain.lower_field,
            &format!("{:.2}", terrain.terrain_lower_limit),
        );
        for (slot, start) in ui
            .terrain
            .start_fields
            .iter()
            .zip(terrain.start_heights.iter())
        {
            set_field_text(&mut fields, *slot, &format!("{start:.2}"));
        }
        for (slot, range) in ui
            .terrain
            .range_fields
            .iter()
            .zip(terrain.height_ranges.iter())
        {
            set_field_text(&mut fields, *slot, &format!("{range:.2}"));
        }
        for (node, texture) in ui
            .terrain
            .textures
            .iter()
            .zip(terrain.detail_textures.iter())
        {
            set_swatch(&mut swatches, *node, *texture);
        }
    }
}

/// Set a terrain swatch's texture in place (only on change), so the thumbnail
/// systems re-paint it. Writing on every seed with the same value would refire
/// the `Changed` filter needlessly, so guard on the current value.
fn set_swatch(swatches: &mut Query<&mut TextureSwatchValue>, node: Option<Entity>, texture: Uuid) {
    if let Some(node) = node
        && let Ok(mut swatch) = swatches.get_mut(node)
        && swatch.0 != TextureKey::from(texture)
    {
        swatch.0 = TextureKey::from(texture);
    }
}

// ---------------------------------------------------------------------------
// Control enable.
// ---------------------------------------------------------------------------

/// Toggle each window's write buttons' visibility and every editable control's
/// [`InteractionDisabled`] to follow the agent's estate rights **in that
/// window's region**, and repaint its checkbox glyphs.
///
/// The controls are found by walking up from each one to the window it lives in
/// ([`host_floater`]): two windows can disagree — one on the region the agent
/// manages and is standing in, one frozen on the region behind them — and a
/// sweep would give both the last window's answer.
#[expect(
    clippy::too_many_arguments,
    reason = "reconciling control enable needs every window, the write buttons, gated controls, \
              disabled set, checks, the ancestry walk, and the text query together"
)]
fn update_control_enable(
    mut windows: Query<(Entity, &mut AboutRegionDirty, &AboutRegionState)>,
    mut write_buttons: Query<(Entity, &mut Visibility), With<WriteButton>>,
    gated: Query<Entity, With<EditGate>>,
    disabled: Query<(), With<InteractionDisabled>>,
    checks: Query<(Entity, &AboutRegionCheck)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    // The windows repainting this frame, and what each one allows.
    let mut repainting: Vec<(Entity, bool)> = Vec::new();
    for (window, mut dirty, state) in &mut windows {
        if !dirty.controls {
            continue;
        }
        dirty.controls = false;
        repainting.push((window, state.can_manage));
    }
    if repainting.is_empty() {
        return;
    }
    let can_manage = |entity: Entity| {
        let host = host_floater(entity, &parents, &floaters)?;
        repainting
            .iter()
            .find_map(|(window, can_manage)| (*window == host).then_some(*can_manage))
    };
    for (entity, mut visibility) in &mut write_buttons {
        let Some(can_manage) = can_manage(entity) else {
            continue;
        };
        let want = if can_manage {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    for entity in &gated {
        let Some(can_manage) = can_manage(entity) else {
            continue;
        };
        let is_disabled = disabled.contains(entity);
        if can_manage && is_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !can_manage && !is_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (entity, check) in &checks {
        let Some(host) = host_floater(entity, &parents, &floaters) else {
            continue;
        };
        let Some(can_manage) = repainting
            .iter()
            .find_map(|(window, can_manage)| (*window == host).then_some(*can_manage))
        else {
            continue;
        };
        let Ok((_window, _dirty, state)) = windows.get(host) else {
            continue;
        };
        let on = check.kind.checked(state);
        set_check_visual(&mut texts, check, on, can_manage);
    }
}

// ---------------------------------------------------------------------------
// Value refreshes.
// ---------------------------------------------------------------------------

/// Refresh the Region tab's read-only identity values in place.
/// Refresh each window's Region tab read-only identity values in place, from
/// **its** region snapshot rather than the live one.
fn update_region_tab(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
    mut commands: Commands,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.region_values {
            continue;
        }
        dirty.region_values = false;
        let region = state.identity.as_ref();
        // The title carries the region's name (a plain string, not a Fluent
        // key): two windows on two regions are otherwise identical strips.
        if let Some(name) = region
            .and_then(|region| region.sim_name.as_ref())
            .map(RegionName::to_string)
            && let Ok((mut title, _color)) = texts.get_mut(ui.title_text)
        {
            name.clone_into(&mut title.0);
            commands.entity(ui.title_text).remove::<Translated>();
        }
        let handles = &ui.region;
        set_value_node(&mut texts, handles.name, &region_name(region, &translator));
        set_value_node(
            &mut texts,
            handles.region_type,
            &product_text(region.map(|region| region.product), &translator),
        );
        let owner = region.and_then(|region| region.owner()).map(AgentKey::from);
        set_name_link(
            &mut links,
            handles.owner,
            NameTarget::from_option(region.is_some(), owner),
        );
        set_value_node(
            &mut texts,
            handles.grid_position,
            &region.map_or_else(
                || translator.get("about-region-loading"),
                |region| {
                    format!(
                        "{}, {}",
                        region.grid_coordinates.x(),
                        region.grid_coordinates.y()
                    )
                },
            ),
        );
    }
}

/// Refresh the Debug tab's read-only region name in place.
/// Refresh each window's Debug tab read-only region name in place.
fn update_debug_tab(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.debug_values {
            continue;
        }
        dirty.debug_values = false;
        let region = state.identity.as_ref();
        set_value_node(&mut texts, ui.debug.name, &region_name(region, &translator));
    }
}

/// Refresh the Terrain tab's region name in place (its fields and swatches are
/// seeded from the terrain draft by [`seed_edit_fields`]).
/// Refresh each window's Terrain tab region name in place (its fields and
/// swatches are seeded from the terrain draft by [`seed_edit_fields`]).
fn update_terrain_tab(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.terrain_values {
            continue;
        }
        dirty.terrain_values = false;
        let identity = state.identity.as_ref();
        set_value_node(
            &mut texts,
            ui.terrain.name,
            &region_name(identity, &translator),
        );
    }
}

/// Refresh the Estate tab's read-only values in place.
/// Refresh each window's Estate tab read-only values in place.
fn update_estate_tab(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.estate_values {
            continue;
        }
        dirty.estate_values = false;
        let handles = &ui.estate;
        let loading = translator.get("about-region-loading");
        // The `getinfo` reply ([`EstateInfo`]) needs estate-manager rights, so a
        // plain resident never receives it; fall back to the covenant reply
        // (`EstateCovenantReply`), which carries the estate name and owner too.
        let estate_name = state
            .estate
            .as_ref()
            .map(|estate| estate.estate_name.clone())
            .or_else(|| {
                state
                    .covenant
                    .as_ref()
                    .map(|covenant| covenant.estate_name.clone())
            });
        // A nil owner id (Aditi's covenant reply for some estates) maps to no link.
        let estate_owner = state
            .estate
            .as_ref()
            .map(|estate| estate.estate_owner)
            .or_else(|| {
                state
                    .covenant
                    .as_ref()
                    .map(|covenant| covenant.estate_owner_id)
            })
            .filter(|id| !id.is_nil())
            .map(AgentKey::from);
        set_value_node(
            &mut texts,
            handles.name,
            &estate_name.unwrap_or_else(|| loading.clone()),
        );
        // A known estate (either reply arrived) resolves to the owner or `(none)`;
        // before any reply it is `(loading)`.
        let estate_known = state.estate.is_some() || state.covenant.is_some();
        set_name_link(
            &mut links,
            handles.owner,
            NameTarget::from_option(estate_known, estate_owner),
        );
        // The abuse email only comes from `getinfo`; show `(none)` once we know the
        // estate but got no email, and only `(loading)` before any estate reply.
        let none = translator.get("about-region-none");
        let abuse_email = match &state.estate {
            Some(estate) if !estate.abuse_email.is_empty() => estate.abuse_email.clone(),
            Some(_estate) => none.clone(),
            None if state.covenant.is_some() => none,
            None => loading.clone(),
        };
        set_value_node(&mut texts, handles.abuse_email, &abuse_email);
    }
}

/// Refresh the Covenant tab's read-only values in place.
/// Refresh each window's Covenant tab read-only values in place.
fn update_covenant_tab(
    mut windows: Query<(&mut AboutRegionDirty, &AboutRegionUi, &AboutRegionState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.covenant_values {
            continue;
        }
        dirty.covenant_values = false;
        let handles = &ui.covenant;
        let region = state.identity.as_ref();
        if let Some(covenant) = &state.covenant {
            set_value_node(&mut texts, handles.estate, &covenant.estate_name);
            let owner = (!covenant.estate_owner_id.is_nil())
                .then(|| AgentKey::from(covenant.estate_owner_id));
            set_value_node(
                &mut texts,
                handles.timestamp,
                &format_unix_date(i64::from(covenant.covenant_timestamp)),
            );
            set_name_link(
                &mut links,
                handles.estate_owner,
                NameTarget::from_option(true, owner),
            );
        } else {
            set_name_link::<AgentKey>(&mut links, handles.estate_owner, NameTarget::Loading);
        }
        set_value_node(
            &mut texts,
            handles.text,
            &covenant_body(
                state.covenant.as_ref(),
                state.covenant_text.as_deref(),
                &translator,
            ),
        );
        set_value_node(
            &mut texts,
            handles.region,
            &region_name(region, &translator),
        );
        set_value_node(
            &mut texts,
            handles.region_type,
            &product_text(region.map(|region| region.product), &translator),
        );
        set_value_node(
            &mut texts,
            handles.region_rating,
            &maturity_text(region.map(|region| region.maturity), &translator),
        );
        let flags = region.map(|region| RegionFlags::from_bits(region.region_flags));
        set_value_node(&mut texts, handles.resale, &resale_text(flags, &translator));
        set_value_node(
            &mut texts,
            handles.subdivide,
            &subdivide_text(flags, &translator),
        );
    }
}

// ---------------------------------------------------------------------------
// Access-list views + tables.
// ---------------------------------------------------------------------------

/// Rebuild the estate-managers view when the list or the name cache changes.
/// Rebuild each window's estate-managers view when its list or the name cache changes.
fn sync_managers_view(
    mut windows: Query<(&AboutRegionState, &mut ManagersView, &AboutRegionUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            AccessList::Managers,
            state.managers_revision,
            &state.managers,
            &mut view.rows,
            &mut view.built,
            ui.access.managers_viewport,
            &avatars,
            &groups,
            avatars.is_changed() || groups.is_changed(),
            &mut lists,
        );
    }
}

/// Rebuild the allowed-residents view.
/// Rebuild each window's allowed-residents view.
fn sync_allowed_view(
    mut windows: Query<(&AboutRegionState, &mut AllowedView, &AboutRegionUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            AccessList::Allowed,
            state.allowed_revision,
            &state.allowed,
            &mut view.rows,
            &mut view.built,
            ui.access.allowed_viewport,
            &avatars,
            &groups,
            avatars.is_changed() || groups.is_changed(),
            &mut lists,
        );
    }
}

/// Rebuild the allowed-groups view.
/// Rebuild each window's allowed-groups view.
fn sync_allowed_groups_view(
    mut windows: Query<(&AboutRegionState, &mut AllowedGroupsView, &AboutRegionUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            AccessList::AllowedGroups,
            state.allowed_groups_revision,
            &state.allowed_groups,
            &mut view.rows,
            &mut view.built,
            ui.access.allowed_groups_viewport,
            &avatars,
            &groups,
            avatars.is_changed() || groups.is_changed(),
            &mut lists,
        );
    }
}

/// Rebuild the banned-residents view.
/// Rebuild each window's banned-residents view.
fn sync_banned_view(
    mut windows: Query<(&AboutRegionState, &mut BannedView, &AboutRegionUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            AccessList::Banned,
            state.banned_revision,
            &state.banned,
            &mut view.rows,
            &mut view.built,
            ui.access.banned_viewport,
            &avatars,
            &groups,
            avatars.is_changed() || groups.is_changed(),
            &mut lists,
        );
    }
}

/// The shared rebuild of an access-list view (resolving names) + item count.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared access-view rebuild threads the list kind, its revision, the row sink, the \
              viewport, the avatar / group name sources, and the names-changed flag"
)]
fn sync_access_view(
    list: AccessList,
    revision: u64,
    ids: &[Uuid],
    rows: &mut Vec<AccessRowData>,
    built: &mut u64,
    viewport: Option<Entity>,
    avatars: &AvatarState,
    groups: &GroupsModel,
    names_changed: bool,
    lists: &mut Query<&mut VirtualList>,
) {
    if *built == revision && !names_changed {
        return;
    }
    *built = revision;
    rows.clear();
    rows.extend(ids.iter().map(|id| AccessRowData {
        name: if list.is_group() {
            groups
                .group_name(GroupKey::from(*id))
                .map_or_else(|| format!("({id})"), str::to_owned)
        } else {
            avatars.label_text(AgentKey::from(*id))
        },
        id: *id,
    }));
    if let Some(viewport) = viewport
        && let Ok(mut virtual_list) = lists.get_mut(viewport)
    {
        virtual_list.item_count = rows.len();
    }
}

/// Build each newly-pooled access row's cells + Remove button once.
/// Build each newly-pooled access row's cells + Remove button once, in
/// whichever window's list it was pooled into.
fn populate_access_rows(
    mut commands: Commands,
    windows: Query<&AboutRegionUi>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        let parent = child_of.parent();
        for ui in &windows {
            let access = &ui.access;
            let matched = [
                (
                    access.managers_viewport,
                    access.managers_table,
                    &MANAGERS_TABLE,
                    AccessList::Managers,
                ),
                (
                    access.allowed_viewport,
                    access.allowed_table,
                    &ALLOWED_TABLE,
                    AccessList::Allowed,
                ),
                (
                    access.allowed_groups_viewport,
                    access.allowed_groups_table,
                    &ALLOWED_GROUPS_TABLE,
                    AccessList::AllowedGroups,
                ),
                (
                    access.banned_viewport,
                    access.banned_table,
                    &BANNED_TABLE,
                    AccessList::Banned,
                ),
            ]
            .into_iter()
            .find(|(viewport, _table, _spec, _list)| *viewport == Some(parent));
            let Some((_viewport, Some(table), spec, list)) = matched else {
                continue;
            };
            let cells = spawn_table_row(&mut commands, row_entity, table, spec);
            if let Some(custom) = cells.cell(1) {
                spawn_remove_button(&mut commands, custom, list, row_entity);
            }
            break;
        }
    }
}

/// Every window's four access-list views, state and handles — the row binder's
/// read of the windows, named because the tuple is past the point of reading
/// well inline.
type RegionBindWindows<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        Ref<'static, ManagersView>,
        Ref<'static, AllowedView>,
        Ref<'static, AllowedGroupsView>,
        Ref<'static, BannedView>,
        Ref<'static, AboutRegionState>,
        &'static AboutRegionUi,
    ),
>;

/// Bind each pooled access row to its window's resolved name, and reveal that
/// window's Remove buttons only when the agent may manage its estate.
fn bind_access_rows(
    windows: RegionBindWindows,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    removes: Query<Entity, With<RemoveAccessButton>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (window, managers, allowed, allowed_groups, banned, state, ui) in &windows {
        let refresh = managers.is_changed()
            || allowed.is_changed()
            || allowed_groups.is_changed()
            || banned.is_changed()
            || state.is_changed();
        let access = &ui.access;
        for (row, child_of, cells) in &rows {
            let parent = child_of.parent();
            let view = if Some(parent) == access.managers_viewport {
                &managers.rows
            } else if Some(parent) == access.allowed_viewport {
                &allowed.rows
            } else if Some(parent) == access.allowed_groups_viewport {
                &allowed_groups.rows
            } else if Some(parent) == access.banned_viewport {
                &banned.rows
            } else {
                continue;
            };
            if !refresh && !row.is_changed() {
                continue;
            }
            let Some(data) = row.index.and_then(|index| view.get(index)) else {
                continue;
            };
            set_cell(&mut texts, cells, 0, &data.name);
        }
        let want = if state.can_manage {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for entity in &removes {
            if host_floater(entity, &parents, &floaters) != Some(window) {
                continue;
            }
            if let Ok(mut vis) = visibility.get_mut(entity)
                && *vis != want
            {
                *vis = want;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Edit observers / handlers.
// ---------------------------------------------------------------------------

/// Toggle a checkbox, flipping its backing draft field.
/// Toggle a checkbox, flipping the draft field of the window it was pressed in.
fn on_about_region_check(
    press: On<Pointer<Press>>,
    checks: Query<&AboutRegionCheck>,
    mut windows: Query<&mut AboutRegionState>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(check) = checks.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    if !state.can_manage {
        return;
    }
    check.kind.toggle(&mut state);
    let on = check.kind.checked(&state);
    set_check_visual(&mut texts, check, on, true);
}

/// Dispatch a floater action-button press.
/// Dispatch a floater action-button press, in the window it was pressed in.
#[expect(
    clippy::too_many_arguments,
    reason = "the dispatcher fans out to every button kind, reading the pressed window, its \
              fields and the picker / command outputs"
)]
fn on_about_region_action(
    press: On<Pointer<Press>>,
    actions: Query<&AboutRegionAction>,
    mut windows: Query<(&mut AboutRegionState, &AboutRegionUi)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    fields: Query<&EditableText>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut pickers: MessageWriter<OpenAvatarPicker>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = windows.get_mut(window) else {
        return;
    };
    // A window whose region the agent has left has no rights: every write here
    // goes out on the current circuit (see the module header).
    if !state.can_manage {
        return;
    }
    let read = |entity: Option<Entity>| {
        entity
            .and_then(|field| fields.get(field).ok())
            .map(|field| field.value().to_string())
    };
    match action {
        AboutRegionAction::Apply => {
            if let Some(limit) =
                read(ui.region.agent_limit_field).and_then(|value| value.trim().parse::<i32>().ok())
            {
                state.draft.agent_limit = limit;
            }
            if let Some(bonus) = read(ui.region.object_bonus_field)
                .and_then(|value| value.trim().parse::<f32>().ok())
            {
                state.draft.object_bonus = bonus;
            }
            sl_commands.write(SlCommand(Command::SetRegionInfo(state.draft.clone())));
            sl_commands.write(SlCommand(Command::RequestRegionInfo));
        }
        AboutRegionAction::ApplyDebug => {
            sl_commands.write(SlCommand(Command::SetRegionDebug(state.debug_draft)));
            sl_commands.write(SlCommand(Command::RequestRegionInfo));
        }
        AboutRegionAction::ApplyTerrain => {
            read_terrain_fields(&mut state, ui, &read);
            sl_commands.write(SlCommand(Command::SetRegionTerrain(
                state.terrain_draft.clone(),
            )));
        }
        AboutRegionAction::ApplyEstate => {
            let Some(estate) = &state.estate else {
                return;
            };
            // Preserve the estate's other flags; fixed-sun estates are no longer
            // supported (the reference clears the bit on any change).
            let flags = state
                .estate_draft
                .with(EstateFlags::SUN_FIXED, false)
                .bits();
            let update = EstateInfoUpdate {
                estate_name: estate.estate_name.clone(),
                flags,
                sun_hour: 0.0,
            };
            sl_commands.write(SlCommand(Command::SetEstateInfo(update)));
        }
        AboutRegionAction::TeleportHomeOne => {
            state.pending_pick = Some(PICK_TELEPORT);
            pickers.write(OpenAvatarPicker::one(PICK_TELEPORT));
        }
        AboutRegionAction::TeleportHomeAll => {
            sl_commands.write(SlCommand(Command::TeleportHomeAllUsers));
        }
        AboutRegionAction::Restart => {
            let seconds = read(ui.debug.restart_field)
                .and_then(|value| value.trim().parse::<i32>().ok())
                .unwrap_or(120);
            sl_commands.write(SlCommand(Command::RestartRegion { seconds }));
        }
        AboutRegionAction::CancelRestart => {
            sl_commands.write(SlCommand(Command::RestartRegion { seconds: -1 }));
        }
        AboutRegionAction::SendEstateMessage => {
            if let Some(message) = read(ui.estate.message_field)
                && !message.trim().is_empty()
            {
                sl_commands.write(SlCommand(Command::SendEstateMessage { message }));
            }
        }
        AboutRegionAction::KickEstate => {
            state.pending_pick = Some(PICK_KICK);
            pickers.write(OpenAvatarPicker::one(PICK_KICK));
        }
        // The three estate access lists take a multi-pick, as the reference's do
        // ("avatar picker yes multi-select"); a kick or a send-home is about one
        // resident, so those stay single.
        AboutRegionAction::AddManager => {
            state.pending_pick = Some(PICK_MANAGER);
            pickers.write(OpenAvatarPicker::many(PICK_MANAGER));
        }
        AboutRegionAction::AddAllowed => {
            state.pending_pick = Some(PICK_ALLOWED);
            pickers.write(OpenAvatarPicker::many(PICK_ALLOWED));
        }
        AboutRegionAction::AddBanned => {
            state.pending_pick = Some(PICK_BANNED);
            pickers.write(OpenAvatarPicker::many(PICK_BANNED));
        }
    }
}

/// One window's four access-list views and its state — the remove observer's
/// read of a window, named because the tuple is past the point of reading well
/// inline.
type RegionRemoveWindows<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static ManagersView,
        &'static AllowedView,
        &'static AllowedGroupsView,
        &'static BannedView,
        &'static mut AboutRegionState,
    ),
>;

/// Resolve and act on a per-row access Remove press, in the window it was
/// pressed in.
fn on_remove_access(
    press: On<Pointer<Press>>,
    buttons: Query<&RemoveAccessButton>,
    rows: Query<&VirtualRow>,
    mut windows: RegionRemoveWindows,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(press.entity) else {
        return;
    };
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((managers, allowed, allowed_groups, banned, mut state)) = windows.get_mut(window) else {
        return;
    };
    if !state.can_manage {
        return;
    }
    let Ok(row) = rows.get(button.row) else {
        return;
    };
    let Some(index) = row.index else {
        return;
    };
    let view = match button.list {
        AccessList::Managers => &managers.rows,
        AccessList::Allowed => &allowed.rows,
        AccessList::AllowedGroups => &allowed_groups.rows,
        AccessList::Banned => &banned.rows,
    };
    let Some(id) = view.get(index).map(|entry| entry.id) else {
        return;
    };
    commands.write(SlCommand(Command::UpdateEstateAccess {
        delta: button.list.remove_delta(),
        target: button.list.target(id),
    }));
    remove_from_list(&mut state, button.list, id);
}

/// Fold a maturity combo pick into the draft.
/// Fold a maturity combo pick into the draft of the window whose combo it was.
fn apply_combo_edits(
    mut changed: MessageReader<ComboChanged>,
    mut windows: Query<(&AboutRegionUi, &mut AboutRegionState)>,
) {
    let frame: Vec<ComboChanged> = changed.read().copied().collect();
    if frame.is_empty() {
        return;
    }
    for (ui, mut state) in &mut windows {
        for event in &frame {
            if Some(event.combo) == ui.region.maturity_combo {
                state.draft.maturity = maturity_from_index(event.active);
            }
        }
    }
}

/// Fold the avatar picks into the estate action that opened the picker — each
/// chosen resident in turn, since the access lists open a multi-picker.
/// Fold the avatar picks into the estate action of the window that asked — each
/// chosen resident in turn, since the access lists open a multi-picker.
///
/// The picker echoes a tag rather than an entity, so the window is the one
/// holding a matching claim ([`AboutRegionState::pending_pick`]).
fn apply_avatar_picks(
    mut picked: MessageReader<AvatarPicked>,
    mut windows: Query<&mut AboutRegionState>,
    mut commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<AvatarPicked> = picked.read().cloned().collect();
    if frame.is_empty() {
        return;
    }
    for event in &frame {
        for mut state in &mut windows {
            if state.pending_pick != Some(event.requester) || !state.can_manage {
                continue;
            }
            state.pending_pick = None;
            for chosen in &event.picks {
                let agent = chosen.agent;
                match event.requester {
                    PICK_TELEPORT => {
                        commands.write(SlCommand(Command::TeleportHomeUser { target: agent }));
                    }
                    PICK_KICK => {
                        commands.write(SlCommand(Command::KickEstateUser { target: agent }));
                    }
                    PICK_MANAGER => {
                        add_access_entry(&mut state, AccessList::Managers, agent, &mut commands);
                    }
                    PICK_ALLOWED => {
                        add_access_entry(&mut state, AccessList::Allowed, agent, &mut commands);
                    }
                    PICK_BANNED => {
                        add_access_entry(&mut state, AccessList::Banned, agent, &mut commands);
                    }
                    _other => {}
                }
            }
        }
    }
}

/// Fold a terrain texture pick into the terrain draft slot and repaint its
/// swatch thumbnail (via [`TextureSwatchValue`]).
/// Fold a terrain texture pick into the terrain draft slot of the window whose
/// swatch asked for it, and repaint that swatch's thumbnail (via
/// [`TextureSwatchValue`]).
fn apply_texture_edits(
    mut picked: MessageReader<TexturePicked>,
    mut swatches: Query<(&TerrainSwatch, &mut TextureSwatchValue)>,
    mut windows: Query<&mut AboutRegionState>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
) {
    for event in picked.read() {
        if !event.final_pick {
            continue;
        }
        // The pick belongs to the window the pressed swatch lives in — the
        // picker echoes the swatch, and the swatch names its window.
        let Some(window) = host_floater(event.requester, &parents, &floaters) else {
            continue;
        };
        let Ok(mut state) = windows.get_mut(window) else {
            continue;
        };
        if !state.can_manage {
            continue;
        }
        let Ok((swatch, mut value)) = swatches.get_mut(event.requester) else {
            continue;
        };
        if let Some(slot) = state.terrain_draft.detail_textures.get_mut(swatch.slot) {
            *slot = event.texture.uuid();
        }
        if value.0 != event.texture {
            value.0 = event.texture;
        }
    }
}

/// Read the terrain edit fields into the terrain draft on Apply.
fn read_terrain_fields(
    state: &mut AboutRegionState,
    ui: &AboutRegionUi,
    read: &dyn Fn(Option<Entity>) -> Option<String>,
) {
    let parse = |value: Option<String>| value.and_then(|value| value.trim().parse::<f32>().ok());
    if let Some(value) = parse(read(ui.terrain.water_field)) {
        state.terrain_draft.water_height = value;
    }
    if let Some(value) = parse(read(ui.terrain.raise_field)) {
        state.terrain_draft.terrain_raise_limit = value;
    }
    if let Some(value) = parse(read(ui.terrain.lower_field)) {
        state.terrain_draft.terrain_lower_limit = value;
    }
    for (slot, field) in ui.terrain.start_fields.iter().enumerate() {
        if let Some(value) = parse(read(*field))
            && let Some(dst) = state.terrain_draft.start_heights.get_mut(slot)
        {
            *dst = value;
        }
    }
    for (slot, field) in ui.terrain.range_fields.iter().enumerate() {
        if let Some(value) = parse(read(*field))
            && let Some(dst) = state.terrain_draft.height_ranges.get_mut(slot)
        {
            *dst = value;
        }
    }
}

/// Append an agent to an estate access list and commit the delta.
fn add_access_entry(
    state: &mut AboutRegionState,
    list: AccessList,
    agent: AgentKey,
    commands: &mut MessageWriter<SlCommand>,
) {
    let id = agent.0.0;
    {
        let (target, revision) = state.list_mut(list);
        if target.contains(&id) {
            return;
        }
        target.push(id);
        *revision = revision.wrapping_add(1);
    }
    commands.write(SlCommand(Command::UpdateEstateAccess {
        delta: list.add_delta(),
        target: list.target(id),
    }));
}

/// Remove an id from an estate access list (the optimistic local update).
fn remove_from_list(state: &mut AboutRegionState, list: AccessList, id: Uuid) {
    let (target, revision) = state.list_mut(list);
    if let Some(position) = target.iter().position(|entry| *entry == id) {
        target.remove(position);
        *revision = revision.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Value formatting.
// ---------------------------------------------------------------------------

/// The region name, or a loading placeholder.
fn region_name(region: Option<&sl_client_bevy::RegionIdentity>, translator: &Translator) -> String {
    region
        .and_then(|region| region.sim_name.as_ref())
        .map_or_else(
            || translator.get("about-region-loading"),
            ToString::to_string,
        )
}

/// The product-type label.
fn product_text(product: Option<ProductType>, translator: &Translator) -> String {
    let key = match product {
        Some(ProductType::FullRegion) => "about-region-product-full",
        Some(ProductType::Homestead) => "about-region-product-homestead",
        Some(ProductType::Openspace) => "about-region-product-openspace",
        // `Unknown`, `None`, or a future variant.
        _other => "about-region-product-unknown",
    };
    translator.get(key)
}

/// The maturity-rating label.
fn maturity_text(maturity: Option<Maturity>, translator: &Translator) -> String {
    let key = match maturity {
        Some(Maturity::Pg) => "about-region-rating-pg",
        Some(Maturity::Mature) => "about-region-rating-mature",
        Some(Maturity::Adult) => "about-region-rating-adult",
        // `Unknown`, `None`, or a future variant.
        _other => "about-region-rating-unknown",
    };
    translator.get(key)
}

/// The resale-clause text.
fn resale_text(flags: Option<RegionFlags>, translator: &Translator) -> String {
    let key = match flags {
        Some(flags) if flags.contains(RegionFlags::BLOCK_LAND_RESELL) => {
            "about-region-resale-blocked"
        }
        Some(_flags) => "about-region-resale-allowed",
        None => "about-region-loading",
    };
    translator.get(key)
}

/// The subdivide-clause text.
fn subdivide_text(flags: Option<RegionFlags>, translator: &Translator) -> String {
    let key = match flags {
        Some(flags) if flags.contains(RegionFlags::ALLOW_PARCEL_CHANGES) => {
            "about-region-subdivide-allowed"
        }
        Some(_flags) => "about-region-subdivide-blocked",
        None => "about-region-loading",
    };
    translator.get(key)
}

/// The covenant body text, or an appropriate placeholder.
fn covenant_body(
    covenant: Option<&EstateCovenant>,
    text: Option<&str>,
    translator: &Translator,
) -> String {
    if let Some(text) = text {
        return text.to_owned();
    }
    let has_covenant = covenant.is_some_and(|covenant| covenant.covenant_id.is_some());
    if has_covenant {
        translator.get("about-region-covenant-loading")
    } else {
        translator.get("about-region-covenant-none")
    }
}

/// The maturity combo index for a rating.
const fn maturity_index(maturity: Maturity) -> usize {
    match maturity {
        Maturity::Mature => 1,
        Maturity::Adult => 2,
        // `Pg`, `Unknown`, or a future variant.
        _other => 0,
    }
}

/// The maturity for a combo index.
const fn maturity_from_index(index: usize) -> Maturity {
    match index {
        1 => Maturity::Mature,
        2 => Maturity::Adult,
        _other => Maturity::Pg,
    }
}

// ---------------------------------------------------------------------------
// In-place setters.
// ---------------------------------------------------------------------------

/// Set a retained value node's text in place (only on change).
fn set_value_node(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    node: Option<Entity>,
    value: &str,
) {
    if let Some(node) = node
        && let Ok((mut text, _color)) = texts.get_mut(node)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Set a table cell's text in place.
fn set_cell(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    cells: &crate::ui_table::TableRowCells,
    column: usize,
    value: &str,
) {
    if let Some(cell) = cells.cell(column) {
        set_table_cell(texts, cell, value, LABEL_COLOR);
    }
}

/// Set a checkbox's glyph and label in place, greying both when disabled.
fn set_check_visual(
    texts: &mut Query<(&mut Text, &mut TextColor)>,
    check: &AboutRegionCheck,
    on: bool,
    enabled: bool,
) {
    let glyph = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
    let glyph_color = if !enabled {
        DISABLED_COLOR
    } else if on {
        CHECK_COLOR
    } else {
        DIM_LABEL_COLOR
    };
    if let Ok((mut text, mut color)) = texts.get_mut(check.glyph) {
        if text.0 != glyph {
            glyph.clone_into(&mut text.0);
        }
        let wanted = TextColor(glyph_color);
        if *color != wanted {
            *color = wanted;
        }
    }
    let label_color = TextColor(if enabled { LABEL_COLOR } else { DISABLED_COLOR });
    if let Ok((_text, mut color)) = texts.get_mut(check.label)
        && *color != label_color
    {
        *color = label_color;
    }
}

/// Seed a text field's content in place, skipping an actively-edited field.
#[expect(
    clippy::cmp_owned,
    reason = "the editor's SplitString has no borrow-free comparison against &str; this guard runs \
              only on a discrete reseed, not per frame"
)]
fn set_field_text(fields: &mut Query<&mut EditableText>, field: Option<Entity>, value: &str) {
    if let Some(field) = field
        && let Ok(mut editable) = fields.get_mut(field)
        && !editable.is_composing()
        && editable.value().to_string() != value
    {
        editable.editor_mut().set_text(value);
    }
}

/// Set a combo's selection in place (a programmatic write emits no `ComboChanged`).
fn set_combo(combos: &mut Query<&mut ComboSelection>, combo: Option<Entity>, active: usize) {
    if let Some(combo) = combo
        && let Ok(mut selection) = combos.get_mut(combo)
        && selection.active != active
    {
        selection.active = active;
    }
}

// ---------------------------------------------------------------------------
// Spawn helpers.
// ---------------------------------------------------------------------------

/// A plain wrapping row.
fn spawn_row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id()
}

/// A wrapping row leading with a translated dim label.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = spawn_row(commands, parent);
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    row_entity
}

/// A translated section label on its own line.
fn spawn_section_label(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A wrapped translated note paragraph.
fn spawn_note(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(0.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

/// An empty value node the caller updates in place.
fn spawn_value_node(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id()
}

/// A wrapped, clipped read-only text value node (covenant body).
fn spawn_value_block(commands: &mut Commands, parent: Entity) -> Entity {
    let block = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                max_height: Val::Px(180.0),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
            ChildOf(block),
        ))
        .id()
}

/// A single-line / numeric edit field, gated on estate rights.
fn spawn_edit_field(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    kind: TextInputKind,
    width_glyphs: f32,
    tab_index: i32,
    max_characters: usize,
) -> Entity {
    let field = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs,
            tab_index,
            max_characters: Some(max_characters),
            ..TextInputSpec::new(element, kind)
        },
    );
    commands.entity(field).insert(EditGate);
    field
}

/// A translated action button dispatching `action`. `write` tags it as a write
/// button (hidden when the agent cannot manage the estate).
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: AboutRegionAction,
    tab_index: i32,
    write: bool,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("about-region-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_about_region_action)
        .id();
    if write {
        commands.entity(button).insert(WriteButton);
    }
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// The shared Apply button for the region-settings tab.
fn spawn_apply_button(commands: &mut Commands, parent: Entity, tab_index: i32) {
    let row_entity = spawn_row(commands, parent);
    spawn_action_button(
        commands,
        row_entity,
        "about-region-apply",
        AboutRegionAction::Apply,
        tab_index,
        true,
    );
}

/// A write action button on its own row (Apply / Add / …).
fn spawn_row_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: AboutRegionAction,
    tab_index: i32,
) {
    let row_entity = spawn_row(commands, parent);
    spawn_action_button(commands, row_entity, label_key, action, tab_index, true);
}

/// A translated label in `color` on `parent`.
fn spawn_key_label(commands: &mut Commands, parent: Entity, key: &'static str, color: Color) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(color),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A small float edit field for a terrain value, gated on estate rights.
fn spawn_terrain_field(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    tab_index: i32,
) -> Entity {
    spawn_edit_field(
        commands,
        parent,
        element,
        TextInputKind::Float,
        6.0,
        tab_index,
        10,
    )
}

/// A terrain detail-texture swatch: the reusable [`spawn_texture_swatch`] widget
/// (a thumbnail that opens the picker on click and, being `EditGate`-gated, is
/// disabled read-only for non-managers — the shared `open_picker_from_swatch`
/// honours `InteractionDisabled`), tagged [`TerrainSwatch`] so the pick reply
/// routes back to `slot`.
fn spawn_detail_swatch(commands: &mut Commands, parent: Entity, slot: usize) -> Entity {
    let swatch = spawn_texture_swatch(
        commands,
        parent,
        "about-region-terrain-detail",
        5,
        TextureKey::from(Uuid::nil()),
    );
    commands
        .entity(swatch)
        .insert((TerrainSwatch { slot }, EditGate));
    swatch
}

/// A checkbox: a clickable glyph leading a translated label, gated on estate
/// rights ([`EditGate`]).
fn spawn_check(commands: &mut Commands, parent: Entity, label_key: &'static str, kind: CheckKind) {
    let row_entity = spawn_row(commands, parent);
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    let label = commands
        .spawn((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id();
    commands
        .entity(row_entity)
        .insert((
            Button,
            AboutRegionCheck { kind, glyph, label },
            EditGate,
            Pickable::default(),
        ))
        .add_child(glyph)
        .add_child(label)
        .observe(on_about_region_check);
}

/// A maturity combo on `parent`, gated on estate rights.
fn spawn_maturity_combo(commands: &mut Commands, parent: Entity, tab_index: i32) -> Entity {
    let labels: Vec<String> = MATURITY_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let combo = spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element: "about-region-maturity-combo",
            labels: &labels,
            active: 0,
            tab_index,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );
    commands.entity(combo).insert(EditGate);
    combo
}

/// A per-row access Remove button in a table's custom cell.
fn spawn_remove_button(commands: &mut Commands, cell: Entity, list: AccessList, row: Entity) {
    let button = commands
        .spawn((
            Button,
            RemoveAccessButton { list, row },
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            ChildOf(cell),
        ))
        .observe(on_remove_access)
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("about-region-remove"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

#[cfg(test)]
mod tests {
    use super::{
        AboutRegionState, AccessList, CheckKind, freshest_region_flags, maturity_from_index,
        maturity_index,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        EstateAccessDelta, EstateFlags, Maturity, OwnerKey, RegionInfoUpdate, Uuid,
    };

    /// The maturity ↔ combo-index mapping round-trips for every real rating, and
    /// the combo indices agree with the option key order.
    #[test]
    fn maturity_index_round_trips() {
        for maturity in [Maturity::Pg, Maturity::Mature, Maturity::Adult] {
            assert_eq!(maturity_from_index(maturity_index(maturity)), maturity);
        }
        assert_eq!(maturity_index(Maturity::Pg), 0);
        assert_eq!(maturity_index(Maturity::Mature), 1);
        assert_eq!(maturity_index(Maturity::Adult), 2);
        // An unknown rating falls back to the first (General) option.
        assert_eq!(maturity_index(Maturity::Unknown), 0);
        assert_eq!(maturity_from_index(0), Maturity::Pg);
    }

    /// Toggling a region checkbox flips exactly its region-draft field and reads
    /// back the change.
    #[test]
    fn toggle_flips_the_region_draft_field() {
        let mut state = AboutRegionState::default();
        let before = CheckKind::BlockFly.checked(&state);
        CheckKind::BlockFly.toggle(&mut state);
        assert_eq!(CheckKind::BlockFly.checked(&state), !before);
        assert_eq!(state.draft.block_fly, !before);
    }

    /// A debug checkbox is backed by the debug draft (not the live flags).
    #[test]
    fn debug_check_drives_the_debug_draft() {
        let mut state = AboutRegionState::default();
        assert!(!CheckKind::DisableScripts.checked(&state));
        CheckKind::DisableScripts.toggle(&mut state);
        assert!(CheckKind::DisableScripts.checked(&state));
        assert!(state.debug_draft.disable_scripts);
    }

    /// An estate checkbox toggles exactly its estate flag bit, leaving the other
    /// estate bits untouched.
    #[test]
    fn estate_check_toggles_only_its_bit() {
        let mut state = AboutRegionState {
            estate_draft: EstateFlags::from_bits(EstateFlags::DENY_ANONYMOUS.bits()),
            ..Default::default()
        };
        assert!(!CheckKind::EstatePublicAccess.checked(&state));
        CheckKind::EstatePublicAccess.toggle(&mut state);
        assert!(CheckKind::EstatePublicAccess.checked(&state));
        assert!(state.estate_draft.contains(EstateFlags::EXTERNALLY_VISIBLE));
        // The pre-existing bit survives.
        assert!(state.estate_draft.contains(EstateFlags::DENY_ANONYMOUS));
    }

    /// `allow_land_resell` is the inverse of the `BLOCK_LAND_RESELL` flag, so a
    /// checked "Allow land resell" is reflected in the draft.
    #[test]
    fn resell_checkbox_reads_the_draft() {
        let state = AboutRegionState {
            draft: RegionInfoUpdate {
                allow_land_resell: true,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(CheckKind::AllowLandResell.checked(&state));
    }

    /// Each access list maps to the matching add / remove deltas, and only the
    /// groups list targets a group key.
    #[test]
    fn access_list_deltas_and_targets() {
        assert_eq!(
            AccessList::Managers.add_delta(),
            EstateAccessDelta::ManagerAdd
        );
        assert_eq!(
            AccessList::Managers.remove_delta(),
            EstateAccessDelta::ManagerRemove
        );
        assert_eq!(
            AccessList::Allowed.add_delta(),
            EstateAccessDelta::AllowedAgentAdd
        );
        assert_eq!(
            AccessList::AllowedGroups.add_delta(),
            EstateAccessDelta::AllowedGroupAdd
        );
        assert_eq!(
            AccessList::Banned.add_delta(),
            EstateAccessDelta::BannedAgentAdd
        );

        assert!(AccessList::AllowedGroups.is_group());
        assert!(!AccessList::Allowed.is_group());

        let id = Uuid::from_u128(0x1234);
        assert!(matches!(
            AccessList::AllowedGroups.target(id),
            OwnerKey::Group(_)
        ));
        assert!(matches!(AccessList::Banned.target(id), OwnerKey::Agent(_)));
    }

    /// The region form seeds its flag checkboxes from the **`RegionInfo`**, not
    /// from the handshake.
    ///
    /// `SlRegionIdentity` is written only by the `RegionHandshake` that arrives
    /// on entry, so its flags never move again for the life of the visit.
    /// `SlRegionLimits` is written by every `RegionInfo`, including the one a
    /// simulator pushes to the whole region when another estate manager saves.
    /// Reading the frozen copy meant **Apply** — which sends the whole form —
    /// re-asserted flags as they were on arrival and reverted that manager
    /// ([[viewer-floaters-never-reread-after-a-push]]).
    #[test]
    fn the_region_draft_prefers_the_region_info_flags() {
        let handshake = 0b0001;
        let pushed = 0b1010;
        assert_eq!(
            freshest_region_flags(handshake, Some(pushed)),
            pushed,
            "a RegionInfo's flags are newer than the handshake's and must win"
        );
        // A push that clears every flag is still a push, not an absent one.
        assert_eq!(freshest_region_flags(handshake, Some(0)), 0);
        // Before any RegionInfo the handshake is all there is.
        assert_eq!(freshest_region_flags(handshake, None), handshake);
    }

    /// **One window per region** (`viewer-keyed-floater-audit`), and the freeze
    /// a window keeps once the agent has left its region.
    mod instances {
        use super::super::{AboutRegionPlugin, AboutRegionState, OpenAboutRegion, region_key};
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::ui::UiRoot;
        use crate::world_api::{AvatarState, GroupsModel};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            GridCoordinates, Maturity, ProductType, RegionHandle, RegionIdentity, RegionName,
            RegionTerrainComposition, SlCommand, SlCurrentRegion, SlEvent, SlRegionIdentity, Uuid,
        };

        /// A boxed error so tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// A region record the agent may manage, named and identified by `id`.
        fn region(id: u128, name: &str) -> RegionIdentity {
            RegionIdentity {
                sim_name: RegionName::try_new(name).ok(),
                region_id: Uuid::from_u128(id),
                region_handle: RegionHandle::new(0),
                grid_coordinates: GridCoordinates::new(1000, 1000),
                region_flags: 0,
                region_flags_extended: 0,
                region_protocols: 0,
                maturity: Maturity::Pg,
                product: ProductType::Unknown,
                product_sku: String::new(),
                product_name: String::new(),
                cpu_class_id: 0,
                cpu_ratio: 0,
                sim_owner: Uuid::nil(),
                is_estate_manager: true,
                water_height: 20.0,
                billable_factor: 1.0,
                terrain: RegionTerrainComposition {
                    detail_textures: [Uuid::nil(); 4],
                    start_heights: [0.0; 4],
                    height_ranges: [0.0; 4],
                },
            }
        }

        /// An app with the floater manager, this module's plugin, and the world
        /// facts its systems read — no grid, no window.
        fn region_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<crate::ui_combo::ComboChanged>()
                .add_message::<crate::world_api::OpenTexturePicker>()
                .add_message::<crate::world_api::TexturePicked>()
                .add_message::<crate::world_api::OpenAvatarPicker>()
                .add_message::<crate::world_api::AvatarPicked>()
                .init_resource::<AvatarState>()
                .init_resource::<GroupsModel>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<bevy::input_focus::InputFocus>()
                .add_plugins((FloaterPlugin, AboutRegionPlugin));
            crate::i18n::install_untranslated(&mut app);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Make `identity` the region the agent is in, replacing whichever was.
        fn stand_in(app: &mut App, identity: &RegionIdentity) {
            let live: Vec<Entity> = app
                .world_mut()
                .query_filtered::<Entity, With<SlCurrentRegion>>()
                .iter(app.world())
                .collect();
            for entity in live {
                app.world_mut().entity_mut(entity).despawn();
            }
            app.world_mut()
                .spawn((SlCurrentRegion, SlRegionIdentity(identity.clone())));
            app.update();
        }

        /// Ask for Region / Estate, the way the World menu does.
        fn open(app: &mut App) {
            app.world_mut().write_message(OpenAboutRegion);
            app.update();
        }

        /// Every live Region / Estate window, with the region it is about.
        fn windows(app: &mut App) -> Vec<(Entity, Uuid, bool)> {
            app.world_mut()
                .query::<(Entity, &AboutRegionState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.region, state.is_current))
                .collect()
        }

        /// Two regions are two windows, each keyed by its own region.
        #[test]
        fn two_regions_open_two_windows() -> Result<(), TestError> {
            let (first, second) = (region(0xA1, "Alpha"), region(0xB2, "Beta"));
            let mut app = region_app();
            stand_in(&mut app, &first);
            open(&mut app);
            stand_in(&mut app, &second);
            open(&mut app);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second region reused the first window"
            );
            let world = app.world();
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _region, _current)| {
                    world.get::<Floater>(*window).and_then(Floater::key)
                })
                .collect();
            assert!(keys.contains(&Some(&region_key(&first))));
            assert!(keys.contains(&Some(&region_key(&second))));
            Ok(())
        }

        /// Re-opening in the same region raises its window instead of making a
        /// second.
        #[test]
        fn reopening_in_one_region_reuses_its_window() -> Result<(), TestError> {
            let here = region(0xA1, "Alpha");
            let mut app = region_app();
            stand_in(&mut app, &here);
            open(&mut app);
            open(&mut app);
            assert_eq!(windows(&mut app).len(), 1);
            Ok(())
        }

        /// **A window the agent has walked out of is frozen and read-only.**
        /// Every reply it reads is about the current region and every write goes
        /// out on the current circuit, so it keeps its snapshot and offers
        /// nothing to press. Walking back wakes it.
        #[test]
        fn leaving_a_region_freezes_its_window() -> Result<(), TestError> {
            let (first, second) = (region(0xA1, "Alpha"), region(0xB2, "Beta"));
            let mut app = region_app();
            stand_in(&mut app, &first);
            open(&mut app);
            let window = windows(&mut app)
                .into_iter()
                .find_map(|(window, id, _current)| (id == first.region_id).then_some(window))
                .ok_or("the first region has no window")?;
            assert!(
                app.world()
                    .get::<AboutRegionState>(window)
                    .is_some_and(|state| state.can_manage),
                "an estate manager standing in the region cannot manage it"
            );

            stand_in(&mut app, &second);
            let left = app
                .world()
                .get::<AboutRegionState>(window)
                .ok_or("the window vanished")?;
            assert!(!left.is_current, "the window followed the agent out");
            assert!(!left.can_manage, "a left-behind window still offers writes");
            assert!(
                left.identity
                    .as_ref()
                    .is_some_and(|identity| identity.region_id == first.region_id),
                "the window lost the region it was about"
            );

            stand_in(&mut app, &first);
            assert!(
                app.world()
                    .get::<AboutRegionState>(window)
                    .is_some_and(|state| state.is_current && state.can_manage),
                "coming back did not wake the window"
            );
            Ok(())
        }

        /// Closing one region's window leaves the other open.
        #[test]
        fn closing_one_region_leaves_the_other() -> Result<(), TestError> {
            let (first, second) = (region(0xA1, "Alpha"), region(0xB2, "Beta"));
            let mut app = region_app();
            stand_in(&mut app, &first);
            open(&mut app);
            stand_in(&mut app, &second);
            open(&mut app);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, id, _current)| (id == first.region_id).then_some(window))
                .ok_or("the first region has no window")?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: target,
                    op: FloaterOp::Close,
                });
            app.update();

            let live = windows(&mut app);
            assert_eq!(live.len(), 1);
            assert_eq!(
                live.first().map(|(_window, id, _current)| *id),
                Some(second.region_id)
            );
            Ok(())
        }
    }
}
