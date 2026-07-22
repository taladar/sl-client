//! Item **Properties** and per-type **Open** previews
//! (`viewer-inventory-open-and-properties`): the item-properties floater —
//! name / description editing, creator / owner / acquired, the permission
//! toggles and sale settings, written back via `UpdateInventoryItem` — and
//! the small per-type preview floaters behind the context menu's Open:
//! a notecard reader, a texture / snapshot preview, About Landmark (with
//! Teleport), and an animation preview (play in-world / stop).
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
//! Reference (Firestorm, read-only): `llfloaterproperties.cpp`,
//! `skins/vintage/xui/en/floater_inventory_item_properties.xml`,
//! `llpreview{notecard,texture,anim}.cpp`, "About Landmark".

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AnimationKey, AssetKey, AssetType, Command, InventoryItem, InventoryType, ItemInfo,
    LindenAmount, Permissions, SaleType, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    TextureKey, TransactionId, Uuid, to_bevy_image,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::inventory::query_folder_page;
use crate::render_priority::AVATAR_BOOST_PRIORITY;
use crate::textures::TextureManager;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;

/// The chrome font size, in logical pixels.
const PROPS_FONT_SIZE: f32 = 14.0;

/// The label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A toggle's check glyph colour.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A button's background / border.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);
/// A button's border colour.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The checked / unchecked glyphs.
const CHECKED_GLYPH: &str = "\u{2611}";
/// The unchecked glyph.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The notecard / landmark preview text viewport height, in logical pixels.
const PREVIEW_TEXT_HEIGHT: f32 = 260.0;

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
pub(crate) struct OpenItemPreview {
    /// The item to preview.
    pub(crate) item: ItemInfo,
}

/// Whether this viewer has a preview for an item's type — gates the context
/// menu's Open entry.
pub(crate) const fn previewable(inv_type: InventoryType) -> bool {
    matches!(
        inv_type,
        InventoryType::Notecard
            | InventoryType::Texture
            | InventoryType::Snapshot
            | InventoryType::Landmark
            | InventoryType::Animation
    )
}

// ---------------------------------------------------------------------------
// Properties floater.
// ---------------------------------------------------------------------------

/// The properties floater's live state: the item shown, and the editable
/// permission / sale bits as currently displayed.
#[derive(Resource, Debug, Default)]
pub(crate) struct ItemPropertiesState {
    /// The item the floater shows (as last received).
    item: Option<ItemInfo>,
}

/// Entity handles for the properties floater.
#[derive(Resource)]
pub(crate) struct ItemPropertiesUi {
    /// The floater root.
    panel: Entity,
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
pub(crate) struct InventoryPropertiesPlugin;

impl Plugin for InventoryPropertiesPlugin {
    /// Register messages, state and systems; spawn the (hidden) floaters.
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemPropertiesState>()
            .init_resource::<PreviewState>()
            .add_message::<OpenItemProperties>()
            .add_message::<OpenItemPreview>()
            .add_systems(
                Startup,
                spawn_preview_floaters.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_properties,
                    commit_text_edits,
                    open_previews,
                    ingest_preview_assets,
                    poll_texture_preview,
                )
                    .chain(),
            );
    }
}

/// Spawn the properties floater and the four preview floaters, all hidden.
fn spawn_preview_floaters(mut commands: Commands, root: Res<UiRoot>) {
    // Properties.
    let properties = spawn_floater(
        &mut commands,
        root.0,
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
        },
    );
    commands
        .entity(properties.title_text)
        .insert(Translated::new("item-properties-title"));
    commands.insert_resource(ItemPropertiesUi {
        panel: properties.root,
        content: properties.content,
        name_field: None,
        desc_field: None,
        price_field: None,
    });

    // Notecard.
    let notecard = spawn_preview_floater(&mut commands, root.0, "preview-notecard", "Notecard");
    // Texture.
    let texture = spawn_preview_floater(&mut commands, root.0, "preview-texture", "Texture");
    // Landmark.
    let landmark = spawn_preview_floater(&mut commands, root.0, "preview-landmark", "Landmark");
    // Animation.
    let animation = spawn_preview_floater(&mut commands, root.0, "preview-animation", "Animation");
    commands.insert_resource(PreviewUi {
        notecard,
        texture,
        landmark,
        animation,
    });
}

/// Spawn one preview floater shell, returning its handles.
fn spawn_preview_floater(
    commands: &mut Commands,
    root: Entity,
    id: &'static str,
    title: &str,
) -> PreviewFloater {
    let handle = spawn_floater(
        commands,
        root,
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
        },
    );
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
    /// The notecard reader.
    notecard: PreviewFloater,
    /// The texture / snapshot preview.
    texture: PreviewFloater,
    /// The About Landmark floater.
    landmark: PreviewFloater,
    /// The animation preview.
    animation: PreviewFloater,
}

