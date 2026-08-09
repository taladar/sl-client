//! **Avatar / object inspector popups** (`viewer-inspector-popups`): the small,
//! self-dismissing mini-profile the reference pops when a resident or object name
//! is clicked — the `LLInspectAvatar` / `LLInspectObject` / `LLInspectRemoteObject`
//! family.
//!
//! # What it is
//!
//! A lightweight, cursor-anchored card — *not* a floater — that appears next to a
//! clicked name and **dismisses itself**: it lingers a moment after the pointer
//! leaves, and auto-closes after a short while if never hovered (the reference
//! `LLInspect` "fade on mouse-leave, capped lifetime" behaviour). Only one is ever
//! live; opening another replaces it. `Escape` closes it too.
//!
//! Two flavours, each fed by the linkification layer's clickable entity links
//! ([`crate::url_linkify`]):
//!
//! - **avatar** (`secondlife:///app/agent/<id>/inspect`, `LLInspectAvatar`): the
//!   resident's name, a snippet of their profile "about" text (fetched with
//!   [`Command::RequestAvatarProperties`]), and the quick actions — View Profile,
//!   IM, Add Friend, Offer Teleport.
//! - **object** — two sub-kinds sharing the one card:
//!   - a **remote** object (`secondlife:///app/objectim/<id>?...`,
//!     `LLInspectRemoteObject`): the name / owner / location ride in the link's
//!     query, so nothing needs fetching; the actions are Show on Map (route the
//!     carried SLURL back through the dispatcher) and Block.
//!   - an **in-world** object (`secondlife:///app/object/<id>/inspect`,
//!     `LLInspectObject`): only the key is known, so the name / owner /
//!     description are resolved from an [`Command::RequestObjectPropertiesFamily`]
//!     reply; the action is Block. (Touch / Sit / Buy need an in-region local id
//!     the click does not carry, so they are out of reach of a text-link
//!     inspector — the reference's in-world inspector operates on a picked object.)
//!
//! # Split with the dispatcher
//!
//! This module owns the `agent/.../inspect`, `objectim` and
//! `app/object/.../inspect` link targets; every other SLURL / app link is the
//! [`crate::slurl_dispatch`]'s ([[viewer-slurl-parse-dispatch]]). The two read the
//! same [`LinkActivated`] stream and partition it by target kind. Show on Map
//! hands its SLURL back to the dispatcher via [`DispatchSlurl`].
//!
//! Reference (Firestorm, read-only): `llinspectavatar.cpp`, `llinspectobject.cpp`,
//! `llinspectremoteobject.cpp`, `llinspect.cpp` (the self-dismiss base),
//! `llurlentry` (`.../inspect` actions).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::PrimaryWindow;

use sl_client_bevy::{
    AgentKey, Command, MuteFlags, MuteType, ObjectKey, OwnerKey, SlCommand, SlEvent, SlSessionEvent,
};

use crate::avatar_profile::OpenAvatarProfile;
use crate::avatars::AvatarState;
use crate::conversations::{ConversationKey, OpenConversation};
use crate::i18n::Translator;
use crate::linkified_text::LinkActivated;
use crate::slurl_dispatch::DispatchSlurl;
use crate::ui::{UiRoot, column, row};
use crate::ui_font::UiFont;
use crate::ui_name_link::{NameLink, NameLinkSpec, NameTarget, set_name_link, spawn_name_link};
use crate::url_linkify::LinkTarget;

/// The card's fixed width, in logical pixels.
const CARD_WIDTH: f32 = 280.0;

/// The card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// The gap between the card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 5.0;

/// The gap between the action buttons, in logical pixels.
const BUTTON_GAP: f32 = 5.0;

/// The name / title text size, in logical pixels.
const TITLE_FONT_SIZE: f32 = 15.0;

/// The body / button text size, in logical pixels.
const BODY_FONT_SIZE: f32 = 13.0;

/// The card's offset from the click point, in logical pixels (down-right, the
/// reference inspector placement).
const CURSOR_OFFSET: Vec2 = Vec2::new(12.0, 12.0);

/// A rough card-height reserve (logical px) kept off the window's bottom edge when
/// anchoring, so a downward-growing card stays on screen.
const CARD_HEIGHT_RESERVE: f32 = 40.0;

