//! The **About Landmark** floater (`viewer-about-landmark-floater`): the full
//! detail view for a landmark inventory item — the destination region's name
//! and coordinates, the destination parcel's name / description / snapshot /
//! maturity / owner / traffic, a copyable SLURL, editable item title / notes,
//! and the Teleport button. Opened by the inventory context menu's
//! **About Landmark** entry and by **Open** on a landmark
//! ([`crate::inventory_properties`] forwards its Landmark previews here).
//!
//! # Data flow
//!
//! The landmark asset body only carries the destination's region **id** and
//! region-local position, so the details resolve in three async steps, each
//! folded into the floater in place:
//!
//! 1. `FetchAsset` (landmark) → [`parse_landmark`] → region id + local
//!    position (the region line's fallback shows these raw).
//! 2. `RequestRemoteParcelId` (the `RemoteParcelRequest` capability) resolves
//!    region id + position → the grid-wide parcel id.
//! 3. `RequestParcelInfo` (`ParcelInfoRequest`) resolves the parcel id → a
//!    `ParcelInfoReply` carrying the region **name**, parcel name /
//!    description / snapshot / flags / owner / traffic — everything else the
//!    floater shows.
//!
//! The floater's chrome is spawned once at startup; the content column is
//! rebuilt per open (the picker-list pattern) and every later async update
//! mutates the built nodes in place — nothing is despawned per data update.
//!
//! Reference (Firestorm, read-only): `llpanellandmarkinfo.cpp`,
//! `llpanelplaceinfo.cpp`, `llfloatercreatelandmark.cpp`,
//! `llremoteparcelrequest.cpp`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AgentKey, AssetKey, AssetType, Command, GroupKey, ItemInfo, OwnerKey, ParcelDetails, ParcelKey,
    RegionCoordinates, RegionHandle, RegionName, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    TextureKey, Uuid, to_bevy_image,
};

use crate::avatars::AvatarState;
use crate::clipboard::{ViewerClipboard, copy_to_clipboard};
use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{Translated, Translator};
use crate::inventory::OpenAboutLandmark;
use crate::inventory_properties::{
    LandmarkAsset, format_unix_date, parse_landmark, send_item_update,
};
use crate::textures::TextureManager;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::GroupsModel;

/// The floater's font size, in logical pixels.
const ABOUT_FONT_SIZE: f32 = 14.0;

/// The value / label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer secondary label.
const DIM_LABEL_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// The parcel snapshot's box, matching the profile snapshot's 16:9.
const SNAPSHOT_SIZE: Vec2 = Vec2::new(272.0, 153.0);

/// The parcel description block's width, in logical pixels.
const DESCRIPTION_WIDTH: f32 = 420.0;

/// How long a parcel resolve may stay unanswered before the floater shows
/// "(parcel details unavailable)", in seconds.
const RESOLVE_TIMEOUT_SECONDS: f64 = 10.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Resources.
// ---------------------------------------------------------------------------

/// The floater's entities: the startup-spawned chrome, and the per-open value
/// nodes the async updates write into (all `None` until the first open).
#[derive(Resource)]
struct AboutLandmarkUi {
    /// The floater root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The rebuilt-per-open content column.
    content: Entity,
    /// The title text node (set to the item's name on open).
    title_text: Entity,
    /// The parcel snapshot's image box.
    snapshot_box: Option<Entity>,
    /// The snapshot box's placeholder label ("(loading)" / "(no image)").
    snapshot_label: Option<Entity>,
    /// The region line's value node (`SimName (x, y, z)`).
    region_text: Option<Entity>,
    /// The parcel name's value node.
    parcel_text: Option<Entity>,
    /// The parcel description's text node.
    description_text: Option<Entity>,
    /// The maturity rating's value node.
    maturity_text: Option<Entity>,
    /// The parcel owner's value node.
    owner_text: Option<Entity>,
    /// The traffic (dwell) value node.
    traffic_text: Option<Entity>,
    /// The area value node.
    area_text: Option<Entity>,
    /// The item creator's value node.
    creator_text: Option<Entity>,
    /// The SLURL value node.
    slurl_text: Option<Entity>,
    /// The item title editor (`None` when the item is not editable).
    name_field: Option<Entity>,
    /// The item notes editor (`None` when the item is not editable).
    notes_field: Option<Entity>,
}

