//! The **script web-page request toast host** (`viewer-dialog-script-load-url`):
//! the panel a scripted object's `llLoadURL` pops, mirroring the reference
//! `LoadWebPage` notification.
//!
//! # What it renders
//!
//! When a script calls `llLoadURL`, the simulator sends a `LoadURL` message —
//! "object 'X' owned by Y wants to take you to a web page", with the script's
//! message and the target URL. `sl-proto` decodes it
//! ([`SlSessionEvent::LoadUrl`]) but, before this host, no viewer system consumed
//! it, so scripted web links (vendor pages, info kiosks) silently vanished. This
//! host raises a card into the **shared notification-host channel**
//! ([`crate::notification_host`]) — top-trailing, priority-ordered,
//! overflow-cycled — with:
//!
//! - a **heading** naming the ask ("Open a web page?");
//! - a **title** line naming the object and its owner (`'Object' owned by Owner`,
//!   the reference `object '[OBJECTNAME]', owned by '[NAME]'`);
//! - the script **message**, when the script sent one;
//! - the **target URL**, rendered verbatim so the user can vet where the link
//!   goes **before** deciding — the card **never auto-opens** anything;
//! - the built-in **Load** (open the URL), **Block** (mute the object) and
//!   **Ignore** (dismiss) actions.
//!
//! A card **sticks** ([`NotificationKind::Alert`]) until the user answers it:
//! Load opens the URL in the embedded web browser ([`OpenWebBrowser`], the
//! `viewer-media-prim-browser` web floater); Block mutes the object; Ignore and
//! the close **×** tear it down with no action. Like a script dialog a
//! `LoadURL` toast is **not persisted** across a relog — the ask does not survive
//! the session, so a stored card would point at a stale object.
//!
//! # The owner name resolves after the fact
//!
//! Unlike a script dialog, the `LoadURL` message carries only the owner's *key*,
//! not their name. The card is raised immediately with a `(loading…)` owner
//! placeholder and the owner name is requested ([`Command::RequestAvatarNames`] /
//! [`Command::RequestGroupNames`]); when the reply arrives
//! ([`resolve_load_url_owner_names`]) the title line is rewritten in place. The
//! pending state rides on the title entity itself ([`PendingOwnerName`]), so a
//! dismissed card drops its resolution with no dangling bookkeeping.
//!
//! # Links in the body are deferred
//!
//! Both the message and the URL are rendered as plain text: turning URLs / SLURLs
//! into clickable links is the shared linkification layer
//! ([[viewer-url-linkification]]), tracked for this card as
//! [[viewer-load-url-body-links]] (the sibling of the script-dialog
//! [[viewer-script-dialog-body-links]] and the group-notice
//! [[viewer-group-notice-body-links]]). Flood throttling is the separate
//! [[viewer-anti-spam-filter]] hook.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use std::collections::HashMap;

use sl_client_bevy::{
    AgentKey, Command, GroupKey, LoadUrlRequest, MuteType, ObjectKey, OwnerKey, SlCommand, SlEvent,
    SlSessionEvent,
};

use crate::i18n::{TransArgs, Translator};
use crate::linkified_text::{LinkTextStyle, spawn_linkified_text};
use crate::mutes::RequestBlock;
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notifications::{
    NotificationId, NotificationKind, NotificationManager, NotificationPriority,
};
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::web_floater::OpenWebBrowser;

/// The catalogue-template sentinel a `LoadURL` toast reports as (it is not a real
/// [`crate::notifications::NOTIFICATIONS`] entry — the card is bespoke — but the
/// shared toast machinery wants a stable name for its history / response
/// bookkeeping). Named for the reference `LoadWebPage` notification.
const LOAD_URL_TEMPLATE: &str = "LoadWebPage";

/// The element id the gallery specimen and its inert actions report under.
const LOAD_URL_ELEMENT: &str = "load-url-toast";

/// The skin class a card wears (`.sk-toast`), so the card inherits the toast
/// surface styling shared with the catalogue toasts and the sibling cards.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the heading / title / body text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the load-url accent is painted on
/// it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The gap between the action buttons, in logical pixels.
const BUTTON_GAP: f32 = 6.0;

/// The card body / button text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The heading line's text size, in logical pixels.
const HEADING_FONT_SIZE: f32 = 15.0;