/// How long (seconds) the card stays after opening when the pointer never comes to
/// rest on it — the reference `LLInspect` capped lifetime.
const AUTO_CLOSE_SECONDS: f64 = 8.0;

/// How long (seconds) the card lingers after the pointer leaves it, before it
/// fades — the reference mouse-leave grace.
const LINGER_SECONDS: f64 = 0.7;

/// The longest profile-about snippet the avatar card shows, in characters, before
/// it is elided.
const SNIPPET_CHARS: usize = 160;

/// The card background (a dark, near-opaque surface).
const CARD_BACKGROUND: Color = Color::srgba(0.08, 0.09, 0.13, 0.98);

/// The card border colour.
const CARD_BORDER: Color = Color::srgb(0.32, 0.36, 0.44);

/// The title (name) text colour.
const TITLE_COLOR: Color = Color::srgb(0.94, 0.96, 1.0);

/// The body text colour.
const BODY_COLOR: Color = Color::srgb(0.82, 0.85, 0.90);

/// A dimmer label colour (the "Owner:" prefix).
const LABEL_COLOR: Color = Color::srgb(0.60, 0.64, 0.72);

/// An action button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// An action button's border.
const BUTTON_BORDER: Color = Color::srgb(0.38, 0.46, 0.58);

// ---------------------------------------------------------------------------
// Public messages.
// ---------------------------------------------------------------------------

/// Open the avatar inspector on `agent`, anchored near `at` (a screen position,
/// normally the click point).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenAvatarInspector {
    /// The resident to inspect.
    pub(crate) agent: AgentKey,
    /// The screen anchor (the click point).
    pub(crate) at: Vec2,
}

/// Open the object inspector, anchored near `at`.
#[derive(Message, Debug, Clone)]
pub(crate) struct OpenObjectInspector {
    /// The screen anchor (the click point).
    pub(crate) at: Vec2,
    /// Which object, and what is already known about it.
    pub(crate) target: ObjectInspectTarget,
}

