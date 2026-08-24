//! The **About Land** floater (`viewer-parcel-options-general` +
//! `viewer-parcel-options-access-media`): the parcel information surface, all
//! nine reference tabs — **General**, **Covenant**, **Objects**, **Options**,
//! **Media**, **Sound**, **Access**, **Experiences**, **Environment**.
//!
//! # Subject-bound
//!
//! The floater opens on a **particular parcel** ([`OpenAboutLand`]) — the parcel
//! the top-bar location read-out was clicked on (the agent's current parcel), or
//! the parcel a land-pie right-click landed on. Like the avatar and group
//! profiles it is exempt from floater persistence
//! ([`crate::floater_persist::FloaterPersistExempt`]).
//!
//! # Build once, update in place (no despawn)
//!
//! Every tab's structure is spawned **once** at start-up and never torn down.
//! Replies update values *in place*: value labels via [`set_value_node`],
//! checkbox glyphs via [`set_check_visual`], combos by writing their
//! [`ComboSelection`](crate::ui_combo), edit fields by seeding
//! `EditableText::editor_mut().set_text` on a fresh subject, and the three
//! variable lists (object owners, allow, ban) through the **table widget**
//! ([`crate::ui_table`]) — a bounded, scrolling viewport that pools and binds its
//! rows, never despawning them. This is the group-profile floater's discipline:
//! churn is the root cause of the profile despawn panics (`never-hide-errors`).
//!
//! # Editing and disabled controls
//!
//! Editable controls mutate a single [`ParcelUpdate`] draft, seeded at open; the
//! **Apply** button commits it with [`Command::UpdateParcel`], and the access
//! **Add** / **Remove** buttons rewrite a list with
//! [`Command::UpdateParcelAccessList`]. When the agent does not own the parcel
//! (or the floater is the read-only "place profile" view) every editable control
//! carries [`bevy::ui::InteractionDisabled`] — the widgets grey out and refuse
//! input — and the write buttons hide. Controls with no protocol write path
//! (per-parcel media type / size / loop, the avatar-sound toggles, per-parcel
//! experiences, environment editing) are shown as **permanently disabled**
//! controls reflecting the grid's value, not as prose notes.
//!
//! Reference (Firestorm, read-only): `llfloaterland`, `llpanelland*`; the
//! `ParcelPropertiesUpdate`, `ParcelAccessListUpdate` messages.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use sl_client_bevy::{
    AgentKey, Asset, AssetKey, AssetType, Command, EnvironmentSettings, EstateCovenant,
    LindenAmount, Maturity, OwnerKey, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope,
    ParcelCategory, ParcelFlags, ParcelInfo, ParcelMediaUpdateInfo, ParcelObjectOwner,
    ParcelUpdate, ProductType, RegionCoordinates, RegionFlags, RegionLocalParcelId, ScopedParcelId,
    SlAgentParcel, SlCommand, SlCurrentRegion, SlEvent, SlIdentity, SlParcel, SlRegionIdentity,
    SlSessionEvent, TextureKey, Uuid,
};

use crate::avatars::AvatarState;
use crate::environment::EnvironmentState;
use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_panel,
    spawn_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::status_bar::AgentRegionPosition;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
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
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::GroupsModel;
use crate::world_api::{AvatarPicked, OpenAvatarPicker};
use crate::world_api::{OpenTexturePicker, PickerKind, TexturePicked};

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

/// The bounded height of each list (object owners, allow, ban), in logical
/// pixels — the widget scrolls beyond it rather than growing the tab.
const LIST_HEIGHT: f32 = 150.0;

/// One list row's height, in logical pixels.
const ROW_HEIGHT: f32 = 22.0;

