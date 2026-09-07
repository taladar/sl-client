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
    AgentKey, AssetKey, AssetType, Command, GroupKey, InventoryKey, ItemInfo, OwnerKey,
    ParcelDetails, ParcelKey, RegionCoordinates, RegionHandle, RegionName, SlCommand, SlEvent,
    SlIdentity, SlSessionEvent, TextureKey, Uuid, to_bevy_image,
};

use crate::clipboard::{ViewerClipboard, copy_to_clipboard};
use crate::floater::{
    Floater, FloaterCaps, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen, KeyedFloaters,
    host_floater,
};
use crate::i18n::{Translated, Translator};
use crate::inventory::OpenAboutLandmark;
use crate::inventory_properties::{
    LandmarkAsset, format_unix_date, parse_landmark, send_item_update,
};
use crate::ui::{column, row};
use crate::ui_font::UiFont;
use crate::world_api::AVATAR_BOOST_PRIORITY;
use crate::world_api::AvatarState;
use crate::world_api::GroupsModel;
use crate::world_api::{BoostTexture, DecodedTextures};

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

/// One About Landmark window's entities — a component on the window, since the
/// floater opens per landmark ([`FloaterKey`]): its content column and the
/// value nodes the async updates write into.
#[derive(Component)]
struct AboutLandmarkUi {
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
/// position — so a reply cannot be matched to its request by content. With one
/// window that was merely untidy ("the newest open wins"); with a window per
/// landmark it would be wrong, since two windows can await different parcels
/// at once.
///
/// So the resolves are **serialised**: [`ParcelResolveQueue`] holds the windows
/// that have asked, one request is in flight at a time, and each reply belongs
/// to the window at the head of the queue. A window whose
/// [`deadline`](Self::deadline) passes leaves the queue and the next request
/// goes out. The real fix is a protocol-level echo (the capability is a
/// per-request POST, so the answer *could* carry its question) — filed as
/// `viewer-remote-parcel-id-uncorrelated`.
#[derive(Component, Debug, Default)]
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

/// The windows waiting on a `RemoteParcelRequest`, oldest first.
///
/// The capability's reply names no request (see [`AboutLandmarkState`]), so
/// only one is allowed in flight and the answer belongs to the window at the
/// head. A window that times out or closes leaves the queue, and the next
/// window's request goes out.
#[derive(Resource, Debug, Default)]
struct ParcelResolveQueue {
    /// The waiting windows, oldest first; the head owns the next reply.
    waiting: std::collections::VecDeque<Entity>,
    /// Whether the head's request has gone out and its answer is still due.
    in_flight: bool,
}

impl Plugin for AboutLandmarkPlugin {
    /// Register the message, the resolve queue and the systems.
    ///
    /// Nothing spawns at `Startup`: a window exists only while a landmark is
    /// open, so `open_about_landmark` spawns the instance and builds it.
    fn build(&self, app: &mut App) {
        app.init_resource::<ParcelResolveQueue>()
            .add_message::<OpenAboutLandmark>()
            .add_systems(
                Update,
                (
                    // After the manager's command pass — see `FloaterSystems`:
                    // the inventory row that opens a landmark also raises the
                    // window it sits in, and the later raise wins.
                    open_about_landmark.after(FloaterSystems::Commands),
                    // The resolve queue is folded before it is driven: an
                    // answer (or a timeout) frees the head, and the window
                    // behind it asks its question on the same frame rather
                    // than waiting one out.
                    (
                        ingest_landmark_asset,
                        ingest_parcel_replies,
                        expire_resolve,
                        drive_parcel_resolves,
                        poll_snapshot,
                        refresh_names,
                        commit_landmark_edits,
                    )
                        .chain()
                        .run_if(any_with_component::<AboutLandmarkState>),
                )
                    .chain(),
            );
    }
}

/// The about landmark floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn about_landmark_floater_spec() -> FloaterSpec {
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
    }
}

