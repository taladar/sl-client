//! Item **Properties** and per-type **Open** previews
//! (`viewer-inventory-open-and-properties`): the item-properties floater —
//! name / description editing, creator / owner / acquired, the permission
//! toggles and sale settings, written back via `UpdateInventoryItem` — and
//! the small per-type preview floaters behind the context menu's Open:
//! a notecard reader, a texture / snapshot preview, and an animation preview
//! (play in-world / stop). A landmark's Open forwards to the full About
//! Landmark floater (`crate::about_landmark`).
//!
//! # Layout follows the Vintage skin
//!
//! The properties floater mirrors the **legacy single-window** layout the
//! Vintage skin keeps (`floater_inventory_item_properties.xml`,
//! `llfloaterproperties.cpp`) — one flat page, no thumbnail control — rather
//! than the default skin's sidepanel.
//!
//! # Rebuilt per open
//!
//! Each floater's content is torn down and rebuilt when it opens on an item
//! (the picker-list pattern), so the fields carry the item's values as their
//! initial text and nothing needs a programmatic text-set API. Name /
//! description commit on `Enter`; the permission / sale toggles commit
//! immediately on click.
//!
//! # One properties window per item
//!
//! The properties floater is a **keyed floater** ([`FloaterKey`]): inspecting a
//! second item opens a second window rather than re-pointing the first, so two
//! items' permissions can be compared side by side (the reference registers
//! `LLFloaterProperties` per item id). Each window's item and field entities
//! are components on it, and a permission toggle repaints **its** window by
//! re-opening on the updated snapshot — which is why an open on an item that
//! is already up rebuilds rather than merely raising, unlike the asset editors
//! where a rebuild would discard unsaved text.
//!
//! The two per-type **previews** below are still singletons.
//!
//! Reference (Firestorm, read-only): `llfloaterproperties.cpp`,
//! `skins/vintage/xui/en/floater_inventory_item_properties.xml`,
//! `llpreview{notecard,texture,anim}.cpp`, "About Landmark".

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AnimationKey, AssetKey, Command, InventoryItem, InventoryKey, InventoryType, ItemInfo,
    LindenAmount, Permissions, SaleInfo, SaleType, SettingsKind, SlCommand, SlIdentity, TextureKey,
    TransactionId, Uuid, to_bevy_image,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen, KeyedFloaters,
    host_floater, spawn_floater,
};
use crate::i18n::Translated;
use crate::inventory::query_folder_page;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, row};
use crate::ui_font::UiFont;
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::{BoostTexture, DecodedTextures};

/// The chrome font size, in logical pixels.
const PROPS_FONT_SIZE: f32 = 14.0;

/// The label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A toggle's check glyph colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A **read-only** check's glyph colour — the live check's green, muted, so the
/// "You can" row reads as a statement of fact rather than a control that will
/// not respond. See `spawn_static_check`.
const STATIC_CHECK_COLOR: Color = Color::srgb(0.42, 0.60, 0.45);

/// A button's background / border.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The checked / unchecked glyphs.
const CHECKED_GLYPH: &str = "\u{2611}";
/// The unchecked glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The price a **newly offered** item starts at, in L$, when its own price is
/// unreadable — only reached if the field holds something unparsable, since an
/// item now always carries a price of its own (`SaleInfo`).
const DEFAULT_SALE_PRICE: u64 = 10;

/// The texture preview's largest edge, in logical pixels.
const TEXTURE_PREVIEW_EDGE: f32 = 256.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

/// Open the properties floater on an item.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenItemProperties {
    /// The item to show.
    pub(crate) item: ItemInfo,
}

/// Open the per-type preview for an item (the context menu's Open).
#[derive(Message, Debug, Clone)]
pub struct OpenItemPreview {
    /// The item to preview.
    pub item: ItemInfo,
}

/// Whether this viewer has a preview for an item's type — gates the context
/// menu's Open entry.
pub(crate) const fn previewable(inv_type: InventoryType) -> bool {
    matches!(
        inv_type,
        InventoryType::Notecard
            | InventoryType::Script
            | InventoryType::Texture
            | InventoryType::Snapshot
            | InventoryType::Landmark
            | InventoryType::Animation
            | InventoryType::Settings
    )
}

// ---------------------------------------------------------------------------
// Properties floater.
// ---------------------------------------------------------------------------

/// One open properties window's live state — a **component on the window**,
/// since the floater opens per item ([`FloaterKey`]): the item shown, with the
/// editable permission / sale bits as currently displayed.
#[derive(Component, Debug)]
pub(crate) struct ItemPropertiesState {
    /// The item this window shows (as last received). Always set — the window
    /// exists because an item was opened — but kept as an `Option` because a
    /// toggle takes it apart and puts an updated snapshot back.
    item: Option<ItemInfo>,
}

/// One properties window's entity handles — a component beside its
/// [`ItemPropertiesState`], so two open items keep their own fields.
#[derive(Component)]
pub(crate) struct ItemPropertiesUi {
    /// The rebuilt-per-open content column.
    content: Entity,
    /// The name field (rebuilt per open; nil when read-only).
    name_field: Option<Entity>,
    /// The description field.
    desc_field: Option<Entity>,
    /// The sale-price field.
    price_field: Option<Entity>,
}

/// A permission / sale toggle in the properties floater, naming what it
/// flips.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PropsToggle {
    /// Share with the set group (group mask modify/copy/move).
    ShareWithGroup,
    /// Allow anyone to copy (everyone mask copy).
    EveryoneCopy,
    /// Next owner may modify.
    NextModify,
    /// Next owner may copy.
    NextCopy,
    /// Next owner may transfer.
    NextTransfer,
    /// The item is for sale.
    ForSale,
    /// Cycle the sale type (Original → Copy → Contents).
    SaleType,
}

/// The plugin owning the properties floater and the preview floaters.
#[derive(Debug)]
pub struct InventoryPropertiesPlugin;

impl Plugin for InventoryPropertiesPlugin {
    /// Register messages and systems; spawn the (hidden) preview floaters.
    ///
    /// The **properties** floater spawns nothing at `Startup`: it opens per
    /// item, so `open_properties` spawns the instance and builds its content.
    /// The two per-type previews are still singletons (a separate entry in
    /// `viewer-keyed-floater-audit`).
    fn build(&self, app: &mut App) {
        app.init_resource::<PreviewState>()
            .add_message::<OpenItemProperties>()
            .add_message::<OpenItemPreview>()
            .add_systems(
                Startup,
                spawn_preview_floaters.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the inventory row that opens a properties window also
                    // raises the inventory floater it sits in, and the later
                    // raise wins.
                    open_properties.after(FloaterSystems::Commands),
                    commit_text_edits.run_if(any_with_component::<ItemPropertiesState>),
                    open_previews,
                    poll_texture_preview,
                )
                    .chain(),
            );
    }
}