/// The floater's live state: the shown item, the parsed landmark, and the
/// parcel resolve's progress.
///
/// # Correlation
///
/// The `RemoteParcelRequest` reply ([`SlSessionEvent::RemoteParcelId`])
/// carries **only** the parcel id — no echo of the requested region /
/// position — so replies cannot be matched to requests. This floater is
/// currently the only `RequestRemoteParcelId` sender in the viewer, and it
/// keeps a single await slot: a reply is taken as the answer to the newest
/// open while its [`deadline`](Self::deadline) is live. A reply landing after
/// a rapid re-open is attributed to the new landmark (bounded by the
/// deadline); any later consumer of the capability must add real correlation.
#[derive(Resource, Debug, Default)]
struct AboutLandmarkState {
    /// The item shown (as last received / edited).
    item: Option<ItemInfo>,
    /// The parsed landmark asset, once fetched.
    landmark: Option<LandmarkAsset>,
    /// The landmark asset awaited from `FetchAsset`.
    pending_asset: Option<Uuid>,
    /// Whether a `RemoteParcelRequest` reply is awaited.
    awaiting_remote: bool,
    /// The resolved grid-wide parcel id, once known.
    parcel_id: Option<ParcelKey>,
    /// The resolved parcel details, once received.
    details: Option<ParcelDetails>,
    /// The snapshot texture awaited from the texture pipeline, with the image
    /// box to fill.
    pending_snapshot: Option<(TextureKey, Entity)>,
    /// The absolute time (seconds) after which the resolve is abandoned.
    deadline: Option<f64>,
    /// The copyable SLURL, once the region name is known.
    slurl: Option<String>,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Wires the About Landmark floater: the open message, the resolve chain, the
/// snapshot poll, the name refreshes, and the title / notes editing.
#[derive(Debug)]
pub struct AboutLandmarkPlugin;

impl Plugin for AboutLandmarkPlugin {
    /// Register the message, state and systems; spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<AboutLandmarkState>()
            .add_message::<OpenAboutLandmark>()
            .add_systems(
                Startup,
                spawn_about_landmark_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    open_about_landmark,
                    ingest_landmark_asset,
                    ingest_parcel_replies,
                    poll_snapshot,
                    refresh_names,
                    commit_landmark_edits,
                    expire_resolve,
                )
                    .chain(),
            );
    }
}

/// Spawn the floater shell, hidden.
fn spawn_about_landmark_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: "about-landmark",
            title: "About Landmark".to_owned(),
            position: Vec2::new(420.0, 110.0),
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
    // Subject-bound: the shown landmark is not persisted, so neither is the
    // floater (like the item-properties / preview floaters).
    commands
        .entity(handle.root)
        .insert(crate::floater_persist::FloaterPersistExempt);
    commands
        .entity(handle.title_text)
        .insert(Translated::new("about-landmark-title"));
    commands.insert_resource(AboutLandmarkUi {
        panel: handle.root,
        content: handle.content,
        title_text: handle.title_text,
        snapshot_box: None,
        snapshot_label: None,
        region_text: None,
        parcel_text: None,
        description_text: None,
        maturity_text: None,
        owner_text: None,
        traffic_text: None,
        area_text: None,
        creator_text: None,
        slurl_text: None,
        name_field: None,
        notes_field: None,
    });
}

// ---------------------------------------------------------------------------
// Open: rebuild the content column on an item.
// ---------------------------------------------------------------------------