/// The width bound for a full-width text line (heading / title / body / URL),
/// spanning the card content width less its padding and border — so a wrapped
/// paragraph is the sole inline occupant of a decoration-free box (the
/// `viewer-text-node-padding-measure` constraint).
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the object / owner title line).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The load-url accent painted on a card's border, its Load button and its URL
/// line — an amber distinct from the script-dialog teal and the group-notice
/// blue, reading as "an external web link: look before you leap".
const ACCENT_COLOR: Color = Color::srgb(0.92, 0.70, 0.36);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The plugin: drives the `LoadURL` cards into the shared notification channel and
/// resolves their owner names after the fact.
pub(crate) struct LoadUrlPlugin;

impl Plugin for LoadUrlPlugin {
    /// Ingest received `LoadURL` messages into the shared toast channel and
    /// rewrite each card's owner name once the name reply arrives.
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (ingest_load_urls, resolve_load_url_owner_names));
    }
}

/// The owner-name resolution state carried by a live card's title text entity: the
/// owner key still awaiting a name and the object name needed to reformat the
/// title. Removed once [`resolve_load_url_owner_names`] rewrites the line, or with
/// the entity when the card is dismissed — so there is no separate bookkeeping to
/// prune.
#[derive(Component)]
struct PendingOwnerName {
    /// The owner whose name is being resolved.
    owner: OwnerKey,
    /// The object name, for reformatting the title once the owner name lands.
    object_name: String,
}

/// Read the event stream; for each received `LoadURL`, build its card, raise it
/// into the shared toast channel — so a web-page request stacks, orders and
/// overflow-cycles alongside the catalogue notifications and the sibling cards
/// ([`crate::notification_host`]) — and kick off the owner-name lookup.
fn ingest_load_urls(
    mut events: MessageReader<SlEvent>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    translator: Translator,
    mut sl: MessageWriter<SlCommand>,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    for event in events.read() {
        let SlSessionEvent::LoadUrl(request) = &event.0 else {
            continue;
        };
        spawn_load_url_card(
            &mut commands,
            &channel,
            &mut manager,
            &translator,
            &mut sl,
            request,
        );
    }
}

/// The resolved content of one `LoadURL` card, ready to render — the live path
/// resolves the decoded request + i18n into this; the gallery specimen builds it
/// from literals, so both render through the one [`build_load_url_card`] (the
/// registry rule, [`crate::ui_element`]).
struct LoadUrlContent {
    /// The heading line ("Open a web page?").
    heading: String,
    /// The title line naming the object and its owner.
    title: String,
    /// The script message (plain text; linkification deferred). May be empty.
    message: String,
    /// The target URL, rendered verbatim so the user can vet it.
    url: String,
    /// The Load (open the URL) button label.
    load_label: String,
    /// The Block (mute) button label.
    block_label: String,
    /// The Ignore (dismiss) button label.
    ignore_label: String,
}

/// The entities [`build_load_url_card`] produced that a caller wires: the card
/// root, the title text (the live host rewrites it once the owner name resolves),
/// and the Load / Block / Ignore / close boxes.
struct LoadUrlCard {
    /// The card root node (left with no parent — the caller adopts / parents it).
    root: Entity,
    /// The title text line, so the live host can rewrite the owner name in place.
    title: Entity,
    /// The Load (open the URL) button box.
    load: Entity,
    /// The Block (mute) button box.
    block: Entity,
    /// The Ignore (dismiss) button box.
    ignore: Entity,
    /// The close (×) button box.
    close: Entity,
}

/// Build a `LoadURL` card's node tree from resolved [`LoadUrlContent`], returning
/// the entities a caller wires. The **root is left with no parent**: the live host
/// adopts it into the shared toast channel via [`adopt_toast`], the gallery
/// specimen parents it under its cell.
fn build_load_url_card(commands: &mut Commands, content: &LoadUrlContent) -> LoadUrlCard {
    let root = commands
        .spawn((
            Node {
                max_width: Val::Px(CARD_MAX_WIDTH),
                padding: UiRect::all(Val::Px(CARD_PADDING)),
                border: UiRect::all(Val::Px(CARD_BORDER)),
                ..column(Val::Px(CARD_ROW_GAP))
            },
            BackgroundColor(CARD_BACKGROUND),
            BorderColor::all(ACCENT_COLOR),
            ClassList::new_with_classes([CARD_CLASS]),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("load-url-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss affordance.
    let close = spawn_close_button(commands, root);

    // The heading (primary), the object / owner title (dim), the optional script
    // message (primary), then the URL (accent) — each a width-bounded box so a
    // long name / paragraph / link wraps within the card.
    spawn_bounded_text(
        commands,
        root,
        &content.heading,
        HEADING_FONT_SIZE,
        TEXT_COLOR,
    );
    let title = spawn_bounded_text(commands, root, &content.title, FONT_SIZE, DIM_TEXT_COLOR)
        .unwrap_or(root);
    // The script message is linkified — its http(s) URLs / SLURLs render as
    // clickable links (viewer-load-url-body-links), exactly as nearby chat and the
    // other toast bodies do. The URL line below stays verbatim: it is the explicit
    // target the user vets before pressing Load.
    spawn_bounded_linked_text(commands, root, &content.message, FONT_SIZE, TEXT_COLOR);
    spawn_bounded_text(commands, root, &content.url, FONT_SIZE, ACCENT_COLOR);

    // The bottom action row: Load (the accent default), then Block and Ignore,
    // trailing-aligned.
    let action_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                justify_content: JustifyContent::End,
                ..row(Val::Px(BUTTON_GAP))
            },
            Name::new("load-url-actions"),
            ChildOf(root),
        ))
        .id();
    // Load is the default action, so it wears the accent.
    let load = spawn_action_button(commands, action_row, &content.load_label, true, 1);
    let block = spawn_action_button(commands, action_row, &content.block_label, false, 2);
    let ignore = spawn_action_button(commands, action_row, &content.ignore_label, false, 3);

    LoadUrlCard {
        root,
        title,
        load,
        block,
        ignore,
        close,
    }
}