/// The Item Properties floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn item_properties_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "item-properties",
        title: "Item Properties".to_owned(),
        position: Vec2::new(360.0, 90.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// The texture-preview floater's [`FloaterSpec`], for the `FLOATERS` registry.
#[must_use]
pub fn texture_preview_floater_spec() -> FloaterSpec {
    preview_floater_spec("preview-texture", "Texture")
}

/// The animation-preview floater's [`FloaterSpec`], for the `FLOATERS` registry.
#[must_use]
pub fn animation_preview_floater_spec() -> FloaterSpec {
    preview_floater_spec("preview-animation", "Animation")
}

/// One preview floater's [`FloaterSpec`]. The two previews differ only in their
/// id and title, so the shell is written once and named twice — once per
/// registry entry, because a registry entry is a window and there are two of
/// them.
fn preview_floater_spec(id: &'static str, title: &str) -> FloaterSpec {
    FloaterSpec {
        id,
        title: title.to_owned(),
        position: Vec2::new(420.0, 120.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Spawn the preview floaters, hidden. (The properties floater is keyed, so it
/// is spawned per item by [`open_properties`].)
fn spawn_preview_floaters(mut commands: Commands, root: Res<UiRoot>) {
    // Notecards open in their own editor floater (`crate::edit_notecard`),
    // landmarks in the About Landmark floater (`crate::about_landmark`) — not
    // here.
    // Texture.
    let texture = spawn_preview_floater(&mut commands, root.0, texture_preview_floater_spec());
    // Animation.
    let animation = spawn_preview_floater(&mut commands, root.0, animation_preview_floater_spec());
    commands.insert_resource(PreviewUi { texture, animation });
}

/// Spawn one preview floater shell, returning its handles.
fn spawn_preview_floater(
    commands: &mut Commands,
    root: Entity,
    spec: FloaterSpec,
) -> PreviewFloater {
    let handle = spawn_floater(commands, root, spec);
    // Subject-bound, like the properties floater: not persisted.
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    PreviewFloater {
        panel: handle.root,
        content: handle.content,
        title_text: handle.title_text,
    }
}

/// One preview floater's entities.
#[derive(Debug, Clone, Copy)]
struct PreviewFloater {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The rebuilt-per-open content column.
    content: Entity,
    /// The title text node (set to the item's name on open).
    title_text: Entity,
}

/// The preview floaters' entities.
#[derive(Resource)]
struct PreviewUi {
    /// The texture / snapshot preview.
    texture: PreviewFloater,
    /// The animation preview.
    animation: PreviewFloater,
}

/// The [`FloaterKey`] of the window showing `item`'s properties — one window
/// per inventory item, keyed by its id. A [subject](FloaterKey::Subject) key,
/// so nothing is persisted: a stored rectangle per item ever inspected would
/// grow the settings file without bound.
fn properties_key(item: InventoryKey) -> FloaterKey {
    FloaterKey::subject(&item)
}

/// Open (or repaint) an item's properties window.
///
/// Every open of the frame is honoured, and an item already on screen is
/// **rebuilt** rather than merely raised — unlike the asset editors, where a
/// re-open would discard unsaved text. Here the re-open *is* the repaint: a
/// permission toggle sends its update and re-opens the floater on the new
/// snapshot, which is how every checkbox in the window follows the change.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open stream, the \
              keyed-window opener, the per-window state and handles, the identity and name \
              sources, and the spawn / command outputs"
)]
fn open_properties(
    mut opens: MessageReader<OpenItemProperties>,
    mut floaters: KeyedFloaters,
    mut windows: Query<(&mut ItemPropertiesState, &mut ItemPropertiesUi)>,
    identity: Res<SlIdentity>,
    avatars: Res<crate::world_api::AvatarState>,
    children: Query<&Children>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for open in opens.read().cloned() {
        let item = open.item;
        let opened = floaters.open(item_properties_floater_spec(), properties_key(item.item_id));
        match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                // A fresh window: its content is built straight into the chrome
                // handle, and the fields it hands back are its first `ItemPropertiesUi`.
                commands
                    .entity(handle.title_text)
                    .insert(Translated::new("item-properties-title"));
                let fields = build_properties_content(
                    &mut commands,
                    handle.content,
                    &item,
                    &identity,
                    &avatars,
                    &mut sl_commands,
                );
                commands.entity(handle.root).insert((
                    ItemPropertiesState {
                        item: Some(item.clone()),
                    },
                    ItemPropertiesUi {
                        content: handle.content,
                        name_field: fields.name,
                        desc_field: fields.description,
                        price_field: fields.price,
                    },
                ));
            }
            KeyedFloaterOpen::Existing(window) => {
                let Ok((mut state, mut ui)) = windows.get_mut(window) else {
                    continue;
                };
                state.item = Some(item.clone());
                // Tear the old content down and repaint from the new snapshot.
                if let Ok(existing) = children.get(ui.content) {
                    for child in existing.iter().collect::<Vec<_>>() {
                        commands.entity(child).despawn();
                    }
                }
                let fields = build_properties_content(
                    &mut commands,
                    ui.content,
                    &item,
                    &identity,
                    &avatars,
                    &mut sl_commands,
                );
                ui.name_field = fields.name;
                ui.desc_field = fields.description;
                ui.price_field = fields.price;
            }
        }
    }
}

/// Which of a properties window's controls a given item lets the agent touch.
///
/// The reference computes exactly these from the item's permission masks
/// (`LLFloaterProperties::refresh`), and each one is about a *different* bit —
/// so a single "is it mine" flag, which is what this floater used to gate
/// everything on, offered controls that could not do what they promised. The
/// masks are the item's own: `owner` is what the agent may do with it now,
/// `base` what the creator ever permitted, `next_owner` what the next owner
/// gets.
#[expect(
    clippy::struct_excessive_bools,
    reason = "seven independent yes/no answers, one per control the window offers — which is \
              the whole point: collapsing them is exactly the bug this type fixed, and a \
              bitflags newtype would only obscure which control each answer is about"
)]
#[derive(Debug, Clone, Copy)]
struct PermissionGates {
    /// The item's name and description may be edited (the agent may modify it).
    modifiable: bool,
    /// The group-share toggle is live.
    share_with_group: bool,
    /// The anyone-copy toggle is live: you cannot let anyone copy what you may
    /// not copy **and** pass on.
    everyone_copy: bool,
    /// The sale row (For Sale, its type, its price) is live: selling is passing
    /// the item on, so it needs transfer.
    sale: bool,
    /// The next-owner modify toggle is live — bounded by what the creator
    /// permitted, not by what this owner happens to hold.
    next_modify: bool,
    /// The next-owner copy toggle is live (likewise the creator's bound).
    next_copy: bool,
    /// The next-owner transfer toggle is live — the reference gates it on the
    /// next owner's *copy* bit, since forbidding transfer only means something
    /// for an item they can copy.
    next_transfer: bool,
}