/// Rebuild and show the floater on the last open request: tear the old
/// content down, spawn the row skeleton seeded with the item-side values and
/// "(loading)" placeholders, and start the landmark asset fetch.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the open stream, the \
              floater state and handles, the identity and name sources, the translator, and \
              the spawn / visibility outputs"
)]
fn open_about_landmark(
    mut opens: MessageReader<OpenAboutLandmark>,
    mut state: ResMut<AboutLandmarkState>,
    ui: Option<ResMut<AboutLandmarkUi>>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    translator: Translator,
    children: Query<&Children>,
    mut panels: Query<&mut UiPanelShown>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let Some(open) = opens.read().last().cloned() else {
        return;
    };
    let item = open.item;

    // Tear the old content down (a discrete open, not a data update).
    if let Ok(existing) = children.get(ui.content) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
    if let Ok(mut text) = texts.get_mut(ui.title_text) {
        item.name.clone_into(&mut text.0);
    }
    let content = ui.content;
    let editable = matches!(item.owner, OwnerKey::Agent(agent) if Some(agent) == identity.agent_id);
    let loading = translator.get("about-landmark-loading");

    // Snapshot box.
    let snapshot_box = commands
        .spawn((
            Node {
                width: Val::Px(SNAPSHOT_SIZE.x),
                height: Val::Px(SNAPSHOT_SIZE.y),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            ChildOf(content),
        ))
        .id();
    let snapshot_label = commands
        .spawn((
            Text::new(loading.clone()),
            UiFont::Sans.at(ABOUT_FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            ChildOf(snapshot_box),
        ))
        .id();

    // Title / notes: editable for the item's owner, plain values otherwise.
    let name_row = spawn_labeled_row(&mut commands, content, "about-landmark-name");
    let name_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            &mut commands,
            name_row,
            &crate::ui_text_input::TextInputSpec {
                initial: item.name.clone(),
                font_size: ABOUT_FONT_SIZE,
                width_glyphs: 24.0,
                tab_index: 1,
                max_characters: Some(63),
                ..crate::ui_text_input::TextInputSpec::new(
                    "about-landmark-name",
                    crate::ui_text_input::TextInputKind::Line,
                )
            },
        )
    });
    if !editable {
        spawn_value(&mut commands, name_row, item.name.clone(), LABEL_COLOR);
    }
    let notes_row = spawn_labeled_row(&mut commands, content, "about-landmark-notes");
    let notes_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            &mut commands,
            notes_row,
            &crate::ui_text_input::TextInputSpec {
                initial: item.description.clone(),
                font_size: ABOUT_FONT_SIZE,
                width_glyphs: 24.0,
                tab_index: 2,
                max_characters: Some(127),
                ..crate::ui_text_input::TextInputSpec::new(
                    "about-landmark-notes",
                    crate::ui_text_input::TextInputKind::Line,
                )
            },
        )
    });
    if !editable {
        spawn_value(
            &mut commands,
            notes_row,
            item.description.clone(),
            LABEL_COLOR,
        );
    }

    // The destination rows, all "(loading)" until their resolve step lands.
    let region_row = spawn_labeled_row(&mut commands, content, "about-landmark-region");
    let region_text = spawn_value(&mut commands, region_row, loading.clone(), LABEL_COLOR);
    let parcel_row = spawn_labeled_row(&mut commands, content, "about-landmark-parcel");
    let parcel_text = spawn_value(&mut commands, parcel_row, loading.clone(), LABEL_COLOR);
    let description_text = commands
        .spawn((
            Node {
                max_width: Val::Px(DESCRIPTION_WIDTH),
                ..column(Val::Px(2.0))
            },
            ChildOf(content),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(ABOUT_FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
        ))
        .id();
    let maturity_row = spawn_labeled_row(&mut commands, content, "about-landmark-maturity");
    let maturity_text = spawn_value(&mut commands, maturity_row, loading.clone(), LABEL_COLOR);
    let owner_row = spawn_labeled_row(&mut commands, content, "about-landmark-owner");
    let owner_text = spawn_value(&mut commands, owner_row, loading.clone(), LABEL_COLOR);
    let traffic_row = spawn_labeled_row(&mut commands, content, "about-landmark-traffic");
    let traffic_text = spawn_value(&mut commands, traffic_row, loading.clone(), LABEL_COLOR);
    let area_row = spawn_labeled_row(&mut commands, content, "about-landmark-area");
    let area_text = spawn_value(&mut commands, area_row, loading.clone(), LABEL_COLOR);

    // Item-side rows: creator and acquired date.
    let creator_row = spawn_labeled_row(&mut commands, content, "about-landmark-creator");
    let creator_text = spawn_value(
        &mut commands,
        creator_row,
        agent_label(item.creator_id, &avatars),
        DIM_LABEL_COLOR,
    );
    if avatars.name_of(item.creator_id).is_none() {
        sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![
            item.creator_id,
        ])));
    }
    let acquired_row = spawn_labeled_row(&mut commands, content, "about-landmark-acquired");
    spawn_value(
        &mut commands,
        acquired_row,
        format_unix_date(i64::from(item.creation_date)),
        DIM_LABEL_COLOR,
    );

    // SLURL row: fills once the region name resolves.
    let slurl_row = spawn_labeled_row(&mut commands, content, "about-landmark-slurl");
    let slurl_text = spawn_value(&mut commands, slurl_row, loading, DIM_LABEL_COLOR);

    // Buttons: Teleport (works off the asset id alone) and Copy SLURL (a
    // no-op until the SLURL resolves — the row above shows the state).
    let buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let asset_id = item.asset_id;
    let teleport = spawn_button(&mut commands, buttons, "landmark-teleport", 3);
    commands.entity(teleport).observe(
        move |press: On<Pointer<Press>>, mut commands: MessageWriter<SlCommand>| {
            if press.button == PointerButton::Primary {
                commands.write(SlCommand(Command::TeleportViaLandmark {
                    landmark: Some(AssetKey::from(asset_id)),
                }));
            }
        },
    );
    let copy = spawn_button(&mut commands, buttons, "about-landmark-copy-slurl", 4);
    commands.entity(copy).observe(
        |press: On<Pointer<Press>>,
         state: Res<AboutLandmarkState>,
         clipboard: Res<ViewerClipboard>| {
            if press.button == PointerButton::Primary
                && let Some(slurl) = state.slurl.as_deref()
            {
                copy_to_clipboard(&clipboard, slurl);
            }
        },
    );

    // Reset the resolve state and start the chain with the asset fetch.
    *state = AboutLandmarkState {
        item: Some(item.clone()),
        pending_asset: Some(item.asset_id),
        ..AboutLandmarkState::default()
    };
    sl_commands.write(SlCommand(Command::FetchAsset {
        asset_id: AssetKey::from(item.asset_id),
        asset_type: AssetType::Landmark,
        byte_range: None,
    }));

    ui.snapshot_box = Some(snapshot_box);
    ui.snapshot_label = Some(snapshot_label);
    ui.region_text = Some(region_text);
    ui.parcel_text = Some(parcel_text);
    ui.description_text = Some(description_text);
    ui.maturity_text = Some(maturity_text);
    ui.owner_text = Some(owner_text);
    ui.traffic_text = Some(traffic_text);
    ui.area_text = Some(area_text);
    ui.creator_text = Some(creator_text);
    ui.slurl_text = Some(slurl_text);
    ui.name_field = name_field;
    ui.notes_field = notes_field;
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = true;
    }
}