/// Which object an [`OpenObjectInspector`] addresses, and the data the link
/// already carried.
#[derive(Debug, Clone)]
pub(crate) enum ObjectInspectTarget {
    /// A remote object announced in chat (`objectim`): its name / owner / location
    /// came in the link, so nothing is fetched.
    Remote {
        /// The object's key.
        key: ObjectKey,
        /// The object's name from the link.
        name: String,
        /// The object's owner, if the link named one.
        owner: Option<OwnerKey>,
        /// The object's location SLURL, if the link carried one (drives Show on
        /// Map).
        slurl: Option<String>,
    },
    /// An in-world object (`app/object/<id>/inspect`): only the key is known, so
    /// the name / owner / description come from an `ObjectPropertiesFamily` reply.
    InWorld {
        /// The object's key.
        key: ObjectKey,
    },
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// The single live inspector card, so opening another replaces it.
#[derive(Resource, Debug, Default)]
struct ActiveInspector(Option<Entity>);

/// A request to close the live inspector — written by an action button (after it
/// fires) and read by [`drive_inspector_lifecycle`].
#[derive(Message, Debug, Clone, Copy, Default)]
struct CloseInspector;

/// What a live card is inspecting, plus the entities its content updates write
/// into — the card is built once and updated in place (never rebuilt).
#[derive(Component, Debug)]
struct InspectorPopup {
    /// What is being inspected (routes the content-update replies).
    subject: InspectorSubject,
    /// The name / title text node.
    title_entity: Entity,
    /// The secondary detail text node (about snippet / description / location).
    detail_entity: Entity,
    /// The owner clickable name-link node (objects only), bound once the owner is
    /// known — it resolves the owner's name and opens their profile on click.
    owner_link: Option<Entity>,
    /// The owner to bind into [`owner_link`](Self::owner_link) (objects), once
    /// known.
    owner: Option<OwnerKey>,
    /// When (seconds) to auto-close if the pointer is not on the card; `None`
    /// while hovered.
    close_at: Option<f64>,
}

/// What an [`InspectorPopup`] is about, so a reply can be matched to it.
#[derive(Debug, Clone, Copy)]
enum InspectorSubject {
    /// A resident — the about snippet resolves from `AvatarProperties`.
    Avatar(AgentKey),
    /// An object — the name / owner / description resolve from
    /// `ObjectPropertiesFamily` (an in-world object) or came with the link.
    Object(ObjectKey),
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// Wires the inspector popups: the link routing, the two open handlers, the
/// content-reply / owner-name updaters, and the self-dismiss lifecycle.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InspectorPopupPlugin;

impl Plugin for InspectorPopupPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OpenAvatarInspector>()
            .add_message::<OpenObjectInspector>()
            .add_message::<CloseInspector>()
            .init_resource::<ActiveInspector>()
            .add_systems(
                Update,
                (
                    open_inspectors_from_links,
                    open_avatar_inspector,
                    open_object_inspector,
                    update_inspector_content,
                    bind_inspector_owner,
                    update_inspector_hover,
                    drive_inspector_lifecycle,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Link routing.
// ---------------------------------------------------------------------------

/// Route the inspector's share of the [`LinkActivated`] stream: an
/// `agent/<id>/inspect` link opens the avatar inspector; an `objectim` link the
/// remote object inspector; an `app/object/<id>/inspect` link the in-world object
/// inspector. Every card anchors at the current cursor position (the click
/// point). Cross-grid links are skipped (their data is not in our caches).
fn open_inspectors_from_links(
    mut activated: MessageReader<LinkActivated>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut avatar_inspectors: MessageWriter<OpenAvatarInspector>,
    mut object_inspectors: MessageWriter<OpenObjectInspector>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .unwrap_or(Vec2::ZERO);
    for event in activated.read() {
        match &event.target {
            LinkTarget::Agent {
                key,
                action,
                grid: None,
            } if action.eq_ignore_ascii_case("inspect") => {
                avatar_inspectors.write(OpenAvatarInspector {
                    agent: *key,
                    at: cursor,
                });
            }
            LinkTarget::Object {
                key,
                name,
                owner,
                slurl,
                grid: None,
            } => {
                object_inspectors.write(OpenObjectInspector {
                    at: cursor,
                    target: ObjectInspectTarget::Remote {
                        key: *key,
                        name: name.clone(),
                        owner: owner.map(|id| OwnerKey::Agent(AgentKey::from(id))),
                        slurl: slurl.clone(),
                    },
                });
            }
            LinkTarget::ObjectAction {
                key,
                action,
                grid: None,
            } if action.eq_ignore_ascii_case("inspect") => {
                object_inspectors.write(OpenObjectInspector {
                    at: cursor,
                    target: ObjectInspectTarget::InWorld { key: *key },
                });
            }
            _other => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Opening a card.
// ---------------------------------------------------------------------------

/// Open (or replace) the avatar inspector: build the card, wire the quick
/// actions, request the resident's name and profile about-text, and store it as
/// the live inspector.
#[expect(
    clippy::too_many_arguments,
    reason = "an open handler fuses the request stream, the live-card slot, the \
              window / UI-root anchors, the name cache, the translator, the clock, \
              and the two command channels"
)]
fn open_avatar_inspector(
    mut requests: MessageReader<OpenAvatarInspector>,
    mut active: ResMut<ActiveInspector>,
    windows: Query<&Window, With<PrimaryWindow>>,
    root: Res<UiRoot>,
    avatars: Res<AvatarState>,
    translator: Translator,
    time: Res<Time>,
    mut commands: Commands,
    mut sl: MessageWriter<SlCommand>,
) {
    let requests: Vec<OpenAvatarInspector> = requests.read().copied().collect();
    let Some(request) = requests.last().copied() else {
        return;
    };
    close_active(&mut commands, &mut active);

    let card = build_card(&mut commands, &root, &windows, request.at);
    set_text(
        &mut commands,
        card.title,
        &avatars.label_text(request.agent),
    );
    set_text(
        &mut commands,
        card.detail,
        &translator.get("inspector-loading"),
    );

    // The quick actions the viewer wires (the reference LLInspectAvatar buttons).
    let agent = request.agent;
    add_button(
        &mut commands,
        card.actions,
        &translator.get("inspector-button-profile"),
        1,
    )
    .observe(
        move |_activate: On<Activate>,
              mut profiles: MessageWriter<OpenAvatarProfile>,
              mut close: MessageWriter<CloseInspector>| {
            profiles.write(OpenAvatarProfile { agent });
            close.write(CloseInspector);
        },
    );
    add_button(
        &mut commands,
        card.actions,
        &translator.get("inspector-button-im"),
        2,
    )
    .observe(
        move |_activate: On<Activate>,
              mut conversations: MessageWriter<OpenConversation>,
              mut close: MessageWriter<CloseInspector>| {
            conversations.write(OpenConversation {
                key: ConversationKey::Direct(agent),
            });
            close.write(CloseInspector);
        },
    );
    add_button(
        &mut commands,
        card.actions,
        &translator.get("inspector-button-add-friend"),
        3,
    )
    .observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut close: MessageWriter<CloseInspector>| {
            sl.write(SlCommand(Command::OfferFriendship {
                to_agent_id: agent,
                message: String::new(),
            }));
            close.write(CloseInspector);
        },
    );
    add_button(
        &mut commands,
        card.actions,
        &translator.get("inspector-button-offer-teleport"),
        4,
    )
    .observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut close: MessageWriter<CloseInspector>| {
            sl.write(SlCommand(Command::OfferTeleport {
                targets: vec![agent],
                message: String::new(),
            }));
            close.write(CloseInspector);
        },
    );

    finish_card(
        &mut commands,
        &mut active,
        time.elapsed_secs_f64(),
        card,
        InspectorSubject::Avatar(request.agent),
        None,
        None,
    );

    sl.write(SlCommand(Command::RequestAvatarProperties(request.agent)));
    if avatars.name_of(request.agent).is_none() {
        sl.write(SlCommand(Command::RequestAvatarNames(vec![request.agent])));
    }
}

/// Open (or replace) the object inspector: build the card from what the link
/// carried, wire the actions, and (for an in-world object) request its
/// properties.
#[expect(
    clippy::too_many_arguments,
    reason = "an open handler fuses the request stream, the live-card slot, the \
              window / UI-root anchors, the translator, the clock, and the two \
              command channels"
)]
fn open_object_inspector(
    mut requests: MessageReader<OpenObjectInspector>,
    mut active: ResMut<ActiveInspector>,
    windows: Query<&Window, With<PrimaryWindow>>,
    root: Res<UiRoot>,
    translator: Translator,
    time: Res<Time>,
    mut commands: Commands,
    mut sl: MessageWriter<SlCommand>,
) {
    let requests: Vec<OpenObjectInspector> = requests.read().cloned().collect();
    let Some(request) = requests.last().cloned() else {
        return;
    };
    close_active(&mut commands, &mut active);

    let card = build_card(&mut commands, &root, &windows, request.at);
    let owner_link = spawn_owner_row(&mut commands, card.body, &translator);

    let (key, owner, block_name) = match request.target {
        ObjectInspectTarget::Remote {
            key,
            name,
            owner,
            slurl,
        } => {
            let shown = object_title(&name, &translator);
            set_text(&mut commands, card.title, &shown);
            set_text(&mut commands, card.detail, slurl.as_deref().unwrap_or(""));
            if let Some(slurl) = slurl {
                let url = as_location_url(&slurl);
                add_button(
                    &mut commands,
                    card.actions,
                    &translator.get("inspector-button-show-on-map"),
                    1,
                )
                .observe(
                    move |_activate: On<Activate>,
                          mut dispatch: MessageWriter<DispatchSlurl>,
                          mut close: MessageWriter<CloseInspector>| {
                        dispatch.write(DispatchSlurl { url: url.clone() });
                        close.write(CloseInspector);
                    },
                );
            }
            (key, owner, shown)
        }
        ObjectInspectTarget::InWorld { key } => {
            set_text(
                &mut commands,
                card.title,
                &translator.get("inspector-loading"),
            );
            set_text(&mut commands, card.detail, "");
            sl.write(SlCommand(Command::RequestObjectPropertiesFamily {
                request_flags: 0,
                object_id: key,
            }));
            (key, None, String::new())
        }
    };

    // Block (mute) the object — both sub-kinds.
    let block_id = key.uuid();
    add_button(
        &mut commands,
        card.actions,
        &translator.get("inspector-button-block"),
        2,
    )
    .observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut close: MessageWriter<CloseInspector>| {
            sl.write(SlCommand(Command::Mute {
                id: block_id,
                name: block_name.clone(),
                mute_type: MuteType::Object,
                flags: MuteFlags::default(),
            }));
            close.write(CloseInspector);
        },
    );