impl PermissionGates {
    /// Read the gates off `item`, for an agent who does (`owned`) or does not
    /// own it. A non-owner touches nothing.
    const fn of(item: &ItemInfo, owned: bool) -> Self {
        let owner = item.permissions.owner;
        let base = item.permissions.base;
        let next = item.permissions.next_owner;
        let modifiable = owned && owner.contains(Permissions::MODIFY);
        // Selling hands the item on, so the whole sale / next-owner block
        // needs the transfer right as well as modify.
        let sale = modifiable && owner.contains(Permissions::TRANSFER);
        Self {
            modifiable,
            share_with_group: modifiable,
            everyone_copy: modifiable
                && owner.contains(Permissions::COPY)
                && owner.contains(Permissions::TRANSFER),
            sale,
            next_modify: sale && base.contains(Permissions::MODIFY),
            next_copy: sale && base.contains(Permissions::COPY),
            next_transfer: sale && next.contains(Permissions::COPY),
        }
    }
}

/// The editable fields a properties repaint spawned, handed back so the window
/// can read them on a commit.
#[derive(Debug, Default, Clone, Copy)]
struct PropertiesFields {
    /// The name field, when the item is the agent's own (else read-only).
    name: Option<Entity>,
    /// The description field.
    description: Option<Entity>,
    /// The sale-price field.
    price: Option<Entity>,
}

/// Build one properties window's content under `content` from `item`: the name
/// / description rows, the creator / owner / dates block, the permission
/// checkboxes and the sale row.
fn build_properties_content(
    commands: &mut Commands,
    content: Entity,
    item: &ItemInfo,
    identity: &SlIdentity,
    avatars: &crate::world_api::AvatarState,
    sl_commands: &mut MessageWriter<SlCommand>,
) -> PropertiesFields {
    let own = identity.agent_id;
    let owned = matches!(item.owner, sl_client_bevy::OwnerKey::Agent(agent) if Some(agent) == own);
    let gates = PermissionGates::of(item, owned);
    // The item's own text follows the modify bit, as everything below follows
    // the bit it is about.
    let editable = gates.modifiable;

    // Name / description rows.
    let name_row = spawn_labeled_row(commands, content, "item-properties-name");
    let name_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            commands,
            name_row,
            &crate::ui_text_input::TextInputSpec {
                initial: item.name.clone(),
                font_size: PROPS_FONT_SIZE,
                width_glyphs: 24.0,
                tab_index: 1,
                max_characters: Some(63),
                ..crate::ui_text_input::TextInputSpec::new(
                    "item-properties-name",
                    crate::ui_text_input::TextInputKind::Line,
                )
            },
        )
    });
    if !editable {
        spawn_value_label(commands, name_row, item.name.clone(), LABEL_COLOR);
    }
    let desc_row = spawn_labeled_row(commands, content, "item-properties-description");
    let desc_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            commands,
            desc_row,
            &crate::ui_text_input::TextInputSpec {
                initial: item.description.clone(),
                font_size: PROPS_FONT_SIZE,
                width_glyphs: 24.0,
                tab_index: 2,
                max_characters: Some(127),
                ..crate::ui_text_input::TextInputSpec::new(
                    "item-properties-description",
                    crate::ui_text_input::TextInputKind::Line,
                )
            },
        )
    });
    if !editable {
        spawn_value_label(commands, desc_row, item.description.clone(), LABEL_COLOR);
    }

    // Creator / owner / acquired.
    let name_of = |agent: sl_client_bevy::AgentKey| {
        avatars
            .name_of(agent)
            .map_or_else(|| format!("({agent})"), str::to_owned)
    };
    let creator_row = spawn_labeled_row(commands, content, "item-properties-creator");
    spawn_value_label(
        commands,
        creator_row,
        name_of(item.creator_id),
        DIM_LABEL_COLOR,
    );
    let owner_row = spawn_labeled_row(commands, content, "item-properties-owner");
    let owner_label = match item.owner {
        sl_client_bevy::OwnerKey::Agent(agent) => name_of(agent),
        sl_client_bevy::OwnerKey::Group(group) => format!("(group {group})"),
    };
    spawn_value_label(commands, owner_row, owner_label, DIM_LABEL_COLOR);
    // Ask for any unresolved names; the next open shows them.
    let mut wanted = vec![item.creator_id];
    if let sl_client_bevy::OwnerKey::Agent(agent) = item.owner {
        wanted.push(agent);
    }
    let unresolved: Vec<_> = wanted
        .into_iter()
        .filter(|agent| avatars.name_of(*agent).is_none())
        .collect();
    if !unresolved.is_empty() {
        sl_commands.write(SlCommand(Command::RequestAvatarNames(unresolved)));
    }
    let acquired_row = spawn_labeled_row(commands, content, "item-properties-acquired");
    spawn_value_label(
        commands,
        acquired_row,
        format_unix_date(i64::from(item.creation_date)),
        DIM_LABEL_COLOR,
    );

    // "You can:" — the owner mask, read-only.
    let you_row = spawn_labeled_row(commands, content, "item-properties-you-can");
    let owner_mask = item.permissions.owner;
    for (label, bit) in [
        ("item-properties-modify", Permissions::MODIFY),
        ("item-properties-copy", Permissions::COPY),
        ("item-properties-transfer", Permissions::TRANSFER),
    ] {
        spawn_static_check(commands, you_row, label, owner_mask.contains(bit));
    }

    // Group share / everyone copy toggles.
    let share_row = spawn_labeled_row(commands, content, "item-properties-group");
    spawn_props_toggle(
        commands,
        share_row,
        "item-properties-share",
        PropsToggle::ShareWithGroup,
        item.permissions.group.contains(Permissions::COPY),
        gates.share_with_group,
    );
    let anyone_row = spawn_labeled_row(commands, content, "item-properties-anyone");
    spawn_props_toggle(
        commands,
        anyone_row,
        "item-properties-copy",
        PropsToggle::EveryoneCopy,
        item.permissions.everyone.contains(Permissions::COPY),
        gates.everyone_copy,
    );

    // Next owner toggles.
    let next_row = spawn_labeled_row(commands, content, "item-properties-next-owner");
    let next = item.permissions.next_owner;
    for (label, toggle, bit, enabled) in [
        (
            "item-properties-modify",
            PropsToggle::NextModify,
            Permissions::MODIFY,
            gates.next_modify,
        ),
        (
            "item-properties-copy",
            PropsToggle::NextCopy,
            Permissions::COPY,
            gates.next_copy,
        ),
        (
            "item-properties-transfer",
            PropsToggle::NextTransfer,
            Permissions::TRANSFER,
            gates.next_transfer,
        ),
    ] {
        spawn_props_toggle(
            commands,
            next_row,
            label,
            toggle,
            next.contains(bit),
            enabled,
        );
    }

    // For sale + type + price.
    let sale_row = spawn_labeled_row(commands, content, "item-properties-for-sale");
    // The type and the price are separate facts, and the price survives an
    // unticked sale (`SaleInfo`) — so unticking For Sale keeps the number the
    // owner set rather than falling back to a made-up default.
    let (sale_type, sale_price) = (item.sale.sale_type, item.sale.price.clone());
    spawn_props_toggle(
        commands,
        sale_row,
        "item-properties-for-sale",
        PropsToggle::ForSale,
        sale_type != SaleType::NotForSale,
        gates.sale,
    );
    let type_button =
        spawn_text_button(commands, sale_row, sale_type_key(sale_type), 3, gates.sale);
    commands.entity(type_button).insert(PropsToggle::SaleType);
    let price_field = gates.sale.then(|| {
        let field = crate::ui_text_input::spawn_text_input(
            commands,
            sale_row,
            &crate::ui_text_input::TextInputSpec {
                initial: sale_price.0.to_string(),
                font_size: PROPS_FONT_SIZE,
                width_glyphs: 8.0,
                tab_index: 4,
                ..crate::ui_text_input::TextInputSpec::new(
                    "item-properties-price",
                    crate::ui_text_input::TextInputKind::NonNegativeInteger,
                )
            },
        );
        // A price only means something for an item that is **for sale**, and the
        // commit path knows it: it applies a typed price only when a sale
        // exists. Showing a live field for a not-for-sale item therefore
        // offered an edit that was silently dropped, so the field is disabled
        // (greyed, and it refuses focus) until For Sale is ticked — which is
        // what the reference does with its price spinner. Ticking the box
        // re-opens the window, so the field comes back live with the value it
        // was showing.
        if !item.sale.is_for_sale() {
            commands.entity(field).insert(bevy::ui::InteractionDisabled);
        }
        field
    });

    PropertiesFields {
        name: name_field,
        description: desc_field,
        price: price_field,
    }
}

