//! The **Region / Estate** ("About Region") floater
//! (`viewer-region-options-debug` / `-general` / `-terrain` / `-estate`): the
//! region-and-estate information surface. It presents the reference viewer's
//! `llfloaterregioninfo` as tabs — **Region**, **Debug**, **Terrain**,
//! **Estate**, **Covenant**, **Access**, plus placeholder **Environment** and
//! **Experiences** tabs (their write paths — `ExtEnvironment` PUT and the
//! experience service — are their own roadmap items).
//!
//! # Bound to the current region
//!
//! The floater always describes the region the agent is standing in. Like the
//! avatar / group profiles and About Land it is exempt from floater persistence
//! ([`crate::floater_persist::FloaterPersistExempt`]): its "subject" is wherever
//! the agent is, so a restored rectangle / open state would be meaningless.
//!
//! # Build once, update in place (no despawn)
//!
//! Every tab's structure is spawned **once** at start-up and never torn down.
//! Replies update values *in place*: value labels via [`set_value_node`],
//! checkbox glyphs via [`set_check_visual`], the maturity combo by writing its
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
    ProductType, RegionDebugUpdate, RegionFlags, RegionInfoUpdate, RegionTerrainUpdate, SlCommand,
    SlCurrentRegion, SlEvent, SlRegionIdentity, SlRegionLimits, SlSessionEvent, TextureKey, Uuid,
};

use crate::avatar_picker::{AvatarPicked, OpenAvatarPicker};
use crate::avatars::AvatarState;
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::groups::GroupsModel;
use crate::i18n::{Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_name_link::{NameLink, NameLinkSpec, NameTarget, set_name_link, spawn_name_link};
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabContainerHandle, TabPlacement, TabSpec, fill_tab_container,
    spawn_tab_container,
};
use crate::ui_table::{
    TableAlign, TableColumn, TableColumnKind, TableColumnWidth, TableSpec, set_table_cell,
    spawn_table, spawn_table_row,
};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::ui_texture_picker::{TexturePicked, TextureSwatchValue, spawn_texture_swatch};
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};

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
pub(crate) struct OpenAboutRegion;

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The floater's data model.
#[derive(Resource, Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a distinct floater flag (requested / seeded / manage rights)"
)]
struct AboutRegionState {
    /// Whether the floater has been opened at least once (gates event ingest so
    /// stray estate replies are not folded before the first open).
    requested: bool,
    /// Whether the agent may manage the estate (owner or manager); gates editing.
    can_manage: bool,
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

/// The per-tab dirty flags: a value refresh runs only when its data changed.
#[derive(Resource, Debug, Default)]
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
#[derive(Resource, Debug, Default)]
struct ManagersView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The allowed-residents list view model.
#[derive(Resource, Debug, Default)]
struct AllowedView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The allowed-groups list view model.
#[derive(Resource, Debug, Default)]
struct AllowedGroupsView {
    /// The resolved rows.
    rows: Vec<AccessRowData>,
    /// The revision the rows were built from.
    built: u64,
}

/// The banned-residents list view model.
#[derive(Resource, Debug, Default)]
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

/// The floater's live entity handles.
#[derive(Resource, Debug)]
struct AboutRegionUi {
    /// The floater root (carries `UiPanelShown`).
    panel: Entity,
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
pub(crate) struct AboutRegionPlugin;

impl Plugin for AboutRegionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AboutRegionState>()
            .init_resource::<AboutRegionDirty>()
            .init_resource::<ManagersView>()
            .init_resource::<AllowedView>()
            .init_resource::<AllowedGroupsView>()
            .init_resource::<BannedView>()
            .add_message::<OpenAboutRegion>()
            .add_systems(
                Startup,
                spawn_about_region_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_about_region,
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
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_access_rows, bind_access_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// Spawn the (hidden) Region / Estate floater and build every tab once.
fn spawn_about_region_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "about-region",
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
        },
    );
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("about-region-title"));
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
        &mut commands,
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
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
    let panel = |index: usize| tabs.panels.get(index).copied().unwrap_or(handle.content);

    let region = build_region_tab(&mut commands, panel(0));
    let debug = build_debug_tab(&mut commands, panel(1));
    let terrain = build_terrain_tab(&mut commands, panel(2));
    let estate = build_estate_tab(&mut commands, panel(3));
    let covenant = build_covenant_tab(&mut commands, panel(4));
    let access = build_access_tab(&mut commands, panel(5));
    build_placeholder_tab(&mut commands, panel(6), "about-region-env-unimplemented");
    build_placeholder_tab(
        &mut commands,
        panel(7),
        "about-region-experiences-unimplemented",
    );

    commands.insert_resource(AboutRegionUi {
        panel: handle.root,
        region,
        debug,
        terrain,
        estate,
        covenant,
        access,
    });
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