// ---------------------------------------------------------------------------
// The resolve chain.
// ---------------------------------------------------------------------------

/// Fold the fetched landmark asset in: show the raw region id + position as
/// the region line's fallback and fire the `RemoteParcelRequest` resolve.
fn ingest_landmark_asset(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AboutLandmarkState>,
    ui: Option<Res<AboutLandmarkUi>>,
    translator: Translator,
    time: Res<Time>,
    mut texts: Query<&mut Text>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in events.read() {
        let SlSessionEvent::AssetReceived(asset) = &event.0 else {
            continue;
        };
        if state.pending_asset != Some(asset.id) {
            continue;
        }
        state.pending_asset = None;
        let text = String::from_utf8_lossy(&asset.data).into_owned();
        let Some(landmark) = parse_landmark(&text) else {
            set_text(
                &mut texts,
                ui.region_text,
                &translator.get("about-landmark-unreadable"),
            );
            continue;
        };
        set_text(
            &mut texts,
            ui.region_text,
            &region_line(None, landmark.region_id, landmark.position),
        );
        state.landmark = Some(landmark);
        state.awaiting_remote = true;
        state.deadline = Some(time.elapsed_secs_f64() + RESOLVE_TIMEOUT_SECONDS);
        let (x, y, z) = landmark.position;
        sl_commands.write(SlCommand(Command::RequestRemoteParcelId {
            location: RegionCoordinates::new(x, y, z),
            region_id: landmark.region_id,
            region_handle: RegionHandle::new(0),
        }));
    }
}

