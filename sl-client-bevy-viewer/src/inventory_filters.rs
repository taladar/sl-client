//! The **inventory filters** floater and filter model
//! (`viewer-inventory-advanced-filters`): the non-text filters that narrow the
//! inventory tree — by **item type** (the reference's thirteen type
//! checkboxes), by **date** (since login, or newer / older than a given
//! hours + days range), and by **worn** — mirroring the reference viewer's
//! "Show Filters" finder floater (`floater_inventory_view_finder.xml`,
//! driven by `LLFloaterInventoryFinder` / `llinventoryfilter`).
//!
//! # Split: pure filter, thin UI
//!
//! The filter itself is plain data ([`ItemFilter`]) with a pure predicate
//! ([`ItemFilter::passes`]) tested in isolation; the floater is a column of
//! toggle rows, two numeric fields and a reset button that edit an
//! [`InventoryFilterState`] resource. [`crate::inventory`]'s view rebuild
//! reads the resource and — whenever the filter is active — narrows the tree
//! exactly like a text search does (matching items shown inside their
//! expanded ancestor hierarchy).
//!
//! Reference (Firestorm, read-only): `floater_inventory_view_finder.xml`,
//! `llinventoryfilter.{h,cpp}`, `llpanelmaininventory.cpp`
//! (`LLFloaterInventoryFinder`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{InventoryType, ItemInfo};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::ui::{UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;

/// The floater's [`crate::floater::FloaterSpec::id`].
const FILTERS_FLOATER_ID: &str = "inventory-filters";

/// The chrome font size, in logical pixels.
const FILTER_FONT_SIZE: f32 = 14.0;

/// The floater chrome / label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A toggle row's check glyph colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The checked / unchecked box glyphs of a toggle row.
const CHECKED_GLYPH: &str = "\u{2611}";
/// The unchecked box glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

// ---------------------------------------------------------------------------
// The pure filter model.
// ---------------------------------------------------------------------------

/// One of the reference finder's item-type checkboxes, in its display order.
/// Each groups the [`InventoryType`]s the reference's matching filter bit
/// covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TypeFilter {
    /// Animations.
    Animation,
    /// Calling cards.
    CallingCard,
    /// Clothing **and** body parts (the reference's single Clothing box
    /// covers every wearable).
    Clothing,
    /// Gestures.
    Gesture,
    /// Landmarks.
    Landmark,
    /// Materials.
    Material,
    /// Notecards.
    Notecard,
    /// Objects (attachments and mesh included).
    Object,
    /// Scripts.
    Script,
    /// Sounds.
    Sound,
    /// Textures.
    Texture,
    /// Snapshots.
    Snapshot,
    /// Environment settings.
    Settings,
}

impl TypeFilter {
    /// Every type box, in the reference finder's display order.
    pub(crate) const ALL: [Self; 13] = [
        Self::Animation,
        Self::CallingCard,
        Self::Clothing,
        Self::Gesture,
        Self::Landmark,
        Self::Material,
        Self::Notecard,
        Self::Object,
        Self::Script,
        Self::Sound,
        Self::Texture,
        Self::Snapshot,
        Self::Settings,
    ];

    /// This box's bit in a [`TypeFilterSet`].
    const fn bit(self) -> u16 {
        match self {
            Self::Animation => 1 << 0,
            Self::CallingCard => 1 << 1,
            Self::Clothing => 1 << 2,
            Self::Gesture => 1 << 3,
            Self::Landmark => 1 << 4,
            Self::Material => 1 << 5,
            Self::Notecard => 1 << 6,
            Self::Object => 1 << 7,
            Self::Script => 1 << 8,
            Self::Sound => 1 << 9,
            Self::Texture => 1 << 10,
            Self::Snapshot => 1 << 11,
            Self::Settings => 1 << 12,
        }
    }