/// Open the floater and request fresh region / estate data.
fn open_about_region(
    mut requests: MessageReader<OpenAboutRegion>,
    mut state: ResMut<AboutRegionState>,
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: MessageWriter<SlCommand>,
) {
    if requests.read().last().is_none() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    state.requested = true;
    // Force a reseed of the drafts from the (refreshed) region / estate data.
    state.draft_seeded = false;
    state.estate_seeded = false;
    state.clear_access();
    commands.write(SlCommand(Command::RequestRegionInfo));
    commands.write(SlCommand(Command::RequestEstateInfo));
    commands.write(SlCommand(Command::RequestEstateCovenant));
    dirty.mark_all();
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold estate info / covenant / access-list / covenant-asset replies into state.
fn ingest_about_region_events(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AboutRegionState>,
    mut dirty: ResMut<AboutRegionDirty>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !state.requested {
        events.clear();
        return;
    }
    for event in events.read() {
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
            SlSessionEvent::AssetReceived(asset) if state.covenant_pending == Some(asset.id) => {
                state.covenant_pending = None;
                state.covenant_text = Some(decode_covenant(asset));
                dirty.covenant_values = true;
            }
            _other => {}
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

/// Reseed the draft and mark the display tabs dirty when the live region data
/// changes (a `RegionHandshake`, a `RegionInfo` reply, or a teleport), or when a
/// fresh open cleared [`AboutRegionState::draft_seeded`].
#[expect(
    clippy::type_complexity,
    reason = "the region query needs the identity plus the optional limits with change detection"
)]
fn refresh_on_region(
    mut state: ResMut<AboutRegionState>,
    mut dirty: ResMut<AboutRegionDirty>,
    regions: Query<(Ref<SlRegionIdentity>, Option<Ref<SlRegionLimits>>), With<SlCurrentRegion>>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some((identity, limits)) = regions.iter().next() else {
        return;
    };
    let changed =
        identity.is_changed() || limits.as_ref().is_some_and(|limits| limits.is_changed());
    if !changed && state.draft_seeded {
        return;
    }
    state.can_manage = identity.0.is_estate_manager;
    state.draft = seed_draft(&identity, limits.as_deref());
    state.debug_draft = seed_debug_draft(&identity);
    state.terrain_draft = seed_terrain_draft(&identity, limits.as_deref());
    state.draft_seeded = true;
    if !identity.0.sim_owner.is_nil() {
        request_name(AgentKey::from(identity.0.sim_owner), &mut commands);
    }
    dirty.mark_all();
}

/// Build the region-settings draft from the live region identity and limits.
fn seed_draft(identity: &SlRegionIdentity, limits: Option<&SlRegionLimits>) -> RegionInfoUpdate {
    let flags = RegionFlags::from_bits(identity.0.region_flags);
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
        maturity: identity.0.maturity,
    }
}