/// The Fluent key naming a sale type on the cycle button.
const fn sale_type_key(sale_type: SaleType) -> &'static str {
    match sale_type {
        SaleType::Original => "item-properties-sale-original",
        SaleType::Contents => "item-properties-sale-contents",
        _not_or_copy => "item-properties-sale-copy",
    }
}

/// A labelled row: the translated label leading, the caller's content after.
fn spawn_labeled_row(commands: &mut Commands, parent: Entity, label_key: &'static str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Node {
            min_width: Val::Px(90.0),
            ..default()
        },
        ChildOf(row_entity),
    ));
    row_entity
}

/// A plain value label.
fn spawn_value_label(commands: &mut Commands, parent: Entity, value: String, color: Color) {
    commands.spawn((
        Text::new(value),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(color),
        ChildOf(parent),
    ));
}

/// A read-only check + label pair (the "You can" row).
fn spawn_static_check(commands: &mut Commands, parent: Entity, label_key: &'static str, on: bool) {
    // Read-only, and it must **look** it: the "You can" row states what the
    // owner mask already says, and nothing here can change it (only the item's
    // creator or a next-owner setting can). Drawn in the dim label colour and
    // with a dimmed check, so it does not read as a checkbox the user is
    // failing to click — the same distinction the reference draws between its
    // greyed permission display and its live next-owner boxes.
    commands.spawn((
        Text::new(if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(if on {
            STATIC_CHECK_COLOR
        } else {
            DIM_LABEL_COLOR
        }),
        ChildOf(parent),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        ChildOf(parent),
    ));
}

/// A clickable permission / sale toggle. Greyed (non-interactive) when the
/// viewer's agent does not own the item.
fn spawn_props_toggle(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    toggle: PropsToggle,
    on: bool,
    editable: bool,
) {
    let mut entity = commands.spawn((
        Button,
        Node {
            align_items: AlignItems::Center,
            ..row(Val::Px(4.0))
        },
        Pickable::default(),
        Name::new(format!("item-properties:{label_key}")),
        ChildOf(parent),
    ));
    if editable {
        entity.insert(toggle);
        entity.observe(on_toggle_press);
    }
    let host = entity.id();
    commands.spawn((
        Text::new(if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(if on { CHECK_COLOR } else { DIM_LABEL_COLOR }),
        Pickable::IGNORE,
        ChildOf(host),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(if editable {
            LABEL_COLOR
        } else {
            DIM_LABEL_COLOR
        }),
        Pickable::IGNORE,
        ChildOf(host),
    ));
}

/// A permission / sale toggle was clicked: flip the bit on the shown item,
/// send the update, and re-open the floater on the updated snapshot (which
/// repaints every toggle).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the pressed toggle, \
              the two queries that resolve which window it sits in, that window's state and \
              handles, the field values, and the update / repaint outputs"
)]
fn on_toggle_press(
    press: On<Pointer<Press>>,
    toggles: Query<&PropsToggle>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut windows: Query<(&mut ItemPropertiesState, &ItemPropertiesUi)>,
    fields: Query<&EditableText>,
    mut commands: MessageWriter<SlCommand>,
    mut reopen: MessageWriter<OpenItemProperties>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(toggle) = toggles.get(press.entity) else {
        return;
    };
    // The toggle belongs to the window it sits in: with two items' properties
    // open, this must flip *that* item's bit and read *that* window's price.
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = windows.get_mut(window) else {
        return;
    };
    let Some(mut item) = state.item.clone() else {
        return;
    };
    match toggle {
        PropsToggle::ShareWithGroup => {
            let bits = Permissions::MODIFY | Permissions::COPY | Permissions::MOVE;
            if item.permissions.group.contains(Permissions::COPY) {
                item.permissions.group = item.permissions.group.difference(bits);
            } else {
                item.permissions.group = item.permissions.group.union(bits);
            }
        }
        PropsToggle::EveryoneCopy => {
            if item.permissions.everyone.contains(Permissions::COPY) {
                item.permissions.everyone = item.permissions.everyone.difference(Permissions::COPY);
            } else {
                item.permissions.everyone = item.permissions.everyone.union(Permissions::COPY);
            }
        }
        PropsToggle::NextModify => flip_next_owner(&mut item, Permissions::MODIFY),
        PropsToggle::NextCopy => flip_next_owner(&mut item, Permissions::COPY),
        PropsToggle::NextTransfer => flip_next_owner(&mut item, Permissions::TRANSFER),
        PropsToggle::ForSale => {
            // Unticking keeps the price on the item (the wire keeps it too);
            // only the type says whether it is offered.
            item.sale = if item.sale.is_for_sale() {
                SaleInfo::not_for_sale(item.sale.price.clone())
            } else {
                SaleInfo {
                    sale_type: SaleType::Copy,
                    price: sale_price_of(ui, &fields),
                }
            };
        }
        PropsToggle::SaleType => {
            let price = sale_price_of(ui, &fields);
            let sale_type = match item.sale.sale_type {
                SaleType::Original => SaleType::Copy,
                SaleType::Copy => SaleType::Contents,
                _not_or_contents => SaleType::Original,
            };
            item.sale = SaleInfo { sale_type, price };
        }
    }
    send_item_update(&item, &mut commands);
    state.item = Some(item.clone());
    reopen.write(OpenItemProperties { item });
}