    finish_card(
        &mut commands,
        &mut active,
        time.elapsed_secs_f64(),
        card,
        InspectorSubject::Object(key),
        Some(owner_link),
        owner,
    );
}

// ---------------------------------------------------------------------------
// Content updates (replies fill a live card in place).
// ---------------------------------------------------------------------------

/// Update the live card as its replies arrive: the avatar about snippet
/// (`AvatarProperties`), or an in-world object's name / owner / description
/// (`ObjectPropertiesFamily`). Writes only the live card, in place.
fn update_inspector_content(
    mut events: MessageReader<SlEvent>,
    active: Res<ActiveInspector>,
    mut popups: Query<&mut InspectorPopup>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(entity) = active.0 else {
        return;
    };
    let Ok(mut popup) = popups.get_mut(entity) else {
        return;
    };
    for event in events.read() {
        match (popup.subject, &event.0) {
            (InspectorSubject::Avatar(agent), SlSessionEvent::AvatarProperties(properties))
                if properties.avatar_id == agent =>
            {
                let about = properties.about_text.trim();
                let shown = if about.is_empty() {
                    translator.get("inspector-no-bio")
                } else {
                    snippet(about, SNIPPET_CHARS)
                };
                write_text(&mut texts, popup.detail_entity, &shown);
            }
            (
                InspectorSubject::Object(key),
                SlSessionEvent::ObjectPropertiesFamily { properties },
            ) if properties.object_id == key => {
                write_text(
                    &mut texts,
                    popup.title_entity,
                    &object_title(&properties.name, &translator),
                );
                write_text(
                    &mut texts,
                    popup.detail_entity,
                    properties.description.trim(),
                );
                if popup.owner != Some(properties.owner) {
                    popup.owner = Some(properties.owner);
                }
            }
            _other => {}
        }
    }
}