/// Build one `LoadURL` card from a decoded [`LoadUrlRequest`], adopt it into the
/// shared toast channel, wire the live actions, and request the owner name. The
/// card is an [`Alert`](NotificationKind::Alert): it **sticks** (never auto-fades)
/// and only leaves when the user answers it — Load opens the URL in the embedded
/// browser, Block mutes the object, Ignore / × tears it down.
fn spawn_load_url_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    sl: &mut MessageWriter<SlCommand>,
    request: &LoadUrlRequest,
) -> NotificationId {
    let url = request.url.to_string();
    let content = LoadUrlContent {
        heading: translator.get("load-url-heading"),
        title: format_title(translator, &request.object_name, &owner_loading(translator)),
        message: request.message.clone(),
        url: url.clone(),
        load_label: translator.get("load-url-button-load"),
        block_label: translator.get("load-url-button-block"),
        ignore_label: translator.get("load-url-button-ignore"),
    };
    let card = build_load_url_card(commands, &content);

    // Adopt the card into the shared toast channel so it stacks / orders /
    // overflow-cycles with the catalogue notifications. An `Alert` never
    // auto-expires — only a user answer ends it. Not persisted: the outstanding
    // ask does not survive a relog.
    let id = adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        NotificationPriority::Normal,
        LOAD_URL_TEMPLATE,
        None,
        url.clone(),
    );

    // The owner name is not in the message: resolve it after the fact, riding the
    // pending state on the title entity so a dismissed card drops it cleanly.
    commands.entity(card.title).insert(PendingOwnerName {
        owner: request.owner,
        object_name: request.object_name.clone(),
    });
    request_owner_name(sl, request.owner);

    let root = card.root;

    // Load: open the URL in the embedded browser and tear the card down. Never
    // auto-opened — this only fires on the user's click.
    commands.entity(card.load).observe(
        move |_activate: On<Activate>,
              mut browsers: MessageWriter<OpenWebBrowser>,
              mut resolves: MessageWriter<ResolveNotification>| {
            browsers.write(OpenWebBrowser {
                url: Some(url.clone()),
            });
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Block: mute the object, then tear the card down.
    let object_id: ObjectKey = request.object_id;
    let object_name = request.object_name.clone();
    commands.entity(card.block).observe(
        move |_activate: On<Activate>,
              mut blocks: MessageWriter<RequestBlock>,
              mut resolves: MessageWriter<ResolveNotification>| {
            blocks.write(RequestBlock::new(
                object_id.uuid(),
                object_name.clone(),
                MuteType::Object,
            ));
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Ignore / close ×: tear the card down with no action.
    for button in [card.ignore, card.close] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut resolves: MessageWriter<ResolveNotification>| {
                resolves.write(ResolveNotification {
                    toast: root,
                    button: None,
                });
            },
        );
    }
    id
}

/// Rewrite each live card's owner name in place once its name reply arrives. Reads
/// the resolved-name events, indexes them by key, and for every card still
/// awaiting one of those owners rewrites the title line and drops the
/// [`PendingOwnerName`] marker (so a name reply only ever touches a card once).
fn resolve_load_url_owner_names(
    mut events: MessageReader<SlEvent>,
    translator: Translator,
    mut pending: Query<(Entity, &mut Text, &PendingOwnerName)>,
    mut commands: Commands,
) {
    let mut agents: HashMap<AgentKey, String> = HashMap::new();
    let mut groups: HashMap<GroupKey, String> = HashMap::new();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::AvatarNames(names) => {
                for name in names {
                    agents.insert(name.id, name.legacy_name());
                }
            }
            SlSessionEvent::GroupNames(names) => {
                for name in names {
                    groups.insert(name.id, name.name.clone());
                }
            }
            _other => {}
        }
    }
    if agents.is_empty() && groups.is_empty() {
        return;
    }
    for (entity, mut text, marker) in &mut pending {
        let Some(name) = owner_name(&marker.owner, &agents, &groups) else {
            continue;
        };
        *text = Text::new(format_title(&translator, &marker.object_name, name));
        commands.entity(entity).remove::<PendingOwnerName>();
    }
}