/// The [`FloaterKey`] of the window showing the landmark `item` — one window
/// per landmark, keyed by its inventory id. A subject key, so nothing is
/// persisted.
fn landmark_key(item: InventoryKey) -> FloaterKey {
    FloaterKey::subject(&item)
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
    mut floaters: KeyedFloaters,
    mut windows: Query<(&mut AboutLandmarkState, &mut AboutLandmarkUi)>,
    identity: Res<SlIdentity>,
    avatars: Res<AvatarState>,
    translator: Translator,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for open in opens.read().cloned() {
        let item = open.item;
        let opened = floaters.open(about_landmark_floater_spec(), landmark_key(item.item_id));
        match opened {
            KeyedFloaterOpen::Spawned(handle) => {
                commands
                    .entity(handle.title_text)
                    .insert(Translated::new("about-landmark-title"));
                let filled = fill_landmark_content(
                    &mut commands,
                    handle.content,
                    handle.title_text,
                    &item,
                    &identity,
                    &avatars,
                    &translator,
                    &mut texts,
                    &mut sl_commands,
                );
                commands.entity(handle.root).insert(filled);
            }
            KeyedFloaterOpen::Existing(window) => {
                let Ok((mut state, mut ui)) = windows.get_mut(window) else {
                    continue;
                };
                // A discrete re-open: tear the old content down and rebuild.
                if let Ok(existing) = children.get(ui.content) {
                    for child in existing.iter().collect::<Vec<_>>() {
                        commands.entity(child).despawn();
                    }
                }
                let (fresh_state, fresh_ui) = fill_landmark_content(
                    &mut commands,
                    ui.content,
                    ui.title_text,
                    &item,
                    &identity,
                    &avatars,
                    &translator,
                    &mut texts,
                    &mut sl_commands,
                );
                *state = fresh_state;
                *ui = fresh_ui;
            }
        }
    }
}