    /// The box an item's inventory type belongs under, or `None` for a type
    /// outside the reference's thirteen groups (a category / unknown), which
    /// only an un-narrowed filter shows.
    pub(crate) const fn of(inv_type: InventoryType) -> Option<Self> {
        match inv_type {
            InventoryType::Animation => Some(Self::Animation),
            InventoryType::CallingCard => Some(Self::CallingCard),
            InventoryType::Wearable => Some(Self::Clothing),
            InventoryType::Gesture => Some(Self::Gesture),
            InventoryType::Landmark => Some(Self::Landmark),
            InventoryType::Material => Some(Self::Material),
            InventoryType::Notecard => Some(Self::Notecard),
            InventoryType::Object | InventoryType::Attachment | InventoryType::Mesh => {
                Some(Self::Object)
            }
            InventoryType::Script => Some(Self::Script),
            InventoryType::Sound => Some(Self::Sound),
            InventoryType::Texture => Some(Self::Texture),
            InventoryType::Snapshot => Some(Self::Snapshot),
            InventoryType::Settings => Some(Self::Settings),
            _other => None,
        }
    }

    /// This box's label as a Fluent key.
    pub(crate) const fn label_key(self) -> &'static str {
        match self {
            Self::Animation => "inventory-filter-animations",
            Self::CallingCard => "inventory-filter-calling-cards",
            Self::Clothing => "inventory-filter-clothing",
            Self::Gesture => "inventory-filter-gestures",
            Self::Landmark => "inventory-filter-landmarks",
            Self::Material => "inventory-filter-materials",
            Self::Notecard => "inventory-filter-notecards",
            Self::Object => "inventory-filter-objects",
            Self::Script => "inventory-filter-scripts",
            Self::Sound => "inventory-filter-sounds",
            Self::Texture => "inventory-filter-textures",
            Self::Snapshot => "inventory-filter-snapshots",
            Self::Settings => "inventory-filter-settings",
        }
    }
}

/// The set of type boxes currently ticked, as a bitmask over
/// [`TypeFilter::bit`]. Defaults to all ticked (no narrowing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TypeFilterSet(u16);

impl TypeFilterSet {
    /// The mask with every box ticked (the thirteen bits of
    /// [`TypeFilter::ALL`]).
    const FULL: u16 = TypeFilter::Animation.bit()
        | TypeFilter::CallingCard.bit()
        | TypeFilter::Clothing.bit()
        | TypeFilter::Gesture.bit()
        | TypeFilter::Landmark.bit()
        | TypeFilter::Material.bit()
        | TypeFilter::Notecard.bit()
        | TypeFilter::Object.bit()
        | TypeFilter::Script.bit()
        | TypeFilter::Sound.bit()
        | TypeFilter::Texture.bit()
        | TypeFilter::Snapshot.bit()
        | TypeFilter::Settings.bit();

    /// Every box ticked — the default, which narrows nothing.
    pub(crate) const fn all() -> Self {
        Self(Self::FULL)
    }

    /// No box ticked.
    pub(crate) const fn none() -> Self {
        Self(0)
    }

    /// Whether `filter`'s box is ticked.
    pub(crate) const fn contains(self, filter: TypeFilter) -> bool {
        self.0 & filter.bit() != 0
    }

    /// Tick or untick one box.
    pub(crate) const fn toggle(&mut self, filter: TypeFilter) {
        self.0 ^= filter.bit();
    }

    /// Whether every box is ticked (the un-narrowed state).
    pub(crate) const fn is_full(self) -> bool {
        self.0 == Self::FULL
    }
}

impl Default for TypeFilterSet {
    fn default() -> Self {
        Self::all()
    }
}

/// Which way the hours / days range cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DateDirection {
    /// Keep items **newer** than the cutoff.
    #[default]
    Newer,
    /// Keep items **older** than the cutoff.
    Older,
}

/// The whole filter, plain data — the reference's `LLInventoryFilter`
/// dimensions this viewer supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ItemFilter {
    /// The ticked type boxes.
    pub(crate) types: TypeFilterSet,
    /// Keep only currently worn items.
    pub(crate) worn_only: bool,
    /// Keep only items received since this session's login (the reference's
    /// "Since Logoff" box, against the login timestamp this viewer holds).
    pub(crate) since_login: bool,
    /// Which way the hours/days cutoff cuts.
    pub(crate) direction: DateDirection,
    /// The cutoff distance from now, in whole hours (the finder's Hours +
    /// Days spinners combined). `0` disables the range filter.
    pub(crate) cutoff_hours: u32,
}