/// The resolved name for an owner, looked up from the freshly-arrived name maps by
/// its key kind — `None` when neither map holds this owner yet.
fn owner_name<'a>(
    owner: &OwnerKey,
    agents: &'a HashMap<AgentKey, String>,
    groups: &'a HashMap<GroupKey, String>,
) -> Option<&'a String> {
    match owner {
        OwnerKey::Agent(agent) => agents.get(agent),
        OwnerKey::Group(group) => groups.get(group),
    }
}

/// Request the legacy avatar name (agent owner) or the group name (group owner)
/// for a `LoadURL` card's owner, so [`resolve_load_url_owner_names`] can fill the
/// title in.
fn request_owner_name(sl: &mut MessageWriter<SlCommand>, owner: OwnerKey) {
    match owner {
        OwnerKey::Agent(agent) => {
            sl.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
        }
        OwnerKey::Group(group) => {
            sl.write(SlCommand(Command::RequestGroupNames(vec![group])));
        }
    }
}

/// The card's title line: the object name and its (resolved or still-loading)
/// owner name, via the `load-url-from` template.
fn format_title(translator: &Translator, object_name: &str, owner: &str) -> String {
    translator.format(
        "load-url-from",
        &TransArgs::new()
            .text("object", object_name)
            .text("owner", owner),
    )
}

/// The owner-name placeholder shown while the real name is being resolved.
fn owner_loading(translator: &Translator) -> String {
    translator.get("load-url-owner-loading")
}

/// Spawn one bottom-row action button (Load / Block / Ignore), accent-bordered
/// when it is the default. Returns the clickable box for the caller to wire onto.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    is_default: bool,
    tab: i32,
) -> Entity {
    let border = if is_default {
        ACCENT_COLOR
    } else {
        BUTTON_BORDER
    };
    commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(border),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new(format!("load-url-action:{label}")),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(label.to_owned()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// Spawn the close (×) button in a top-trailing row, returning its box for the
/// caller to wire onto.
fn spawn_close_button(commands: &mut Commands, card: Entity) -> Entity {
    let close_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::End,
                ..row(Val::ZERO)
            },
            Name::new("load-url-close-row"),
            ChildOf(card),
        ))
        .id();
    commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                ..default()
            },
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("load-url-close"),
            ChildOf(close_row),
        ))
        .with_child((
            Text::new(CLOSE_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// A width-bounded **linkified** text line: the caller's text run through the
/// shared linkification widget ([`spawn_linkified_text`]) inside a bounded box, so
/// its URLs / SLURLs render as clickable links and a long paragraph still wraps
/// within the card. Spawns nothing for an empty string.
fn spawn_bounded_linked_text(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    font_size: f32,
    color: Color,
) {
    if text.is_empty() {
        return;
    }
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let mut style = LinkTextStyle::at(font_size);
    style.plain_color = color;
    spawn_linkified_text(commands, box_entity, text, style);
}

/// A width-bounded text line: the caller's text as the sole child of a
/// decoration-free box, so it wraps within the card (the
/// `viewer-text-node-padding-measure` constraint). Returns the inner text entity
/// (so the caller can rewrite it), or `None` for an empty string (which spawns
/// nothing).
fn spawn_bounded_text(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    font_size: f32,
    color: Color,
) -> Option<Entity> {
    if text.is_empty() {
        return None;
    }
    let box_entity = commands
        .spawn((
            Node {
                max_width: Val::Px(FULL_TEXT_MAX_WIDTH),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let text_entity = commands
        .spawn((
            Text::new(text.to_owned()),
            UiFont::Sans.at(font_size),
            TextColor(color),
            ClassList::new_with_classes([TEXT_CLASS]),
            Pickable::IGNORE,
            ChildOf(box_entity),
        ))
        .id();
    Some(text_entity)
}

/// The gallery / `ui_test` specimen: a static `LoadURL` card with a resolved
/// owner name, so the heading / title / message / URL / action layout is swept
/// login-free (a live card needs a scripted object). Registered in
/// [`crate::ui_element::ELEMENTS`]; its buttons report an inert [`UiAction`].
pub(crate) fn spawn_load_url_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let content = LoadUrlContent {
        heading: cx.text("Open a web page?"),
        title: cx.text("'Info Kiosk' owned by Shopkeeper Resident"),
        message: cx.text(SPECIMEN_MESSAGE),
        url: cx.text("https://marketplace.example/store/42"),
        load_label: cx.text("Load"),
        block_label: cx.text("Block"),
        ignore_label: cx.text("Ignore"),
    };
    let card = build_load_url_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card);
    card.root
}

/// Wire a specimen card's buttons to inert [`UiAction`]s (the registry rule: a
/// specimen reaches no session).
fn wire_specimen_actions(commands: &mut Commands, card: &LoadUrlCard) {
    for (button, action) in [
        (card.load, "load"),
        (card.block, "block"),
        (card.ignore, "ignore"),
        (card.close, "close"),
    ] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: LOAD_URL_ELEMENT,
                    action,
                });
            },
        );
    }
}