/// Build one landmark window's content under `content` and return the state and
/// handles it produced — the same body for a window just spawned and for one
/// being re-opened on another landmark.
#[expect(
    clippy::too_many_arguments,
    reason = "the content build takes what it draws from: the spawn target and title node, the \
              item, the identity and name sources, the translator, and the text / command sinks"
)]
fn fill_landmark_content(
    commands: &mut Commands,
    content: Entity,
    title_text: Entity,
    item: &ItemInfo,
    identity: &SlIdentity,
    avatars: &AvatarState,
    translator: &Translator,
    texts: &mut Query<&mut Text>,
    sl_commands: &mut MessageWriter<SlCommand>,
) -> (AboutLandmarkState, AboutLandmarkUi) {
    // The window's title is the landmark's own name.
    if let Ok(mut text) = texts.get_mut(title_text) {
        item.name.clone_into(&mut text.0);
    }
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
    let name_row = spawn_labeled_row(commands, content, "about-landmark-name");
    let name_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            commands,
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
        spawn_value(commands, name_row, item.name.clone(), LABEL_COLOR);
    }
    let notes_row = spawn_labeled_row(commands, content, "about-landmark-notes");
    let notes_field = editable.then(|| {
        crate::ui_text_input::spawn_text_input(
            commands,
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
        spawn_value(commands, notes_row, item.description.clone(), LABEL_COLOR);
    }

    // The destination rows, all "(loading)" until their resolve step lands.
    let region_row = spawn_labeled_row(commands, content, "about-landmark-region");
    let region_text = spawn_value(commands, region_row, loading.clone(), LABEL_COLOR);
    let parcel_row = spawn_labeled_row(commands, content, "about-landmark-parcel");
    let parcel_text = spawn_value(commands, parcel_row, loading.clone(), LABEL_COLOR);
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
    let maturity_row = spawn_labeled_row(commands, content, "about-landmark-maturity");
    let maturity_text = spawn_value(commands, maturity_row, loading.clone(), LABEL_COLOR);
    let owner_row = spawn_labeled_row(commands, content, "about-landmark-owner");
    let owner_text = spawn_value(commands, owner_row, loading.clone(), LABEL_COLOR);
    let traffic_row = spawn_labeled_row(commands, content, "about-landmark-traffic");
    let traffic_text = spawn_value(commands, traffic_row, loading.clone(), LABEL_COLOR);
    let area_row = spawn_labeled_row(commands, content, "about-landmark-area");
    let area_text = spawn_value(commands, area_row, loading.clone(), LABEL_COLOR);

    // Item-side rows: creator and acquired date.
    let creator_row = spawn_labeled_row(commands, content, "about-landmark-creator");
    let creator_text = spawn_value(
        commands,
        creator_row,
        agent_label(item.creator_id, avatars),
        DIM_LABEL_COLOR,
    );
    if avatars.name_of(item.creator_id).is_none() {
        sl_commands.write(SlCommand(Command::RequestAvatarNames(vec![
            item.creator_id,
        ])));
    }
    let acquired_row = spawn_labeled_row(commands, content, "about-landmark-acquired");
    spawn_value(
        commands,
        acquired_row,
        format_unix_date(i64::from(item.creation_date)),
        DIM_LABEL_COLOR,
    );

    // SLURL row: fills once the region name resolves.
    let slurl_row = spawn_labeled_row(commands, content, "about-landmark-slurl");
    let slurl_text = spawn_value(commands, slurl_row, loading, DIM_LABEL_COLOR);

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
    let teleport = spawn_button(commands, buttons, "landmark-teleport", 3);
    commands.entity(teleport).observe(
        move |press: On<Pointer<Press>>, mut commands: MessageWriter<SlCommand>| {
            if press.button == PointerButton::Primary {
                commands.write(SlCommand(Command::TeleportViaLandmark {
                    landmark: Some(AssetKey::from(asset_id)),
                }));
            }
        },
    );
    let copy = spawn_button(commands, buttons, "about-landmark-copy-slurl", 4);
    commands.entity(copy).observe(
        |press: On<Pointer<Press>>,
         parents: Query<&ChildOf>,
         floaters: Query<(Entity, &Floater)>,
         windows: Query<&AboutLandmarkState>,
         clipboard: Res<ViewerClipboard>| {
            if press.button != PointerButton::Primary {
                return;
            }
            // Copy *this* window's SLURL: with two landmarks open, the button
            // belongs to the one it sits in.
            let Some(window) = host_floater(press.entity, &parents, &floaters) else {
                return;
            };
            if let Ok(state) = windows.get(window)
                && let Some(slurl) = state.slurl.as_deref()
            {
                copy_to_clipboard(&clipboard, slurl);
            }
        },
    );

    // The window's state starts fresh for this landmark, and the chain starts
    // with the asset fetch.
    let state = AboutLandmarkState {
        item: Some(item.clone()),
        pending_asset: Some(item.asset_id),
        ..AboutLandmarkState::default()
    };
    sl_commands.write(SlCommand(Command::FetchAsset {
        asset_id: AssetKey::from(item.asset_id),
        asset_type: AssetType::Landmark,
        byte_range: None,
    }));

    (
        state,
        AboutLandmarkUi {
            content,
            title_text,
            snapshot_box: Some(snapshot_box),
            snapshot_label: Some(snapshot_label),
            region_text: Some(region_text),
            parcel_text: Some(parcel_text),
            description_text: Some(description_text),
            maturity_text: Some(maturity_text),
            owner_text: Some(owner_text),
            traffic_text: Some(traffic_text),
            area_text: Some(area_text),
            creator_text: Some(creator_text),
            slurl_text: Some(slurl_text),
            name_field,
            notes_field,
        },
    )
}

// ---------------------------------------------------------------------------
// The resolve chain.
// ---------------------------------------------------------------------------

/// Fold the fetched landmark asset in: show the raw region id + position as
/// the region line's fallback and fire the `RemoteParcelRequest` resolve.
fn ingest_landmark_asset(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<(Entity, &mut AboutLandmarkState, &AboutLandmarkUi)>,
    mut queue: ResMut<ParcelResolveQueue>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    // Collected once and replayed per window: a reader is consumed by the first
    // pass over it, so with two landmarks open the second would see nothing.
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for (window, mut state, ui) in &mut windows {
        for event in &frame {
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
            // Join the resolve queue rather than asking now: the capability's reply
            // names no request, so exactly one may be in flight
            // (`ParcelResolveQueue`). The deadline starts when the request actually
            // goes out, in `drive_parcel_resolves`.
            queue.waiting.push_back(window);
        }
    }
}

/// Send the head of the resolve queue's `RemoteParcelRequest`, one at a time.
///
/// The capability answers with a bare parcel id, so a second request in flight
/// would make the reply ambiguous between two windows. The head owns the
/// answer; its deadline starts here, when the question is actually asked.
fn drive_parcel_resolves(
    mut queue: ResMut<ParcelResolveQueue>,
    mut windows: Query<&mut AboutLandmarkState>,
    time: Res<Time>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if queue.in_flight {
        return;
    }
    // Drop any head that has gone away (a closed window) before asking.
    while let Some(&head) = queue.waiting.front() {
        let Ok(mut state) = windows.get_mut(head) else {
            let _gone = queue.waiting.pop_front();
            continue;
        };
        let Some(landmark) = state.landmark else {
            let _unresolvable = queue.waiting.pop_front();
            continue;
        };
        let (x, y, z) = landmark.position;
        sl_commands.write(SlCommand(Command::RequestRemoteParcelId {
            location: RegionCoordinates::new(x, y, z),
            region_id: landmark.region_id,
            region_handle: RegionHandle::new(0),
        }));
        state.deadline = Some(time.elapsed_secs_f64() + RESOLVE_TIMEOUT_SECONDS);
        queue.in_flight = true;
        return;
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
    mut windows: Query<(Entity, &mut AboutLandmarkState, &AboutLandmarkUi)>,
    mut queue: ResMut<ParcelResolveQueue>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    translator: Translator,
    time: Res<Time>,
    mut boost: MessageWriter<BoostTexture>,
    mut texts: Query<&mut Text>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for event in &frame {
        match &event.0 {
            // One reply, one window. The capability's answer names no request,
            // so it belongs to the window at the head of the queue — the only
            // one that has asked (`ParcelResolveQueue`) — and is consumed
            // there. Offering it to every window in turn would let the window
            // behind the head, which becomes the head the moment the first is
            // answered, take the same answer as its own.
            SlSessionEvent::RemoteParcelId(parcel_id) => {
                let Some(&head) = queue.waiting.front() else {
                    continue;
                };
                let Ok((_window, mut state, _ui)) = windows.get_mut(head) else {
                    continue;
                };
                if !state.awaiting_remote {
                    continue;
                }
                let _answered = queue.waiting.pop_front();
                queue.in_flight = false;
                state.awaiting_remote = false;
                state.parcel_id = Some(*parcel_id);
                state.deadline = Some(time.elapsed_secs_f64() + RESOLVE_TIMEOUT_SECONDS);
                sl_commands.write(SlCommand(Command::RequestParcelInfo {
                    parcel_id: *parcel_id,
                }));
            }
            // Details, unlike the resolve, name the parcel they are about, so
            // every window waiting on that parcel takes them — two landmarks in
            // one parcel are answered by one reply.
            SlSessionEvent::ParcelDetails(details) => {
                for (_window, mut state, ui) in &mut windows {
                    if state.parcel_id != Some(details.parcel_id) || state.details.is_some() {
                        continue;
                    }
                    state.deadline = None;
                    apply_details(
                        details,
                        &mut state,
                        ui,
                        &avatars,
                        &groups,
                        &translator,
                        &mut boost,
                        &mut texts,
                        &mut sl_commands,
                    );
                    state.details = Some(details.clone());
                }
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
    boost: &mut MessageWriter<BoostTexture>,
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
            boost.write(BoostTexture {
                key,
                priority: AVATAR_BOOST_PRIORITY,
            });
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
    mut windows: Query<&mut AboutLandmarkState>,
    store: Res<DecodedTextures>,
    mut images: ResMut<Assets<Image>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for mut state in &mut windows {
        let Some((key, node)) = state.pending_snapshot else {
            continue;
        };
        let Some(decoded) = store.get(key) else {
            continue;
        };
        state.pending_snapshot = None;
        let Ok(mut entity) = commands.get_entity(node) else {
            continue;
        };
        let handle = images.add(to_bevy_image(decoded));
        entity.insert(ImageNode::new(handle));
        if let Ok(existing) = children.get(node) {
            for child in existing.iter().collect::<Vec<_>>() {
                commands.entity(child).despawn();
            }
        }
    }
}

/// Rewrite the creator / parcel-owner rows when the avatar / group name
/// caches change, so a name requested at open / details time fills in.
fn refresh_names(
    windows: Query<(&AboutLandmarkState, &AboutLandmarkUi)>,
    avatars: Res<AvatarState>,
    groups: Res<GroupsModel>,
    mut texts: Query<&mut Text>,
) {
    if !avatars.is_changed() && !groups.is_changed() {
        return;
    }
    for (state, ui) in &windows {
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
}

/// `Enter` in the title / notes fields commits the pending edits as one
/// `UpdateInventoryItem` and renames the floater title.
fn commit_landmark_edits(
    keyboard: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut windows: Query<(&mut AboutLandmarkState, &AboutLandmarkUi)>,
    fields: Query<&EditableText>,
    mut texts: Query<&mut Text>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    // The commit belongs to the window whose field holds the keyboard — with
    // two landmarks open, Enter must save the one being typed in.
    let focused = focus.get();
    let Some((mut state, ui)) = windows.iter_mut().find(|(_state, ui)| {
        [ui.name_field, ui.notes_field]
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
    mut windows: Query<(Entity, &mut AboutLandmarkState, &AboutLandmarkUi)>,
    mut queue: ResMut<ParcelResolveQueue>,
    translator: Translator,
    time: Res<Time>,
    mut texts: Query<&mut Text>,
) {
    for (window, mut state, ui) in &mut windows {
        let Some(deadline) = state.deadline else {
            continue;
        };
        if time.elapsed_secs_f64() < deadline {
            continue;
        }
        state.deadline = None;
        state.awaiting_remote = false;
        // A timed-out head frees the resolve slot, so the next window's
        // request can go out (`ParcelResolveQueue`).
        if queue.waiting.front() == Some(&window) {
            let _expired = queue.waiting.pop_front();
            queue.in_flight = false;
        }
        info!("about landmark: parcel resolve timed out");
        set_text(
            &mut texts,
            ui.parcel_text,
            &translator.get("about-landmark-unavailable"),
        );
    }
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

    /// **One window per landmark** (`viewer-keyed-floater-audit`), and the
    /// serialised parcel resolve the keying forced.
    mod instances {
        use super::super::{
            AboutLandmarkPlugin, AboutLandmarkState, ParcelResolveQueue, landmark_key,
        };
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::inventory::OpenAboutLandmark;
        use crate::ui::UiRoot;
        use crate::world_api::{AvatarState, GroupsModel};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            AgentKey, Asset, AssetType, Command, InventoryFolderKey, InventoryKey, InventoryType,
            ItemInfo, OwnerKey, ParcelKey, Permissions5, SaleInfo, SlCommand, SlEvent, SlIdentity,
            SlSessionEvent, Uuid,
        };

        /// A boxed error so tests use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// One landmark inventory item.
        fn landmark(id: u128, name: &str) -> ItemInfo {
            ItemInfo {
                item_id: InventoryKey::from(Uuid::from_u128(id)),
                folder_id: InventoryFolderKey::from(Uuid::from_u128(0x0F)),
                name: name.to_owned(),
                description: String::new(),
                asset_id: Uuid::from_u128(id.wrapping_add(0x1000)),
                asset_type: AssetType::Landmark,
                inv_type: InventoryType::Landmark,
                flags: 0,
                creation_date: 0,
                owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0xA9))),
                last_owner_id: Uuid::from_u128(0),
                creator_id: AgentKey::from(Uuid::from_u128(0)),
                group: None,
                permissions: Permissions5::default(),
                sale: SaleInfo::default(),
            }
        }

        /// An app with the floater manager, this module's plugin, and the world
        /// facts its systems read — no grid, no window.
        fn landmark_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<crate::world_api::BoostTexture>()
                .init_resource::<AvatarState>()
                .init_resource::<GroupsModel>()
                .init_resource::<SlIdentity>()
                .init_resource::<crate::world_api::DecodedTextures>()
                .init_resource::<Assets<Image>>()
                .init_resource::<Time>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<bevy::input_focus::InputFocus>()
                .add_plugins((FloaterPlugin, AboutLandmarkPlugin));
            crate::i18n::install_untranslated(&mut app);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open a landmark the way the inventory row does.
        fn open(app: &mut App, item: &ItemInfo) {
            app.world_mut()
                .write_message(OpenAboutLandmark { item: item.clone() });
            app.update();
        }

        /// Every live landmark window, as (entity, shown item) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, Option<InventoryKey>)> {
            app.world_mut()
                .query::<(Entity, &AboutLandmarkState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.item.as_ref().map(|item| item.item_id)))
                .collect()
        }

        /// Two landmarks are two windows, each on its own item and keyed by it.
        #[test]
        fn two_landmarks_open_two_windows() -> Result<(), TestError> {
            let (first, second) = (landmark(0xA1, "Home"), landmark(0xB2, "Shop"));
            let mut app = landmark_app();
            open(&mut app, &first);
            open(&mut app, &second);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second landmark reused the first window"
            );
            let world = app.world();
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _item)| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            assert!(keys.contains(&Some(&landmark_key(first.item_id))));
            assert!(keys.contains(&Some(&landmark_key(second.item_id))));
            Ok(())
        }

        /// A `Landmark version 2` body pointing at `region` at 128/128/25.
        fn landmark_body(region: u128) -> Vec<u8> {
            format!(
                "Landmark version 2\nregion_id {}\nlocal_pos 128 128 25\n",
                Uuid::from_u128(region)
            )
            .into_bytes()
        }

        /// Hand both windows their landmark assets, so both join the resolve
        /// queue on the same frame.
        fn deliver_assets(app: &mut App, items: &[&ItemInfo]) {
            for item in items {
                app.world_mut()
                    .write_message(SlEvent(SlSessionEvent::AssetReceived(Box::new(Asset {
                        id: item.asset_id,
                        asset_type: AssetType::Landmark,
                        data: landmark_body(item.asset_id.as_u128()),
                    }))));
            }
            app.update();
        }

        /// How many `RemoteParcelRequest`s went out on the last frame.
        fn resolves_sent(app: &App) -> usize {
            app.world()
                .resource::<Messages<SlCommand>>()
                .iter_current_update_messages()
                .filter(|command| matches!(command.0, Command::RequestRemoteParcelId { .. }))
                .count()
        }

        /// **Only one parcel resolve is in flight.** The capability's reply
        /// names no request, so a second question would make the answer
        /// ambiguous between two windows; the queue asks in turn, and the
        /// reply lands on the window that asked.
        #[test]
        fn parcel_resolves_are_serialised() -> Result<(), TestError> {
            let (first, second) = (landmark(0xA1, "Home"), landmark(0xB2, "Shop"));
            let mut app = landmark_app();
            open(&mut app, &first);
            open(&mut app, &second);
            deliver_assets(&mut app, &[&first, &second]);

            assert_eq!(
                resolves_sent(&app),
                1,
                "both windows asked the capability at once"
            );
            let queue = app.world().resource::<ParcelResolveQueue>();
            assert!(queue.in_flight, "the head's question is not marked asked");
            assert_eq!(queue.waiting.len(), 2, "the second window left the queue");
            let head = *queue.waiting.front().ok_or("the queue emptied itself")?;

            // The one answer the capability gives belongs to the head — and the
            // window behind it then gets its turn.
            let parcel = ParcelKey::from(Uuid::from_u128(0xC3));
            app.world_mut()
                .write_message(SlEvent(SlSessionEvent::RemoteParcelId(parcel)));
            app.update();

            let answered = app
                .world()
                .get::<AboutLandmarkState>(head)
                .ok_or("the head window vanished")?;
            assert_eq!(answered.parcel_id, Some(parcel));
            assert_eq!(
                resolves_sent(&app),
                1,
                "the next window's question did not go out once the head was answered"
            );
            let queue = app.world().resource::<ParcelResolveQueue>();
            assert_eq!(queue.waiting.len(), 1, "the answered window stayed queued");
            Ok(())
        }

        /// Closing one landmark's window leaves the other open.
        #[test]
        fn closing_one_landmark_leaves_the_other() -> Result<(), TestError> {
            let (first, second) = (landmark(0xA1, "Home"), landmark(0xB2, "Shop"));
            let mut app = landmark_app();
            open(&mut app, &first);
            open(&mut app, &second);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, item)| (item == Some(first.item_id)).then_some(window))
                .ok_or("the first landmark has no window")?;

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