/// Bind the live object card's owner into its name-link once the owner is known
/// (the link was spawned with deferred commands, so the binding happens the frame
/// after it exists). The shared name-link widget then resolves the name, requests
/// it if absent, and opens the owner's profile on click. Idempotent — it only
/// writes on a real change — so running it every frame is cheap.
fn bind_inspector_owner(
    active: Res<ActiveInspector>,
    popups: Query<&InspectorPopup>,
    mut name_links: Query<&mut NameLink>,
) {
    let Some(entity) = active.0 else {
        return;
    };
    let Ok(popup) = popups.get(entity) else {
        return;
    };
    if let Some(owner) = popup.owner {
        set_name_link(&mut name_links, popup.owner_link, NameTarget::Set(owner));
    }
}

// ---------------------------------------------------------------------------
// Lifecycle: self-dismiss.
// ---------------------------------------------------------------------------

/// Track whether the pointer is resting on the live card (or any of its
/// children): while it is, cancel the auto-close; when it leaves, arm the linger
/// timer so the card fades shortly after.
fn update_inspector_hover(
    active: Res<ActiveInspector>,
    mut popups: Query<&mut InspectorPopup>,
    hover_map: Res<HoverMap>,
    parents: Query<&ChildOf>,
    time: Res<Time>,
) {
    let Some(entity) = active.0 else {
        return;
    };
    let Ok(mut popup) = popups.get_mut(entity) else {
        return;
    };
    let over = hover_map
        .0
        .values()
        .flat_map(|hits| hits.keys())
        .any(|hovered| is_self_or_descendant(*hovered, entity, &parents));
    let was_hovered = popup.close_at.is_none();
    if over {
        if !was_hovered {
            popup.close_at = None;
        }
    } else if was_hovered {
        // Just left: start the linger countdown.
        popup.close_at = Some(time.elapsed_secs_f64() + LINGER_SECONDS);
    }
}

