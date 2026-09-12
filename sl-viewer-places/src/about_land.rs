//! The **About Land** floater (`viewer-parcel-options-general` +
//! `viewer-parcel-options-access-media`): the parcel information surface, all
//! nine reference tabs — **General**, **Covenant**, **Objects**, **Options**,
//! **Media**, **Sound**, **Access**, **Experiences**, **Environment**.
//!
//! # One window per parcel
//!
//! The floater opens on a **particular parcel** ([`OpenAboutLand`]) — the parcel
//! the top-bar location read-out was clicked on (the agent's current parcel), or
//! the parcel a land-pie right-click landed on — and each parcel gets **its own
//! window**, keyed by [`ScopedParcelId`] (the circuit and the region-local id,
//! so the same local id in two regions is two subjects). Everything the window
//! knows lives on its root entity as components, and closing it ends that
//! instance.
//!
//! The reference keeps `LLFloaterLand` a singleton, because it only ever opens
//! on the parcel the agent is standing in. This viewer opens About Land on a
//! parcel it is *not* standing in — a land-pie click across the road, and in
//! time a search hit or a place profile — so comparing two parcels is a real
//! thing to want. That divergence is a deliberate decision, recorded in
//! `viewer-keyed-floater-audit`.
//!
//! A subject-keyed window persists no geometry (the scaffold's rule: a window
//! per subject has no stable identity to remember one under), so unlike the
//! singleton this replaced it needs no persistence exemption.
//!
//! A land-pie open names a **point**, not a parcel, so its window starts under a
//! provisional key and is re-keyed when the simulator says which parcel that
//! point is in — and folds into the parcel's existing window if there already is
//! one.
//!
//! # Build once, update in place (no despawn)
//!
//! Every tab's structure is spawned **once**, when the window opens, and never
//! torn down while it lives.
//! Replies update values *in place*: value labels via `set_value_node`,
//! checkbox glyphs via `set_check_visual`, combos by writing their
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
    AgentKey, Asset, AssetKey, AssetType, CircuitId, Command, EstateCovenant, LandArea,
    LindenAmount, Maturity, OwnerKey, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope,
    ParcelCategory, ParcelFlags, ParcelInfo, ParcelMediaUpdateInfo, ParcelObjectOwner,
    ParcelUpdate, ProductType, RegionCoordinates, RegionFlags, RegionLocalParcelId, ScopedParcelId,
    SlAgentParcel, SlCommand, SlCurrentRegion, SlEvent, SlIdentity, SlParcel, SlRegionIdentity,
    SlSessionEvent, TextureKey, Uuid,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterCommand, FloaterHandle, FloaterKey, FloaterOp, FloaterSpec,
    FloaterSystems, KeyedFloaterOpen, KeyedFloaters, host_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::inventory_properties::format_unix_date;
use crate::land_environment::{
    LandEnvironmentPlugin, LandEnvironmentSubject, LandPanelKind, spawn_land_environment_panel,
};
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
use crate::virtual_list::{VirtualList, VirtualRow, layout_virtual_lists};
use crate::world_api::AgentRegionPosition;
use crate::world_api::AvatarState;
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

/// The focus stop the Environment tab's land panel starts its run of tab
/// indices at, clear of the other tabs' controls.
const ENV_TAB_INDEX: i32 = 30;

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
pub struct OpenAboutLand {
    /// Which parcel to describe.
    pub subject: AboutLandSubject,
    /// Open without edit affordances (the read-only "About this location" view).
    pub read_only: bool,
}

/// How the About Land floater's subject parcel is identified.
#[derive(Debug, Clone, Copy)]
pub enum AboutLandSubject {
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

/// The floater's data model — **one per window** (see the module header), on
/// the window's root entity.
#[derive(Component, Debug, Default)]
struct AboutLandState {
    /// The parcel currently bound, or `None` before the first open.
    target: Option<RegionLocalParcelId>,
    /// Whether the floater is in read-only "place profile" mode.
    read_only: bool,
    /// Whether the agent may edit the bound parcel. Fixed at open.
    can_edit: bool,
    /// The circuit the subject was bound on. A window whose circuit is no
    /// longer the agent's is showing a parcel in a region that has been left:
    /// the same region-local id on the new circuit is a *different* parcel, so
    /// nothing may be published from it (see the Environment tab).
    bound_circuit: Option<CircuitId>,
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
    /// The record [`draft`](Self::draft) was last seeded or merged from — the
    /// **base** of the three-way merge, and `None` until the subject resolves
    /// (which is also what "the draft is not seeded yet" means).
    ///
    /// A draft field that still equals its base is one this resident has not
    /// edited, so an arriving record owns it; a field that has moved away is a
    /// pending edit and is kept. See [`ParcelUpdate::merge_unedited`].
    seeded: Option<ParcelUpdate>,
    /// What the six text fields were last written with, or `None` before the
    /// first write.
    ///
    /// The text fields are not mirrored into [`draft`](Self::draft) until
    /// **Apply** reads them, so the draft's merge cannot see typing in flight
    /// and this is what does. A widget still reading exactly what it was last
    /// given is one nobody has typed in; anything else is the resident's.
    /// Compared against the *widget*, so it must be what the widget was given
    /// and not the merge's base, which has already advanced by then.
    shown_fields: Option<FieldText>,
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
    /// Which access list an avatar pick this window asked for will land in, or
    /// `None` when it has no pick outstanding.
    ///
    /// The avatar picker echoes a `&'static str` tag rather than an entity, so
    /// a pick cannot name its window. It does not have to: the picker is one
    /// window per tag, so at most one About Land window can have a pick
    /// outstanding for a tag, and this is that window's claim on it.
    pending_pick: Option<AccessScope>,
}

/// A monotonic source of `ParcelPropertiesRequest` sequence ids, shared by every
/// About Land window.
///
/// Per-window counters would hand two windows the same id, and the id is the
/// only thing that says which window's question a `ParcelProperties` reply
/// answers.
#[derive(Resource, Debug, Default)]
struct LandSequence(i32);

impl LandSequence {
    /// The next sequence id — never repeated while the session lives.
    const fn next(&mut self) -> i32 {
        self.0 = self.0.wrapping_add(1);
        self.0
    }
}

/// How long a window's object-owner tally may stay unanswered before the next
/// window's request goes out, in seconds.
const OWNER_TALLY_TIMEOUT_SECONDS: f64 = 8.0;

/// The windows waiting for an object-owner tally, oldest first.
///
/// `ParcelObjectOwnersReply` carries **nothing but the owners** — not the
/// parcel, not a sequence id (see the wire template) — so a reply cannot say
/// whose question it answers, and two windows asking at once would each take
/// the other's tally as their own. Only one request is outstanding at a time;
/// every reply while it is belongs to the window that asked, which is also how
/// a tally split over several packets stays whole. Filed as
/// `viewer-parcel-object-owners-uncorrelated`.
#[derive(Resource, Debug, Default)]
struct OwnerTallyQueue {
    /// The windows still to ask for, with the parcel each is about.
    waiting: std::collections::VecDeque<(Entity, ScopedParcelId)>,
    /// The window whose request is outstanding, and when it gives up.
    asking: Option<(Entity, f64)>,
}

impl OwnerTallyQueue {
    /// Queue `window`'s tally request for `parcel`, replacing any request of
    /// its own it has not been answered yet.
    fn ask(&mut self, window: Entity, parcel: ScopedParcelId) {
        self.waiting.retain(|(waiting, _parcel)| *waiting != window);
        self.waiting.push_back((window, parcel));
    }

    /// Whether `window` owns the tally replies arriving now.
    fn owns_reply(&self, window: Entity) -> bool {
        self.asking
            .is_some_and(|(asking, _deadline)| asking == window)
    }

    /// Forget `window` — it closed, or its subject changed.
    fn forget(&mut self, window: Entity) {
        self.waiting.retain(|(waiting, _parcel)| *waiting != window);
        if self.owns_reply(window) {
            self.asking = None;
        }
    }
}

impl AboutLandState {
    /// Clear the model for a fresh open (subject not yet bound).
    fn reset(&mut self, read_only: bool) {
        self.target = None;
        self.read_only = read_only;
        self.can_edit = false;
        self.bound_circuit = None;
        self.parcel = None;
        self.dwell = None;
        self.media = None;
        self.covenant = None;
        self.covenant_text = None;
        self.covenant_pending = None;
        self.owners = Vec::new();
        self.clear_access_lists();
        self.draft = ParcelUpdate::default();
        self.seeded = None;
        self.shown_fields = None;
        self.pending_sequence = None;
        self.pending_pick = None;
        self.owners_revision = self.owners_revision.wrapping_add(1);
    }