/// Fold the parcel resolve replies in: a `RemoteParcelId` advances the chain
/// to `RequestParcelInfo`; the matching `ParcelDetails` fills every
/// destination row, the SLURL and the snapshot request.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the event stream, the \
              floater state and handles, the name caches, the translator, the texture \
              pipeline and the text / command outputs"
)]
fn ingest_parcel_replies(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<AboutLandmarkState>,
    ui: Option<Res<AboutLandmarkUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    time: Res<Time>,
    mut textures: ResMut<TextureManager>,
    mut texts: Query<&mut Text>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RemoteParcelId(parcel_id) => {
                // Uncorrelated single-slot await — see [`AboutLandmarkState`].
                if !state.awaiting_remote || state.deadline.is_none() {
                    continue;
                }
                state.awaiting_remote = false;
                state.parcel_id = Some(*parcel_id);
                state.deadline = Some(time.elapsed_secs_f64() + RESOLVE_TIMEOUT_SECONDS);
                sl_commands.write(SlCommand(Command::RequestParcelInfo {
                    parcel_id: *parcel_id,
                }));
            }
            SlSessionEvent::ParcelDetails(details) => {
                if state.parcel_id != Some(details.parcel_id) || state.details.is_some() {
                    continue;
                }
                state.deadline = None;
                apply_details(
                    details,
                    &mut state,
                    &ui,
                    &avatars,
                    &groups,
                    &translator,
                    &mut textures,
                    &mut texts,
                    &mut sl_commands,
                );
                state.details = Some(details.clone());
            }
            _other => {}
        }
    }
}