/// Flip one next-owner permission bit.
const fn flip_next_owner(item: &mut ItemInfo, bit: Permissions) {
    if item.permissions.next_owner.contains(bit) {
        item.permissions.next_owner = item.permissions.next_owner.difference(bit);
    } else {
        item.permissions.next_owner = item.permissions.next_owner.union(bit);
    }
}

/// The price currently typed in the sale-price field (falling back to the
/// shown item's price, then to 10).
fn sale_price_of(ui: &ItemPropertiesUi, fields: &Query<&EditableText>) -> LindenAmount {
    ui.price_field
        .and_then(|field| fields.get(field).ok())
        .and_then(|field| field.value().to_string().trim().parse::<u64>().ok())
        .map_or(LindenAmount(DEFAULT_SALE_PRICE), LindenAmount)
}

/// `Enter` in the name / description / price fields commits the pending text
/// edits as one `UpdateInventoryItem`.
fn commit_text_edits(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut windows: Query<(&mut ItemPropertiesState, &ItemPropertiesUi)>,
    fields: Query<&EditableText>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    // The commit belongs to the window whose field has the keyboard — with two
    // items open, Enter must save the one being typed in.
    let focused = focus.get();
    let Some((mut state, ui)) = windows.iter_mut().find(|(_state, ui)| {
        [ui.name_field, ui.desc_field, ui.price_field]
            .into_iter()
            .flatten()
            .any(|field| Some(field) == focused)
    }) else {
        return;
    };
    let Some(mut item) = state.item.clone() else {
        return;
    };
    let read = |entity: Option<Entity>| {
        entity
            .and_then(|field| fields.get(field).ok())
            .map(|field| field.value().to_string())
    };
    if let Some(name) = read(ui.name_field) {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            trimmed.clone_into(&mut item.name);
        }
    }
    if let Some(description) = read(ui.desc_field) {
        description.trim().clone_into(&mut item.description);
    }
    // A price is only editable while the item is offered (the field is disabled
    // otherwise), so only then can there be a typed one to commit.
    if item.sale.is_for_sale()
        && let Some(price) = read(ui.price_field).and_then(|price| price.trim().parse::<u64>().ok())
    {
        item.sale.price = LindenAmount(price);
    }
    send_item_update(&item, &mut commands);
    state.item = Some(item);
}

/// Send an `UpdateInventoryItem` for the (edited) item and refresh its
/// folder page (shared with the About Landmark floater's title / notes
/// editing, `crate::about_landmark`).
pub fn send_item_update(item: &ItemInfo, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::UpdateInventoryItem {
        item: Box::new(to_wire_item(item)),
        transaction_id: TransactionId::from(Uuid::nil()),
    }));
    query_folder_page(item.folder_id, commands);
}

/// Rebuild an [`ItemInfo`] into the wire `InventoryItem` an update carries
/// (shared with the COF link renumbering in [`crate::inventory_actions`]).
#[must_use]
pub fn to_wire_item(item: &ItemInfo) -> InventoryItem {
    // Both halves always travel: an unticked sale still carries its price, so a
    // save cannot erase what the grid holds (see `SaleInfo`).
    let (sale_type, sale_price) = (item.sale.sale_type.to_code(), Some(item.sale.price.clone()));
    InventoryItem {
        item_id: item.item_id,
        folder_id: item.folder_id,
        name: item.name.clone(),
        description: item.description.clone(),
        asset_id: item.asset_id,
        item_type: i8::try_from(item.asset_type.to_code()).unwrap_or(-1),
        inv_type: i8::try_from(item.inv_type.to_code()).unwrap_or(-1),
        flags: item.flags,
        sale_type,
        sale_price,
        creation_date: item.creation_date,
        owner: item.owner,
        last_owner_id: item.last_owner_id,
        creator_id: item.creator_id,
        group: item.group,
        permissions: item.permissions,
    }
}

/// Format a unix timestamp as a UTC `YYYY-MM-DD HH:MM` label, via the civil
/// calendar arithmetic (Howard Hinnant's `civil_from_days`).
#[must_use]
pub fn format_unix_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs / 3600;
    let minute = (secs % 3600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Convert days-since-epoch to a `(year, month, day)` civil date.
const fn civil_from_days(days: i64) -> (i64, u8, u8) {
    // Wrapping arithmetic: the algorithm's intermediates cannot overflow for
    // any timestamp the wire can carry (an `i32` creation date), and the
    // workspace lint denies bare operators.
    let z = days.wrapping_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = doe
        .wrapping_sub(doe / 1460)
        .wrapping_add(doe / 36_524)
        .wrapping_sub(doe / 146_096)
        / 365;
    let year = yoe.wrapping_add(era.wrapping_mul(400));
    let doy = doe.wrapping_sub(
        yoe.wrapping_mul(365)
            .wrapping_add(yoe / 4)
            .wrapping_sub(yoe / 100),
    );
    let mp = doy.wrapping_mul(5).wrapping_add(2) / 153;
    let day = doy
        .wrapping_sub(mp.wrapping_mul(153).wrapping_add(2) / 5)
        .wrapping_add(1);
    let month = if mp < 10 {
        mp.wrapping_add(3)
    } else {
        mp.wrapping_sub(9)
    };
    let year = if month <= 2 {
        year.wrapping_add(1)
    } else {
        year
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::as_conversions,
        reason = "month is 1..=12 and day 1..=31 by construction of the civil algorithm"
    )]
    (year, month as u8, day as u8)
}

// ---------------------------------------------------------------------------
// Previews.
// ---------------------------------------------------------------------------

/// The previews' in-flight fetches.
#[derive(Resource, Debug, Default)]
struct PreviewState {
    /// The texture awaited from the texture pipeline, with the node to give
    /// the image to.
    pending_texture: Option<(TextureKey, Entity)>,
    /// The animation shown in the animation preview.
    animation: Option<AssetKey>,
}