/// The object-owners table: type, name, object count.
const OWNERS_TABLE: TableSpec = TableSpec {
    element: "about-land-owners",
    selection: TableSelectionMode::None,
    columns: &[
        TableColumn {
            header_key: "about-land-owners-type",
            token: "type",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 80.0 },
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "about-land-owners-name",
            token: "name",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Flex(1.0),
            align: TableAlign::Start,
            sortable: false,
        },
        TableColumn {
            header_key: "about-land-owners-count",
            token: "count",
            kind: TableColumnKind::Text,
            width: TableColumnWidth::Fixed { default: 60.0 },
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
};

/// The allow-list table.
const ALLOW_TABLE: TableSpec = access_table("about-land-allow");

/// The ban-list table (same shape).
const BAN_TABLE: TableSpec = access_table("about-land-ban");

/// The shared column layout of the allow / ban tables, parameterised by element.
const fn access_table(element: &'static str) -> TableSpec {
    TableSpec {
        element,
        selection: TableSelectionMode::None,
        columns: &[
            TableColumn {
                header_key: "about-land-access-name",
                token: "name",
                kind: TableColumnKind::Text,
                width: TableColumnWidth::Flex(1.0),
                align: TableAlign::Start,
                sortable: false,
            },
            TableColumn {
                header_key: "about-land-access-expiry",
                token: "expiry",
                kind: TableColumnKind::Text,
                width: TableColumnWidth::Fixed { default: 90.0 },
                align: TableAlign::Start,
                sortable: false,
            },
            TableColumn {
                header_key: "about-land-access-remove",
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

// ---------------------------------------------------------------------------
// Open request.
// ---------------------------------------------------------------------------

/// A request to open the About Land floater.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenAboutLand {
    /// Which parcel to describe.
    pub(crate) subject: AboutLandSubject,
    /// Open without edit affordances (the read-only "About this location" view).
    pub(crate) read_only: bool,
}

/// How the About Land floater's subject parcel is identified.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AboutLandSubject {
    /// A known region-local parcel id (the agent's current parcel — the top-bar
    /// read-out and the World menu). Its data is already local.
    CurrentParcel(RegionLocalParcelId),
    /// A region-local ground point (a land-pie right-click). The parcel is
    /// resolved by asking the simulator for the parcel at that point
    /// (`ParcelPropertiesRequest`), so a click on **any** parcel — not just one
    /// already fetched — opens on that parcel, not the agent's own.
    AtPoint {
        /// The region-local east metre.
        x: f32,
        /// The region-local north metre.
        y: f32,
    },
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The floater's data model.
#[derive(Resource, Debug, Default)]
struct AboutLandState {
    /// The parcel currently bound, or `None` before the first open.
    target: Option<RegionLocalParcelId>,
    /// Whether the floater is in read-only "place profile" mode.
    read_only: bool,
    /// Whether the agent may edit the bound parcel. Fixed at open.
    can_edit: bool,
    /// The bound parcel's properties, or `None` until they resolve.
    parcel: Option<ParcelInfo>,
    /// The parcel's dwell (traffic), or `None` until the reply arrives.
    dwell: Option<f32>,
    /// The parcel's media settings (type / desc / size / loop), or `None` until a
    /// `ParcelMediaUpdate` arrives.
    media: Option<ParcelMediaUpdateInfo>,
    /// The estate covenant summary.
    covenant: Option<EstateCovenant>,
    /// The decoded covenant notecard text.
    covenant_text: Option<String>,
    /// The covenant notecard asset id awaited.
    covenant_pending: Option<Uuid>,
    /// The per-owner object tallies.
    owners: Vec<ParcelObjectOwner>,
    /// The parcel's allow list.
    access_allow: Vec<ParcelAccessEntry>,
    /// The parcel's ban list.
    access_ban: Vec<ParcelAccessEntry>,
    /// The pending edit draft, seeded at open; **Apply** commits it.
    draft: ParcelUpdate,
    /// Whether [`draft`](Self::draft) is seeded for the current subject.
    draft_ready: bool,
    /// Bumped when the object-owner tally changes, to rebuild its table view.
    owners_revision: u64,
    /// Bumped when the allow list changes.
    allow_revision: u64,
    /// Bumped when the ban list changes.
    ban_revision: u64,
    /// The `ParcelPropertiesRequest` sequence id awaited when opening on a point
    /// (a land-pie click): the reply with this echoed `sequence_id` binds the
    /// subject. `None` once bound, or when opened on a known parcel.
    pending_sequence: Option<i32>,
    /// A monotonic source of request sequence ids.
    sequence_counter: i32,
}

impl AboutLandState {
    /// Clear the model for a fresh open (subject not yet bound).
    fn reset(&mut self, read_only: bool) {
        self.target = None;
        self.read_only = read_only;
        self.can_edit = false;
        self.parcel = None;
        self.dwell = None;
        self.media = None;
        self.covenant = None;
        self.covenant_text = None;
        self.covenant_pending = None;
        self.owners = Vec::new();
        self.access_allow = Vec::new();
        self.access_ban = Vec::new();
        self.draft = ParcelUpdate::default();
        self.draft_ready = false;
        self.pending_sequence = None;
        self.owners_revision = self.owners_revision.wrapping_add(1);
        self.allow_revision = self.allow_revision.wrapping_add(1);
        self.ban_revision = self.ban_revision.wrapping_add(1);
    }

    /// Bind the resolved parcel as the subject (from a known id, or a point
    /// reply), computing the edit rights.
    fn bind(&mut self, parcel: ParcelInfo, identity: &SlIdentity) {
        self.can_edit = !self.read_only
            && identity.agent_id.is_some_and(|agent| match parcel.owner {
                OwnerKey::Agent(owner) => owner == agent,
                OwnerKey::Group(_group) => false,
            });
        self.target = Some(parcel.local_id);
        self.pending_sequence = None;
        self.parcel = Some(parcel);
        self.seed_draft();
    }

    /// The next request sequence id.
    const fn next_sequence(&mut self) -> i32 {
        self.sequence_counter = self.sequence_counter.wrapping_add(1);
        self.sequence_counter
    }

    /// Seed the edit [`draft`](Self::draft) from the parcel, once per open.
    fn seed_draft(&mut self) {
        if self.draft_ready {
            return;
        }
        if let Some(parcel) = &self.parcel {
            self.draft =
                parcel_update_from(parcel, parcel.name.clone(), parcel.description.clone());
            self.draft_ready = true;
        }
    }

    /// The scoped id for the bound parcel, given the current circuit.
    fn scoped(&self, identity: &SlIdentity) -> Option<ScopedParcelId> {
        Some(ScopedParcelId::new(identity.circuit_id?, self.target?))
    }
}

/// Which sub-panels need an in-place value refresh this frame.
#[derive(Resource, Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent dirty flag per in-place refresh pass"
)]
struct AboutLandDirty {
    /// Seed the edit fields' text from the draft (on a fresh subject).
    seed_fields: bool,
    /// The write controls' enable state / visibility (a rights change).
    controls: bool,
    /// The General tab's read-only values.
    general_values: bool,
    /// The Options / Media / Sound editable + read-only controls.
    editable_values: bool,
    /// The Covenant tab's values.
    covenant_values: bool,
    /// The Objects tab's counts.
    objects_values: bool,
    /// The Access tab's checkboxes / pass fields.
    access_values: bool,
    /// The Environment tab's read-only summary.
    environment_values: bool,
}

impl AboutLandDirty {
    /// Mark every sub-panel dirty (on a fresh open).
    const fn mark_all(&mut self) {
        self.seed_fields = true;
        self.controls = true;
        self.general_values = true;
        self.editable_values = true;
        self.covenant_values = true;
        self.objects_values = true;
        self.access_values = true;
        self.environment_values = true;
    }
}

// ---------------------------------------------------------------------------
// Table view models.
// ---------------------------------------------------------------------------

/// A resolved object-owner row.
#[derive(Debug, Default, Clone)]
struct OwnerRowData {
    /// The owner-kind label (Resident / Group).
    kind: String,
    /// The resolved owner name.
    name: String,
    /// The object count.
    count: String,
}

/// The object-owners table view (rebuilt when the tally or names change).
#[derive(Resource, Debug, Default)]
struct OwnersView {
    /// The rows in display order.
    rows: Vec<OwnerRowData>,
    /// The [`AboutLandState::owners_revision`] this view was built from.
    built: u64,
}

/// A resolved access-list row.
#[derive(Debug, Default, Clone)]
struct AccessRowData {
    /// The resident agent id (for removal).
    id: Uuid,
    /// The resolved resident name.
    name: String,
    /// The pass-expiry display.
    expiry: String,
}

/// The allow-list table view.
#[derive(Resource, Debug, Default)]
struct AllowView {
    /// The rows in display order.
    rows: Vec<AccessRowData>,
    /// The [`AboutLandState::allow_revision`] this view was built from.
    built: u64,
}

/// The ban-list table view.
#[derive(Resource, Debug, Default)]
struct BanView {
    /// The rows in display order.
    rows: Vec<AccessRowData>,
    /// The [`AboutLandState::ban_revision`] this view was built from.
    built: u64,
}

// ---------------------------------------------------------------------------
// Handles.
// ---------------------------------------------------------------------------

/// The retained handles of the General tab.
#[derive(Debug, Default)]
struct GeneralHandles {
    /// The parcel-name edit field.
    name_field: Option<Entity>,
    /// The description edit field.
    desc_field: Option<Entity>,
    /// The parcel-id value node.
    parcel_id: Option<Entity>,
    /// The land-type value node.
    land_type: Option<Entity>,
    /// The content-rating value node.
    rating: Option<Entity>,
    /// The owner-name value node (inside a clickable link button).
    owner: Option<Entity>,
    /// The group-name value node (inside a clickable link button).
    group: Option<Entity>,
    /// The area value node.
    area: Option<Entity>,
    /// The claim-date value node.
    claimed: Option<Entity>,
    /// The dwell / traffic value node.
    traffic: Option<Entity>,
    /// The sale-state value node.
    for_sale: Option<Entity>,
}

/// The retained handles of the Covenant tab.
#[derive(Debug, Default)]
struct CovenantHandles {
    /// The estate-name value node.
    estate: Option<Entity>,
    /// The estate-owner value node.
    estate_owner: Option<Entity>,
    /// The covenant-text value node.
    text: Option<Entity>,
    /// The covenant last-modified value node.
    timestamp: Option<Entity>,
    /// The region-name value node.
    region: Option<Entity>,
    /// The region-type value node.
    region_type: Option<Entity>,
    /// The region content-rating value node.
    region_rating: Option<Entity>,
    /// The resale-clause value node.
    resale: Option<Entity>,
    /// The subdivide-clause value node.
    subdivide: Option<Entity>,
}

/// The retained handles of the Objects tab.
#[derive(Debug, Default)]
struct ObjectHandles {
    /// The region-capacity value node.
    region_capacity: Option<Entity>,
    /// The parcel-capacity value node.
    parcel_capacity: Option<Entity>,
    /// The parcel land-impact value node.
    parcel_impact: Option<Entity>,
    /// The owner-object-count value node.
    owner_objects: Option<Entity>,
    /// The group-object-count value node.
    group_objects: Option<Entity>,
    /// The other-object-count value node.
    other_objects: Option<Entity>,
    /// The selected-object-count value node.
    selected_objects: Option<Entity>,
    /// The auto-return-time value node.
    autoreturn: Option<Entity>,
    /// The object-owners table's virtual-list viewport.
    owners_viewport: Option<Entity>,
    /// The object-owners table root.
    owners_table: Option<Entity>,
}

/// The retained handles of the Options tab.
#[derive(Debug, Default)]
struct OptionsHandles {
    /// The search-category combo anchor.
    category_combo: Option<Entity>,
    /// The teleport-routing combo anchor.
    landing_combo: Option<Entity>,
    /// The snapshot-texture id value node.
    snapshot_value: Option<Entity>,
    /// The landing-point coordinate value node.
    landing_point: Option<Entity>,
}

/// The retained handles of the Media tab.
#[derive(Debug, Default)]
struct MediaHandles {
    /// The media-URL edit field.
    url_field: Option<Entity>,
    /// The replace-texture id value node.
    texture_value: Option<Entity>,
    /// The read-only media-type value node.
    media_type: Option<Entity>,
    /// The read-only media-size value node.
    media_size: Option<Entity>,
}

/// The retained handles of the Sound tab.
#[derive(Debug, Default)]
struct SoundHandles {
    /// The music-URL edit field.
    music_field: Option<Entity>,
}

/// The retained handles of the Access tab.
#[derive(Debug, Default)]
struct AccessHandles {
    /// The pass-price edit field.
    pass_price_field: Option<Entity>,
    /// The pass-hours edit field.
    pass_hours_field: Option<Entity>,
    /// The allow-list table's virtual-list viewport.
    allow_viewport: Option<Entity>,
    /// The allow-list table root.
    allow_table: Option<Entity>,
    /// The ban-list table's virtual-list viewport.
    ban_viewport: Option<Entity>,
    /// The ban-list table root.
    ban_table: Option<Entity>,
}

/// The retained value nodes of the Environment tab.
#[derive(Debug, Default)]
struct EnvironmentHandles {
    /// The "parcel overrides allowed" value node.
    override_allowed: Option<Entity>,
    /// The parcel environment-version value node.
    version: Option<Entity>,
    /// The active day-cycle summary value node.
    day_cycle: Option<Entity>,
}

/// What an editable / read-only checkbox reflects.
#[derive(Debug, Clone, Copy)]
enum CheckKind {
    /// Editable: checked ⇒ the flag bit is set in the draft.
    Flag(ParcelFlags),
    /// Editable, inverted: checked ⇒ the flag bit is clear.
    FlagInverted(ParcelFlags),
    /// Editable: the draft's `media_auto_scale` bool.
    MediaAutoScale,
    /// Read-only: whether anyone's avatar sounds play (`any_av_sounds`).
    AnyAvSounds,
    /// Read-only: whether group avatar sounds play (`group_av_sounds`).
    GroupAvSounds,
    /// Read-only: whether the parcel media loops.
    MediaLoop,
}

impl CheckKind {
    /// Whether this control reads as checked for the current `state`.
    fn checked(self, state: &AboutLandState) -> bool {
        let draft = &state.draft;
        match self {
            Self::Flag(flag) => draft.parcel_flags.contains(flag),
            Self::FlagInverted(flag) => !draft.parcel_flags.contains(flag),
            Self::MediaAutoScale => draft.media_auto_scale,
            Self::AnyAvSounds => state
                .parcel
                .as_ref()
                .is_some_and(|p| p.any_av_sounds.unwrap_or(true)),
            Self::GroupAvSounds => state
                .parcel
                .as_ref()
                .is_some_and(|p| p.group_av_sounds.unwrap_or(true)),
            Self::MediaLoop => state.media.as_ref().is_some_and(|m| m.media_loop),
        }
    }

    /// Toggle this control's backing draft value (a no-op for read-only kinds).
    const fn toggle(self, draft: &mut ParcelUpdate) {
        match self {
            Self::Flag(flag) | Self::FlagInverted(flag) => {
                draft.parcel_flags =
                    ParcelFlags::from_bits(draft.parcel_flags.bits() ^ flag.bits());
            }
            Self::MediaAutoScale => draft.media_auto_scale = !draft.media_auto_scale,
            Self::AnyAvSounds | Self::GroupAvSounds | Self::MediaLoop => {}
        }
    }

    /// Whether the control is editable (has a protocol write path).
    const fn editable(self) -> bool {
        matches!(
            self,
            Self::Flag(_) | Self::FlagInverted(_) | Self::MediaAutoScale
        )
    }
}

/// A checkbox on an editable / read-only tab.
#[derive(Component, Debug, Clone, Copy)]
struct AboutLandCheck {
    /// What the checkbox reflects.
    kind: CheckKind,
    /// The check-glyph text node.
    glyph: Entity,
    /// The label text node (greyed with the glyph when disabled).
    label: Entity,
}

/// A control whose interactivity follows the agent's edit rights: `Owner` is
/// enabled only when the agent owns the parcel; `Never` is always disabled (a
/// read-only reflection of grid data).
#[derive(Component, Debug, Clone, Copy)]
enum EditGate {
    /// Enabled only when the agent owns the parcel and may edit.
    Owner,
    /// Always disabled (no protocol write path).
    Never,
}

/// A marker on every write button (Apply / Add / Set), so their visibility
/// follows the agent's edit rights in one pass.
#[derive(Component, Debug, Clone, Copy)]
struct WriteButton;

/// A per-row access Remove button: which list it targets and the pooled table
/// row it sits in (so a press resolves the current entry via the table view).
#[derive(Component, Debug, Clone, Copy)]
struct RemoveAccessButton {
    /// Which list the row belongs to.
    scope: AccessScope,
    /// The pooled [`VirtualRow`] this button sits in.
    row: Entity,
}

/// Which access list a control targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessScope {
    /// The allow list.
    Allow,
    /// The ban list.
    Ban,
}

impl AccessScope {
    /// The wire scope.
    const fn wire(self) -> ParcelAccessScope {
        match self {
            Self::Allow => ParcelAccessScope::Access,
            Self::Ban => ParcelAccessScope::Ban,
        }
    }
}

/// The floater's live entity handles.
#[derive(Resource, Debug)]
#[expect(
    clippy::struct_field_names,
    reason = "one per-tab handle group per field; the shared postfix is the point"
)]
struct AboutLandUi {
    /// The General tab's handles.
    general_handles: GeneralHandles,
    /// The Covenant tab's handles.
    covenant_handles: CovenantHandles,
    /// The Objects tab's handles.
    object_handles: ObjectHandles,
    /// The Options tab's handles.
    options_handles: OptionsHandles,
    /// The Media tab's handles.
    media_handles: MediaHandles,
    /// The Sound tab's handles.
    sound_handles: SoundHandles,
    /// The Access tab's handles.
    access_handles: AccessHandles,
    /// The Environment tab's handles.
    environment_handles: EnvironmentHandles,
}