/// Build the region-debug draft from the live region flags.
const fn seed_debug_draft(identity: &SlRegionIdentity) -> RegionDebugUpdate {
    let flags = RegionFlags::from_bits(identity.0.region_flags);
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
fn refresh_on_names(
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut dirty: ResMut<AboutRegionDirty>,
) {
    if avatars.is_changed() || groups.is_changed() {
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
fn seed_edit_fields(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    state: Res<AboutRegionState>,
    mut fields: Query<&mut EditableText>,
    mut combos: Query<&mut ComboSelection>,
    mut swatches: Query<&mut TextureSwatchValue>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.seed_fields {
        return;
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

/// Toggle write buttons' visibility and every editable control's
/// [`InteractionDisabled`] to follow the agent's estate rights, and repaint the
/// checkbox glyphs.
#[expect(
    clippy::too_many_arguments,
    reason = "reconciling control enable needs the write buttons, gated controls, disabled set, \
              checks, and text query together"
)]
fn update_control_enable(
    mut dirty: ResMut<AboutRegionDirty>,
    state: Res<AboutRegionState>,
    mut write_buttons: Query<&mut Visibility, With<WriteButton>>,
    gated: Query<Entity, With<EditGate>>,
    disabled: Query<(), With<InteractionDisabled>>,
    checks: Query<&AboutRegionCheck>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut commands: Commands,
) {
    if !dirty.controls {
        return;
    }
    dirty.controls = false;
    let can_manage = state.can_manage;
    let button_vis = if can_manage {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut write_buttons {
        if *visibility != button_vis {
            *visibility = button_vis;
        }
    }
    for entity in &gated {
        let is_disabled = disabled.contains(entity);
        if can_manage && is_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !can_manage && !is_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for check in &checks {
        let on = check.kind.checked(&state);
        set_check_visual(&mut texts, check, on, can_manage);
    }
}

// ---------------------------------------------------------------------------
// Value refreshes.
// ---------------------------------------------------------------------------

/// Refresh the Region tab's read-only identity values in place.
fn update_region_tab(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.region_values {
        return;
    }
    dirty.region_values = false;
    let region = regions.iter().next().map(|region| &region.0);
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

/// Refresh the Debug tab's read-only region name in place.
fn update_debug_tab(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.debug_values {
        return;
    }
    dirty.debug_values = false;
    let region = regions.iter().next().map(|region| &region.0);
    set_value_node(&mut texts, ui.debug.name, &region_name(region, &translator));
}

/// Refresh the Terrain tab's region name in place (its fields and swatches are
/// seeded from the terrain draft by [`seed_edit_fields`]).
fn update_terrain_tab(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    identities: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.terrain_values {
        return;
    }
    dirty.terrain_values = false;
    let identity = identities.iter().next().map(|region| &region.0);
    set_value_node(
        &mut texts,
        ui.terrain.name,
        &region_name(identity, &translator),
    );
}

/// Refresh the Estate tab's read-only values in place.
fn update_estate_tab(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    state: Res<AboutRegionState>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.estate_values {
        return;
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

/// Refresh the Covenant tab's read-only values in place.
fn update_covenant_tab(
    mut dirty: ResMut<AboutRegionDirty>,
    ui: Option<Res<AboutRegionUi>>,
    state: Res<AboutRegionState>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.covenant_values {
        return;
    }
    dirty.covenant_values = false;
    let handles = &ui.covenant;
    let region = regions.iter().next().map(|region| &region.0);
    if let Some(covenant) = &state.covenant {
        set_value_node(&mut texts, handles.estate, &covenant.estate_name);
        let owner =
            (!covenant.estate_owner_id.is_nil()).then(|| AgentKey::from(covenant.estate_owner_id));
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

// ---------------------------------------------------------------------------
// Access-list views + tables.
// ---------------------------------------------------------------------------

/// Rebuild the estate-managers view when the list or the name cache changes.
fn sync_managers_view(
    state: Res<AboutRegionState>,
    view: ResMut<ManagersView>,
    ui: Option<Res<AboutRegionUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        AccessList::Managers,
        state.managers_revision,
        &state.managers,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access.managers_viewport),
        &avatars,
        &groups,
        avatars.is_changed() || groups.is_changed(),
        lists,
    );
}

/// Rebuild the allowed-residents view.
fn sync_allowed_view(
    state: Res<AboutRegionState>,
    view: ResMut<AllowedView>,
    ui: Option<Res<AboutRegionUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        AccessList::Allowed,
        state.allowed_revision,
        &state.allowed,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access.allowed_viewport),
        &avatars,
        &groups,
        avatars.is_changed() || groups.is_changed(),
        lists,
    );
}

/// Rebuild the allowed-groups view.
fn sync_allowed_groups_view(
    state: Res<AboutRegionState>,
    view: ResMut<AllowedGroupsView>,
    ui: Option<Res<AboutRegionUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        AccessList::AllowedGroups,
        state.allowed_groups_revision,
        &state.allowed_groups,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access.allowed_groups_viewport),
        &avatars,
        &groups,
        avatars.is_changed() || groups.is_changed(),
        lists,
    );
}

/// Rebuild the banned-residents view.
fn sync_banned_view(
    state: Res<AboutRegionState>,
    view: ResMut<BannedView>,
    ui: Option<Res<AboutRegionUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        AccessList::Banned,
        state.banned_revision,
        &state.banned,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access.banned_viewport),
        &avatars,
        &groups,
        avatars.is_changed() || groups.is_changed(),
        lists,
    );
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
    mut lists: Query<&mut VirtualList>,
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
            name_of(AgentKey::from(*id), avatars)
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
fn populate_access_rows(
    mut commands: Commands,
    ui: Option<Res<AboutRegionUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        let parent = child_of.parent();
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
    }
}

/// Bind each pooled access row to its resolved name, and reveal the Remove
/// buttons only when the agent may manage the estate.
#[expect(
    clippy::too_many_arguments,
    reason = "binding the four pools needs every view, the state, the UI handles, and the row / \
              remove / visibility / text queries together"
)]
fn bind_access_rows(
    managers: Res<ManagersView>,
    allowed: Res<AllowedView>,
    allowed_groups: Res<AllowedGroupsView>,
    banned: Res<BannedView>,
    state: Res<AboutRegionState>,
    ui: Option<Res<AboutRegionUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    removes: Query<Entity, With<RemoveAccessButton>>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
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
        if let Ok(mut vis) = visibility.get_mut(entity)
            && *vis != want
        {
            *vis = want;
        }
    }
}