/// Route an Open to its type's preview floater.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open stream, the \
              preview state and floaters, the texture pipeline, and the spawn / visibility \
              outputs"
)]
fn open_previews(
    mut opens: MessageReader<OpenItemPreview>,
    ui: Option<Res<PreviewUi>>,
    mut state: ResMut<PreviewState>,
    mut boost: MessageWriter<BoostTexture>,
    children: Query<&Children>,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
    mut notecard_opens: MessageWriter<crate::world_api::OpenNotecard>,
    mut script_opens: MessageWriter<crate::world_api::OpenScript>,
    mut landmark_opens: MessageWriter<crate::inventory::OpenAboutLandmark>,
    mut settings_opens: MessageWriter<crate::world_api::OpenSettingsEditor>,
) {
    let Some(ui) = ui else {
        return;
    };
    for open in opens.read() {
        let item = &open.item;
        match item.inv_type {
            InventoryType::Notecard => {
                // The notecard editor floater owns this type (read / edit / save).
                notecard_opens.write(crate::world_api::OpenNotecard {
                    name: item.name.clone(),
                    asset_id: item.asset_id,
                    editable: item.permissions.owner.contains(Permissions::MODIFY),
                    source: crate::world_api::NotecardSource::Agent {
                        item_id: item.item_id,
                    },
                });
            }
            InventoryType::Script => {
                // The script editor floater owns this type (read / edit / save →
                // compile). The compile backend follows the item's language flag.
                script_opens.write(crate::world_api::OpenScript {
                    name: item.name.clone(),
                    asset_id: item.asset_id,
                    editable: item.permissions.owner.contains(Permissions::MODIFY),
                    source: crate::world_api::ScriptSource::Agent {
                        item_id: item.item_id,
                    },
                    target: crate::world_api::target_for(
                        sl_client_bevy::ScriptLanguage::from_item_flags(item.flags),
                    ),
                });
            }
            InventoryType::Texture | InventoryType::Snapshot => {
                reset_preview(
                    &ui.texture,
                    &item.name,
                    &children,
                    &mut texts,
                    &mut commands,
                );
                let placeholder = commands
                    .spawn((
                        Node {
                            width: Val::Px(TEXTURE_PREVIEW_EDGE),
                            height: Val::Px(TEXTURE_PREVIEW_EDGE),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                        ChildOf(ui.texture.content),
                    ))
                    .with_child((
                        Text::new("(loading)"),
                        UiFont::Sans.at(PROPS_FONT_SIZE),
                        TextColor(DIM_LABEL_COLOR),
                    ))
                    .id();
                let key = TextureKey::from(item.asset_id);
                boost.write(BoostTexture {
                    key,
                    priority: AVATAR_BOOST_PRIORITY,
                });
                state.pending_texture = Some((key, placeholder));
                show(&mut panels, ui.texture.panel);
            }
            InventoryType::Landmark => {
                // The full About Landmark floater owns this type.
                landmark_opens.write(crate::inventory::OpenAboutLandmark { item: item.clone() });
            }
            InventoryType::Settings => {
                // The settings editors own this type (sky / water; a day cycle
                // gets its own editor and is dropped here until it exists). The
                // *kind* is the item's own flag byte, which is the only way to
                // tell one settings item from another without fetching it.
                let Some(kind) = SettingsKind::from_item_flags(item.flags) else {
                    warn!(
                        "settings item {} has no recognisable kind flag; not opening",
                        item.item_id
                    );
                    continue;
                };
                settings_opens.write(crate::world_api::OpenSettingsEditor {
                    name: item.name.clone(),
                    asset_id: item.asset_id,
                    item_id: item.item_id,
                    folder_id: item.folder_id,
                    kind,
                    editable: item.permissions.owner.contains(Permissions::MODIFY),
                });
            }
            InventoryType::Animation => {
                reset_preview(
                    &ui.animation,
                    &item.name,
                    &children,
                    &mut texts,
                    &mut commands,
                );
                let animation = AssetKey::from(item.asset_id);
                state.animation = Some(animation);
                let buttons = commands
                    .spawn((
                        Node {
                            ..row(Val::Px(8.0))
                        },
                        ChildOf(ui.animation.content),
                    ))
                    .id();
                let play =
                    spawn_text_button(&mut commands, buttons, "animation-play-inworld", 1, true);
                commands.entity(play).observe(
                    move |press: On<Pointer<Press>>, mut commands: MessageWriter<SlCommand>| {
                        if press.button == PointerButton::Primary {
                            commands.write(SlCommand(Command::PlayAnimation(AnimationKey::from(
                                animation.uuid(),
                            ))));
                        }
                    },
                );
                let stop = spawn_text_button(&mut commands, buttons, "animation-stop", 2, true);
                commands.entity(stop).observe(
                    move |press: On<Pointer<Press>>, mut commands: MessageWriter<SlCommand>| {
                        if press.button == PointerButton::Primary {
                            commands.write(SlCommand(Command::StopAnimation(AnimationKey::from(
                                animation.uuid(),
                            ))));
                        }
                    },
                );
                show(&mut panels, ui.animation.panel);
            }
            _other => {}
        }
    }
}

/// Clear a preview floater's content and set its title to the item's name.
fn reset_preview(
    floater: &PreviewFloater,
    title: &str,
    children: &Query<&Children>,
    texts: &mut Query<&mut Text>,
    commands: &mut Commands,
) {
    if let Ok(existing) = children.get(floater.content) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
    if let Ok(mut text) = texts.get_mut(floater.title_text) {
        title.clone_into(&mut text.0);
    }
}

/// Show a floater.
fn show(panels: &mut Query<&mut UiPanelShown>, panel: Entity) {
    if let Ok(mut shown) = panels.get_mut(panel) {
        shown.0 = true;
    }
}

/// A bordered translated button (greyed when not `enabled`).
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
    enabled: bool,
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
            Name::new(format!("preview-button:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(PROPS_FONT_SIZE),
            TextColor(if enabled {
                LABEL_COLOR
            } else {
                DIM_LABEL_COLOR
            }),
            Pickable::IGNORE,
        ))
        .id()
}

/// A parsed landmark asset: the tiny `Landmark version 2` text body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandmarkAsset {
    /// The destination region's id.
    pub region_id: Uuid,
    /// The region-local position.
    pub position: (f32, f32, f32),
}

/// Parse a landmark asset body through the shared `sl_wire` codec
/// (`Landmark version 2\nregion_id <uuid>\nlocal_pos <x> <y> <z>`). `None`
/// when malformed, or for a legacy version-1 body (a global position with no
/// region id — nothing here can resolve it).
#[must_use]
pub fn parse_landmark(text: &str) -> Option<LandmarkAsset> {
    match sl_client_bevy::parse_landmark(text).ok()? {
        sl_client_bevy::WireLandmarkAsset::Regional {
            region_id,
            position,
        } => Some(LandmarkAsset {
            region_id,
            position: (position.x(), position.y(), position.z()),
        }),
        sl_client_bevy::WireLandmarkAsset::Global(_) => None,
    }
}

/// Swap the texture preview's placeholder for the decoded image once the
/// texture pipeline holds it.
fn poll_texture_preview(
    mut state: ResMut<PreviewState>,
    store: Res<DecodedTextures>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some((key, node)) = state.pending_texture else {
        return;
    };
    let Some(decoded) = store.get(key) else {
        return;
    };
    let handle = images.add(to_bevy_image(decoded));
    // Replace the placeholder's children with the image.
    if let Ok(existing) = children.get(node) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
    commands.entity(node).insert(ImageNode::new(handle));
    state.pending_texture = None;
}

#[cfg(test)]
mod tests {
    use super::{format_unix_date, parse_landmark};
    use pretty_assertions::assert_eq;