impl ItemFilter {
    /// Whether the filter narrows anything at all — when it does, the tree is
    /// drawn in the search-style narrowed form.
    pub(crate) const fn is_active(&self) -> bool {
        !self.types.is_full() || self.worn_only || self.since_login || self.cutoff_hours > 0
    }

    /// Whether `item` passes the filter. `worn` is whether the item is
    /// currently worn; `now_unix` the current time and `login_unix` the
    /// session's login time, both unix seconds (the item's `creation_date`
    /// scale).
    pub(crate) fn passes(
        &self,
        item: &ItemInfo,
        worn: bool,
        now_unix: i64,
        login_unix: i64,
    ) -> bool {
        if !self.types.is_full() {
            match TypeFilter::of(item.inv_type) {
                Some(group) => {
                    if !self.types.contains(group) {
                        return false;
                    }
                }
                // A type outside the thirteen groups only shows un-narrowed.
                None => return false,
            }
        }
        if self.worn_only && !worn {
            return false;
        }
        let created = i64::from(item.creation_date);
        if self.since_login && created < login_unix {
            return false;
        }
        if self.cutoff_hours > 0 {
            let cutoff = now_unix.saturating_sub(i64::from(self.cutoff_hours).saturating_mul(3600));
            let keep = match self.direction {
                DateDirection::Newer => created >= cutoff,
                DateDirection::Older => created < cutoff,
            };
            if !keep {
                return false;
            }
        }
        true
    }
}

/// The unix time this session started (captured at plugin init, right around
/// login) — the since-login filter's reference point.
#[derive(Resource, Debug)]
pub(crate) struct SessionLoginTime(pub(crate) i64);

impl Default for SessionLoginTime {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        Self(i64::try_from(now).unwrap_or(0))
    }
}

/// The live filter state resource the floater edits and the inventory view
/// reads.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventoryFilterState {
    /// The filter as currently configured.
    pub(crate) filter: ItemFilter,
    /// The days part of the range spinner (kept separate so the two fields
    /// round-trip what the user typed; `filter.cutoff_hours` is the sum).
    days: u32,
    /// The hours part of the range spinner.
    hours: u32,
}

impl InventoryFilterState {
    /// Reset every dimension to its default (the gear menu's Reset Filters).
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Recompute the combined cutoff from the two spinner parts.
    fn recompute_cutoff(&mut self) {
        self.filter.cutoff_hours = self
            .days
            .saturating_mul(24)
            .saturating_add(self.hours)
            .min(1_000_000);
    }
}

// ---------------------------------------------------------------------------
// The floater.
// ---------------------------------------------------------------------------

/// Entity handles for the floater's parts.
#[derive(Resource)]
pub(crate) struct InventoryFiltersUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The hours numeric field.
    hours_field: Entity,
    /// The days numeric field.
    days_field: Entity,
}

impl InventoryFiltersUi {
    /// The floater's panel root, for the gear menu's Show Filters toggle.
    pub(crate) const fn panel(&self) -> Entity {
        self.panel
    }
}

/// A type toggle row, tagged with the box it flips.
#[derive(Component, Debug, Clone, Copy)]
struct TypeToggle(TypeFilter);

/// The worn-only toggle row marker.
#[derive(Component)]
struct WornToggle;

/// The since-login toggle row marker.
#[derive(Component)]
struct SinceLoginToggle;

/// A date-direction radio row, tagged with the direction it selects.
#[derive(Component, Debug, Clone, Copy)]
struct DirectionToggle(DateDirection);

/// The check-glyph text node of a toggle row (the part the sync system
/// repaints).
#[derive(Component)]
struct ToggleGlyph;

/// The plugin that owns the filters floater and its state.
pub(crate) struct InventoryFiltersPlugin;