/// Write a `ParcelDetails` into the floater: every destination row, the
/// SLURL, and the snapshot request.
#[expect(
    clippy::too_many_arguments,
    reason = "a helper extracted from a Bevy system inherits the system's injected resources"
)]
fn apply_details(
    details: &ParcelDetails,
    state: &mut AboutLandmarkState,
    ui: &AboutLandmarkUi,
    avatars: &AvatarState,
    groups: &GroupsModel,
    translator: &Translator,
    textures: &mut TextureManager,
    texts: &mut Query<&mut Text>,
    sl_commands: &mut MessageWriter<SlCommand>,
) {
    let position = state.landmark.map_or((0.0, 0.0, 0.0), |mark| mark.position);
    let region_id = state.landmark.map_or_else(Uuid::nil, |mark| mark.region_id);
    set_text(
        texts,
        ui.region_text,
        &region_line(details.sim_name.as_ref(), region_id, position),
    );
    let parcel_name = if details.name.is_empty() {
        translator.get("about-landmark-parcel-unnamed")
    } else {
        details.name.clone()
    };
    set_text(texts, ui.parcel_text, &parcel_name);
    set_text(texts, ui.description_text, &details.description);
    set_text(
        texts,
        ui.maturity_text,
        &translator.get(maturity_key(details.flags)),
    );
    set_text(
        texts,
        ui.owner_text,
        &parcel_owner_label(details, avatars, groups),
    );
    // Ask for the owner's name if unresolved; `refresh_names` rewrites the row
    // when the reply lands.
    if is_group_owned(details.flags) {
        groups.request_name(GroupKey::from(details.owner_id), sl_commands);
    } else if !details.owner_id.is_nil() {
        let owner = AgentKey::from(details.owner_id);
        if avatars.name_of(owner).is_none() {
            sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![owner])));
        }
    }
    set_text(texts, ui.traffic_text, &format!("{:.0}", details.dwell));
    set_text(texts, ui.area_text, &details.actual_area.to_string());
    // The SLURL uses the landmark's own saved position (the reference
    // behaviour), not the parcel anchor.
    if let Some(name) = details.sim_name.as_ref() {
        let slurl = landmark_slurl(name, position);
        set_text(texts, ui.slurl_text, &slurl);
        state.slurl = Some(slurl);
    }
    // Snapshot through the shared texture pipeline; a parcel without one
    // labels the box instead.
    let snapshot = details
        .snapshot_id
        .filter(|key| *key != TextureKey::from(Uuid::nil()));
    match (snapshot, ui.snapshot_box) {
        (Some(key), Some(node)) => {
            textures.request_boosted(key, AVATAR_BOOST_PRIORITY);
            state.pending_snapshot = Some((key, node));
        }
        _no_snapshot => {
            set_text(
                texts,
                ui.snapshot_label,
                &translator.get("about-landmark-no-image"),
            );
        }
    }
}

/// Swap the snapshot box's "(loading)" label for the decoded image once the
/// texture pipeline holds it. A re-open replaces the box, so a stale pending
/// node is dropped, not applied.
fn poll_snapshot(
    mut state: ResMut<AboutLandmarkState>,
    manager: Res<TextureManager>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some((key, node)) = state.pending_snapshot else {
        return;
    };
    let Some(decoded) = manager.decoded(key) else {
        return;
    };
    state.pending_snapshot = None;
    let Ok(mut entity) = commands.get_entity(node) else {
        return;
    };
    let handle = images.add(to_bevy_image(decoded));
    entity.insert(ImageNode::new(handle));
    if let Ok(existing) = children.get(node) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Rewrite the creator / parcel-owner rows when the avatar / group name
/// caches change, so a name requested at open / details time fills in.
fn refresh_names(
    state: Res<AboutLandmarkState>,
    ui: Option<Res<AboutLandmarkUi>>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut texts: Query<&mut Text>,
) {
    if !avatars.is_changed() && !groups.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    if let Some(item) = state.item.as_ref() {
        set_text(
            &mut texts,
            ui.creator_text,
            &agent_label(item.creator_id, &avatars),
        );
    }
    if let Some(details) = state.details.as_ref() {
        set_text(
            &mut texts,
            ui.owner_text,
            &parcel_owner_label(details, &avatars, &groups),
        );
    }
}

/// `Enter` in the title / notes fields commits the pending edits as one
/// `UpdateInventoryItem` and renames the floater title.
fn commit_landmark_edits(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Option<Res<AboutLandmarkUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<AboutLandmarkState>,
    mut texts: Query<&mut Text>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    let focused = focus.get();
    let editing = [ui.name_field, ui.notes_field]
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
    if let Some(notes) = read(ui.notes_field) {
        notes.trim().clone_into(&mut item.description);
    }
    send_item_update(&item, &mut sl_commands);
    if let Ok(mut text) = texts.get_mut(ui.title_text) {
        item.name.clone_into(&mut text.0);
    }
    state.item = Some(item);
}

/// Show "(parcel details unavailable)" when a resolve step stays unanswered
/// past its deadline (a missing / failing capability, a lost reply). The
/// asset-derived rows, Teleport and the title / notes editing keep working.
fn expire_resolve(
    mut state: ResMut<AboutLandmarkState>,
    ui: Option<Res<AboutLandmarkUi>>,
    translator: Translator,
    time: Res<Time>,
    mut texts: Query<&mut Text>,
) {
    let Some(deadline) = state.deadline else {
        return;
    };
    if time.elapsed_secs_f64() < deadline {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    state.deadline = None;
    state.awaiting_remote = false;
    info!("about landmark: parcel resolve timed out");
    set_text(
        &mut texts,
        ui.parcel_text,
        &translator.get("about-landmark-unavailable"),
    );
}

// ---------------------------------------------------------------------------
// Spawn / write helpers.
// ---------------------------------------------------------------------------

/// A labelled row: the translated label leading, the caller's value after.
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
        UiFont::Sans.at(ABOUT_FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Node {
            min_width: Val::Px(90.0),
            ..default()
        },
        ChildOf(row_entity),
    ));
    row_entity
}