/// Rebuild and show the properties floater when asked.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open stream, the \
              floater state and handles, the identity and name sources, and the spawn / \
              visibility outputs"
)]
fn open_properties(
    mut opens: MessageReader<OpenItemProperties>,
    mut state: ResMut<ItemPropertiesState>,
    mut ui: ResMut<ItemPropertiesUi>,
    identity: Res<SlIdentity>,
    avatars: Res<crate::avatars::AvatarState>,
    children: Query<&Children>,
    mut panels: Query<&mut UiPanelShown>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(open) = opens.read().last().cloned() else {
        return;
    };
    let item = open.item;
    state.item = Some(item.clone());
    // Tear the old content down.
    if let Ok(existing) = children.get(ui.content) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
    let content = ui.content;
    let own = identity.agent_id;
    let editable =
        matches!(item.owner, sl_client_bevy::OwnerKey::Agent(agent) if Some(agent) == own);

    // Name / description rows.
    let name_row = spawn_labeled_row(&mut commands, content, "item-properties-name");
    let name_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            &mut commands,
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
        spawn_value_label(&mut commands, name_row, item.name.clone(), LABEL_COLOR);
    }
    let desc_row = spawn_labeled_row(&mut commands, content, "item-properties-description");
    let desc_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            &mut commands,
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
        spawn_value_label(
            &mut commands,
            desc_row,
            item.description.clone(),
            LABEL_COLOR,
        );
    }

    // Creator / owner / acquired.
    let name_of = |agent: sl_client_bevy::AgentKey| {
        avatars
            .name_of(agent)
            .map_or_else(|| format!("({agent})"), str::to_owned)
    };
    let creator_row = spawn_labeled_row(&mut commands, content, "item-properties-creator");
    spawn_value_label(
        &mut commands,
        creator_row,
        name_of(item.creator_id),
        DIM_LABEL_COLOR,
    );
    let owner_row = spawn_labeled_row(&mut commands, content, "item-properties-owner");
    let owner_label = match item.owner {
        sl_client_bevy::OwnerKey::Agent(agent) => name_of(agent),
        sl_client_bevy::OwnerKey::Group(group) => format!("(group {group})"),
    };
    spawn_value_label(&mut commands, owner_row, owner_label, DIM_LABEL_COLOR);
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
    let acquired_row = spawn_labeled_row(&mut commands, content, "item-properties-acquired");
    spawn_value_label(
        &mut commands,
        acquired_row,
        format_unix_date(i64::from(item.creation_date)),
        DIM_LABEL_COLOR,
    );

    // "You can:" — the owner mask, read-only.
    let you_row = spawn_labeled_row(&mut commands, content, "item-properties-you-can");
    let owner_mask = item.permissions.owner;
    for (label, bit) in [
        ("item-properties-modify", Permissions::MODIFY),
        ("item-properties-copy", Permissions::COPY),
        ("item-properties-transfer", Permissions::TRANSFER),
    ] {
        spawn_static_check(&mut commands, you_row, label, owner_mask.contains(bit));
    }

    // Group share / everyone copy toggles.
    let share_row = spawn_labeled_row(&mut commands, content, "item-properties-group");
    spawn_props_toggle(
        &mut commands,
        share_row,
        "item-properties-share",
        PropsToggle::ShareWithGroup,
        item.permissions.group.contains(Permissions::COPY),
        editable,
    );
    let anyone_row = spawn_labeled_row(&mut commands, content, "item-properties-anyone");
    spawn_props_toggle(
        &mut commands,
        anyone_row,
        "item-properties-copy",
        PropsToggle::EveryoneCopy,
        item.permissions.everyone.contains(Permissions::COPY),
        editable,
    );

    // Next owner toggles.
    let next_row = spawn_labeled_row(&mut commands, content, "item-properties-next-owner");
    let next = item.permissions.next_owner;
    for (label, toggle, bit) in [
        (
            "item-properties-modify",
            PropsToggle::NextModify,
            Permissions::MODIFY,
        ),
        (
            "item-properties-copy",
            PropsToggle::NextCopy,
            Permissions::COPY,
        ),
        (
            "item-properties-transfer",
            PropsToggle::NextTransfer,
            Permissions::TRANSFER,
        ),
    ] {
        spawn_props_toggle(
            &mut commands,
            next_row,
            label,
            toggle,
            next.contains(bit),
            editable,
        );
    }

    // For sale + type + price.
    let sale_row = spawn_labeled_row(&mut commands, content, "item-properties-for-sale");
    let (sale_type, sale_price) = item
        .sale
        .clone()
        .map_or((SaleType::NotForSale, LindenAmount(10)), |sale| {
            (sale.0, sale.1)
        });
    spawn_props_toggle(
        &mut commands,
        sale_row,
        "item-properties-for-sale",
        PropsToggle::ForSale,
        sale_type != SaleType::NotForSale,
        editable,
    );
    let type_button = spawn_text_button(
        &mut commands,
        sale_row,
        sale_type_key(sale_type),
        3,
        editable,
    );
    commands.entity(type_button).insert(PropsToggle::SaleType);
    let price_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            &mut commands,
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
        )
    });

    ui.name_field = name_field;
    ui.desc_field = desc_field;
    ui.price_field = price_field;
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
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
    commands.spawn((
        Text::new(if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(if on { CHECK_COLOR } else { DIM_LABEL_COLOR }),
        ChildOf(parent),
    ));
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(PROPS_FONT_SIZE),
        TextColor(LABEL_COLOR),
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
fn on_toggle_press(
    press: On<Pointer<Press>>,
    toggles: Query<&PropsToggle>,
    mut state: ResMut<ItemPropertiesState>,
    ui: Res<ItemPropertiesUi>,
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
            item.sale = match item.sale {
                Some(_sale) => None,
                None => Some((SaleType::Copy, sale_price_of(&ui, &fields))),
            };
        }
        PropsToggle::SaleType => {
            let price = sale_price_of(&ui, &fields);
            item.sale = Some(match item.sale {
                Some((SaleType::Original, _price)) => (SaleType::Copy, price),
                Some((SaleType::Copy, _price)) => (SaleType::Contents, price),
                _other => (SaleType::Original, price),
            });
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
        .map_or(LindenAmount(10), LindenAmount)
}

/// `Enter` in the name / description / price fields commits the pending text
/// edits as one `UpdateInventoryItem`.
fn commit_text_edits(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Res<ItemPropertiesUi>,
    fields: Query<&EditableText>,
    mut state: ResMut<ItemPropertiesState>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    // Only while one of the floater's fields holds focus.
    let focused = focus.get();
    let editing = [ui.name_field, ui.desc_field, ui.price_field]
        .into_iter()
        .flatten()
        .any(|field| Some(field) == focused);
    if !editing {
        return;
    }
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
    if item.sale.is_some()
        && let Some(price) = read(ui.price_field).and_then(|price| price.trim().parse::<u64>().ok())
        && let Some((sale_type, _old)) = item.sale
    {
        item.sale = Some((sale_type, LindenAmount(price)));
    }
    send_item_update(&item, &mut commands);
    state.item = Some(item);
}

/// Send an `UpdateInventoryItem` for the (edited) item and refresh its
/// folder page.
fn send_item_update(item: &ItemInfo, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::UpdateInventoryItem {
        item: Box::new(to_wire_item(item)),
        transaction_id: TransactionId::from(Uuid::nil()),
    }));
    query_folder_page(item.folder_id, commands);
}