/// Drive the self-dismiss: close on an explicit request (an action fired), on
/// `Escape`, or on the auto-close / linger timer expiring while the pointer is off
/// the card.
fn drive_inspector_lifecycle(
    mut closes: MessageReader<CloseInspector>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut active: ResMut<ActiveInspector>,
    popups: Query<&InspectorPopup>,
    mut commands: Commands,
) {
    let requested = closes.read().count() > 0 || keyboard.just_pressed(KeyCode::Escape);
    let Some(entity) = active.0 else {
        return;
    };
    let expired = popups
        .get(entity)
        .ok()
        .and_then(|popup| popup.close_at)
        .is_some_and(|close_at| time.elapsed_secs_f64() >= close_at);
    if requested || expired {
        close_active(&mut commands, &mut active);
    }
}

/// Despawn the live card (if any) and clear the slot.
fn close_active(commands: &mut Commands, active: &mut ActiveInspector) {
    if let Some(entity) = active.0.take() {
        commands.entity(entity).despawn();
    }
}

/// Whether `entity` is `root` or a descendant of it (walking up the `ChildOf`
/// chain), so a hover on any of the card's children counts as a hover on the card.
fn is_self_or_descendant(entity: Entity, root: Entity, parents: &Query<&ChildOf>) -> bool {
    let mut current = entity;
    loop {
        if current == root {
            return true;
        }
        match parents.get(current) {
            Ok(child_of) => current = child_of.parent(),
            Err(_no_parent) => return false,
        }
    }
}

// ---------------------------------------------------------------------------
// Card construction.
// ---------------------------------------------------------------------------

/// The entities a freshly-built card exposes to its opener.
struct CardParts {
    /// The card root node.
    root: Entity,
    /// The title (name) text node.
    title: Entity,
    /// The detail (about / description / location) text node.
    detail: Entity,
    /// The body column, where the owner row is added.
    body: Entity,
    /// The action button row.
    actions: Entity,
}

/// Build the empty card shell anchored near `at` (clamped on-screen): a bordered
/// absolute box holding a title, a body column (detail + owner row) and an action
/// row. Content is filled by the caller.
fn build_card(
    commands: &mut Commands,
    root: &UiRoot,
    windows: &Query<&Window, With<PrimaryWindow>>,
    at: Vec2,
) -> CardParts {
    let position = clamp_on_screen(windows, at);
    let root_entity = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y),
                width: Val::Px(CARD_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(1.0)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(CARD_BORDER),
            GlobalZIndex(1200),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("inspector-popup"),
            ChildOf(root.0),
        ))
        .id();
    let title = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans
                .at(TITLE_FONT_SIZE)
                .with_font_weight(bevy::text::FontWeight::BOLD),
            TextColor(TITLE_COLOR),
            Pickable::IGNORE,
            ChildOf(root_entity),
        ))
        .id();
    let body = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            Pickable::IGNORE,
            ChildOf(root_entity),
        ))
        .id();
    let detail = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(BODY_FONT_SIZE),
            TextColor(BODY_COLOR),
            Node {
                max_width: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(body),
        ))
        .id();
    let actions = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                ..row(Val::Px(BUTTON_GAP))
            },
            Pickable::IGNORE,
            ChildOf(root_entity),
        ))
        .id();
    CardParts {
        root: root_entity,
        title,
        detail,
        body,
        actions,
    }
}

/// Register a freshly-built card as the live inspector and give it its
/// [`InspectorPopup`] state, arming the auto-close timer.
fn finish_card(
    commands: &mut Commands,
    active: &mut ActiveInspector,
    now: f64,
    card: CardParts,
    subject: InspectorSubject,
    owner_link: Option<Entity>,
    owner: Option<OwnerKey>,
) {
    commands.entity(card.root).insert(InspectorPopup {
        subject,
        title_entity: card.title,
        detail_entity: card.detail,
        owner_link,
        owner,
        close_at: Some(now + AUTO_CLOSE_SECONDS),
    });
    active.0 = Some(card.root);
}

/// Spawn the object card's "Owner:" row — a dim label and the shared clickable
/// name-link — returning the name-link node the owner binder binds. The link
/// resolves the owner's name and opens their profile on click, like every other
/// owner name in the UI.
fn spawn_owner_row(commands: &mut Commands, body: Entity, translator: &Translator) -> Entity {
    let owner_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            Pickable::IGNORE,
            ChildOf(body),
        ))
        .id();
    commands.spawn((
        Text::new(translator.get("inspector-owner")),
        UiFont::Sans.at(BODY_FONT_SIZE),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(owner_row),
    ));
    spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new("inspector-loading", "inspector-owner-unknown"),
    )
}