/// A press-dispatch tag on the floater's buttons.
#[derive(Component, Debug, Clone, Copy)]
enum AboutLandAction {
    /// Commit the edit draft via [`Command::UpdateParcel`].
    Apply,
    /// Re-request the parcel's object-owner tallies.
    RefreshOwners,
    /// Open the texture picker for the snapshot texture.
    PickSnapshot,
    /// Open the texture picker for the media replace-texture.
    PickMediaTexture,
    /// Set the landing point to the agent's current position.
    SetLandingPoint,
    /// Clear the landing point.
    ClearLandingPoint,
    /// Open the avatar picker to add to the allow list.
    AddAllowed,
    /// Open the avatar picker to add to the ban list.
    AddBanned,
}

/// A marker carrying which pick a texture swatch button opens.
#[derive(Component, Debug, Clone, Copy)]
struct SwatchTexture {
    /// The pick action this swatch triggers.
    action: AboutLandAction,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the About Land floater into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AboutLandPlugin;

impl Plugin for AboutLandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AboutLandState>()
            .init_resource::<AboutLandDirty>()
            .init_resource::<OwnersView>()
            .init_resource::<AllowView>()
            .init_resource::<BanView>()
            .add_message::<OpenAboutLand>()
            .add_systems(
                Startup,
                spawn_about_land_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_about_land,
                    ingest_about_land_events,
                    refresh_on_names,
                    seed_edit_fields,
                    update_control_enable,
                    update_general_tab,
                    update_editable_tab,
                    update_covenant_tab,
                    update_objects_tab,
                    update_environment_tab,
                    sync_owners_view,
                    sync_allow_view,
                    sync_ban_view,
                    apply_combo_edits,
                    apply_texture_edits,
                    apply_avatar_picks,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    populate_owner_rows,
                    bind_owner_rows,
                    populate_access_rows,
                    bind_access_rows,
                )
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The About Land floater's stable [`crate::floater::Floater::id`], the key
/// [`open_about_land`] looks the panel up by.
const ABOUT_LAND_FLOATER_ID: &str = "about-land";

/// Spawn the (hidden) About Land floater's chrome; every tab is built once, on
/// the first open ([`DeferredFloaterContent`]).
fn spawn_about_land_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: ABOUT_LAND_FLOATER_ID,
            title: "About Land".to_owned(),
            position: Vec2::new(360.0, 80.0),
            default_size: Some(Vec2::new(500.0, 480.0)),
            min_size: Some(Vec2::new(420.0, 320.0)),
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
        .insert(Translated::new("about-land-title"));
    let builder = commands.register_system(build_about_land_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build (see [`spawn_about_land_floater`]): the tab
/// container and every tab, ending with the [`AboutLandUi`] insert whose
/// appearance wakes the `Option<Res<AboutLandUi>>` populate systems (their
/// [`AboutLandDirty`] flags persist until then, so an open that raced the
/// build loses nothing).
fn build_about_land_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let labels: Vec<String> = [
        "about-land-tab-general",
        "about-land-tab-covenant",
        "about-land-tab-objects",
        "about-land-tab-options",
        "about-land-tab-media",
        "about-land-tab-sound",
        "about-land-tab-access",
        "about-land-tab-experiences",
        "about-land-tab-environment",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let tabs: TabContainerHandle = spawn_tab_container(
        &mut commands,
        handle.content,
        &TabSpec {
            element: "about-land-tabs",
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

    let general_handles = build_general_tab(&mut commands, panel(0));
    let covenant_handles = build_covenant_tab(&mut commands, panel(1));
    let object_handles = build_objects_tab(&mut commands, panel(2));
    let options_handles = build_options_tab(&mut commands, panel(3));
    let media_handles = build_media_tab(&mut commands, panel(4));
    let sound_handles = build_sound_tab(&mut commands, panel(5));
    let access_handles = build_access_tab(&mut commands, panel(6));
    build_experiences_tab(&mut commands, panel(7));
    let environment_handles = build_environment_tab(&mut commands, panel(8));

    commands.insert_resource(AboutLandUi {
        general_handles,
        covenant_handles,
        object_handles,
        options_handles,
        media_handles,
        sound_handles,
        access_handles,
        environment_handles,
    });
}

// ---------------------------------------------------------------------------
// Structure builders.
// ---------------------------------------------------------------------------

/// Build the General tab.
fn build_general_tab(commands: &mut Commands, panel: Entity) -> GeneralHandles {
    let mut handles = GeneralHandles::default();
    let name_row = spawn_labeled_row(commands, panel, "about-land-name");
    handles.name_field = Some(spawn_edit_field(
        commands,
        name_row,
        "about-land-name-field",
        TextInputKind::Line,
        30.0,
        2,
        63,
    ));
    let id_row = spawn_labeled_row(commands, panel, "about-land-parcel-id");
    handles.parcel_id = Some(spawn_value_node(commands, id_row));
    spawn_section_label(commands, panel, "about-land-description");
    handles.desc_field = Some(spawn_multiline_field(
        commands,
        panel,
        "about-land-desc-field",
        3.0,
        3,
        255,
    ));
    let type_row = spawn_labeled_row(commands, panel, "about-land-type");
    handles.land_type = Some(spawn_value_node(commands, type_row));
    let rating_row = spawn_labeled_row(commands, panel, "about-land-rating");
    handles.rating = Some(spawn_value_node(commands, rating_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-land-owner");
    handles.owner = Some(spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new("about-land-loading", "about-land-none")
            .with_group_suffix("about-land-group-owned"),
    ));
    let group_row = spawn_labeled_row(commands, panel, "about-land-group");
    handles.group = Some(spawn_name_link(
        commands,
        group_row,
        NameLinkSpec::new("about-land-loading", "about-land-none"),
    ));
    let area_row = spawn_labeled_row(commands, panel, "about-land-area");
    handles.area = Some(spawn_value_node(commands, area_row));
    let claimed_row = spawn_labeled_row(commands, panel, "about-land-claimed");
    handles.claimed = Some(spawn_value_node(commands, claimed_row));
    let traffic_row = spawn_labeled_row(commands, panel, "about-land-traffic");
    handles.traffic = Some(spawn_value_node(commands, traffic_row));
    let sale_row = spawn_labeled_row(commands, panel, "about-land-for-sale");
    handles.for_sale = Some(spawn_value_node(commands, sale_row));
    spawn_apply_button(commands, panel, 4);
    handles
}

/// Build the Covenant tab (read-only).
fn build_covenant_tab(commands: &mut Commands, panel: Entity) -> CovenantHandles {
    let mut handles = CovenantHandles::default();
    let estate_row = spawn_labeled_row(commands, panel, "about-land-estate");
    handles.estate = Some(spawn_value_node(commands, estate_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-land-estate-owner");
    handles.estate_owner = Some(spawn_value_node(commands, owner_row));
    handles.text = Some(spawn_value_block(commands, panel));
    let timestamp_row = spawn_labeled_row(commands, panel, "about-land-last-modified");
    handles.timestamp = Some(spawn_value_node(commands, timestamp_row));
    let region_row = spawn_labeled_row(commands, panel, "about-land-region");
    handles.region = Some(spawn_value_node(commands, region_row));
    let type_row = spawn_labeled_row(commands, panel, "about-land-region-type");
    handles.region_type = Some(spawn_value_node(commands, type_row));
    let rating_row = spawn_labeled_row(commands, panel, "about-land-region-rating");
    handles.region_rating = Some(spawn_value_node(commands, rating_row));
    let resale_row = spawn_labeled_row(commands, panel, "about-land-resale");
    handles.resale = Some(spawn_value_node(commands, resale_row));
    let subdivide_row = spawn_labeled_row(commands, panel, "about-land-subdivide");
    handles.subdivide = Some(spawn_value_node(commands, subdivide_row));
    handles
}

/// Build the Objects tab: the prim counts and the object-owners table.
fn build_objects_tab(commands: &mut Commands, panel: Entity) -> ObjectHandles {
    let mut handles = ObjectHandles::default();
    let capacity_row = spawn_labeled_row(commands, panel, "about-land-region-capacity");
    handles.region_capacity = Some(spawn_value_node(commands, capacity_row));
    let parcel_capacity_row = spawn_labeled_row(commands, panel, "about-land-parcel-capacity");
    handles.parcel_capacity = Some(spawn_value_node(commands, parcel_capacity_row));
    let impact_row = spawn_labeled_row(commands, panel, "about-land-parcel-impact");
    handles.parcel_impact = Some(spawn_value_node(commands, impact_row));
    let owner_row = spawn_labeled_row(commands, panel, "about-land-owner-objects");
    handles.owner_objects = Some(spawn_value_node(commands, owner_row));
    let group_row = spawn_labeled_row(commands, panel, "about-land-group-objects");
    handles.group_objects = Some(spawn_value_node(commands, group_row));
    let other_row = spawn_labeled_row(commands, panel, "about-land-other-objects");
    handles.other_objects = Some(spawn_value_node(commands, other_row));
    let selected_row = spawn_labeled_row(commands, panel, "about-land-selected-objects");
    handles.selected_objects = Some(spawn_value_node(commands, selected_row));
    let autoreturn_row = spawn_labeled_row(commands, panel, "about-land-autoreturn");
    handles.autoreturn = Some(spawn_value_node(commands, autoreturn_row));

    let header = spawn_row(commands, panel);
    spawn_key_label(
        commands,
        header,
        "about-land-object-owners",
        DIM_LABEL_COLOR,
    );
    spawn_action_button(
        commands,
        header,
        "about-land-refresh",
        AboutLandAction::RefreshOwners,
        3,
        false,
    );
    let table = spawn_bounded_table(commands, panel, &OWNERS_TABLE);
    handles.owners_viewport = Some(table.viewport);
    handles.owners_table = Some(table.root);
    handles
}

/// Build the Options tab.
fn build_options_tab(commands: &mut Commands, panel: Entity) -> OptionsHandles {
    let mut handles = OptionsHandles::default();
    spawn_section_label(commands, panel, "about-land-options-allow");
    spawn_check(
        commands,
        panel,
        "about-land-opt-terraform",
        CheckKind::Flag(ParcelFlags::ALLOW_TERRAFORM),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-fly",
        CheckKind::Flag(ParcelFlags::ALLOW_FLY),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-build",
        CheckKind::Flag(ParcelFlags::CREATE_OBJECTS),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-build-group",
        CheckKind::Flag(ParcelFlags::CREATE_GROUP_OBJECTS),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-entry",
        CheckKind::Flag(ParcelFlags::ALLOW_ALL_OBJECT_ENTRY),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-entry-group",
        CheckKind::Flag(ParcelFlags::ALLOW_GROUP_OBJECT_ENTRY),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-scripts",
        CheckKind::Flag(ParcelFlags::ALLOW_OTHER_SCRIPTS),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-scripts-group",
        CheckKind::Flag(ParcelFlags::ALLOW_GROUP_SCRIPTS),
    );
    spawn_section_label(commands, panel, "about-land-options-land");
    spawn_check(
        commands,
        panel,
        "about-land-opt-safe",
        CheckKind::FlagInverted(ParcelFlags::ALLOW_DAMAGE),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-no-push",
        CheckKind::Flag(ParcelFlags::RESTRICT_PUSHOBJECT),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-search",
        CheckKind::Flag(ParcelFlags::SHOW_DIRECTORY),
    );
    spawn_check(
        commands,
        panel,
        "about-land-opt-mature",
        CheckKind::Flag(ParcelFlags::MATURE_PUBLISH),
    );
    let category_row = spawn_labeled_row(commands, panel, "about-land-category");
    handles.category_combo = Some(spawn_options_combo(
        commands,
        category_row,
        "about-land-category-combo",
        CATEGORY_KEYS,
        5,
    ));
    let snapshot_row = spawn_labeled_row(commands, panel, "about-land-snapshot");
    handles.snapshot_value = Some(spawn_texture_button(
        commands,
        snapshot_row,
        AboutLandAction::PickSnapshot,
        6,
    ));
    let landing_row = spawn_labeled_row(commands, panel, "about-land-landing-point");
    handles.landing_point = Some(spawn_value_node(commands, landing_row));
    let landing_buttons = spawn_row(commands, panel);
    spawn_action_button(
        commands,
        landing_buttons,
        "about-land-landing-set",
        AboutLandAction::SetLandingPoint,
        7,
        true,
    );
    spawn_action_button(
        commands,
        landing_buttons,
        "about-land-landing-clear",
        AboutLandAction::ClearLandingPoint,
        8,
        true,
    );
    let routing_row = spawn_labeled_row(commands, panel, "about-land-teleport-routing");
    handles.landing_combo = Some(spawn_options_combo(
        commands,
        routing_row,
        "about-land-routing-combo",
        ROUTING_KEYS,
        9,
    ));
    spawn_apply_button(commands, panel, 10);
    handles
}

/// Build the Media tab.
fn build_media_tab(commands: &mut Commands, panel: Entity) -> MediaHandles {
    let mut handles = MediaHandles::default();
    let url_row = spawn_labeled_row(commands, panel, "about-land-media-url");
    handles.url_field = Some(spawn_edit_field(
        commands,
        url_row,
        "about-land-media-url-field",
        TextInputKind::Line,
        28.0,
        2,
        255,
    ));
    let texture_row = spawn_labeled_row(commands, panel, "about-land-media-texture");
    handles.texture_value = Some(spawn_texture_button(
        commands,
        texture_row,
        AboutLandAction::PickMediaTexture,
        3,
    ));
    spawn_check(
        commands,
        panel,
        "about-land-media-autoscale",
        CheckKind::MediaAutoScale,
    );
    spawn_check(
        commands,
        panel,
        "about-land-media-loop",
        CheckKind::MediaLoop,
    );
    let type_row = spawn_labeled_row(commands, panel, "about-land-media-type");
    handles.media_type = Some(spawn_disabled_value(commands, type_row));
    let size_row = spawn_labeled_row(commands, panel, "about-land-media-size");
    handles.media_size = Some(spawn_disabled_value(commands, size_row));
    spawn_apply_button(commands, panel, 4);
    handles
}

/// Build the Sound tab.
fn build_sound_tab(commands: &mut Commands, panel: Entity) -> SoundHandles {
    let mut handles = SoundHandles::default();
    let music_row = spawn_labeled_row(commands, panel, "about-land-music-url");
    handles.music_field = Some(spawn_edit_field(
        commands,
        music_row,
        "about-land-music-url-field",
        TextInputKind::Line,
        28.0,
        2,
        255,
    ));
    spawn_check(
        commands,
        panel,
        "about-land-sound-local",
        CheckKind::Flag(ParcelFlags::SOUND_LOCAL),
    );
    spawn_check(
        commands,
        panel,
        "about-land-voice-enable",
        CheckKind::Flag(ParcelFlags::ALLOW_VOICE),
    );
    spawn_check(
        commands,
        panel,
        "about-land-voice-local",
        CheckKind::FlagInverted(ParcelFlags::USE_ESTATE_VOICE_CHAN),
    );
    spawn_check(
        commands,
        panel,
        "about-land-av-sounds",
        CheckKind::AnyAvSounds,
    );
    spawn_check(
        commands,
        panel,
        "about-land-av-sounds-group",
        CheckKind::GroupAvSounds,
    );
    spawn_apply_button(commands, panel, 3);
    handles
}

/// Build the Access tab.
fn build_access_tab(commands: &mut Commands, panel: Entity) -> AccessHandles {
    let mut handles = AccessHandles::default();
    spawn_check(
        commands,
        panel,
        "about-land-access-public",
        CheckKind::FlagInverted(ParcelFlags::USE_ACCESS_LIST),
    );
    spawn_check(
        commands,
        panel,
        "about-land-access-payment",
        CheckKind::Flag(ParcelFlags::DENY_ANONYMOUS),
    );
    spawn_check(
        commands,
        panel,
        "about-land-access-age",
        CheckKind::Flag(ParcelFlags::DENY_AGEUNVERIFIED),
    );
    spawn_check(
        commands,
        panel,
        "about-land-access-group",
        CheckKind::Flag(ParcelFlags::USE_ACCESS_GROUP),
    );
    spawn_check(
        commands,
        panel,
        "about-land-access-passes",
        CheckKind::Flag(ParcelFlags::USE_PASS_LIST),
    );
    let price_row = spawn_labeled_row(commands, panel, "about-land-pass-price");
    handles.pass_price_field = Some(spawn_edit_field(
        commands,
        price_row,
        "about-land-pass-price-field",
        TextInputKind::NonNegativeInteger,
        8.0,
        2,
        8,
    ));
    let hours_row = spawn_labeled_row(commands, panel, "about-land-pass-hours");
    handles.pass_hours_field = Some(spawn_edit_field(
        commands,
        hours_row,
        "about-land-pass-hours-field",
        TextInputKind::Float,
        8.0,
        3,
        8,
    ));

    let allow_header = spawn_row(commands, panel);
    spawn_key_label(
        commands,
        allow_header,
        "about-land-allowed",
        DIM_LABEL_COLOR,
    );
    spawn_action_button(
        commands,
        allow_header,
        "about-land-add",
        AboutLandAction::AddAllowed,
        4,
        true,
    );
    let allow = spawn_bounded_table(commands, panel, &ALLOW_TABLE);
    handles.allow_viewport = Some(allow.viewport);
    handles.allow_table = Some(allow.root);

    let ban_header = spawn_row(commands, panel);
    spawn_key_label(commands, ban_header, "about-land-banned", DIM_LABEL_COLOR);
    spawn_action_button(
        commands,
        ban_header,
        "about-land-add",
        AboutLandAction::AddBanned,
        5,
        true,
    );
    let ban = spawn_bounded_table(commands, panel, &BAN_TABLE);
    handles.ban_viewport = Some(ban.viewport);
    handles.ban_table = Some(ban.root);
    handles
}

/// Build the Experiences tab (a note — no per-parcel experience protocol).
fn build_experiences_tab(commands: &mut Commands, panel: Entity) {
    spawn_note(commands, panel, "about-land-experiences-unavailable");
}

/// Build the Environment tab (read-only summary).
fn build_environment_tab(commands: &mut Commands, panel: Entity) -> EnvironmentHandles {
    let mut handles = EnvironmentHandles::default();
    let override_row = spawn_labeled_row(commands, panel, "about-land-env-override");
    handles.override_allowed = Some(spawn_value_node(commands, override_row));
    let version_row = spawn_labeled_row(commands, panel, "about-land-env-version");
    handles.version = Some(spawn_value_node(commands, version_row));
    let cycle_row = spawn_labeled_row(commands, panel, "about-land-env-day-cycle");
    handles.day_cycle = Some(spawn_value_node(commands, cycle_row));
    spawn_note(commands, panel, "about-land-env-edit-note");
    handles
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

/// Open the floater on the requested parcel and request its tab data.
#[expect(
    clippy::too_many_arguments,
    reason = "the open reads the identity / parcel model to resolve the subject and fires every \
              tab's fetch"
)]
fn open_about_land(
    mut requests: MessageReader<OpenAboutLand>,
    mut state: ResMut<AboutLandState>,
    mut dirty: ResMut<AboutLandDirty>,
    floaters: Query<(Entity, &Floater)>,
    identity: Res<SlIdentity>,
    parcels: Query<&SlParcel>,
    regions: Query<&Children, With<SlCurrentRegion>>,
    agent_parcel: Res<SlAgentParcel>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(request) = requests.read().last().copied() else {
        return;
    };
    state.reset(request.read_only);
    match request.subject {
        AboutLandSubject::CurrentParcel(local_id) => {
            // The parcel data is already local (the agent's current parcel).
            let parcel = find_parcel(&parcels, &regions, local_id)
                .cloned()
                .or_else(|| {
                    agent_parcel
                        .current
                        .as_ref()
                        .filter(|parcel| parcel.local_id == local_id)
                        .cloned()
                });
            if let Some(parcel) = parcel {
                state.bind(parcel, &identity);
                if let Some(scoped) = state.scoped(&identity) {
                    request_tab_data(scoped, &mut commands);
                }
            } else {
                // No local copy — ask the sim for it by id and bind on the reply.
                state.target = Some(local_id);
                if let Some(scoped) = state.scoped(&identity) {
                    let sequence_id = state.next_sequence();
                    state.pending_sequence = Some(sequence_id);
                    commands.write(SlCommand(Command::RequestParcelPropertiesById {
                        local_id: scoped,
                        sequence_id,
                    }));
                }
            }
        }
        AboutLandSubject::AtPoint { x, y } => {
            // Ask the sim which parcel contains the clicked point; bind on the
            // reply (matched by the echoed sequence id).
            let sequence_id = state.next_sequence();
            state.pending_sequence = Some(sequence_id);
            commands.write(SlCommand(Command::RequestParcelProperties {
                west: x,
                south: y,
                east: x,
                north: y,
                sequence_id,
            }));
        }
    }
    // The estate covenant is region-scoped (needs no parcel), so request it now.
    commands.write(SlCommand(Command::RequestEstateCovenant));
    dirty.mark_all();

    // By stable id, not `AboutLandUi` — this very open may be the first, which
    // is what triggers the deferred content build; the populate systems then
    // consume the dirty flags set above once the UI exists.
    if let Some(panel) = floater_panel(&floaters, ABOUT_LAND_FLOATER_ID)
        && let Ok(mut shown) = panels.get_mut(panel)
    {
        shown.0 = true;
    }
}

/// Request a bound parcel's per-parcel tab data (owners, dwell, access lists).
fn request_tab_data(scoped: ScopedParcelId, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::RequestParcelObjectOwners {
        local_id: scoped,
    }));
    commands.write(SlCommand(Command::RequestParcelDwell { local_id: scoped }));
    commands.write(SlCommand(Command::RequestParcelAccessList {
        local_id: scoped,
        scope: ParcelAccessScope::Access,
    }));
    commands.write(SlCommand(Command::RequestParcelAccessList {
        local_id: scoped,
        scope: ParcelAccessScope::Ban,
    }));
}

/// The parcel matching `local_id` among the current region's parcel children.
fn find_parcel<'a>(
    parcels: &'a Query<&SlParcel>,
    regions: &Query<&Children, With<SlCurrentRegion>>,
    local_id: RegionLocalParcelId,
) -> Option<&'a ParcelInfo> {
    for children in regions {
        for child in children {
            if let Ok(parcel) = parcels.get(*child)
                && parcel.0.local_id == local_id
            {
                return Some(&parcel.0);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold parcel / covenant / dwell / owner / access / media replies into the model.
fn ingest_about_land_events(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AboutLandState>,
    mut dirty: ResMut<AboutLandDirty>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    // Process while a subject is bound, or a point-open is awaiting its reply.
    if state.target.is_none() && state.pending_sequence.is_none() {
        events.clear();
        return;
    }
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::ParcelProperties(parcel)
                if state.pending_sequence == Some(parcel.sequence_id) =>
            {
                // The awaited point / id resolve: bind this parcel as the subject
                // and fetch the rest of its tab data.
                state.bind((**parcel).clone(), &identity);
                if let Some(scoped) = state.scoped(&identity) {
                    request_tab_data(scoped, &mut commands);
                }
                dirty.mark_all();
            }
            SlSessionEvent::ParcelProperties(parcel) if Some(parcel.local_id) == state.target => {
                state.parcel = Some((**parcel).clone());
                state.seed_draft();
                dirty.general_values = true;
                dirty.objects_values = true;
                dirty.editable_values = true;
                dirty.environment_values = true;
            }
            SlSessionEvent::ParcelDwell {
                local_id, dwell, ..
            } if Some(local_id.id()) == state.target => {
                state.dwell = Some(*dwell);
                dirty.general_values = true;
            }
            SlSessionEvent::ParcelObjectOwners { owners } => {
                state.owners.clone_from(owners);
                request_names_for_owners(&state, &mut commands);
                state.owners_revision = state.owners_revision.wrapping_add(1);
                dirty.objects_values = true;
            }
            SlSessionEvent::ParcelAccessList {
                local_id,
                scope,
                entries,
            } if Some(local_id.id()) == state.target => {
                match scope {
                    ParcelAccessScope::Access => {
                        state.access_allow.clone_from(entries);
                        state.allow_revision = state.allow_revision.wrapping_add(1);
                    }
                    ParcelAccessScope::Ban => {
                        state.access_ban.clone_from(entries);
                        state.ban_revision = state.ban_revision.wrapping_add(1);
                    }
                }
                request_names_for_access(&state, &mut commands);
            }
            SlSessionEvent::ParcelMediaUpdate(media) => {
                state.media = Some(media.clone());
                dirty.editable_values = true;
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
                if let Some(agent) = estate_owner_agent(covenant) {
                    request_name(agent, &mut commands);
                }
                state.covenant = Some(covenant.clone());
                dirty.covenant_values = true;
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

/// Re-run the name-dependent value updates when the avatar / group name caches
/// change, so an owner / resident shown as a UUID resolves to a name once its
/// reply lands (the tables re-sync themselves on the same signal).
fn refresh_on_names(
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut dirty: ResMut<AboutLandDirty>,
) {
    if avatars.is_changed() || groups.is_changed() {
        dirty.general_values = true;
    }
}

/// The estate owner as an [`AgentKey`], or `None` when nil.
fn estate_owner_agent(covenant: &EstateCovenant) -> Option<AgentKey> {
    (!covenant.estate_owner_id.is_nil()).then(|| AgentKey::from(covenant.estate_owner_id))
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

/// Request display names for every agent owner in the object-owner tally.
fn request_names_for_owners(state: &AboutLandState, commands: &mut MessageWriter<SlCommand>) {
    let agents: Vec<AgentKey> = state
        .owners
        .iter()
        .filter_map(|owner| match owner.owner {
            OwnerKey::Agent(agent) => Some(agent),
            OwnerKey::Group(_group) => None,
        })
        .collect();
    if !agents.is_empty() {
        commands.write(SlCommand(Command::RequestAvatarNames(agents)));
    }
}

/// Request display names for every agent in the allow / ban lists.
fn request_names_for_access(state: &AboutLandState, commands: &mut MessageWriter<SlCommand>) {
    let agents: Vec<AgentKey> = state
        .access_allow
        .iter()
        .chain(state.access_ban.iter())
        .map(|entry| AgentKey::from(entry.id))
        .collect();
    if !agents.is_empty() {
        commands.write(SlCommand(Command::RequestAvatarNames(agents)));
    }
}

/// Request a single agent's display name.
fn request_name(agent: AgentKey, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
}

// ---------------------------------------------------------------------------
// In-place value updates.
// ---------------------------------------------------------------------------

/// Seed the edit fields' text from the draft on a fresh subject.
fn seed_edit_fields(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    mut fields: Query<&mut EditableText>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.seed_fields {
        return;
    }
    dirty.seed_fields = false;
    let draft = &state.draft;
    set_field_text(&mut fields, ui.general_handles.name_field, &draft.name);
    set_field_text(
        &mut fields,
        ui.general_handles.desc_field,
        &draft.description,
    );
    set_field_text(
        &mut fields,
        ui.media_handles.url_field,
        &url_text(draft.media_url.as_ref()),
    );
    set_field_text(
        &mut fields,
        ui.sound_handles.music_field,
        &url_text(draft.music_url.as_ref()),
    );
    set_field_text(
        &mut fields,
        ui.access_handles.pass_price_field,
        &draft.pass_price.0.to_string(),
    );
    set_field_text(
        &mut fields,
        ui.access_handles.pass_hours_field,
        &format!("{:.0}", draft.pass_hours),
    );
}

/// Toggle write buttons' visibility and every editable control's
/// [`InteractionDisabled`] to follow the agent's rights.
fn update_control_enable(
    mut dirty: ResMut<AboutLandDirty>,
    state: Res<AboutLandState>,
    mut write_buttons: Query<&mut Visibility, With<WriteButton>>,
    gated: Query<(Entity, &EditGate)>,
    disabled: Query<(), With<InteractionDisabled>>,
    mut commands: Commands,
) {
    if !dirty.controls {
        return;
    }
    dirty.controls = false;
    let can_edit = state.can_edit;
    let button_vis = if can_edit {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut write_buttons {
        if *visibility != button_vis {
            *visibility = button_vis;
        }
    }
    for (entity, gate) in &gated {
        let enabled = match gate {
            EditGate::Owner => can_edit,
            EditGate::Never => false,
        };
        let is_disabled = disabled.contains(entity);
        if enabled && is_disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !is_disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

/// Refresh the General tab's read-only values in place.
fn update_general_tab(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.general_values {
        return;
    }
    dirty.general_values = false;
    let texts = &mut texts;
    let handles = &ui.general_handles;
    let region = regions.iter().next().map(|region| &region.0);
    let Some(parcel) = &state.parcel else {
        return;
    };
    set_value_node(texts, handles.parcel_id, &parcel.local_id.0.to_string());
    set_value_node(
        texts,
        handles.land_type,
        &product_text(region.map(|r| r.product), &translator),
    );
    set_value_node(
        texts,
        handles.rating,
        &maturity_text(region.map(|r| r.maturity), &translator),
    );
    // The parcel owner is always present in the reply (an agent or a deeded
    // group); the widget annotates a group owner with "(group owned)".
    set_name_link(
        &mut links,
        handles.owner,
        NameTarget::from_option(true, Some(parcel.owner)),
    );
    set_name_link(
        &mut links,
        handles.group,
        NameTarget::from_option(true, parcel.group),
    );
    set_value_node(texts, handles.area, &parcel.area.to_string());
    set_value_node(
        texts,
        handles.claimed,
        &format_unix_date(i64::from(parcel.claim_date)),
    );
    set_value_node(
        texts,
        handles.traffic,
        &state
            .dwell
            .map_or_else(|| translator.get("about-land-loading"), format_dwell),
    );
    set_value_node(texts, handles.for_sale, &sale_text(parcel, &translator));
}

/// Refresh the Options / Media / Sound controls in place: checkbox glyphs (with
/// their enabled greying), combos, texture ids, media read-outs, landing point.
fn update_editable_tab(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    checks: Query<&AboutLandCheck>,
    mut combos: Query<&mut ComboSelection>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.editable_values {
        return;
    }
    dirty.editable_values = false;
    let texts = &mut texts;
    let can_edit = state.can_edit;
    for check in &checks {
        let on = check.kind.checked(&state);
        let enabled = can_edit && check.kind.editable();
        set_check_visual(texts, check, on, enabled);
    }
    let draft = &state.draft;
    set_combo(
        &mut combos,
        ui.options_handles.category_combo,
        usize::from(draft.category.to_u8()),
    );
    set_combo(
        &mut combos,
        ui.options_handles.landing_combo,
        usize::from(draft.landing_type.min(2)),
    );
    set_value_node(
        texts,
        ui.options_handles.snapshot_value,
        &texture_label(draft.snapshot_id),
    );
    set_value_node(
        texts,
        ui.media_handles.texture_value,
        &texture_label(draft.media_id),
    );
    set_value_node(
        texts,
        ui.options_handles.landing_point,
        &coord_text(&draft.user_location),
    );
    let media_type = state.media.as_ref().map_or_else(
        || translator.get("about-land-none"),
        |m| m.media_type.clone(),
    );
    set_value_node(texts, ui.media_handles.media_type, &media_type);
    set_value_node(
        texts,
        ui.media_handles.media_size,
        &media_size_text(state.media.as_ref(), &translator),
    );
}

/// Refresh the Covenant tab's values in place.
fn update_covenant_tab(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    avatars: Res<AvatarState>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.covenant_values {
        return;
    }
    dirty.covenant_values = false;
    let texts = &mut texts;
    let handles = &ui.covenant_handles;
    let region = regions.iter().next().map(|region| &region.0);
    if let Some(covenant) = &state.covenant {
        set_value_node(texts, handles.estate, &covenant.estate_name);
        set_value_node(
            texts,
            handles.estate_owner,
            &estate_owner_agent(covenant).map_or_else(
                || translator.get("about-land-none"),
                |agent| name_of(agent, &avatars),
            ),
        );
        set_value_node(
            texts,
            handles.timestamp,
            &format_unix_date(i64::from(covenant.covenant_timestamp)),
        );
    }
    set_value_node(
        texts,
        handles.text,
        &covenant_body(
            state.covenant.as_ref(),
            state.covenant_text.as_deref(),
            &translator,
        ),
    );
    set_value_node(
        texts,
        handles.region,
        &region
            .and_then(|r| r.sim_name.as_ref())
            .map_or_else(|| translator.get("about-land-loading"), ToString::to_string),
    );
    set_value_node(
        texts,
        handles.region_type,
        &product_text(region.map(|r| r.product), &translator),
    );
    set_value_node(
        texts,
        handles.region_rating,
        &maturity_text(region.map(|r| r.maturity), &translator),
    );
    let region_flags = region.map(|r| RegionFlags::from_bits(r.region_flags));
    set_value_node(
        texts,
        handles.resale,
        &resale_text(region_flags, &translator),
    );
    set_value_node(
        texts,
        handles.subdivide,
        &subdivide_text(region_flags, &translator),
    );
}

/// Refresh the Objects tab's counts in place.
fn update_objects_tab(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.objects_values {
        return;
    }
    dirty.objects_values = false;
    let texts = &mut texts;
    let handles = &ui.object_handles;
    let Some(parcel) = &state.parcel else {
        return;
    };
    set_value_node(
        texts,
        handles.region_capacity,
        &format!(
            "{} / {}",
            parcel.sim_wide_total_prims, parcel.sim_wide_max_prims
        ),
    );
    set_value_node(
        texts,
        handles.parcel_capacity,
        &parcel.max_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.parcel_impact,
        &parcel.total_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.owner_objects,
        &parcel.owner_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.group_objects,
        &parcel.group_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.other_objects,
        &parcel.other_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.selected_objects,
        &parcel.selected_prims.to_string(),
    );
    set_value_node(
        texts,
        handles.autoreturn,
        &parcel.other_clean_time.to_string(),
    );
}

/// Refresh the Environment tab's read-only summary in place.
fn update_environment_tab(
    mut dirty: ResMut<AboutLandDirty>,
    ui: Option<Res<AboutLandUi>>,
    state: Res<AboutLandState>,
    environment: Option<Res<EnvironmentState>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if !dirty.environment_values {
        return;
    }
    dirty.environment_values = false;
    let texts = &mut texts;
    let handles = &ui.environment_handles;
    if let Some(parcel) = &state.parcel {
        let allowed = if parcel.region_allow_environment_override {
            translator.get("about-land-yes")
        } else {
            translator.get("about-land-no")
        };
        set_value_node(texts, handles.override_allowed, &allowed);
        set_value_node(
            texts,
            handles.version,
            &parcel.parcel_environment_version.to_string(),
        );
    }
    let summary = environment.map_or_else(
        || translator.get("about-land-loading"),
        |env| day_cycle_summary(&env.settings),
    );
    set_value_node(texts, handles.day_cycle, &summary);
}

// ---------------------------------------------------------------------------
// Table view sync + populate + bind.
// ---------------------------------------------------------------------------

/// Rebuild the object-owners view (resolving names) when the tally or the name
/// caches change, and keep the virtual list's item count in step.
fn sync_owners_view(
    state: Res<AboutLandState>,
    mut view: ResMut<OwnersView>,
    ui: Option<Res<AboutLandUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    if view.built == state.owners_revision && !avatars.is_changed() && !groups.is_changed() {
        return;
    }
    view.built = state.owners_revision;
    view.rows = state
        .owners
        .iter()
        .map(|owner| {
            let (kind_key, name) = match owner.owner {
                OwnerKey::Agent(agent) => ("about-land-owner-agent", name_of(agent, &avatars)),
                OwnerKey::Group(group) => (
                    "about-land-owner-group",
                    groups
                        .group_name(group)
                        .map_or_else(|| format!("({group})"), str::to_owned),
                ),
            };
            OwnerRowData {
                kind: translator.get(kind_key),
                name,
                count: owner.count.to_string(),
            }
        })
        .collect();
    if let Some(viewport) = ui.object_handles.owners_viewport
        && let Ok(mut list) = lists.get_mut(viewport)
    {
        list.item_count = view.rows.len();
    }
}

/// Rebuild the allow-list view.
fn sync_allow_view(
    state: Res<AboutLandState>,
    view: ResMut<AllowView>,
    ui: Option<Res<AboutLandUi>>,
    avatars: Res<AvatarState>,
    translator: Translator,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        state.allow_revision,
        &state.access_allow,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access_handles.allow_viewport),
        &avatars,
        avatars.is_changed(),
        &translator,
        lists,
    );
}

/// Rebuild the ban-list view.
fn sync_ban_view(
    state: Res<AboutLandState>,
    view: ResMut<BanView>,
    ui: Option<Res<AboutLandUi>>,
    avatars: Res<AvatarState>,
    translator: Translator,
    lists: Query<&mut VirtualList>,
) {
    let view = view.into_inner();
    sync_access_view(
        state.ban_revision,
        &state.access_ban,
        &mut view.rows,
        &mut view.built,
        ui.and_then(|ui| ui.access_handles.ban_viewport),
        &avatars,
        avatars.is_changed(),
        &translator,
        lists,
    );
}

/// The shared rebuild of an access-list view (resolving names) + item count.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared access-view rebuild threads the source revision, the row sink, the \
              viewport, and the name / translator sources"
)]
fn sync_access_view(
    revision: u64,
    entries: &[ParcelAccessEntry],
    rows: &mut Vec<AccessRowData>,
    built: &mut u64,
    viewport: Option<Entity>,
    avatars: &AvatarState,
    avatars_changed: bool,
    translator: &Translator,
    mut lists: Query<&mut VirtualList>,
) {
    if *built == revision && !avatars_changed {
        return;
    }
    *built = revision;
    *rows = entries
        .iter()
        .map(|entry| AccessRowData {
            id: entry.id,
            name: name_of(AgentKey::from(entry.id), avatars),
            expiry: expiry_text(entry.time, translator),
        })
        .collect();
    if let Some(viewport) = viewport
        && let Ok(mut list) = lists.get_mut(viewport)
    {
        list.item_count = rows.len();
    }
}

/// Build each newly-pooled owner row's cells once.
fn populate_owner_rows(
    mut commands: Commands,
    ui: Option<Res<AboutLandUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(viewport) = ui.object_handles.owners_viewport else {
        return;
    };
    let Some(table) = ui.object_handles.owners_table else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != viewport {
            continue;
        }
        spawn_table_row(&mut commands, row_entity, table, &OWNERS_TABLE);
    }
}

/// Bind each pooled owner row to its [`OwnerRowData`].
fn bind_owner_rows(
    view: Res<OwnersView>,
    ui: Option<Res<AboutLandUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Some(viewport) = ui.object_handles.owners_viewport else {
        return;
    };
    let refresh = view.is_changed();
    for (row, child_of, cells) in &rows {
        if child_of.parent() != viewport {
            continue;
        }
        if !refresh && !row.is_changed() {
            continue;
        }
        let Some(data) = row.index.and_then(|index| view.rows.get(index)) else {
            continue;
        };
        set_cell(&mut texts, cells, 0, &data.kind);
        set_cell(&mut texts, cells, 1, &data.name);
        set_cell(&mut texts, cells, 2, &data.count);
    }
}

/// Build each newly-pooled access row's cells + Remove button once.
fn populate_access_rows(
    mut commands: Commands,
    ui: Option<Res<AboutLandUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        let parent = child_of.parent();
        let (viewport, table, spec, scope) = if Some(parent) == ui.access_handles.allow_viewport {
            (
                ui.access_handles.allow_viewport,
                ui.access_handles.allow_table,
                &ALLOW_TABLE,
                AccessScope::Allow,
            )
        } else if Some(parent) == ui.access_handles.ban_viewport {
            (
                ui.access_handles.ban_viewport,
                ui.access_handles.ban_table,
                &BAN_TABLE,
                AccessScope::Ban,
            )
        } else {
            continue;
        };
        let (Some(_viewport), Some(table)) = (viewport, table) else {
            continue;
        };
        let cells = spawn_table_row(&mut commands, row_entity, table, spec);
        if let Some(custom) = cells.cell(2) {
            spawn_remove_button(&mut commands, custom, scope, row_entity);
        }
    }
}

/// Bind each pooled access row to its [`AccessRowData`].
#[expect(
    clippy::too_many_arguments,
    reason = "binding the allow / ban pools needs both views, the state, the UI handles, and the \
              row / remove / visibility / text queries together"
)]
fn bind_access_rows(
    allow: Res<AllowView>,
    ban: Res<BanView>,
    state: Res<AboutLandState>,
    ui: Option<Res<AboutLandUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    removes: Query<Entity, With<RemoveAccessButton>>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let refresh = allow.is_changed() || ban.is_changed() || state.is_changed();
    for (row, child_of, cells) in &rows {
        let parent = child_of.parent();
        let view = if Some(parent) == ui.access_handles.allow_viewport {
            &allow.rows
        } else if Some(parent) == ui.access_handles.ban_viewport {
            &ban.rows
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
        set_cell(&mut texts, cells, 1, &data.expiry);
    }
    // Show each Remove button only when the agent may edit (a parked row hides
    // the whole row, so this only ever reveals buttons on bound rows).
    let want = if state.can_edit {
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

/// Toggle an editable checkbox.
fn on_about_land_check(
    press: On<Pointer<Press>>,
    checks: Query<&AboutLandCheck>,
    mut state: ResMut<AboutLandState>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(check) = checks.get(press.entity) else {
        return;
    };
    if !state.can_edit || !check.kind.editable() {
        return;
    }
    check.kind.toggle(&mut state.draft);
    let on = check.kind.checked(&state);
    set_check_visual(&mut texts, check, on, true);
}

/// Dispatch a floater button press.
#[expect(
    clippy::too_many_arguments,
    reason = "the dispatcher fans out to every button kind, reading the edit fields and the agent \
              position to route each"
)]
fn on_about_land_action(
    press: On<Pointer<Press>>,
    actions: Query<&AboutLandAction>,
    mut state: ResMut<AboutLandState>,
    ui: Res<AboutLandUi>,
    identity: Res<SlIdentity>,
    agent_position: Res<AgentRegionPosition>,
    fields: Query<&EditableText>,
    mut sl_commands: MessageWriter<SlCommand>,
    mut pickers: MessageWriter<OpenAvatarPicker>,
    mut texture_pickers: MessageWriter<OpenTexturePicker>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(action) = actions.get(press.entity) else {
        return;
    };
    // The owners refresh is a read; the rest write.
    let is_read = matches!(action, AboutLandAction::RefreshOwners);
    if !is_read && !state.can_edit {
        return;
    }
    let Some(scoped) = state.scoped(&identity) else {
        return;
    };
    match action {
        AboutLandAction::Apply => apply_draft(&mut state, &ui, &fields, scoped, &mut sl_commands),
        AboutLandAction::RefreshOwners => {
            sl_commands.write(SlCommand(Command::RequestParcelObjectOwners {
                local_id: scoped,
            }));
        }
        AboutLandAction::PickSnapshot => {
            texture_pickers.write(OpenTexturePicker {
                requester: press.entity,
                current: state
                    .draft
                    .snapshot_id
                    .unwrap_or_else(|| TextureKey::from(Uuid::nil())),
                kind: PickerKind::Texture,
            });
        }
        AboutLandAction::PickMediaTexture => {
            texture_pickers.write(OpenTexturePicker {
                requester: press.entity,
                current: state
                    .draft
                    .media_id
                    .unwrap_or_else(|| TextureKey::from(Uuid::nil())),
                kind: PickerKind::Texture,
            });
        }
        AboutLandAction::SetLandingPoint => {
            if let Some(position) = agent_position.position() {
                state.draft.user_location =
                    RegionCoordinates::new(position.x, position.y, position.z);
            }
        }
        AboutLandAction::ClearLandingPoint => {
            state.draft.user_location = RegionCoordinates::new(0.0, 0.0, 0.0);
        }
        // Both access lists take a multi-pick. The reference only does that on
        // the ban list — its allow list was never updated when the ban path
        // grew one — and two buttons side by side that answer a modified click
        // differently is worse than the small divergence.
        AboutLandAction::AddAllowed => {
            pickers.write(OpenAvatarPicker::many("about-land-allow"));
        }
        AboutLandAction::AddBanned => {
            pickers.write(OpenAvatarPicker::many("about-land-ban"));
        }
    }
}

/// Resolve and act on a per-row access Remove press.
#[expect(
    clippy::too_many_arguments,
    reason = "the remove observer reads the pressed button, its row, both views, the state, the \
              identity, and the command writer"
)]
fn on_remove_access(
    press: On<Pointer<Press>>,
    buttons: Query<&RemoveAccessButton>,
    rows: Query<&VirtualRow>,
    allow: Res<AllowView>,
    ban: Res<BanView>,
    mut state: ResMut<AboutLandState>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    if press.button != PointerButton::Primary || !state.can_edit {
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
    let id = match button.scope {
        AccessScope::Allow => allow.rows.get(index).map(|entry| entry.id),
        AccessScope::Ban => ban.rows.get(index).map(|entry| entry.id),
    };
    let (Some(id), Some(scoped)) = (id, state.scoped(&identity)) else {
        return;
    };
    remove_access_entry(&mut state, button.scope, id, scoped, &mut commands);
}

/// Fold a combo pick into the draft.
fn apply_combo_edits(
    mut changed: MessageReader<ComboChanged>,
    ui: Option<Res<AboutLandUi>>,
    mut state: ResMut<AboutLandState>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in changed.read() {
        if Some(event.combo) == ui.options_handles.category_combo {
            state.draft.category = ParcelCategory::from_u8(u8::try_from(event.active).unwrap_or(0));
        } else if Some(event.combo) == ui.options_handles.landing_combo {
            state.draft.landing_type = u8::try_from(event.active).unwrap_or(0);
        }
    }
}

/// Fold a texture pick into the draft and its button label.
fn apply_texture_edits(
    mut picked: MessageReader<TexturePicked>,
    ui: Option<Res<AboutLandUi>>,
    swatches: Query<&SwatchTexture>,
    mut state: ResMut<AboutLandState>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in picked.read() {
        if !event.final_pick {
            continue;
        }
        let Ok(swatch) = swatches.get(event.requester) else {
            continue;
        };
        let texture = (event.texture.uuid() != Uuid::nil()).then_some(event.texture);
        match swatch.action {
            AboutLandAction::PickSnapshot => {
                state.draft.snapshot_id = texture;
                set_value_node(
                    &mut texts,
                    ui.options_handles.snapshot_value,
                    &texture_label(texture),
                );
            }
            AboutLandAction::PickMediaTexture => {
                state.draft.media_id = texture;
                set_value_node(
                    &mut texts,
                    ui.media_handles.texture_value,
                    &texture_label(texture),
                );
            }
            _other => {}
        }
    }
}

/// Fold the avatar picks into the allow / ban list and commit them.
fn apply_avatar_picks(
    mut picked: MessageReader<AvatarPicked>,
    mut state: ResMut<AboutLandState>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(scoped) = state.scoped(&identity) else {
        return;
    };
    for event in picked.read() {
        let scope = match event.requester {
            "about-land-allow" => AccessScope::Allow,
            "about-land-ban" => AccessScope::Ban,
            _other => continue,
        };
        for chosen in &event.picks {
            add_access_entry(&mut state, scope, chosen.agent, scoped, &mut commands);
        }
    }
}

/// Compose the draft from the edit fields and commit it with a refresh.
fn apply_draft(
    state: &mut AboutLandState,
    ui: &AboutLandUi,
    fields: &Query<&EditableText>,
    scoped: ScopedParcelId,
    commands: &mut MessageWriter<SlCommand>,
) {
    if !state.can_edit {
        return;
    }
    let read = |entity: Option<Entity>| {
        entity
            .and_then(|field| fields.get(field).ok())
            .map(|field| field.value().to_string())
    };
    let draft = &mut state.draft;
    if let Some(name) = read(ui.general_handles.name_field) {
        draft.name = name;
    }
    if let Some(description) = read(ui.general_handles.desc_field) {
        draft.description = description;
    }
    draft.media_url = read(ui.media_handles.url_field).and_then(parse_url);
    draft.music_url = read(ui.sound_handles.music_field).and_then(parse_url);
    if let Some(price) =
        read(ui.access_handles.pass_price_field).and_then(|v| v.trim().parse::<u64>().ok())
    {
        draft.pass_price = LindenAmount(price);
    }
    if let Some(hours) =
        read(ui.access_handles.pass_hours_field).and_then(|v| v.trim().parse::<f32>().ok())
    {
        draft.pass_hours = hours;
    }
    draft.local_id = scoped.id();
    commands.write(SlCommand(Command::UpdateParcel(draft.clone())));
    commands.write(SlCommand(Command::RequestParcelPropertiesById {
        local_id: scoped,
        sequence_id: 0,
    }));
}

/// Append an agent to an access list and commit it.
fn add_access_entry(
    state: &mut AboutLandState,
    scope: AccessScope,
    agent: AgentKey,
    scoped: ScopedParcelId,
    commands: &mut MessageWriter<SlCommand>,
) {
    if !state.can_edit {
        return;
    }
    let id = agent.0.0;
    let list = match scope {
        AccessScope::Allow => &mut state.access_allow,
        AccessScope::Ban => &mut state.access_ban,
    };
    if list.iter().any(|entry| entry.id == id) {
        return;
    }
    list.push(ParcelAccessEntry {
        id,
        time: 0,
        flags: ParcelAccessFlags::NONE,
    });
    bump_access_revision(state, scope);
    send_access_list(state, scope, scoped, commands);
}

/// Remove an agent from an access list and commit it.
fn remove_access_entry(
    state: &mut AboutLandState,
    scope: AccessScope,
    id: Uuid,
    scoped: ScopedParcelId,
    commands: &mut MessageWriter<SlCommand>,
) {
    if !state.can_edit {
        return;
    }
    let list = match scope {
        AccessScope::Allow => &mut state.access_allow,
        AccessScope::Ban => &mut state.access_ban,
    };
    list.retain(|entry| entry.id != id);
    bump_access_revision(state, scope);
    send_access_list(state, scope, scoped, commands);
}

/// Bump the revision of the given access list, so its table view rebuilds.
const fn bump_access_revision(state: &mut AboutLandState, scope: AccessScope) {
    match scope {
        AccessScope::Allow => state.allow_revision = state.allow_revision.wrapping_add(1),
        AccessScope::Ban => state.ban_revision = state.ban_revision.wrapping_add(1),
    }
}

/// Send the current allow / ban list for `scope` to the grid.
fn send_access_list(
    state: &AboutLandState,
    scope: AccessScope,
    scoped: ScopedParcelId,
    commands: &mut MessageWriter<SlCommand>,
) {
    let entries = match scope {
        AccessScope::Allow => state.access_allow.clone(),
        AccessScope::Ban => state.access_ban.clone(),
    };
    commands.write(SlCommand(Command::UpdateParcelAccessList {
        local_id: scoped,
        scope: scope.wire(),
        entries,
    }));
}

/// Build a [`ParcelUpdate`] preserving every field of `parcel` except `name` /
/// `description`.
fn parcel_update_from(parcel: &ParcelInfo, name: String, description: String) -> ParcelUpdate {
    ParcelUpdate {
        local_id: parcel.local_id,
        parcel_flags: ParcelFlags::from_bits(parcel.raw_parcel_flags),
        sale_price: parcel.sale_price.clone(),
        name,
        description,
        music_url: parcel.music_url.clone(),
        media_url: parcel.media_url.clone(),
        media_id: parcel.media_id,
        media_auto_scale: parcel.media_auto_scale,
        group_id: parcel.group,
        pass_price: parcel.pass_price.clone(),
        pass_hours: parcel.pass_hours,
        category: parcel.category,
        auth_buyer_id: parcel.auth_buyer_id,
        snapshot_id: parcel.snapshot_id,
        user_location: parcel.user_location,
        user_look_at: parcel.user_look_at,
        landing_type: parcel.landing_type.to_u8(),
    }
}

// ---------------------------------------------------------------------------
// Value formatting.
// ---------------------------------------------------------------------------

/// The Fluent keys for the search-category combo, by [`ParcelCategory`] code.
const CATEGORY_KEYS: &[&str] = &[
    "about-land-cat-none",
    "about-land-cat-linden",
    "about-land-cat-residential",
    "about-land-cat-commercial",
    "about-land-cat-industrial",
    "about-land-cat-park",
    "about-land-cat-other",
    "about-land-cat-adult",
];

/// The Fluent keys for the teleport-routing combo (`LandingType` 0/1/2).
const ROUTING_KEYS: &[&str] = &[
    "about-land-routing-blocked",
    "about-land-routing-landing",
    "about-land-routing-anywhere",
];

/// The land-type display text for a region product type.
fn product_text(product: Option<ProductType>, translator: &Translator) -> String {
    let key = match product {
        Some(ProductType::FullRegion) => "about-land-product-full",
        Some(ProductType::Homestead) => "about-land-product-homestead",
        Some(ProductType::Openspace) => "about-land-product-openspace",
        _unknown => "about-land-product-unknown",
    };
    translator.get(key)
}

/// The content-rating display text for a maturity.
fn maturity_text(maturity: Option<Maturity>, translator: &Translator) -> String {
    let key = match maturity {
        Some(Maturity::Pg) => "about-land-rating-pg",
        Some(Maturity::Mature) => "about-land-rating-mature",
        Some(Maturity::Adult) => "about-land-rating-adult",
        _unknown => "about-land-rating-unknown",
    };
    translator.get(key)
}

/// The sale-state display text.
fn sale_text(parcel: &ParcelInfo, translator: &Translator) -> String {
    match &parcel.sale_price {
        Some(price) => translator.format(
            "about-land-sale-price",
            &TransArgs::new()
                .int("price", price_amount(price))
                .int("persqm", per_square_metre(price, parcel.area.0)),
        ),
        None => translator.get("about-land-not-for-sale"),
    }
}

/// A sale price's L$ amount as a signed integer.
fn price_amount(price: &LindenAmount) -> i64 {
    i64::try_from(price.0).unwrap_or(i64::MAX)
}

/// The per-square-metre L$ rate for a `price` over `area` m² (0 for zero area).
fn per_square_metre(price: &LindenAmount, area: u32) -> i64 {
    price_amount(price)
        .checked_div(i64::from(area))
        .unwrap_or(0)
}

/// Format a dwell (traffic) value with one decimal place.
fn format_dwell(dwell: f32) -> String {
    format!("{dwell:.1}")
}

/// The media-size display text, or "(none)".
fn media_size_text(media: Option<&ParcelMediaUpdateInfo>, translator: &Translator) -> String {
    match media {
        Some(media) => match (media.media_width, media.media_height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w} × {h}"),
            _auto => translator.get("about-land-media-auto-size"),
        },
        None => translator.get("about-land-none"),
    }
}

/// The covenant body text.
fn covenant_body(
    covenant: Option<&EstateCovenant>,
    text: Option<&str>,
    translator: &Translator,
) -> String {
    if let Some(text) = text {
        if text.trim().is_empty() {
            translator.get("about-land-covenant-none")
        } else {
            text.to_owned()
        }
    } else if covenant.is_some_and(|c| c.covenant_id.is_none()) {
        translator.get("about-land-covenant-none")
    } else if covenant.is_some() {
        translator.get("about-land-covenant-loading")
    } else {
        translator.get("about-land-loading")
    }
}

/// The resale-clause text.
fn resale_text(flags: Option<RegionFlags>, translator: &Translator) -> String {
    let key = match flags {
        Some(flags) if flags.contains(RegionFlags::BLOCK_LAND_RESELL) => {
            "about-land-resale-blocked"
        }
        Some(_flags) => "about-land-resale-allowed",
        None => "about-land-loading",
    };
    translator.get(key)
}

/// The subdivide-clause text.
fn subdivide_text(flags: Option<RegionFlags>, translator: &Translator) -> String {
    let key = match flags {
        Some(flags) if flags.contains(RegionFlags::ALLOW_PARCEL_CHANGES) => {
            "about-land-subdivide-allowed"
        }
        Some(_flags) => "about-land-subdivide-blocked",
        None => "about-land-loading",
    };
    translator.get(key)
}

/// A one-line summary of the active day cycle.
fn day_cycle_summary(settings: &EnvironmentSettings) -> String {
    format!(
        "{} — {} sky / {} water, day {}s",
        settings.day_cycle.name,
        settings.day_cycle.sky_frames.len(),
        settings.day_cycle.water_frames.len(),
        settings.day_length,
    )
}

/// The display label for an optional texture id.
fn texture_label(id: Option<TextureKey>) -> String {
    id.map_or_else(|| "(none)".to_owned(), |id| id.to_string())
}

/// A URL's string, or empty.
fn url_text(url: Option<&url::Url>) -> String {
    url.map(ToString::to_string).unwrap_or_default()
}

/// Parse a possibly-empty URL string (empty ⇒ `None`).
fn parse_url(value: String) -> Option<url::Url> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        url::Url::parse(trimmed).ok()
    }
}

/// Format a region-local landing point coordinate.
fn coord_text(coord: &RegionCoordinates) -> String {
    format!("{:.0}, {:.0}, {:.0}", coord.x(), coord.y(), coord.z())
}

/// An access entry's expiry display: "Always" for `0`, else the date.
fn expiry_text(time: i32, translator: &Translator) -> String {
    if time == 0 {
        translator.get("about-land-always")
    } else {
        format_unix_date(i64::from(time))
    }
}

/// The display name for an agent, falling back to its id in parentheses.
fn name_of(agent: AgentKey, avatars: &AvatarState) -> String {
    avatars
        .name_of(agent)
        .map_or_else(|| format!("({agent})"), str::to_owned)
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

/// A translated label in `color`.
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

/// An always-disabled read-only value node (greyed to read as non-editable).
fn spawn_disabled_value(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DISABLED_COLOR),
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

/// A single-line edit field, gated on parcel ownership.
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
    commands.entity(field).insert(EditGate::Owner);
    field
}

/// A multi-line edit field, gated on parcel ownership.
fn spawn_multiline_field(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    visible_lines: f32,
    tab_index: i32,
    max_characters: usize,
) -> Entity {
    let field = spawn_text_input(
        commands,
        parent,
        &TextInputSpec {
            font_size: FONT_SIZE,
            visible_lines,
            tab_index,
            max_characters: Some(max_characters),
            ..TextInputSpec::new(element, TextInputKind::Multiline)
        },
    );
    commands.entity(field).insert(EditGate::Owner);
    field
}

/// A translated action button dispatching `action`. `write` tags it as a write
/// button (hidden when the agent cannot edit).
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    action: AboutLandAction,
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
            Name::new(format!("about-land-button:{label_key}")),
            ChildOf(parent),
        ))
        .observe(on_about_land_action)
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