    /// The civil-date formatter round-trips known timestamps.
    #[test]
    fn dates_format_as_utc() {
        assert_eq!(format_unix_date(0), "1970-01-01 00:00");
        // 2026-07-22 00:00:00 UTC.
        assert_eq!(format_unix_date(1_784_678_400), "2026-07-22 00:00");
        // A leap day.
        assert_eq!(format_unix_date(951_782_400), "2000-02-29 00:00");
    }

    /// The landmark body parser reads the reference's tiny text format and
    /// rejects malformed bodies.
    #[test]
    fn landmarks_parse_region_and_position() {
        let parsed = parse_landmark(
            "Landmark version 2\nregion_id 3b6b7c62-8f8f-4e34-9c1a-79c2e2ba0fd1\nlocal_pos 128.5 64.25 22\n",
        );
        let Some(parsed) = parsed else {
            assert!(parsed.is_some(), "a well-formed landmark must parse");
            return;
        };
        assert_eq!(
            parsed.region_id.to_string(),
            "3b6b7c62-8f8f-4e34-9c1a-79c2e2ba0fd1"
        );
        assert_eq!(parsed.position, (128.5, 64.25, 22.0));
        assert!(parse_landmark("Landmark version 2\n").is_none());
        assert!(parse_landmark("").is_none());
    }

    /// **One window per item** (`viewer-keyed-floater-audit`): the open path,
    /// driven by the message an inventory row's Properties entry writes.
    mod instances {
        use super::super::{
            InventoryPropertiesPlugin, ItemPropertiesState, ItemPropertiesUi, OpenItemProperties,
            PermissionGates, properties_key,
        };
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::ui::UiRoot;
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            AgentKey, AssetType, InventoryFolderKey, InventoryKey, InventoryType, ItemInfo,
            LindenAmount, OwnerKey, Permissions, Permissions5, SaleInfo, SaleType, SlCommand,
            SlIdentity, Uuid,
        };

        /// A boxed error so tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// One inventory item, owned by the logged-in agent so its fields are
        /// editable (the read-only path spawns no fields to tell apart).
        fn item(id: u128, name: &str) -> ItemInfo {
            ItemInfo {
                item_id: InventoryKey::from(Uuid::from_u128(id)),
                folder_id: InventoryFolderKey::from(Uuid::from_u128(0x0F)),
                name: name.to_owned(),
                description: String::new(),
                asset_id: Uuid::from_u128(id.wrapping_add(0x1000)),
                asset_type: AssetType::Object,
                inv_type: InventoryType::Object,
                flags: 0,
                creation_date: 0,
                owner: OwnerKey::Agent(owner()),
                last_owner_id: Uuid::from_u128(0),
                creator_id: AgentKey::from(Uuid::from_u128(0)),
                group: None,
                // Fully permissive, so the controls under test are live: every
                // one of them is gated on a specific bit now
                // (`PermissionGates`), and a default (empty) mask would spawn a
                // window with nothing to click.
                permissions: Permissions5 {
                    base: all_rights(),
                    owner: all_rights(),
                    group: Permissions::empty(),
                    everyone: Permissions::empty(),
                    next_owner: all_rights(),
                },
                sale: SaleInfo::default(),
            }
        }

        /// Modify + copy + transfer, the mask a fully permissive item carries.
        fn all_rights() -> Permissions {
            Permissions::MODIFY | Permissions::COPY | Permissions::TRANSFER
        }

        /// The agent whose inventory these items are in.
        fn owner() -> AgentKey {
            AgentKey::from(Uuid::from_u128(0xA9))
        }