/// Set a text node's contents at build time (a deferred insert, since the node
/// was just spawned and is not yet queryable).
fn set_text(commands: &mut Commands, entity: Entity, text: &str) {
    commands.entity(entity).insert(Text::new(text.to_owned()));
}

/// Rewrite a live text node in place (only on a real change), for the reply /
/// name-cache updates.
fn write_text(texts: &mut Query<&mut Text>, entity: Entity, wanted: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != wanted
    {
        wanted.clone_into(&mut text.0);
    }
}

/// Spawn one action button, returning its [`EntityCommands`] so the caller chains
/// its click observer.
fn add_button<'commands>(
    commands: &'commands mut Commands,
    parent: Entity,
    label: &str,
    tab: i32,
) -> bevy::ecs::system::EntityCommands<'commands> {
    let mut button = commands.spawn((
        Button,
        TabIndex(tab),
        Node {
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(BUTTON_BACKGROUND),
        BorderColor::all(BUTTON_BORDER),
        Name::new(format!("inspector-action:{label}")),
        ChildOf(parent),
    ));
    button.with_child((
        Text::new(label.to_owned()),
        UiFont::Sans.at(BODY_FONT_SIZE),
        TextColor(TITLE_COLOR),
    ));
    button
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// The card's top-left position: the click point plus the offset, clamped so the
/// whole card stays inside the window.
fn clamp_on_screen(windows: &Query<&Window, With<PrimaryWindow>>, at: Vec2) -> Vec2 {
    let wanted = Vec2::new(at.x + CURSOR_OFFSET.x, at.y + CURSOR_OFFSET.y);
    let Ok(window) = windows.single() else {
        return wanted;
    };
    let max_x = (window.width() - CARD_WIDTH).max(0.0);
    let max_y = (window.height() - CARD_HEIGHT_RESERVE).max(0.0);
    Vec2::new(wanted.x.clamp(0.0, max_x), wanted.y.clamp(0.0, max_y))
}

/// An object title: its name, or a localized placeholder when the object is
/// unnamed.
fn object_title(name: &str, translator: &Translator) -> String {
    if name.trim().is_empty() {
        translator.get("inspector-object-unnamed")
    } else {
        name.to_owned()
    }
}

/// Turn an `objectim` `slurl` query value into a dispatchable location URL: a bare
/// `Region/x/y/z` gets the `secondlife://` scheme; an already-schemed value is
/// used verbatim.
fn as_location_url(slurl: &str) -> String {
    if slurl.contains("://") {
        slurl.to_owned()
    } else {
        format!("secondlife://{slurl}")
    }
}

/// Elide `text` to at most `max` characters, appending an ellipsis when cut.
fn snippet(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_owned();
    }
    let mut out: String = trimmed.chars().take(max).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use super::{SNIPPET_CHARS, as_location_url, snippet};
    use pretty_assertions::assert_eq;

    /// A bare region/coords SLURL gains the scheme; a full one is untouched.
    #[test]
    fn location_url_adds_scheme_only_when_missing() {
        assert_eq!(
            as_location_url("Ahern/128/128/24"),
            "secondlife://Ahern/128/128/24"
        );
        assert_eq!(
            as_location_url("secondlife://Ahern/128/128/24"),
            "secondlife://Ahern/128/128/24"
        );
        assert_eq!(
            as_location_url("hop://grid.example.org/Sandbox/10/20"),
            "hop://grid.example.org/Sandbox/10/20"
        );
    }

    /// A short about-text is shown whole; a long one is elided with an ellipsis and
    /// stays within the cap.
    #[test]
    fn snippet_elides_long_text() {
        assert_eq!(snippet("  Hello world  ", SNIPPET_CHARS), "Hello world");
        let long = "a".repeat(SNIPPET_CHARS + 40);
        let cut = snippet(&long, SNIPPET_CHARS);
        assert_eq!(
            cut.chars().count(),
            SNIPPET_CHARS + 1,
            "the cap plus the ellipsis"
        );
        assert!(cut.ends_with('\u{2026}'));
    }
}