/// A plain value label, returning its text entity for in-place updates.
fn spawn_value(commands: &mut Commands, parent: Entity, value: String, color: Color) -> Entity {
    commands
        .spawn((
            Text::new(value),
            UiFont::Sans.at(ABOUT_FONT_SIZE),
            TextColor(color),
            ChildOf(parent),
        ))
        .id()
}

/// A bordered translated button.
fn spawn_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            bevy::input_focus::tab_navigation::TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.34, 0.40, 0.52)),
            BackgroundColor(Color::srgb(0.13, 0.15, 0.20)),
            Pickable::default(),
            Name::new(format!("about-landmark:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(ABOUT_FONT_SIZE),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

/// Write `value` into an optional text node, only on a real change.
fn set_text(texts: &mut Query<&mut Text>, node: Option<Entity>, value: &str) {
    if let Some(node) = node
        && let Ok(mut text) = texts.get_mut(node)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// An agent's display label: the cached name, else the id in parentheses.
fn agent_label(agent: AgentKey, avatars: &AvatarState) -> String {
    avatars
        .name_of(agent)
        .map_or_else(|| format!("({agent})"), str::to_owned)
}

/// The parcel owner's display label: the group / agent name per the reply's
/// group-owned flag, falling back to the raw id while unresolved.
fn parcel_owner_label(
    details: &ParcelDetails,
    avatars: &AvatarState,
    groups: &GroupsModel,
) -> String {
    if details.owner_id.is_nil() {
        return String::new();
    }
    if is_group_owned(details.flags) {
        let group = GroupKey::from(details.owner_id);
        groups
            .group_name(group)
            .map_or_else(|| format!("({group})"), str::to_owned)
    } else {
        agent_label(AgentKey::from(details.owner_id), avatars)
    }
}

// ---------------------------------------------------------------------------
// Pure helpers.
// ---------------------------------------------------------------------------

/// The Fluent key for a `ParcelInfoReply` flags byte's maturity rating
/// (`0x2` adult, `0x1` mature, else general — the reference's decode).
const fn maturity_key(flags: u8) -> &'static str {
    if flags & 0x2 != 0 {
        "about-landmark-maturity-adult"
    } else if flags & 0x1 != 0 {
        "about-landmark-maturity-mature"
    } else {
        "about-landmark-maturity-pg"
    }
}

/// Whether a `ParcelInfoReply` flags byte marks the parcel group-owned
/// (`0x4`, the reference's decode).
const fn is_group_owned(flags: u8) -> bool {
    flags & 0x4 != 0
}

/// The region line: `Name (x, y, z)` once the region name is known, the raw
/// region id otherwise (the asset-only fallback).
fn region_line(
    sim_name: Option<&RegionName>,
    region_id: Uuid,
    position: (f32, f32, f32),
) -> String {
    let (x, y, z) = position;
    let coords = format!("({x:.0}, {y:.0}, {z:.0})");
    match sim_name {
        Some(name) => format!("{name} {coords}"),
        None => format!("{region_id} {coords}"),
    }
}

/// The landmark's maps-URL SLURL, from the resolved region name and the
/// landmark's own saved position. Coordinates clamp to the classic 256 m
/// SLURL grid (a var-region position past 255 m clamps — the standard SLURL
/// form cannot express it).
fn landmark_slurl(sim_name: &RegionName, position: (f32, f32, f32)) -> String {
    let (x, y, z) = position;
    sl_types::map::Location::new(
        sim_name.clone(),
        local_coord_u8(x),
        local_coord_u8(y),
        local_coord_u16(z),
    )
    .as_maps_url()
}

/// Clamp a region-local x / y coordinate to the SLURL's `u8` range.
const fn local_coord_u8(value: f32) -> u8 {
    let clamped = value.round().clamp(0.0, 255.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 255] just above"
    )]
    let out = clamped as u8;
    out
}