impl Plugin for InventoryFiltersPlugin {
    /// Register the state, spawn the floater, and keep the toggle glyphs and
    /// numeric fields folded into the state.
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryFilterState>()
            .init_resource::<SessionLoginTime>()
            .add_systems(
                Startup,
                spawn_filters_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(Update, (read_range_fields, sync_toggle_glyphs).chain());
    }
}

/// Spawn the filters floater (hidden until the gear menu opens it): the
/// thirteen type toggles with All / None buttons, the worn and since-login
/// toggles, the newer / older direction pair, and the hours / days fields.
fn spawn_filters_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: FILTERS_FLOATER_ID,
            title: "Inventory Filters".to_owned(),
            position: Vec2::new(380.0, 80.0),
            default_size: None,
            min_size: None,
            dock_host: None,
            caps: FloaterCaps {
                resizable: false,
                minimizable: false,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("inventory-filters-title"));
    let content = handle.content;

    // The thirteen type boxes, in the reference finder's order.
    let types_column = commands
        .spawn((
            Node {
                ..column(Val::Px(2.0))
            },
            ChildOf(content),
        ))
        .id();
    for (index, filter) in TypeFilter::ALL.into_iter().enumerate() {
        let toggle = spawn_toggle_row(
            &mut commands,
            types_column,
            filter.label_key(),
            i32::try_from(index).unwrap_or(0).saturating_add(1),
        );
        commands.entity(toggle).insert(TypeToggle(filter)).observe(
            move |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
                if press.button == PointerButton::Primary {
                    state.filter.types.toggle(filter);
                }
            },
        );
    }

    // All / None.
    let all_none_row = commands
        .spawn((
            Node {
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let all_button = spawn_text_button(&mut commands, all_none_row, "inventory-filter-all", 20);
    commands.entity(all_button).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
            if press.button == PointerButton::Primary {
                state.filter.types = TypeFilterSet::all();
            }
        },
    );
    let none_button = spawn_text_button(&mut commands, all_none_row, "inventory-filter-none", 21);
    commands.entity(none_button).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
            if press.button == PointerButton::Primary {
                state.filter.types = TypeFilterSet::none();
            }
        },
    );

    // Worn / since-login.
    let worn = spawn_toggle_row(&mut commands, content, "inventory-filter-worn", 22);
    commands.entity(worn).insert(WornToggle).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
            if press.button == PointerButton::Primary {
                state.filter.worn_only = !state.filter.worn_only;
            }
        },
    );
    let since = spawn_toggle_row(&mut commands, content, "inventory-filter-since-login", 23);
    commands.entity(since).insert(SinceLoginToggle).observe(
        |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
            if press.button == PointerButton::Primary {
                state.filter.since_login = !state.filter.since_login;
            }
        },
    );

    // Newer-than / older-than direction (a two-way radio).
    let newer = spawn_toggle_row(&mut commands, content, "inventory-filter-newer-than", 24);
    commands
        .entity(newer)
        .insert(DirectionToggle(DateDirection::Newer))
        .observe(
            |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
                if press.button == PointerButton::Primary {
                    state.filter.direction = DateDirection::Newer;
                }
            },
        );
    let older = spawn_toggle_row(&mut commands, content, "inventory-filter-older-than", 25);
    commands
        .entity(older)
        .insert(DirectionToggle(DateDirection::Older))
        .observe(
            |press: On<Pointer<Press>>, mut state: ResMut<InventoryFilterState>| {
                if press.button == PointerButton::Primary {
                    state.filter.direction = DateDirection::Older;
                }
            },
        );

    // Hours / days range fields.
    let range_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let hours_field = crate::ui_text_input::spawn_text_input(
        &mut commands,
        range_row,
        &crate::ui_text_input::TextInputSpec {
            initial: "0".to_owned(),
            font_size: FILTER_FONT_SIZE,
            width_glyphs: 6.0,
            tab_index: 26,
            ..crate::ui_text_input::TextInputSpec::new(
                "inventory-filter-hours",
                crate::ui_text_input::TextInputKind::NonNegativeInteger,
            )
        },
    );
    spawn_label(&mut commands, range_row, "inventory-filter-hours-label");
    let days_field = crate::ui_text_input::spawn_text_input(
        &mut commands,
        range_row,
        &crate::ui_text_input::TextInputSpec {
            initial: "0".to_owned(),
            font_size: FILTER_FONT_SIZE,
            width_glyphs: 6.0,
            tab_index: 27,
            ..crate::ui_text_input::TextInputSpec::new(
                "inventory-filter-days",
                crate::ui_text_input::TextInputKind::NonNegativeInteger,
            )
        },
    );
    spawn_label(&mut commands, range_row, "inventory-filter-days-label");

    // Reset.
    let reset_row = commands
        .spawn((
            Node {
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let reset = spawn_text_button(&mut commands, reset_row, "inventory-filter-reset", 28);
    commands.entity(reset).observe(
        |press: On<Pointer<Press>>,
         mut state: ResMut<InventoryFilterState>,
         ui: Option<Res<InventoryFiltersUi>>,
         mut fields: Query<&mut EditableText>| {
            if press.button != PointerButton::Primary {
                return;
            }
            apply_reset(&mut state, ui.as_deref(), &mut fields);
        },
    );

    commands.insert_resource(InventoryFiltersUi {
        panel: handle.root,
        hours_field,
        days_field,
    });
}

/// Spawn one toggle row: a check glyph and a translated label on a clickable
/// row. The caller attaches the marker component and the press observer.
fn spawn_toggle_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    let toggle = commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Pickable::default(),
            Name::new(format!("inventory-filter:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(CHECKED_GLYPH),
        UiFont::Sans.at(FILTER_FONT_SIZE),
        TextColor(CHECK_COLOR),
        ToggleGlyph,
        Pickable::IGNORE,
        ChildOf(toggle),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FILTER_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(toggle),
    ));
    toggle
}

/// Spawn a bordered text button with a translated label.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("inventory-filter-button:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(FILTER_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// Spawn a plain translated label.
fn spawn_label(commands: &mut Commands, parent: Entity, label_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FILTER_FONT_SIZE),
        TextColor(LABEL_COLOR),
        ChildOf(parent),
    ));
}