/// The shared Apply button for an editable tab.
fn spawn_apply_button(commands: &mut Commands, parent: Entity, tab_index: i32) {
    let row_entity = spawn_row(commands, parent);
    spawn_action_button(
        commands,
        row_entity,
        "about-land-apply",
        AboutLandAction::Apply,
        tab_index,
        true,
    );
}

/// A checkbox: a clickable glyph leading a translated label. Editable checkboxes
/// carry [`EditGate::Owner`]; read-only ones carry [`EditGate::Never`].
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
    let gate = if kind.editable() {
        EditGate::Owner
    } else {
        EditGate::Never
    };
    commands
        .entity(row_entity)
        .insert((
            Button,
            AboutLandCheck { kind, glyph, label },
            gate,
            Pickable::default(),
        ))
        .add_child(glyph)
        .add_child(label)
        .observe(on_about_land_check);
}

/// A combo on `parent` from translated option keys, gated on ownership.
fn spawn_options_combo(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    keys: &[&'static str],
    tab_index: i32,
) -> Entity {
    let labels: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
    let combo = spawn_combo(
        commands,
        parent,
        &ComboSpec {
            element,
            labels: &labels,
            active: 0,
            tab_index,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );
    commands.entity(combo).insert(EditGate::Owner);
    combo
}

/// A texture-picker button showing the current id; returns the id value node.
fn spawn_texture_button(
    commands: &mut Commands,
    parent: Entity,
    action: AboutLandAction,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            action,
            SwatchTexture { action },
            WriteButton,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            ChildOf(parent),
        ))
        .observe(on_about_land_action)
        .id();
    spawn_value_node(commands, button)
}

/// A per-row access Remove button in a table's custom cell.
fn spawn_remove_button(commands: &mut Commands, cell: Entity, scope: AccessScope, row: Entity) {
    let button = commands
        .spawn((
            Button,
            RemoveAccessButton { scope, row },
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
        Translated::new("about-land-remove"),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
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
    check: &AboutLandCheck,
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
              only on a discrete open, not per frame"
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