/// Clamp an altitude to the SLURL's `u16` range.
const fn local_coord_u16(value: f32) -> u16 {
    let clamped = value.round().clamp(0.0, 4095.0);
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [0, 4095] just above"
    )]
    let out = clamped as u16;
    out
}

#[cfg(test)]
mod tests {
    use super::{is_group_owned, landmark_slurl, maturity_key, region_line};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{RegionName, Uuid};

    /// The maturity flag bits map like the reference: adult wins over mature,
    /// no bits means general.
    #[test]
    fn maturity_maps_flag_bits() {
        assert_eq!(maturity_key(0x0), "about-landmark-maturity-pg");
        assert_eq!(maturity_key(0x1), "about-landmark-maturity-mature");
        assert_eq!(maturity_key(0x2), "about-landmark-maturity-adult");
        assert_eq!(maturity_key(0x3), "about-landmark-maturity-adult");
        // Group-owned does not affect the rating.
        assert_eq!(maturity_key(0x4), "about-landmark-maturity-pg");
    }

    /// The group-owned bit is `0x4` alone.
    #[test]
    fn group_owned_reads_bit_2() {
        assert!(is_group_owned(0x4));
        assert!(is_group_owned(0x7));
        assert!(!is_group_owned(0x3));
        assert!(!is_group_owned(0x0));
    }

    /// The SLURL uses the maps-URL form, escapes the region name and clamps
    /// out-of-range coordinates.
    #[test]
    fn slurls_format_and_clamp() -> Result<(), sl_types::map::RegionNameError> {
        let name = RegionName::try_new("Da Boom")?;
        assert_eq!(
            landmark_slurl(&name, (128.4, 64.6, 22.0)),
            "https://maps.secondlife.com/secondlife/Da%20Boom/128/65/22"
        );
        assert_eq!(
            landmark_slurl(&name, (999.0, -3.0, 9999.0)),
            "https://maps.secondlife.com/secondlife/Da%20Boom/255/0/4095"
        );
        Ok(())
    }

    /// The region line shows the name once known and the raw id before.
    #[test]
    fn region_line_prefers_the_name() -> Result<(), sl_types::map::RegionNameError> {
        let id = Uuid::from_u128(0x1234);
        assert_eq!(
            region_line(None, id, (12.4, 200.6, 30.0)),
            format!("{id} (12, 201, 30)")
        );
        let name = RegionName::try_new("Default Region")?;
        assert_eq!(
            region_line(Some(&name), id, (12.4, 200.6, 30.0)),
            "Default Region (12, 201, 30)"
        );
        Ok(())
    }
}