/// Fold the hours / days numeric fields into the state when they change.
fn read_range_fields(
    ui: Option<Res<InventoryFiltersUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<InventoryFilterState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let read = |entity: Entity| -> Option<u32> {
        fields
            .get(entity)
            .ok()
            .and_then(|field| field.value().to_string().trim().parse::<u32>().ok())
    };
    let hours = read(ui.hours_field).unwrap_or(0);
    let days = read(ui.days_field).unwrap_or(0);
    if hours != state.hours || days != state.days {
        state.hours = hours;
        state.days = days;
        state.recompute_cutoff();
    }
}

/// Reset the filter to its defaults and blank the two range fields so the
/// display matches (an empty field reads back as zero). Shared by the
/// floater's Reset button and the gear menu's Reset Filters entry.
pub(crate) fn apply_reset(
    state: &mut InventoryFilterState,
    ui: Option<&InventoryFiltersUi>,
    fields: &mut Query<&mut EditableText>,
) {
    state.reset();
    if let Some(ui) = ui {
        for field in [ui.hours_field, ui.days_field] {
            if let Ok(mut text) = fields.get_mut(field) {
                text.clear();
            }
        }
    }
}

/// Repaint every toggle row's check glyph from the state (write-guarded, so a
/// quiet frame costs comparisons only).
fn sync_toggle_glyphs(
    state: Res<InventoryFilterState>,
    types: Query<(&TypeToggle, &Children)>,
    worn: Query<&Children, With<WornToggle>>,
    since: Query<&Children, With<SinceLoginToggle>>,
    directions: Query<(&DirectionToggle, &Children)>,
    mut glyphs: Query<&mut Text, With<ToggleGlyph>>,
) {
    if !state.is_changed() {
        return;
    }
    let mut set = |children: &Children, on: bool| {
        for child in children {
            if let Ok(mut text) = glyphs.get_mut(*child) {
                let wanted = if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH };
                if text.0 != wanted {
                    wanted.clone_into(&mut text.0);
                }
            }
        }
    };
    for (toggle, children) in &types {
        set(children, state.filter.types.contains(toggle.0));
    }
    for children in &worn {
        set(children, state.filter.worn_only);
    }
    for children in &since {
        set(children, state.filter.since_login);
    }
    for (toggle, children) in &directions {
        set(children, state.filter.direction == toggle.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{DateDirection, ItemFilter, TypeFilter, TypeFilterSet};
    use sl_client_bevy::{
        AgentKey, AssetType, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo,
        Permissions5, Uuid,
    };

    /// A minimal item of a type, created at a unix time.
    fn item(inv_type: InventoryType, created: i32) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(1)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(2)),
            name: "Thing".to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(3),
            asset_type: AssetType::Object,
            inv_type,
            flags: 0,
            sale: None,
            creation_date: created,
            owner: sl_client_bevy::OwnerKey::Agent(AgentKey::from(Uuid::from_u128(4))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(4)),
            group: None,
            permissions: Permissions5::default(),
        }
    }

    /// The default filter narrows nothing and passes everything.
    #[test]
    fn default_filter_is_inactive_and_passes() {
        let filter = ItemFilter::default();
        assert!(!filter.is_active());
        assert!(filter.passes(&item(InventoryType::Texture, 100), false, 1_000, 500));
        // Even a type outside the thirteen groups passes un-narrowed.
        assert!(filter.passes(&item(InventoryType::Category, 100), false, 1_000, 500));
    }

    /// Unticking a type box hides that group (and the outside-group types),
    /// keeps the rest.
    #[test]
    fn type_boxes_narrow_by_group() {
        let mut filter = ItemFilter::default();
        filter.types.toggle(TypeFilter::Texture);
        assert!(filter.is_active());
        assert!(!filter.passes(&item(InventoryType::Texture, 0), false, 0, 0));
        assert!(filter.passes(&item(InventoryType::Sound, 0), false, 0, 0));
        // Attachments and mesh ride under the Objects box.
        assert!(filter.passes(&item(InventoryType::Attachment, 0), false, 0, 0));
        // Outside the groups: hidden once narrowed.
        assert!(!filter.passes(&item(InventoryType::Category, 0), false, 0, 0));
        // The None mask hides every group.
        filter.types = TypeFilterSet::none();
        assert!(!filter.passes(&item(InventoryType::Sound, 0), false, 0, 0));
    }

    /// The worn and since-login switches gate on the caller-provided facts.
    #[test]
    fn worn_and_since_login_gate() {
        let filter = ItemFilter {
            worn_only: true,
            ..ItemFilter::default()
        };
        assert!(filter.passes(&item(InventoryType::Wearable, 0), true, 0, 0));
        assert!(!filter.passes(&item(InventoryType::Wearable, 0), false, 0, 0));

        let filter = ItemFilter {
            since_login: true,
            ..ItemFilter::default()
        };
        assert!(filter.passes(&item(InventoryType::Texture, 900), false, 1_000, 800));
        assert!(!filter.passes(&item(InventoryType::Texture, 700), false, 1_000, 800));
    }

    /// The hours/days cutoff keeps newer or older items by direction.
    #[test]
    fn date_range_cuts_both_ways() {
        // Cutoff two hours ago; now = 10_000.
        let newer = ItemFilter {
            cutoff_hours: 2,
            direction: DateDirection::Newer,
            ..ItemFilter::default()
        };
        let old_item = item(InventoryType::Texture, 10_000 - 3 * 3600);
        let new_item = item(InventoryType::Texture, 10_000 - 3600);
        assert!(newer.passes(&new_item, false, 10_000, 0));
        assert!(!newer.passes(&old_item, false, 10_000, 0));

        let older = ItemFilter {
            cutoff_hours: 2,
            direction: DateDirection::Older,
            ..ItemFilter::default()
        };
        assert!(older.passes(&old_item, false, 10_000, 0));
        assert!(!older.passes(&new_item, false, 10_000, 0));
    }
}