/// The specimen's message prose — long enough to force the wrap the matrix sweeps.
const SPECIMEN_MESSAGE: &str = "Visit our store to see the full catalogue and today's specials.";

#[cfg(test)]
mod tests {
    use super::{owner_name, request_owner_name};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AgentKey, Command, GroupKey, OwnerKey, SlCommand, Uuid};
    use std::collections::HashMap;

    /// An agent owner resolves from the agent map and ignores the group map; a
    /// group owner does the reverse; an owner in neither map is unresolved.
    #[test]
    fn owner_name_looks_up_by_key_kind() {
        let agent = AgentKey::from(Uuid::from_u128(0xa9e7));
        let group = GroupKey::from(Uuid::from_u128(0x6409));
        let mut agents: HashMap<AgentKey, String> = HashMap::new();
        agents.insert(agent, "Shopkeeper Resident".to_owned());
        let mut groups: HashMap<GroupKey, String> = HashMap::new();
        groups.insert(group, "Merchants United".to_owned());

        assert_eq!(
            owner_name(&OwnerKey::Agent(agent), &agents, &groups),
            Some(&"Shopkeeper Resident".to_owned())
        );
        assert_eq!(
            owner_name(&OwnerKey::Group(group), &agents, &groups),
            Some(&"Merchants United".to_owned())
        );
        // An owner whose name has not arrived in either map is still unresolved.
        let other = AgentKey::from(Uuid::from_u128(0xdead));
        assert_eq!(owner_name(&OwnerKey::Agent(other), &agents, &groups), None);
    }

    /// An agent owner requests an avatar name; a group owner requests a group
    /// name — each a single-key lookup for exactly that owner. (`Command` derives
    /// neither `PartialEq` nor `Clone`, so the routing is matched by reference.)
    #[test]
    fn request_owner_name_picks_the_matching_lookup() {
        let agent = AgentKey::from(Uuid::from_u128(0x1111));
        let group = GroupKey::from(Uuid::from_u128(0x2222));

        assert!(first_command_matches(OwnerKey::Agent(agent), |command| {
            matches!(command, Command::RequestAvatarNames(ids) if ids == &vec![agent])
        }));
        assert!(first_command_matches(OwnerKey::Group(group), |command| {
            matches!(command, Command::RequestGroupNames(ids) if ids == &vec![group])
        }));
    }

    /// Drive [`request_owner_name`] through a throwaway Bevy app and report whether
    /// `check` accepts the one [`Command`] it wrote (`false` if none was written),
    /// so the routing is asserted without a live session — and without
    /// `Command: Clone` or a `panic!` / `expect` the restriction lints forbid.
    fn first_command_matches(owner: OwnerKey, check: impl FnOnce(&Command) -> bool) -> bool {
        use bevy::prelude::*;
        let mut app = App::new();
        app.add_message::<SlCommand>();
        app.add_systems(Update, move |mut sl: MessageWriter<SlCommand>| {
            request_owner_name(&mut sl, owner);
        });
        app.update();
        let messages = app.world().resource::<Messages<SlCommand>>();
        let mut cursor = messages.get_cursor();
        let Some(command) = cursor.read(messages).next() else {
            return false;
        };
        check(&command.0)
    }
}