/// Rebuild an [`ItemInfo`] into the wire `InventoryItem` an update carries
/// (shared with the COF link renumbering in [`crate::inventory_actions`]).
pub(crate) fn to_wire_item(item: &ItemInfo) -> InventoryItem {
    let (sale_type, sale_price) = match item.sale.clone() {
        Some((sale_type, price)) => (sale_type.to_code(), Some(price)),
        None => (SaleType::NotForSale.to_code(), None),
    };
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
pub(crate) fn format_unix_date(unix: i64) -> String {
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
    /// The notecard asset awaited (`FetchAsset` sent).
    pending_notecard: Option<Uuid>,
    /// The landmark asset awaited.
    pending_landmark: Option<Uuid>,
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
    mut textures: ResMut<TextureManager>,
    children: Query<&Children>,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for open in opens.read() {
        let item = &open.item;
        match item.inv_type {
            InventoryType::Notecard => {
                reset_preview(
                    &ui.notecard,
                    &item.name,
                    &children,
                    &mut texts,
                    &mut commands,
                );
                spawn_preview_text(
                    &mut commands,
                    ui.notecard.content,
                    "(loading)".to_owned(),
                    NotecardText,
                );
                state.pending_notecard = Some(item.asset_id);
                sl_commands.write(SlCommand(Command::FetchAsset {
                    asset_id: AssetKey::from(item.asset_id),
                    asset_type: AssetType::Notecard,
                    byte_range: None,
                }));
                show(&mut panels, ui.notecard.panel);
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
                textures.request_boosted(key, AVATAR_BOOST_PRIORITY);
                state.pending_texture = Some((key, placeholder));
                show(&mut panels, ui.texture.panel);
            }
            InventoryType::Landmark => {
                reset_preview(
                    &ui.landmark,
                    &item.name,
                    &children,
                    &mut texts,
                    &mut commands,
                );
                spawn_preview_text(
                    &mut commands,
                    ui.landmark.content,
                    "(loading)".to_owned(),
                    LandmarkText,
                );
                // The Teleport button works regardless of the asset fetch.
                let asset_id = item.asset_id;
                let teleport = spawn_text_button(
                    &mut commands,
                    ui.landmark.content,
                    "landmark-teleport",
                    1,
                    true,
                );
                commands.entity(teleport).observe(
                    move |press: On<Pointer<Press>>, mut commands: MessageWriter<SlCommand>| {
                        if press.button == PointerButton::Primary {
                            commands.write(SlCommand(Command::TeleportViaLandmark {
                                landmark: Some(AssetKey::from(asset_id)),
                            }));
                        }
                    },
                );
                state.pending_landmark = Some(item.asset_id);
                sl_commands.write(SlCommand(Command::FetchAsset {
                    asset_id: AssetKey::from(item.asset_id),
                    asset_type: AssetType::Landmark,
                    byte_range: None,
                }));
                show(&mut panels, ui.landmark.panel);
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

/// The marker on the notecard preview's text node.
#[derive(Component)]
struct NotecardText;

/// The marker on the landmark preview's text node.
#[derive(Component)]
struct LandmarkText;

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

/// Spawn a preview's wrapped text block.
fn spawn_preview_text(
    commands: &mut Commands,
    parent: Entity,
    text: String,
    marker: impl Component,
) {
    commands
        .spawn((
            Node {
                max_width: Val::Px(420.0),
                max_height: Val::Px(PREVIEW_TEXT_HEIGHT),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::new(text),
            UiFont::Sans.at(PROPS_FONT_SIZE),
            TextColor(LABEL_COLOR),
            marker,
        ));
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

/// Fold fetched notecard / landmark assets into their previews.
fn ingest_preview_assets(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<PreviewState>,
    mut notecard_texts: Query<&mut Text, (With<NotecardText>, Without<LandmarkText>)>,
    mut landmark_texts: Query<&mut Text, (With<LandmarkText>, Without<NotecardText>)>,
) {
    for event in events.read() {
        let SlSessionEvent::AssetReceived(asset) = &event.0 else {
            continue;
        };
        if state.pending_notecard == Some(asset.id) {
            state.pending_notecard = None;
            let text = match sl_notecard::Notecard::decode(&asset.data) {
                Ok(notecard) => notecard.text,
                Err(error) => format!("(failed to decode notecard: {error})"),
            };
            for mut node in &mut notecard_texts {
                node.0.clone_from(&text);
            }
        }
        if state.pending_landmark == Some(asset.id) {
            state.pending_landmark = None;
            let text = String::from_utf8_lossy(&asset.data).into_owned();
            let describe = parse_landmark(&text).map_or_else(
                || "(unreadable landmark)".to_owned(),
                |landmark| {
                    format!(
                        "Region: {}\nPosition: {:.1}, {:.1}, {:.1}",
                        landmark.region_id,
                        landmark.position.0,
                        landmark.position.1,
                        landmark.position.2
                    )
                },
            );
            for mut node in &mut landmark_texts {
                node.0.clone_from(&describe);
            }
        }
    }
}

/// A parsed landmark asset: the tiny `Landmark version 2` text body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LandmarkAsset {
    /// The destination region's id.
    pub(crate) region_id: Uuid,
    /// The region-local position.
    pub(crate) position: (f32, f32, f32),
}

/// Parse a landmark asset body (`Landmark version 2\nregion_id <uuid>\n
/// local_pos <x> <y> <z>`). `None` when malformed.
pub(crate) fn parse_landmark(text: &str) -> Option<LandmarkAsset> {
    let mut region_id = None;
    let mut position = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("region_id ") {
            region_id = rest.trim().parse::<Uuid>().ok();
        } else if let Some(rest) = line.strip_prefix("local_pos ") {
            let mut parts = rest.split_whitespace();
            let x = parts.next()?.parse::<f32>().ok()?;
            let y = parts.next()?.parse::<f32>().ok()?;
            let z = parts.next()?.parse::<f32>().ok()?;
            position = Some((x, y, z));
        }
    }
    Some(LandmarkAsset {
        region_id: region_id?,
        position: position?,
    })
}

/// Swap the texture preview's placeholder for the decoded image once the
/// texture pipeline holds it.
fn poll_texture_preview(
    mut state: ResMut<PreviewState>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some((key, node)) = state.pending_texture else {
        return;
    };
    let Some(decoded) = manager.decoded(key) else {
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
}