    /// Empty both access lists and bump their revisions.
    ///
    /// A reply to `RequestParcelAccessList` arrives as **one or more** packets
    /// that [`merge_access_reply`] *unions* into the list, so the accumulator
    /// has to be emptied when the list is requested — not when a packet lands.
    fn clear_access_lists(&mut self) {
        self.access_allow = Vec::new();
        self.access_ban = Vec::new();
        self.allow_revision = self.allow_revision.wrapping_add(1);
        self.ban_revision = self.ban_revision.wrapping_add(1);
    }

    /// Bind the resolved parcel as the subject (from a known id, or a point
    /// reply), computing the edit rights.
    fn bind(&mut self, parcel: ParcelInfo, identity: &SlIdentity) {
        self.rebind_rights(&parcel, identity);
        self.bound_circuit = identity.circuit_id;
        self.target = Some(parcel.local_id);
        self.pending_sequence = None;
        self.parcel = Some(parcel);
        self.seed_draft();
    }

    /// Recompute whether the agent may edit the bound parcel — at bind, and
    /// again when a re-open changes the window's mode.
    fn rebind_rights(&mut self, parcel: &ParcelInfo, identity: &SlIdentity) {
        self.can_edit = !self.read_only
            && identity.agent_id.is_some_and(|agent| match parcel.owner {
                OwnerKey::Agent(owner) => owner == agent,
                OwnerKey::Group(_group) => false,
            });
    }

    /// Seed the edit [`draft`](Self::draft) from the parcel, once per open.
    fn seed_draft(&mut self) {
        if self.seeded.is_some() {
            return;
        }
        if let Some(parcel) = &self.parcel {
            self.draft = parcel.to_update();
            self.seeded = Some(self.draft.clone());
        }
    }

    /// Fold a freshly arrived record into the draft, keeping this resident's
    /// pending edits and taking the grid's word for everything else.
    ///
    /// Returns whether any draft field moved, so the caller can re-seed the
    /// text fields that follow it.
    ///
    /// Every `ParcelProperties` for the bound parcel comes through here, not
    /// only the sequence-zero pushes another resident's save produces. Three
    /// things arrive on this path and the merge is the right answer to all of
    /// them, which is why none of them is special-cased: a foreign push (carry
    /// their change), the read-back this floater requests after its own
    /// **Apply** (agrees with the draft, so nothing moves), and an ordinary
    /// refresh. Telling them apart is not possible anyway — `apply_draft`
    /// requests its read-back with `sequence_id: 0`, the very value that marks
    /// an unsolicited push.
    fn merge_parcel(&mut self, parcel: &ParcelInfo) -> bool {
        self.merge_update(parcel.to_update())
    }

    /// The half of [`merge_parcel`](Self::merge_parcel) that does not need a
    /// whole [`ParcelInfo`] to exercise.
    fn merge_update(&mut self, fresh: ParcelUpdate) -> bool {
        let Some(base) = self.seeded.as_ref() else {
            // Not seeded yet: there is no base to merge against, and
            // `seed_draft` is what establishes one.
            return false;
        };
        let moved = self.draft.merge_unedited(base, &fresh);
        self.seeded = Some(fresh);
        moved
    }

    /// The scoped id for the bound parcel, given the current circuit.
    fn scoped(&self, identity: &SlIdentity) -> Option<ScopedParcelId> {
        Some(ScopedParcelId::new(identity.circuit_id?, self.target?))
    }
}

/// The six About Land text fields, rendered.
///
/// Both what a pass is about to write and what the previous pass wrote, so the
/// two can be compared field by field — see
/// [`AboutLandState::shown_fields`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FieldText {
    /// The parcel name.
    name: String,
    /// The parcel description.
    description: String,
    /// The streaming media URL, empty for none.
    media_url: String,
    /// The streaming music URL, empty for none.
    music_url: String,
    /// The parcel pass price in L$.
    pass_price: String,
    /// The parcel pass duration in hours.
    pass_hours: String,
}

impl FieldText {
    /// Render a draft's six editable text values.
    fn from_draft(draft: &ParcelUpdate) -> Self {
        Self {
            name: draft.name.clone(),
            description: draft.description.clone(),
            media_url: url_text(draft.media_url.as_ref()),
            music_url: url_text(draft.music_url.as_ref()),
            pass_price: draft.pass_price.0.to_string(),
            pass_hours: format!("{:.0}", draft.pass_hours),
        }
    }
}

/// How much of the edit fields' text the next [`seed_edit_fields`] pass may
/// rewrite.
///
/// The six text fields are the one part of the form that is **not** mirrored
/// into [`AboutLandState::draft`] until **Apply** reads them, so the resident's
/// pending typing lives in the widget rather than in the draft. That makes the
/// draft's three-way merge blind to it, and this is how the widgets get the
/// same protection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum FieldSeed {
    /// Leave the fields alone.
    #[default]
    None,
    /// Rewrite every field — a fresh subject, whose text has nothing to do with
    /// what the widgets are showing.
    All,
    /// Rewrite only a field whose text still equals what was last seeded into
    /// it, leaving anything the resident has typed since.
    Unedited,
}

/// Which sub-panels need an in-place value refresh this frame, for one window.
#[derive(Component, Debug, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent dirty flag per in-place refresh pass"
)]
struct AboutLandDirty {
    /// How much of the edit fields' text to seed from the draft.
    seed_fields: FieldSeed,
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
    /// The Environment tab's read-only summary.
    environment_values: bool,
}