// ---------------------------------------------------------------------------
// Edit observers / handlers.
// ---------------------------------------------------------------------------

/// Toggle a checkbox, flipping its backing draft field.
fn on_about_region_check(
    press: On<Pointer<Press>>,
    checks: Query<&AboutRegionCheck>,
    mut state: ResMut<AboutRegionState>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(check) = checks.get(press.entity) else {
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
fn on_about_region_action(
    press: On<Pointer<Press>>,
    actions: Query<&AboutRegionAction>,
    mut state: ResMut<AboutRegionState>,
    ui: Res<AboutRegionUi>,
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
            read_terrain_fields(&mut state, &ui, &read);
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
            pickers.write(OpenAvatarPicker {
                requester: PICK_TELEPORT,
            });
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
            pickers.write(OpenAvatarPicker {
                requester: PICK_KICK,
            });
        }
        AboutRegionAction::AddManager => {
            pickers.write(OpenAvatarPicker {
                requester: PICK_MANAGER,
            });
        }
        AboutRegionAction::AddAllowed => {
            pickers.write(OpenAvatarPicker {
                requester: PICK_ALLOWED,
            });
        }
        AboutRegionAction::AddBanned => {
            pickers.write(OpenAvatarPicker {
                requester: PICK_BANNED,
            });
        }
    }
}

/// Resolve and act on a per-row access Remove press.
#[expect(
    clippy::too_many_arguments,
    reason = "resolving a remove needs the pressed button, its row, all four list views, the \
              state, and the command writer"
)]
fn on_remove_access(
    press: On<Pointer<Press>>,
    buttons: Query<&RemoveAccessButton>,
    rows: Query<&VirtualRow>,
    managers: Res<ManagersView>,
    allowed: Res<AllowedView>,
    allowed_groups: Res<AllowedGroupsView>,
    banned: Res<BannedView>,
    mut state: ResMut<AboutRegionState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || !state.can_manage {
        return;
    }
    let Ok(button) = buttons.get(press.entity) else {
        return;
    };
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
fn apply_combo_edits(
    mut changed: MessageReader<ComboChanged>,
    ui: Option<Res<AboutRegionUi>>,
    mut state: ResMut<AboutRegionState>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in changed.read() {
        if Some(event.combo) == ui.region.maturity_combo {
            state.draft.maturity = maturity_from_index(event.active);
        }
    }
}

/// Fold an avatar pick into the estate action that opened the picker.
fn apply_avatar_picks(
    mut picked: MessageReader<AvatarPicked>,
    mut state: ResMut<AboutRegionState>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in picked.read() {
        if !state.can_manage {
            continue;
        }
        let agent = event.agent;
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
            PICK_ALLOWED => add_access_entry(&mut state, AccessList::Allowed, agent, &mut commands),
            PICK_BANNED => add_access_entry(&mut state, AccessList::Banned, agent, &mut commands),
            _other => {}
        }
    }
}

/// Fold a terrain texture pick into the terrain draft slot and repaint its
/// swatch thumbnail (via [`TextureSwatchValue`]).
fn apply_texture_edits(
    mut picked: MessageReader<TexturePicked>,
    mut swatches: Query<(&TerrainSwatch, &mut TextureSwatchValue)>,
    mut state: ResMut<AboutRegionState>,
) {
    for event in picked.read() {
        if !event.final_pick || !state.can_manage {
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

/// The display name for an agent, falling back to its id in parentheses.
fn name_of(agent: AgentKey, avatars: &AvatarState) -> String {
    avatars
        .name_of(agent)
        .map_or_else(|| format!("({agent})"), str::to_owned)
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
    use super::{AboutRegionState, AccessList, CheckKind, maturity_from_index, maturity_index};
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
}