        /// An app with the floater manager and the properties plugin.
        fn properties_app() -> App {
            let mut app = App::new();
            let identity = SlIdentity {
                agent_id: Some(owner()),
                ..SlIdentity::default()
            };
            app.add_message::<SlCommand>()
                .insert_resource(identity)
                .init_resource::<crate::world_api::AvatarState>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<bevy::input_focus::InputFocus>()
                // The plugin's other half — the per-type previews — reads the
                // texture pipeline and hands notecards / scripts / landmarks to
                // their own floaters. Those seams are stood up empty: the
                // properties window under test never uses them, but every
                // system in the plugin must be able to run.
                .add_message::<crate::world_api::BoostTexture>()
                .add_message::<crate::world_api::OpenNotecard>()
                .add_message::<crate::world_api::OpenScript>()
                .add_message::<crate::inventory::OpenAboutLandmark>()
                .add_message::<crate::world_api::OpenSettingsEditor>()
                .init_resource::<crate::world_api::DecodedTextures>()
                .init_resource::<Assets<Image>>()
                .add_plugins((FloaterPlugin, InventoryPropertiesPlugin));
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open an item's properties the way the inventory row does.
        fn open(app: &mut App, item: &ItemInfo) {
            app.world_mut()
                .write_message(OpenItemProperties { item: item.clone() });
            app.update();
        }

        /// Every live properties window, as (entity, shown item id) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, Option<InventoryKey>)> {
            app.world_mut()
                .query::<(Entity, &ItemPropertiesState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.item.as_ref().map(|item| item.item_id)))
                .collect()
        }

        /// Two items are two windows, each showing its own item and carrying
        /// its own field entities.
        #[test]
        fn two_items_open_two_windows() -> Result<(), TestError> {
            let (first, second) = (item(0xA1, "A hat"), item(0xB2, "A chair"));
            let mut app = properties_app();
            open(&mut app, &first);
            open(&mut app, &second);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second item reused the first window"
            );
            let shown: Vec<Option<InventoryKey>> =
                open_windows.iter().map(|(_window, item)| *item).collect();
            assert!(shown.contains(&Some(first.item_id)) && shown.contains(&Some(second.item_id)));

            let world = app.world();
            let contents: Vec<Entity> = open_windows
                .iter()
                .filter_map(|(window, _item)| world.get::<ItemPropertiesUi>(*window))
                .map(|ui| ui.content)
                .collect();
            assert!(
                contents.first() != contents.get(1),
                "both windows build into one content column"
            );
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _item)| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            assert!(keys.contains(&Some(&properties_key(first.item_id))));
            assert!(keys.contains(&Some(&properties_key(second.item_id))));
            Ok(())
        }

        /// **Re-opening an item repaints its own window** rather than adding
        /// one — which is how a permission toggle refreshes every checkbox.
        #[test]
        fn reopening_an_item_repaints_its_window() -> Result<(), TestError> {
            let mut first = item(0xA1, "A hat");
            let mut app = properties_app();
            open(&mut app, &first);
            let window = windows(&mut app)
                .first()
                .map(|(window, _item)| *window)
                .ok_or("the item opened no window")?;

            // The toggle path: an updated snapshot re-opened on the same item.
            first.description = "now described".to_owned();
            open(&mut app, &first);

            let left = windows(&mut app);
            assert_eq!(left.len(), 1, "a repaint spawned a second window");
            assert_eq!(left.first().map(|(entity, _item)| *entity), Some(window));
            let described = app
                .world()
                .get::<ItemPropertiesState>(window)
                .and_then(|state| state.item.as_ref().map(|item| item.description.clone()));
            assert_eq!(
                described,
                Some("now described".to_owned()),
                "the window kept the stale snapshot"
            );
            Ok(())
        }

        /// **A price you cannot save is a price you cannot type.**
        ///
        /// The commit applies a typed price only to an item that is for sale
        /// (a price without a sale has nowhere to live, on the wire or in
        /// `ItemInfo`), so a live price field on a not-for-sale item offered an
        /// edit that was silently dropped. It is disabled until For Sale is
        /// ticked — the reference's own gating of its price spinner.
        #[test]
        fn the_price_field_is_dead_until_the_item_is_for_sale() -> Result<(), TestError> {
            let not_for_sale = item(0xC3, "A lamp");
            let mut for_sale = item(0xD4, "A rug");
            for_sale.sale = SaleInfo {
                sale_type: SaleType::Copy,
                price: LindenAmount(250),
            };

            let mut app = properties_app();
            open(&mut app, &not_for_sale);
            open(&mut app, &for_sale);

            let mut fields: Vec<(InventoryKey, bool)> = Vec::new();
            for (window, shown) in windows(&mut app) {
                let Some(item_id) = shown else { continue };
                let price = app
                    .world()
                    .get::<ItemPropertiesUi>(window)
                    .and_then(|ui| ui.price_field)
                    .ok_or("an editable item has no price field")?;
                let disabled = app
                    .world()
                    .get::<bevy::ui::InteractionDisabled>(price)
                    .is_some();
                fields.push((item_id, disabled));
            }
            fields.sort_by_key(|(item_id, _disabled)| item_id.to_string());
            assert_eq!(
                fields,
                vec![(not_for_sale.item_id, true), (for_sale.item_id, false)],
                "the not-for-sale item's price must be disabled, the for-sale one's live"
            );
            Ok(())
        }

        /// **Unticking For Sale keeps the price.**
        ///
        /// The price and the sale type are separate facts on the wire and in
        /// `SaleInfo`, and the reference keeps both: unticking must not throw
        /// the number away (the display would fall back to a made-up default,
        /// and the save would write a zero over what the grid holds).
        #[test]
        fn unticking_for_sale_keeps_the_price() {
            let mut offered = item(0xE5, "A bench");
            offered.sale = SaleInfo {
                sale_type: SaleType::Copy,
                price: LindenAmount(250),
            };

            // What the For Sale toggle does to the item.
            let unticked = SaleInfo::not_for_sale(offered.sale.price.clone());
            assert!(!unticked.is_for_sale(), "it is no longer offered");
            assert_eq!(
                unticked.price,
                LindenAmount(250),
                "and it still remembers what it was offered at"
            );

            // And what the wire then carries — the price rides along, so a save
            // cannot erase the grid's copy.
            let wire = super::super::to_wire_item(&ItemInfo {
                sale: unticked,
                ..offered
            });
            assert_eq!(wire.sale_type, SaleType::NotForSale.to_code());
            assert_eq!(wire.sale_price, Some(LindenAmount(250)));
        }

        /// **Every control follows the bit it is about.**
        ///
        /// The reference gates each of these separately
        /// (`LLFloaterProperties::refresh`), and a single "is it mine" flag —
        /// which is what this floater used to use — offered controls that
        /// could not do what they promised: letting anyone copy an item you
        /// may not copy, or granting a next owner a right the creator never
        /// permitted.
        #[test]
        fn each_control_follows_its_own_permission_bit() {
            let all = Permissions::MODIFY | Permissions::COPY | Permissions::TRANSFER;

            // Someone else's item: nothing is live.
            let mut theirs = item(0xF1, "Not yours");
            theirs.owner = OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xDEAD)));
            let gates = PermissionGates::of(&theirs, false);
            assert!(!gates.modifiable && !gates.share_with_group && !gates.sale);

            // Mine, fully permissive: everything is live.
            let mut mine = item(0xF2, "Mine");
            mine.permissions.owner = all;
            mine.permissions.base = all;
            mine.permissions.next_owner = all;
            let gates = PermissionGates::of(&mine, true);
            assert!(gates.modifiable && gates.share_with_group && gates.everyone_copy);
            assert!(gates.sale && gates.next_modify && gates.next_copy && gates.next_transfer);

            // No-copy: anyone-copy goes dead, the rest stays.
            let mut no_copy = mine.clone();
            no_copy.permissions.owner = Permissions::MODIFY | Permissions::TRANSFER;
            let gates = PermissionGates::of(&no_copy, true);
            assert!(!gates.everyone_copy, "cannot share a copy you cannot make");
            assert!(gates.share_with_group && gates.sale);

            // No-transfer: the whole sale / next-owner block goes dead, since
            // selling is passing the item on.
            let mut no_transfer = mine.clone();
            no_transfer.permissions.owner = Permissions::MODIFY | Permissions::COPY;
            let gates = PermissionGates::of(&no_transfer, true);
            assert!(!gates.sale && !gates.next_modify && !gates.next_copy);
            assert!(!gates.everyone_copy, "nor pass on what you cannot transfer");
            assert!(gates.modifiable && gates.share_with_group);

            // A creator who forbade copying bounds the next owner, however
            // permissive this owner's own rights are.
            let mut base_bound = mine.clone();
            base_bound.permissions.base = Permissions::MODIFY | Permissions::TRANSFER;
            let gates = PermissionGates::of(&base_bound, true);
            assert!(!gates.next_copy, "the creator's bound wins");
            assert!(gates.next_modify);

            // Next-owner transfer is about a copy they can hold, so it follows
            // their copy bit (the reference's own gate).
            let mut next_no_copy = mine.clone();
            next_no_copy.permissions.next_owner = Permissions::MODIFY | Permissions::TRANSFER;
            let gates = PermissionGates::of(&next_no_copy, true);
            assert!(!gates.next_transfer);

            // No modify: the text and every toggle go dead together.
            let mut no_modify = mine.clone();
            no_modify.permissions.owner = Permissions::COPY | Permissions::TRANSFER;
            let gates = PermissionGates::of(&no_modify, true);
            assert!(!gates.modifiable && !gates.share_with_group && !gates.sale);
        }

        /// Closing one item's window leaves the other open.
        #[test]
        fn closing_one_item_leaves_the_other() -> Result<(), TestError> {
            let (first, second) = (item(0xA1, "A hat"), item(0xB2, "A chair"));
            let mut app = properties_app();
            open(&mut app, &first);
            open(&mut app, &second);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, item)| (item == Some(first.item_id)).then_some(window))
                .ok_or("the first item has no window")?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: target,
                    op: FloaterOp::Close,
                });
            app.update();

            let left = windows(&mut app);
            assert_eq!(left.len(), 1);
            assert_eq!(
                left.first().and_then(|(_window, item)| *item),
                Some(second.item_id)
            );
            Ok(())
        }
    }
}