impl AboutLandDirty {
    /// Mark every sub-panel dirty (on a fresh open).
    const fn mark_all(&mut self) {
        self.seed_fields = FieldSeed::All;
        self.controls = true;
        self.general_values = true;
        self.editable_values = true;
        self.covenant_values = true;
        self.objects_values = true;
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

/// The object-owners table view (rebuilt when the tally or names change), for
/// one window.
#[derive(Component, Debug, Default)]
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

/// The allow-list table view, for one window.
#[derive(Component, Debug, Default)]
struct AllowView {
    /// The rows in display order.
    rows: Vec<AccessRowData>,
    /// The [`AboutLandState::allow_revision`] this view was built from.
    built: u64,
}

/// The ban-list table view, for one window.
#[derive(Component, Debug, Default)]
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

/// The retained nodes of the Environment tab: the two facts the *parcel*
/// record carries, above the shared land-environment panel that does the
/// editing.
#[derive(Debug, Default)]
struct EnvironmentHandles {
    /// The "parcel overrides allowed" value node.
    override_allowed: Option<Entity>,
    /// The parcel environment-version value node.
    version: Option<Entity>,
    /// The shared land-environment panel, scoped to this window's parcel.
    panel: Option<Entity>,
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

/// One window's live entity handles.
#[derive(Component, Debug)]
struct AboutLandUi {
    /// The floater's title text node — rewritten with the parcel's name, so two
    /// windows are tellable apart in the title bar and the window list.
    title_text: Entity,
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
pub struct AboutLandPlugin;

impl Plugin for AboutLandPlugin {
    fn build(&self, app: &mut App) {
        // The Environment tab is the shared land-environment panel, which the
        // Region / Estate floater hosts too — whichever plugin is built first
        // brings its systems.
        if !app.is_plugin_added::<LandEnvironmentPlugin>() {
            app.add_plugins(LandEnvironmentPlugin);
        }
        app.init_resource::<LandSequence>()
            .init_resource::<OwnerTallyQueue>()
            .add_message::<OpenAboutLand>()
            .add_systems(
                Update,
                // After the manager's command pass — see `FloaterSystems`: the
                // land pie that opens a parcel also raises whatever it was
                // clicked through, and the later raise wins.
                open_about_land
                    .after(FloaterSystems::Commands)
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    ingest_about_land_events,
                    drive_owner_tallies,
                    refresh_on_names,
                    seed_edit_fields,
                    update_control_enable,
                    update_general_tab,
                    update_editable_tab,
                    update_covenant_tab,
                    update_objects_tab,
                    update_environment_tab,
                    aim_environment_panel,
                    sync_owners_view,
                    sync_allow_view,
                    sync_ban_view,
                    apply_combo_edits,
                    apply_texture_edits,
                    apply_avatar_picks,
                )
                    .chain()
                    .after(open_about_land)
                    .before(layout_virtual_lists)
                    .run_if(any_with_component::<AboutLandState>),
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
                    .after(layout_virtual_lists)
                    .run_if(any_with_component::<AboutLandState>),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The About Land floater's stable [`crate::floater::Floater::id`], the key
/// [`open_about_land`] looks the panel up by.
const ABOUT_LAND_FLOATER_ID: &str = "about-land";

/// The about land floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn about_land_floater_spec() -> FloaterSpec {
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
    }
}

/// Build one window's content: the tab container and every tab, returning the
/// handles the update passes write through.
///
/// Called once per window, as it is spawned — a keyed instance's content is
/// built into the window it belongs to, not deferred to a first open that no
/// longer exists ([`KeyedFloaters`]).
fn build_land_content(commands: &mut Commands, handle: FloaterHandle) -> AboutLandUi {
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
        commands,
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
    fill_tab_container(commands, TabPlacement::BlockStart, &tabs);
    let panel = |index: usize| tabs.panels.get(index).copied().unwrap_or(handle.content);

    let general_handles = build_general_tab(commands, panel(0));
    let covenant_handles = build_covenant_tab(commands, panel(1));
    let object_handles = build_objects_tab(commands, panel(2));
    let options_handles = build_options_tab(commands, panel(3));
    let media_handles = build_media_tab(commands, panel(4));
    let sound_handles = build_sound_tab(commands, panel(5));
    let access_handles = build_access_tab(commands, panel(6));
    build_experiences_tab(commands, panel(7));
    let environment_handles = build_environment_tab(commands, panel(8));

    AboutLandUi {
        title_text: handle.title_text,
        general_handles,
        covenant_handles,
        object_handles,
        options_handles,
        media_handles,
        sound_handles,
        access_handles,
        environment_handles,
    }
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

/// Build the Environment tab: the parcel record's two read-only facts, then
/// the shared land-environment panel that publishes to this parcel.
fn build_environment_tab(commands: &mut Commands, panel: Entity) -> EnvironmentHandles {
    let mut handles = EnvironmentHandles::default();
    let override_row = spawn_labeled_row(commands, panel, "about-land-env-override");
    handles.override_allowed = Some(spawn_value_node(commands, override_row));
    let version_row = spawn_labeled_row(commands, panel, "about-land-env-version");
    handles.version = Some(spawn_value_node(commands, version_row));
    handles.panel = Some(spawn_land_environment_panel(
        commands,
        panel,
        LandPanelKind::Parcel,
        ENV_TAB_INDEX,
    ));
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

/// The [`FloaterKey`] of the window showing `parcel`: the circuit and the
/// region-local id, so the same local id on two circuits is two windows.
fn parcel_key(parcel: ScopedParcelId) -> FloaterKey {
    FloaterKey::subject(&format!("{}/{}", parcel.circuit(), parcel.id().0))
}

/// The provisional [`FloaterKey`] of a window opened on a **point** — a land-pie
/// click — before the simulator says which parcel that point is in.
///
/// Re-keyed to [`parcel_key`] on the binding reply (see
/// [`bind_point_window`]); two clicks in one parcel therefore start as two
/// windows and fold into one.
fn point_key(x: f32, y: f32) -> FloaterKey {
    FloaterKey::subject(&format!("point/{x:.1}/{y:.1}"))
}

/// Open a window on the requested parcel — this parcel's window if it is
/// already up — and request its tab data.
#[expect(
    clippy::too_many_arguments,
    reason = "the open reads the identity / parcel model to resolve the subject, spawns or \
              reuses the subject's window, and fires every tab's fetch"
)]
fn open_about_land(
    mut requests: MessageReader<OpenAboutLand>,
    mut windows: KeyedFloaters,
    mut lands: Query<(&mut AboutLandState, &mut AboutLandDirty)>,
    mut sequence: ResMut<LandSequence>,
    mut tallies: ResMut<OwnerTallyQueue>,
    identity: Res<SlIdentity>,
    parcels: Query<&SlParcel>,
    regions: Query<&Children, With<SlCurrentRegion>>,
    agent_parcel: Res<SlAgentParcel>,
    mut spawner: Commands,
    mut commands: MessageWriter<SlCommand>,
) {
    for request in requests.read().copied().collect::<Vec<_>>() {
        // What the window will show, and what it is keyed by. A point open
        // knows neither yet — it opens under a provisional key and is re-keyed
        // when the reply says which parcel the click landed in.
        let bound = match request.subject {
            AboutLandSubject::CurrentParcel(local_id) => find_parcel(&parcels, &regions, local_id)
                .cloned()
                .or_else(|| {
                    agent_parcel
                        .current
                        .as_ref()
                        .filter(|parcel| parcel.local_id == local_id)
                        .cloned()
                }),
            AboutLandSubject::AtPoint { .. } => None,
        };
        let key = match request.subject {
            AboutLandSubject::CurrentParcel(local_id) => identity
                .circuit_id
                .map(|circuit| parcel_key(ScopedParcelId::new(circuit, local_id))),
            AboutLandSubject::AtPoint { x, y } => Some(point_key(x, y)),
        };
        let Some(key) = key else {
            // No circuit: nothing this window could ask about a parcel would
            // reach a simulator.
            continue;
        };
        let opened = windows.open(about_land_floater_spec(), key);
        let window = opened.root();
        if let KeyedFloaterOpen::Spawned(handle) = opened {
            let ui = build_land_content(&mut spawner, handle);
            spawner
                .entity(handle.title_text)
                .insert(Translated::new("about-land-title"));
            // Seeded here rather than after the insert: the components only
            // reach the world when this frame's commands flush, so a window
            // spawned now is not queryable yet.
            let mut state = AboutLandState::default();
            let mut dirty = AboutLandDirty::default();
            start_land_open(
                window,
                &mut state,
                &mut dirty,
                request,
                bound,
                &identity,
                &mut sequence,
                &mut tallies,
                &mut commands,
            );
            spawner.entity(handle.root).insert((
                state,
                dirty,
                OwnersView::default(),
                AllowView::default(),
                BanView::default(),
                ui,
            ));
            continue;
        }
        let Ok((mut state, mut dirty)) = lands.get_mut(window) else {
            continue;
        };
        start_land_open(
            window,
            &mut state,
            &mut dirty,
            request,
            bound,
            &identity,
            &mut sequence,
            &mut tallies,
            &mut commands,
        );
    }
}

/// Bind (or re-bind) one window to an open request and fire its fetches.
///
/// A window already showing this parcel keeps everything it has — the only
/// thing an open changes is the **mode**: asking for the editable view of a
/// parcel whose read-only window is up (or the other way round) re-gates its
/// controls rather than leaving the resident in a window that will not let them
/// do what they asked for.
#[expect(
    clippy::too_many_arguments,
    reason = "the shared open path threads the window, its state and dirty flags, the request, \
              the resolved parcel, the identity, and the sequence / tally books"
)]
fn start_land_open(
    window: Entity,
    state: &mut AboutLandState,
    dirty: &mut AboutLandDirty,
    request: OpenAboutLand,
    bound: Option<ParcelInfo>,
    identity: &SlIdentity,
    sequence: &mut LandSequence,
    tallies: &mut OwnerTallyQueue,
    commands: &mut MessageWriter<SlCommand>,
) {
    let already = state.target.is_some() && state.target == bound.as_ref().map(|p| p.local_id);
    if already {
        // The same parcel's window, re-opened: keep its data and its pending
        // edits, and only re-gate for the requested mode.
        if state.read_only != request.read_only {
            state.read_only = request.read_only;
            if let Some(parcel) = state.parcel.clone() {
                state.rebind_rights(&parcel, identity);
            }
            dirty.controls = true;
            dirty.editable_values = true;
        }
        return;
    }
    state.reset(request.read_only);
    match request.subject {
        AboutLandSubject::CurrentParcel(local_id) => {
            if let Some(parcel) = bound {
                state.bind(parcel, identity);
                if let Some(scoped) = state.scoped(identity) {
                    request_tab_data(state, window, scoped, tallies, commands);
                }
            } else {
                // No local copy — ask the sim for it by id and bind on the reply.
                state.target = Some(local_id);
                if let Some(scoped) = state.scoped(identity) {
                    let sequence_id = sequence.next();
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
            let sequence_id = sequence.next();
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
}

/// Request a bound parcel's per-parcel tab data (owners, dwell, access lists).
///
/// Empties the access lists first: their replies are accumulated by
/// [`merge_access_reply`], so each fresh request has to start from nothing or
/// entries the grid has since dropped would survive.
fn request_tab_data(
    state: &mut AboutLandState,
    window: Entity,
    scoped: ScopedParcelId,
    tallies: &mut OwnerTallyQueue,
    commands: &mut MessageWriter<SlCommand>,
) {
    state.clear_access_lists();
    // The object-owner tally is asked for through the queue: its reply names no
    // parcel, so only one window may have one outstanding
    // (`OwnerTallyQueue`).
    tallies.ask(window, scoped);
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

/// Fold parcel / covenant / dwell / owner / access / media replies into every
/// window's model.
///
/// A frame's events are collected once and replayed per window: a
/// [`MessageReader`] is consumed by the first pass over it, so with two windows
/// open the second would see nothing.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the event stream, every \
              window's model, the identity, the sequence / tally books, the keying outputs and \
              the command writer"
)]
fn ingest_about_land_events(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(Entity, &mut AboutLandState, &mut AboutLandDirty)>,
    mut floaters: Query<&mut Floater>,
    mut tallies: ResMut<OwnerTallyQueue>,
    identity: Res<SlIdentity>,
    agent_parcel: Res<SlAgentParcel>,
    mut closes: MessageWriter<FloaterCommand>,
    mut commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    // Which parcels already have a window, so a point open that lands on one of
    // them folds into it instead of becoming a second window on one parcel.
    let bound: Vec<(Entity, RegionLocalParcelId)> = windows
        .iter()
        .filter_map(|(window, state, _dirty)| Some((window, state.target?)))
        .collect();
    for (window, mut state, mut dirty) in &mut windows {
        // Process while a subject is bound, or a point-open is awaiting its reply.
        if state.target.is_none() && state.pending_sequence.is_none() {
            continue;
        }
        for event in &frame {
            match &event.0 {
                SlSessionEvent::ParcelProperties(parcel)
                    if state.pending_sequence == Some(parcel.sequence_id) =>
                {
                    // The awaited point / id resolve: bind this parcel as the
                    // subject and fetch the rest of its tab data.
                    state.bind((**parcel).clone(), &identity);
                    let folded = bind_point_window(
                        window,
                        parcel.local_id,
                        &bound,
                        &identity,
                        &mut floaters,
                        &mut closes,
                    );
                    if folded {
                        // This click's parcel already has a window; that one
                        // keeps the subject and this one is closing.
                        tallies.forget(window);
                        break;
                    }
                    if let Some(scoped) = state.scoped(&identity) {
                        request_tab_data(&mut state, window, scoped, &mut tallies, &mut commands);
                    }
                    dirty.mark_all();
                }
                SlSessionEvent::ParcelProperties(parcel)
                    if Some(parcel.local_id) == state.target =>
                {
                    state.parcel = Some((**parcel).clone());
                    // Seeds on the first record for this subject, merges on every
                    // one after it. Without the merge the draft stayed as it was at
                    // open and **Apply** re-asserted all eighteen fields, reverting
                    // whatever another resident changed in between
                    // ([[viewer-floaters-never-reread-after-a-push]]).
                    state.seed_draft();
                    if state.merge_parcel(parcel) {
                        dirty.seed_fields = FieldSeed::Unedited;
                    }
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
                // The tally names no parcel, so it belongs to the window whose
                // request is outstanding (`OwnerTallyQueue`).
                SlSessionEvent::ParcelObjectOwners { owners } if tallies.owns_reply(window) => {
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
                    // One request is answered by **one or more** packets, each
                    // carrying a slice of the list — so a packet is folded in, not
                    // swapped for the list. The accumulator was emptied when the
                    // list was requested (`request_tab_data`).
                    let changed = {
                        // Deref the `Mut` once: borrowing two fields through it
                        // separately would be two whole-component borrows.
                        let land = &mut *state;
                        let (list, revision) = match scope {
                            ParcelAccessScope::Access => {
                                (&mut land.access_allow, &mut land.allow_revision)
                            }
                            ParcelAccessScope::Ban => {
                                (&mut land.access_ban, &mut land.ban_revision)
                            }
                        };
                        if merge_access_reply(list, entries) {
                            *revision = revision.wrapping_add(1);
                            true
                        } else {
                            false
                        }
                    };
                    if changed {
                        request_names_for_access(&state, &mut commands);
                    }
                }
                // A media push names no parcel either, but it is always about
                // the parcel the agent is standing in — so only that parcel's
                // window takes it, rather than every window showing the
                // neighbours' media.
                SlSessionEvent::ParcelMediaUpdate(media)
                    if agent_parcel
                        .current
                        .as_ref()
                        .is_some_and(|parcel| Some(parcel.local_id) == state.target) =>
                {
                    state.media = Some(media.clone());
                    dirty.editable_values = true;
                }
                // The covenant is estate-scoped, so every window in the estate
                // shows the same one.
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

/// Settle a point-opened window onto the parcel its click landed in: fold it
/// into that parcel's existing window if there is one, and otherwise re-key it
/// from its provisional [`point_key`] to the parcel's own.
///
/// Returns whether this window folded (and is therefore closing).
fn bind_point_window(
    window: Entity,
    local_id: RegionLocalParcelId,
    bound: &[(Entity, RegionLocalParcelId)],
    identity: &SlIdentity,
    floaters: &mut Query<&mut Floater>,
    closes: &mut MessageWriter<FloaterCommand>,
) -> bool {
    let Some(circuit) = identity.circuit_id else {
        return false;
    };
    let key = parcel_key(ScopedParcelId::new(circuit, local_id));
    let Ok(mut floater) = floaters.get_mut(window) else {
        return false;
    };
    if floater.key() == Some(&key) {
        // Opened on a known parcel: it was keyed by it from the start.
        return false;
    }
    if let Some((existing, _parcel)) = bound
        .iter()
        .find(|(other, parcel)| *other != window && *parcel == local_id)
    {
        // Two clicks in one parcel: the first window keeps it.
        closes.write(FloaterCommand {
            floater: *existing,
            op: FloaterOp::BringToFront,
        });
        closes.write(FloaterCommand {
            floater: window,
            op: FloaterOp::Close,
        });
        return true;
    }
    floater.rekey(key);
    false
}

/// Send the outstanding object-owner tally request, one window at a time.
///
/// The reply names no parcel (`OwnerTallyQueue`), so a second request in flight
/// would make it ambiguous; a window that goes unanswered releases its turn
/// after [`OWNER_TALLY_TIMEOUT_SECONDS`].
fn drive_owner_tallies(
    mut tallies: ResMut<OwnerTallyQueue>,
    windows: Query<(), With<AboutLandState>>,
    time: Res<Time>,
    mut commands: MessageWriter<SlCommand>,
) {
    let now = time.elapsed_secs_f64();
    if let Some((asking, deadline)) = tallies.asking {
        if windows.contains(asking) && now < deadline {
            return;
        }
        tallies.asking = None;
    }
    while let Some((window, parcel)) = tallies.waiting.pop_front() {
        if !windows.contains(window) {
            continue;
        }
        commands.write(SlCommand(Command::RequestParcelObjectOwners {
            local_id: parcel,
        }));
        tallies.asking = Some((window, now + OWNER_TALLY_TIMEOUT_SECONDS));
        return;
    }
}

/// Re-run the name-dependent value updates when the avatar / group name caches
/// change, so an owner / resident shown as a UUID resolves to a name once its
/// reply lands (the tables re-sync themselves on the same signal).
fn refresh_on_names(
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut windows: Query<&mut AboutLandDirty>,
) {
    if !avatars.is_changed() && !groups.is_changed() {
        return;
    }
    for mut dirty in &mut windows {
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

/// Seed the edit fields' text from the draft.
///
/// [`FieldSeed::All`] on a fresh subject, [`FieldSeed::Unedited`] after a record
/// arrived and moved the draft under the resident. In the second mode a field is
/// only rewritten if its text still equals what was last seeded into it, so
/// typing that has not been applied yet is never overwritten by somebody else's
/// save landing mid-sentence.
/// Seed one window's edit fields' text from its draft.
///
/// [`FieldSeed::All`] on a fresh subject, [`FieldSeed::Unedited`] after a record
/// arrived and moved the draft under the resident. In the second mode a field is
/// only rewritten if its text still equals what was last seeded into it, so
/// typing that has not been applied yet is never overwritten by somebody else's
/// save landing mid-sentence.
fn seed_edit_fields(
    mut windows: Query<(&mut AboutLandDirty, &AboutLandUi, &mut AboutLandState)>,
    mut fields: Query<&mut EditableText>,
) {
    for (mut dirty, ui, mut state) in &mut windows {
        let mode = dirty.seed_fields;
        if mode == FieldSeed::None {
            continue;
        }
        dirty.seed_fields = FieldSeed::None;
        let wanted = FieldText::from_draft(&state.draft);
        // What the previous pass wrote. In `Unedited` mode a field that no longer
        // reads as it was written has been typed in, and is left alone; in `All`
        // mode the subject itself changed, so the old text means nothing.
        let shown = match mode {
            FieldSeed::None | FieldSeed::All => None,
            FieldSeed::Unedited => state.shown_fields.clone(),
        };
        // What this pass leaves on screen: the value it wrote, or the resident's
        // own text where it declined to write. Recording the latter is what stops
        // the *next* push from reading the typing as "changed back".
        let mut left = wanted.clone();
        // Each row carries its own slot in `left`, so there is no index to keep in
        // step with the field order.
        let rows: [(Option<Entity>, &str, Option<&str>, &mut String); 6] = [
            (
                ui.general_handles.name_field,
                &wanted.name,
                shown.as_ref().map(|shown| shown.name.as_str()),
                &mut left.name,
            ),
            (
                ui.general_handles.desc_field,
                &wanted.description,
                shown.as_ref().map(|shown| shown.description.as_str()),
                &mut left.description,
            ),
            (
                ui.media_handles.url_field,
                &wanted.media_url,
                shown.as_ref().map(|shown| shown.media_url.as_str()),
                &mut left.media_url,
            ),
            (
                ui.sound_handles.music_field,
                &wanted.music_url,
                shown.as_ref().map(|shown| shown.music_url.as_str()),
                &mut left.music_url,
            ),
            (
                ui.access_handles.pass_price_field,
                &wanted.pass_price,
                shown.as_ref().map(|shown| shown.pass_price.as_str()),
                &mut left.pass_price,
            ),
            (
                ui.access_handles.pass_hours_field,
                &wanted.pass_hours,
                shown.as_ref().map(|shown| shown.pass_hours.as_str()),
                &mut left.pass_hours,
            ),
        ];
        for (field, want, previous, slot) in rows {
            if let Some(previous) = previous
                && !field_reads(&fields, field, previous)
            {
                if let Some(text) = field_text(&fields, field) {
                    *slot = text;
                }
                continue;
            }
            set_field_text(&mut fields, field, want);
        }
        state.shown_fields = Some(left);
    }
}

/// A field's current text, if it exists.
fn field_text(fields: &Query<&mut EditableText>, field: Option<Entity>) -> Option<String> {
    fields
        .get(field?)
        .ok()
        .map(|editable| editable.value().to_string())
}

/// Whether `field`'s current text is exactly `value`.
///
/// A missing field reads as matching, so a form built without it is seeded
/// rather than skipped.
fn field_reads(fields: &Query<&mut EditableText>, field: Option<Entity>, value: &str) -> bool {
    field_text(fields, field).is_none_or(|text| text == value)
}

/// Toggle write buttons' visibility and every editable control's
/// [`InteractionDisabled`] to follow the agent's rights.
/// Toggle each window's write buttons' visibility and every editable control's
/// [`InteractionDisabled`] to follow the agent's rights **in that window**.
///
/// The controls are found by walking up from each one to the window it lives in
/// ([`host_floater`]), rather than by sweeping every control in the viewer: two
/// About Land windows can disagree about `can_edit` — one on a parcel this
/// resident owns, one on a neighbour's — and a sweep would give both the last
/// window's answer.
fn update_control_enable(
    mut windows: Query<(Entity, &mut AboutLandDirty, &AboutLandState)>,
    mut write_buttons: Query<(Entity, &mut Visibility), With<WriteButton>>,
    gated: Query<(Entity, &EditGate)>,
    disabled: Query<(), With<InteractionDisabled>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut commands: Commands,
) {
    // The windows repainting this frame, and what each one allows.
    let mut repainting: Vec<(Entity, bool)> = Vec::new();
    for (window, mut dirty, state) in &mut windows {
        if !dirty.controls {
            continue;
        }
        dirty.controls = false;
        repainting.push((window, state.can_edit));
    }
    if repainting.is_empty() {
        return;
    }
    let can_edit = |entity: Entity| {
        let host = host_floater(entity, &parents, &floaters)?;
        repainting
            .iter()
            .find_map(|(window, can_edit)| (*window == host).then_some(*can_edit))
    };
    for (entity, mut visibility) in &mut write_buttons {
        let Some(can_edit) = can_edit(entity) else {
            continue;
        };
        let want = if can_edit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    for (entity, gate) in &gated {
        let Some(can_edit) = can_edit(entity) else {
            continue;
        };
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
/// Refresh each window's General tab read-only values in place.
fn update_general_tab(
    mut windows: Query<(&mut AboutLandDirty, &AboutLandUi, &AboutLandState)>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut links: Query<&mut NameLink>,
    mut commands: Commands,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.general_values {
            continue;
        }
        dirty.general_values = false;
        let texts = &mut texts;
        let handles = &ui.general_handles;
        let region = regions.iter().next().map(|region| &region.0);
        let Some(parcel) = &state.parcel else {
            continue;
        };
        // The title carries the parcel's name (a plain string, not a Fluent
        // key): two windows on two parcels are otherwise identical strips.
        if !parcel.name.is_empty()
            && let Ok((mut title, _color)) = texts.get_mut(ui.title_text)
        {
            parcel.name.clone_into(&mut title.0);
            commands.entity(ui.title_text).remove::<Translated>();
        }
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
}

/// Refresh the Options / Media / Sound controls in place: checkbox glyphs (with
/// their enabled greying), combos, texture ids, media read-outs, landing point.
/// Refresh each window's Options / Media / Sound controls in place: checkbox
/// glyphs (with their enabled greying), combos, texture ids, media read-outs,
/// landing point.
///
/// The checkboxes are matched to their window by walking up from each one
/// ([`host_floater`]) — see [`update_control_enable`] for why a sweep is wrong.
fn update_editable_tab(
    mut windows: Query<(Entity, &mut AboutLandDirty, &AboutLandUi, &AboutLandState)>,
    checks: Query<(Entity, &AboutLandCheck)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut combos: Query<&mut ComboSelection>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (window, mut dirty, ui, state) in &mut windows {
        if !dirty.editable_values {
            continue;
        }
        dirty.editable_values = false;
        let texts = &mut texts;
        let can_edit = state.can_edit;
        for (entity, check) in &checks {
            if host_floater(entity, &parents, &floaters) != Some(window) {
                continue;
            }
            let on = check.kind.checked(state);
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
}

/// Refresh the Covenant tab's values in place.
/// Refresh each window's Covenant tab values in place.
fn update_covenant_tab(
    mut windows: Query<(&mut AboutLandDirty, &AboutLandUi, &AboutLandState)>,
    avatars: Res<AvatarState>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.covenant_values {
            continue;
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
                    |agent| avatars.label_text(agent),
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
}

/// Refresh the Objects tab's counts in place.
/// Refresh each window's Objects tab read-only values in place.
fn update_objects_tab(
    mut windows: Query<(&mut AboutLandDirty, &AboutLandUi, &AboutLandState)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.objects_values {
            continue;
        }
        dirty.objects_values = false;
        let texts = &mut texts;
        let handles = &ui.object_handles;
        let Some(parcel) = &state.parcel else {
            continue;
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
}

/// Refresh each window's Environment tab header — the two facts the *parcel
/// record* carries, which the land-environment panel below has no access to.
fn update_environment_tab(
    mut windows: Query<(&mut AboutLandDirty, &AboutLandUi, &AboutLandState)>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (mut dirty, ui, state) in &mut windows {
        if !dirty.environment_values {
            continue;
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
    }
}

/// Keep each window's Environment tab pointed at its parcel.
///
/// A window whose region the agent has left, or whose parcel has not resolved
/// yet, hands the panel a subject it refuses to publish from — which is the
/// same freeze the rest of this floater takes, spelled where the panel can act
/// on it.
fn aim_environment_panel(
    windows: Query<(&AboutLandState, &AboutLandUi)>,
    identity: Res<SlIdentity>,
    mut panels: Query<&mut LandEnvironmentSubject>,
) {
    for (state, ui) in &windows {
        let Some(entity) = ui.environment_handles.panel else {
            continue;
        };
        let Ok(mut subject) = panels.get_mut(entity) else {
            continue;
        };
        let parcel = state.parcel.as_ref();
        let wanted = LandEnvironmentSubject {
            parcel_id: parcel.map(|parcel| parcel.local_id.0),
            live: state.bound_circuit.is_some() && state.bound_circuit == identity.circuit_id,
            editable: state.can_edit,
            allow_override: parcel.is_some_and(|parcel| parcel.region_allow_environment_override),
            area: parcel.map_or(LandArea::ZERO, |parcel| parcel.area),
        };
        if *subject != wanted {
            *subject = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Table view sync + populate + bind.
// ---------------------------------------------------------------------------

/// Rebuild the object-owners view (resolving names) when the tally or the name
/// caches change, and keep the virtual list's item count in step.
/// Rebuild each window's object-owners view (resolving names) when its tally or
/// the name caches change, and keep its virtual list's item count in step.
fn sync_owners_view(
    mut windows: Query<(&AboutLandState, &mut OwnersView, &AboutLandUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, mut view, ui) in &mut windows {
        if view.built == state.owners_revision && !avatars.is_changed() && !groups.is_changed() {
            continue;
        }
        view.built = state.owners_revision;
        view.rows = state
            .owners
            .iter()
            .map(|owner| {
                let (kind_key, name) = match owner.owner {
                    OwnerKey::Agent(agent) => ("about-land-owner-agent", avatars.label_text(agent)),
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
}

/// Rebuild the allow-list view.
/// Rebuild each window's allow-list view.
fn sync_allow_view(
    mut windows: Query<(&AboutLandState, &mut AllowView, &AboutLandUi)>,
    avatars: Res<AvatarState>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            state.allow_revision,
            &state.access_allow,
            &mut view.rows,
            &mut view.built,
            ui.access_handles.allow_viewport,
            &avatars,
            avatars.is_changed(),
            &translator,
            &mut lists,
        );
    }
}

/// Rebuild the ban-list view.
/// Rebuild each window's ban-list view.
fn sync_ban_view(
    mut windows: Query<(&AboutLandState, &mut BanView, &AboutLandUi)>,
    avatars: Res<AvatarState>,
    translator: Translator,
    mut lists: Query<&mut VirtualList>,
) {
    for (state, view, ui) in &mut windows {
        let view = view.into_inner();
        sync_access_view(
            state.ban_revision,
            &state.access_ban,
            &mut view.rows,
            &mut view.built,
            ui.access_handles.ban_viewport,
            &avatars,
            avatars.is_changed(),
            &translator,
            &mut lists,
        );
    }
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
    lists: &mut Query<&mut VirtualList>,
) {
    if *built == revision && !avatars_changed {
        return;
    }
    *built = revision;
    *rows = entries
        .iter()
        .map(|entry| AccessRowData {
            id: entry.id,
            name: avatars.label_text(AgentKey::from(entry.id)),
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
/// Build each newly-pooled owner row's cells once, in whichever window's list
/// it was pooled into.
fn populate_owner_rows(
    mut commands: Commands,
    windows: Query<&AboutLandUi>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        for ui in &windows {
            let (Some(viewport), Some(table)) = (
                ui.object_handles.owners_viewport,
                ui.object_handles.owners_table,
            ) else {
                continue;
            };
            if child_of.parent() != viewport {
                continue;
            }
            spawn_table_row(&mut commands, row_entity, table, &OWNERS_TABLE);
            break;
        }
    }
}

/// Bind each pooled owner row to its [`OwnerRowData`].
/// Bind each pooled owner row to its window's [`OwnerRowData`].
fn bind_owner_rows(
    windows: Query<(Ref<OwnersView>, &AboutLandUi)>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (view, ui) in &windows {
        let Some(viewport) = ui.object_handles.owners_viewport else {
            continue;
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
}

/// Build each newly-pooled access row's cells + Remove button once.
/// Build each newly-pooled access row's cells + Remove button once, in
/// whichever window's list it was pooled into.
fn populate_access_rows(
    mut commands: Commands,
    windows: Query<&AboutLandUi>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    for (row_entity, child_of) in &new_rows {
        let parent = child_of.parent();
        for ui in &windows {
            let (table, spec, scope) = if Some(parent) == ui.access_handles.allow_viewport {
                (
                    ui.access_handles.allow_table,
                    &ALLOW_TABLE,
                    AccessScope::Allow,
                )
            } else if Some(parent) == ui.access_handles.ban_viewport {
                (ui.access_handles.ban_table, &BAN_TABLE, AccessScope::Ban)
            } else {
                continue;
            };
            let Some(table) = table else {
                continue;
            };
            let cells = spawn_table_row(&mut commands, row_entity, table, spec);
            if let Some(custom) = cells.cell(2) {
                spawn_remove_button(&mut commands, custom, scope, row_entity);
            }
            break;
        }
    }
}

/// Every window's access-list views, state and handles — the row binder's read
/// of the windows, named because the tuple is past the point of reading well
/// inline.
type AccessBindWindows<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        Ref<'static, AllowView>,
        Ref<'static, BanView>,
        Ref<'static, AboutLandState>,
        &'static AboutLandUi,
    ),
>;

/// Bind each pooled access row to its window's [`AccessRowData`].
fn bind_access_rows(
    windows: AccessBindWindows,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &crate::ui_table::TableRowCells)>,
    removes: Query<Entity, With<RemoveAccessButton>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for (window, allow, ban, state, ui) in &windows {
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
        // Show each Remove button only when the agent may edit **this** parcel
        // (a parked row hides the whole row, so this only ever reveals buttons
        // on bound rows).
        let want = if state.can_edit {
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

/// Toggle an editable checkbox.
/// Toggle an editable checkbox, in the window it was pressed in.
fn on_about_land_check(
    press: On<Pointer<Press>>,
    checks: Query<&AboutLandCheck>,
    mut windows: Query<&mut AboutLandState>,
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
    if !state.can_edit || !check.kind.editable() {
        return;
    }
    check.kind.toggle(&mut state.draft);
    let on = check.kind.checked(&state);
    set_check_visual(&mut texts, check, on, true);
}

/// Dispatch a floater button press, in the window it was pressed in.
#[expect(
    clippy::too_many_arguments,
    reason = "the dispatcher fans out to every button kind, reading the pressed window, the edit \
              fields and the agent position to route each"
)]
fn on_about_land_action(
    press: On<Pointer<Press>>,
    actions: Query<&AboutLandAction>,
    mut windows: Query<(&mut AboutLandState, &AboutLandUi)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    identity: Res<SlIdentity>,
    agent_position: Res<AgentRegionPosition>,
    mut tallies: ResMut<OwnerTallyQueue>,
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
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = windows.get_mut(window) else {
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
        AboutLandAction::Apply => apply_draft(&mut state, ui, &fields, scoped, &mut sl_commands),
        AboutLandAction::RefreshOwners => tallies.ask(window, scoped),
        AboutLandAction::PickSnapshot => {
            texture_pickers.write(OpenTexturePicker {
                requester: press.entity,
                field: Box::from("about-land-snapshot"),
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
                field: Box::from("about-land-media-texture"),
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
        //
        // The claim is what the pick comes back to (`pending_pick`): the picker
        // echoes a tag, not a window.
        AboutLandAction::AddAllowed => {
            state.pending_pick = Some(AccessScope::Allow);
            pickers.write(OpenAvatarPicker::many("about-land-allow"));
        }
        AboutLandAction::AddBanned => {
            state.pending_pick = Some(AccessScope::Ban);
            pickers.write(OpenAvatarPicker::many("about-land-ban"));
        }
    }
}

/// Resolve and act on a per-row access Remove press, in the window it was
/// pressed in.
#[expect(
    clippy::too_many_arguments,
    reason = "the remove observer reads the pressed button, its row, its window's views and \
              state, the identity, and the command writer"
)]
fn on_remove_access(
    press: On<Pointer<Press>>,
    buttons: Query<&RemoveAccessButton>,
    rows: Query<&VirtualRow>,
    mut windows: Query<(&AllowView, &BanView, &mut AboutLandState)>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    identity: Res<SlIdentity>,
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
    let Ok((allow, ban, mut state)) = windows.get_mut(window) else {
        return;
    };
    if !state.can_edit {
        return;
    }
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
/// Fold a combo pick into the draft of the window whose combo it was.
fn apply_combo_edits(
    mut changed: MessageReader<ComboChanged>,
    mut windows: Query<(&AboutLandUi, &mut AboutLandState)>,
) {
    let frame: Vec<ComboChanged> = changed.read().copied().collect();
    if frame.is_empty() {
        return;
    }
    for (ui, mut state) in &mut windows {
        for event in &frame {
            if Some(event.combo) == ui.options_handles.category_combo {
                state.draft.category =
                    ParcelCategory::from_u8(u8::try_from(event.active).unwrap_or(0));
            } else if Some(event.combo) == ui.options_handles.landing_combo {
                state.draft.landing_type = u8::try_from(event.active).unwrap_or(0);
            }
        }
    }
}

/// Fold a texture pick into the draft and its button label.
/// Fold a texture pick into the draft and button label of the window whose
/// swatch asked for it.
fn apply_texture_edits(
    mut picked: MessageReader<TexturePicked>,
    mut windows: Query<(&AboutLandUi, &mut AboutLandState)>,
    swatches: Query<&SwatchTexture>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    for event in picked.read() {
        if !event.final_pick {
            continue;
        }
        let Ok(swatch) = swatches.get(event.requester) else {
            continue;
        };
        // The pick belongs to the window the pressed swatch lives in — the
        // picker echoes the button, and the button names its window.
        let Some(window) = host_floater(event.requester, &parents, &floaters) else {
            continue;
        };
        let Ok((ui, mut state)) = windows.get_mut(window) else {
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
/// Fold the avatar picks into the allow / ban list of the window that asked,
/// and commit them.
///
/// The picker echoes a `&'static str` tag rather than an entity, so the window
/// is the one holding a matching claim ([`AboutLandState::pending_pick`]) — at
/// most one, because the picker itself is one window per tag.
fn apply_avatar_picks(
    mut picked: MessageReader<AvatarPicked>,
    mut windows: Query<(Entity, &mut AboutLandState)>,
    identity: Res<SlIdentity>,
    mut commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<AvatarPicked> = picked.read().cloned().collect();
    if frame.is_empty() {
        return;
    }
    for event in &frame {
        let scope = match event.requester {
            "about-land-allow" => AccessScope::Allow,
            "about-land-ban" => AccessScope::Ban,
            _other => continue,
        };
        for (_window, mut state) in &mut windows {
            if state.pending_pick != Some(scope) {
                continue;
            }
            state.pending_pick = None;
            let Some(scoped) = state.scoped(&identity) else {
                continue;
            };
            for chosen in &event.picks {
                add_access_entry(&mut state, scope, chosen.agent, scoped, &mut commands);
            }
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

/// Fold one `ParcelAccessListReply` packet into the accumulated list, returning
/// whether it changed anything.
///
/// A parcel's allow / ban list is answered as **one or more** packets, each
/// carrying a slice of the entries, so a packet is a *union* rather than a
/// replacement. The reference accumulates the same way — `unpackAccessEntries`
/// does `(*list)[entry.mID] = entry;` into a map it never clears
/// (`llparcel.cpp`) — and it likewise ignores the wire `SequenceID`, which
/// `processParcelAccessListReply` reads into a local marked `//ignored`
/// (`llviewerparcelmgr.cpp`). The accumulator is emptied when the list is
/// *requested*, in [`request_tab_data`], not when a packet arrives.
///
/// Replacing per packet is what made banning one more resident on a list long
/// enough to span packets silently drop every entry outside the last packet:
/// [`send_access_list`] then uploads the truncated list as the parcel's whole
/// list.
///
/// A repeated id updates its existing entry in place, mirroring the reference's
/// map insert; arrival order is otherwise preserved.
fn merge_access_reply(existing: &mut Vec<ParcelAccessEntry>, reply: &[ParcelAccessEntry]) -> bool {
    let mut changed = false;
    for entry in reply {
        if let Some(held) = existing.iter_mut().find(|held| held.id == entry.id) {
            if *held != *entry {
                *held = *entry;
                changed = true;
            }
        } else {
            existing.push(*entry);
            changed = true;
        }
    }
    changed
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

#[cfg(test)]
mod tests {
    use super::{AboutLandState, ParcelUpdate, merge_access_reply};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{ParcelAccessEntry, ParcelAccessFlags, Uuid};

    /// A ban-list entry for the nth synthetic agent.
    fn entry(n: u8) -> ParcelAccessEntry {
        ParcelAccessEntry {
            id: Uuid::from_bytes([n; 16]),
            time: 0,
            flags: ParcelAccessFlags::NONE,
        }
    }

    /// The ids of a list, in order.
    fn ids(list: &[ParcelAccessEntry]) -> Vec<Uuid> {
        list.iter().map(|entry| entry.id).collect()
    }

    /// The regression this function exists for: one request is answered by
    /// several packets, and the later ones must not replace the earlier.
    #[test]
    fn successive_packets_union_rather_than_replace() {
        let mut list = Vec::new();
        assert!(merge_access_reply(&mut list, &[entry(1), entry(2)]));
        assert!(merge_access_reply(&mut list, &[entry(3)]));
        assert_eq!(ids(&list), vec![entry(1).id, entry(2).id, entry(3).id]);
    }

    /// A repeated id updates its entry in place — the reference's map insert —
    /// rather than appending a duplicate.
    #[test]
    fn a_repeated_id_updates_in_place() {
        let mut list = vec![entry(1)];
        let mut refreshed = entry(1);
        refreshed.time = 1_700_000_000;
        assert!(merge_access_reply(&mut list, &[refreshed]));
        // One entry, carrying the newer packet's payload.
        assert_eq!(list, vec![refreshed]);
    }

    /// Re-delivering an identical packet changes nothing, so the table view is
    /// not rebuilt for it.
    #[test]
    fn an_identical_packet_reports_no_change() {
        let mut list = Vec::new();
        assert!(merge_access_reply(&mut list, &[entry(1), entry(2)]));
        assert!(!merge_access_reply(&mut list, &[entry(1), entry(2)]));
        assert_eq!(ids(&list), vec![entry(1).id, entry(2).id]);
    }

    /// An empty list is answered with no entries (the sim's nil-agent sentinel
    /// is dropped in `sl-proto`), which must leave the accumulator alone.
    #[test]
    fn an_empty_packet_leaves_the_list_alone() {
        let mut list = vec![entry(1)];
        assert!(!merge_access_reply(&mut list, &[]));
        assert_eq!(ids(&list), vec![entry(1).id]);
    }

    /// A record arriving before the draft is seeded has no base to merge
    /// against, so it changes nothing and waits for `seed_draft`.
    #[test]
    fn a_record_before_the_draft_is_seeded_changes_nothing() {
        let mut state = AboutLandState::default();
        let fresh = ParcelUpdate {
            name: "Theirs".to_owned(),
            ..ParcelUpdate::default()
        };
        assert!(!state.merge_update(fresh));
        assert_eq!(state.draft, ParcelUpdate::default());
        assert!(
            state.seeded.is_none(),
            "an unseeded form must not adopt a base it never seeded from"
        );
    }

    /// The bug this floater was filed for: a record pushed because *somebody
    /// else* saved must reach the fields this resident has not edited, and must
    /// not touch the one they have.
    ///
    /// Without it **Apply** re-asserted all eighteen fields as they stood at
    /// open, reverting the other resident's change — and no grid can stop that,
    /// because a `ParcelPropertiesUpdate` carries the whole record and a
    /// simulator cannot tell a re-asserted field from an unchanged one.
    #[test]
    fn a_push_reaches_the_fields_this_resident_did_not_edit() {
        let opened = ParcelUpdate {
            name: "Before".to_owned(),
            description: "As opened".to_owned(),
            ..ParcelUpdate::default()
        };
        let mut state = AboutLandState {
            draft: opened.clone(),
            seeded: Some(opened),
            ..AboutLandState::default()
        };
        // This resident retitled the parcel but has not applied it yet.
        state.draft.name = "Mine".to_owned();

        assert!(state.merge_update(ParcelUpdate {
            name: "Before".to_owned(),
            description: "Theirs".to_owned(),
            ..ParcelUpdate::default()
        }));
        assert_eq!(state.draft.name, "Mine");
        assert_eq!(
            state.draft.description, "Theirs",
            "an Apply now carries their change forward instead of reverting it"
        );
    }

    /// Merging advances the base, so the floater settles instead of re-applying
    /// the same record forever — and the read-back the floater requests after
    /// its own **Apply** comes through this path too.
    #[test]
    fn merging_advances_the_base_so_a_repeat_is_not_a_change() {
        let opened = ParcelUpdate::default();
        let mut state = AboutLandState {
            draft: opened.clone(),
            seeded: Some(opened),
            ..AboutLandState::default()
        };
        let fresh = ParcelUpdate {
            description: "Theirs".to_owned(),
            ..ParcelUpdate::default()
        };
        assert!(state.merge_update(fresh.clone()));
        assert!(
            !state.merge_update(fresh),
            "the same record twice is one change"
        );
    }

    /// Because packets accumulate, the emptying happens at request time — so a
    /// second request cannot inherit entries the grid has since dropped.
    #[test]
    fn clearing_empties_both_lists_and_bumps_both_revisions() {
        let mut state = AboutLandState {
            access_allow: vec![entry(1)],
            access_ban: vec![entry(2)],
            ..AboutLandState::default()
        };
        let (allow, ban) = (state.allow_revision, state.ban_revision);
        state.clear_access_lists();
        assert!(state.access_allow.is_empty());
        assert!(state.access_ban.is_empty());
        assert_ne!(state.allow_revision, allow);
        assert_ne!(state.ban_revision, ban);
    }

    /// **One window per parcel** (`viewer-keyed-floater-audit`), and the one
    /// request the keying had to serialise.
    mod instances {
        use super::super::{
            AboutLandPlugin, AboutLandState, AboutLandSubject, OpenAboutLand, OwnerTallyQueue,
            parcel_key,
        };
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::ui::UiRoot;
        use crate::world_api::{AgentRegionPosition, AvatarState, GroupsModel};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            AgentKey, CircuitId, Command, RegionLocalParcelId, ScopedParcelId, SlAgentParcel,
            SlCommand, SlEvent, SlIdentity, Uuid,
        };

        /// A boxed error so tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The circuit every test's identity is on.
        const fn circuit() -> CircuitId {
            CircuitId::new(1)
        }

        /// An app with the floater manager, this module's plugin, and the world
        /// facts its systems read — no grid, no window.
        fn land_app() -> App {
            let mut app = App::new();
            let identity = SlIdentity {
                agent_id: Some(AgentKey::from(Uuid::from_u128(0xA9))),
                circuit_id: Some(circuit()),
                ..SlIdentity::default()
            };
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<crate::ui_combo::ComboChanged>()
                .add_message::<crate::world_api::OpenTexturePicker>()
                .add_message::<crate::world_api::TexturePicked>()
                .add_message::<crate::world_api::OpenAvatarPicker>()
                .add_message::<crate::world_api::AvatarPicked>()
                .insert_resource(identity)
                .init_resource::<AvatarState>()
                .init_resource::<GroupsModel>()
                .init_resource::<SlAgentParcel>()
                .init_resource::<AgentRegionPosition>()
                .init_resource::<Time>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<bevy::input_focus::InputFocus>()
                .add_plugins((FloaterPlugin, AboutLandPlugin));
            crate::i18n::install_untranslated(&mut app);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open About Land on a region-local parcel id, the way the top bar and
        /// the World menu do.
        fn open(app: &mut App, local_id: i32) {
            app.world_mut().write_message(OpenAboutLand {
                subject: AboutLandSubject::CurrentParcel(RegionLocalParcelId(local_id)),
                read_only: false,
            });
            app.update();
        }

        /// Every live About Land window.
        fn windows(app: &mut App) -> Vec<Entity> {
            app.world_mut()
                .query_filtered::<Entity, With<AboutLandState>>()
                .iter(app.world())
                .collect()
        }

        /// Two parcels are two windows, each keyed by its own scoped id.
        #[test]
        fn two_parcels_open_two_windows() -> Result<(), TestError> {
            let mut app = land_app();
            open(&mut app, 7);
            open(&mut app, 9);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second parcel reused the first window"
            );
            let world = app.world();
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|window| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            for local_id in [7_i32, 9] {
                let key = parcel_key(ScopedParcelId::new(
                    circuit(),
                    RegionLocalParcelId(local_id),
                ));
                assert!(keys.contains(&Some(&key)), "no window keyed by {local_id}");
            }
            Ok(())
        }

        /// Re-opening a parcel raises its window instead of making a second.
        #[test]
        fn reopening_a_parcel_reuses_its_window() -> Result<(), TestError> {
            let mut app = land_app();
            open(&mut app, 7);
            open(&mut app, 7);
            assert_eq!(windows(&mut app).len(), 1);
            Ok(())
        }

        /// Closing one parcel's window leaves the other open.
        #[test]
        fn closing_one_parcel_leaves_the_other() -> Result<(), TestError> {
            let mut app = land_app();
            open(&mut app, 7);
            open(&mut app, 9);
            let target = *windows(&mut app).first().ok_or("no window opened")?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: target,
                    op: FloaterOp::Close,
                });
            app.update();

            let left = windows(&mut app);
            assert_eq!(left.len(), 1);
            assert!(!left.contains(&target));
            Ok(())
        }

        /// **Only one object-owner tally is outstanding.** The reply carries
        /// nothing but the owners — no parcel, no sequence id — so a second
        /// request in flight would let one window take the other's tally.
        #[test]
        fn owner_tallies_are_serialised() -> Result<(), TestError> {
            let mut app = land_app();
            let first = app.world_mut().spawn(AboutLandState::default()).id();
            let second = app.world_mut().spawn(AboutLandState::default()).id();
            {
                let mut tallies = app.world_mut().resource_mut::<OwnerTallyQueue>();
                tallies.ask(
                    first,
                    ScopedParcelId::new(circuit(), RegionLocalParcelId(7)),
                );
                tallies.ask(
                    second,
                    ScopedParcelId::new(circuit(), RegionLocalParcelId(9)),
                );
            }
            app.update();

            let asked = app
                .world()
                .resource::<Messages<SlCommand>>()
                .iter_current_update_messages()
                .filter(|command| matches!(command.0, Command::RequestParcelObjectOwners { .. }))
                .count();
            assert_eq!(asked, 1, "both windows asked for a tally at once");
            let tallies = app.world().resource::<OwnerTallyQueue>();
            assert!(tallies.owns_reply(first), "the reply is nobody's, or wrong");
            assert!(!tallies.owns_reply(second));
            Ok(())
        }

        /// A window that closes releases its turn, so the next one asks.
        #[test]
        fn a_closed_window_releases_the_tally_turn() -> Result<(), TestError> {
            let mut app = land_app();
            let first = app.world_mut().spawn(AboutLandState::default()).id();
            let second = app.world_mut().spawn(AboutLandState::default()).id();
            {
                let mut tallies = app.world_mut().resource_mut::<OwnerTallyQueue>();
                tallies.ask(
                    first,
                    ScopedParcelId::new(circuit(), RegionLocalParcelId(7)),
                );
                tallies.ask(
                    second,
                    ScopedParcelId::new(circuit(), RegionLocalParcelId(9)),
                );
            }
            app.update();
            app.world_mut().entity_mut(first).despawn();
            app.update();

            let tallies = app.world().resource::<OwnerTallyQueue>();
            assert!(
                tallies.owns_reply(second),
                "the second window never got its turn"
            );
            Ok(())
        }
    }
}
