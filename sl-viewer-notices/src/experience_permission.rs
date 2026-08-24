//! The **experience-acceptance toast host** (`viewer-experience-permission-dialog`):
//! the panel a scripted object pops when it wants to run *under an experience*,
//! mirroring the reference `ScriptQuestionExperience` notification.
//!
//! # What it renders
//!
//! An in-world script that calls `llRequestPermissions` **under an experience**
//! makes the simulator send a `ScriptQuestion` whose `Experience.ExperienceID`
//! names that experience. `sl-proto` decodes it as an ordinary
//! [`SlSessionEvent::ScriptPermissionRequest`] carrying `experience_id`, and the
//! sibling script permission host ([`crate::script_permission`]) **skips** such a
//! request (unless it is also a money-caution request, which keeps the caution
//! card) so this host can raise the reference experience card instead — exactly
//! as the reference `process_script_question` shows `ScriptQuestionExperience`
//! rather than `ScriptQuestion` for it.
//!
//! Accepting an experience is a lasting choice — it goes on the agent's list of
//! approved experiences and the prompt does not return — so the card needs the
//! experience's **name** and **scope** (grid-wide vs land), which the
//! `ScriptQuestion` message does not carry. Like the reference (which fetches the
//! experience details before adding the notification) this host **defers**: it
//! records the pending request keyed by experience id, fetches the metadata
//! ([`Command::RequestExperienceInfo`]), and raises the card only once
//! [`SlSessionEvent::ExperienceInfo`] resolves the name / scope. The card shows:
//!
//! - an **intro** naming the object, its owner and the experience's scope
//!   (`'Object', an object owned by Owner, requests your participation in the
//!   grid-wide experience:`);
//! - the **experience name** (its own accent line — the reference `[EXPERIENCE]`
//!   profile SLURL, rendered here as plain text pending the shared linkification
//!   layer, [[viewer-url-linkification]]);
//! - the reference note that acceptance is remembered until revoked from the
//!   experience profile;
//! - the `[QUESTIONS]` **permission lines** the experience's scripts will be able
//!   to act on (the same wording the standard permission card lists, via the
//!   shared [`crate::script_permission`] machinery);
//! - **Is this OK?** and the **Yes** / **No** / **Block Experience** / **Block
//!   Object** actions.
//!
//! # The four actions
//!
//! A card **sticks** ([`NotificationKind::Alert`]) until the user answers it. Each
//! answer replies to the outstanding request with a `ScriptAnswerYes`
//! ([`Command::AnswerScriptPermissions`], carrying the experience id so the
//! session grant mirror records it):
//!
//! - **Yes** grants the recognised requested permissions **and** admits the
//!   experience ([`Command::SetExperiencePermission`] `Allow`) — the reference
//!   `experience_permission` "Allow" post, which is why the prompt does not return;
//! - **No** denies (an empty grant) and leaves the experience unset;
//! - **Block Experience** denies **and** blocks the experience
//!   ([`ExperiencePermission::Block`]) — the reference `BlockExperience` button;
//! - **Block Object** denies **and** mutes the holding object (the reference
//!   `Mute` button).
//!
//! The close **×** denies conservatively (an empty grant, never a silent accept),
//! matching [`crate::script_permission`]. Like every sibling script dialog the
//! prompt is **not persisted** across a relog: the simulator's outstanding request
//! does not survive the session.
//!
//! The managed list of already-accepted / blocked experiences (and the *forget*
//! that takes an experience back off either list) is the companion Experiences
//! floater, [`crate::experiences_floater`].

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use std::collections::{HashMap, HashSet};

use sl_client_bevy::{
    Command, ExperienceInfo, ExperienceKey, ExperiencePermission, InventoryKey, MuteType,
    ObjectKey, ScriptPermissions, SlCommand, SlEvent, SlSessionEvent, Uuid,
};

use crate::i18n::{TransArgs, Translator};
use crate::linkified_text::{LinkTextStyle, spawn_linkified_text};
use crate::notification_host::{NotificationChannelRoot, ResolveNotification, adopt_toast};
use crate::notifications::{
    NotificationId, NotificationKind, NotificationManager, NotificationPriority,
};
use crate::script_permission::{is_caution, other_permission_keys, recognized_mask};
use crate::ui::{column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::world_api::RequestBlock;

/// The catalogue-template sentinel an experience card reports as (it is not a real
/// [`crate::notifications::NOTIFICATIONS`] entry — the card is bespoke — but the
/// shared toast machinery wants a stable name for its history / response
/// bookkeeping). Named for the reference `ScriptQuestionExperience` notification.
const EXPERIENCE_TEMPLATE: &str = "ScriptQuestionExperience";

/// The element id the gallery specimen and its inert actions report under.
const EXPERIENCE_ELEMENT: &str = "experience-permission-toast";

/// The skin class a card wears (`.sk-toast`), so it inherits the toast surface
/// styling shared with the catalogue toasts and the sibling cards.
const CARD_CLASS: &str = "sk-toast";

/// The skin class the intro / body text wears (`.sk-toast-text`).
const TEXT_CLASS: &str = "sk-toast-text";

/// The skin class a card button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The close-button glyph (a multiplication sign), matching the reference toast.
const CLOSE_GLYPH: &str = "\u{00d7}";

/// The bullet glyph prefixing each `[QUESTIONS]` permission line.
const BULLET_GLYPH: &str = "\u{2022}\u{00a0}";

/// A card's widest allowed width, in logical pixels.
const CARD_MAX_WIDTH: f32 = 360.0;

/// A card's inner padding, in logical pixels.
const CARD_PADDING: f32 = 10.0;

/// A card's border width, in logical pixels — the experience accent is painted on
/// it.
const CARD_BORDER: f32 = 2.0;

/// The gap between a card's stacked rows, in logical pixels.
const CARD_ROW_GAP: f32 = 6.0;

/// The gap between the action buttons, in logical pixels.
const BUTTON_GAP: f32 = 6.0;

/// The card body / button text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The intro / experience-name text size, in logical pixels.
const LEAD_FONT_SIZE: f32 = 15.0;

/// The width bound for a full-width text line, spanning the card content width
/// less its padding and border — so a wrapped paragraph is the sole inline
/// occupant of a decoration-free box (the `viewer-text-node-padding-measure`
/// constraint).
const FULL_TEXT_MAX_WIDTH: f32 = CARD_MAX_WIDTH - 2.0 * CARD_PADDING - 2.0 * CARD_BORDER;

/// A card's fallback background, used when no skin is loaded — the skin's
/// `.sk-toast` (`var(--surface-bg)`) overrides it.
const CARD_BACKGROUND: Color = Color::srgba(0.10, 0.12, 0.16, 0.98);

/// A card's fallback body text colour — the skin's `.sk-toast-text` overrides it.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the note and the requested-permission lines).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The experience accent painted on a card's border, its experience-name line and
/// its default (Yes) button — an emerald distinct from the script-dialog teal, the
/// load-url amber, the group-notice blue and the script-permission indigo, so the
/// cards read apart.
const ACCENT_COLOR: Color = Color::srgb(0.42, 0.82, 0.60);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// The plugin: defers each experience `ScriptQuestion` until its metadata resolves,
/// then drives the reference `ScriptQuestionExperience` card into the shared
/// notification channel.
#[derive(Debug)]
pub struct ExperiencePermissionPlugin;

impl Plugin for ExperiencePermissionPlugin {
    /// Record and fetch each experience permission request, then raise its card
    /// once the experience metadata arrives.
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingExperienceQuestions>()
            .add_systems(
                Update,
                (ingest_experience_questions, resolve_experience_questions).chain(),
            );
    }
}

/// One experience permission request awaiting its experience metadata. Holds every
/// fact the card and its replies need, so the resolve step needs nothing but the
/// fetched [`ExperienceInfo`].
#[derive(Debug, Clone)]
struct PendingExperienceQuestion {
    /// The task (object) id holding the script.
    task_id: ObjectKey,
    /// The script item id within the object.
    item_id: InventoryKey,
    /// The holding object's name.
    object_name: String,
    /// The object owner's name.
    object_owner: String,
    /// The permissions the script requested.
    permissions: ScriptPermissions,
}

/// The requests awaiting their experience metadata, keyed by experience id (one id
/// can gather several outstanding requests before its metadata lands). Drained as
/// each [`SlSessionEvent::ExperienceInfo`] resolves an id.
#[derive(Resource, Debug, Default)]
struct PendingExperienceQuestions {
    /// The outstanding requests per experience id.
    by_experience: HashMap<ExperienceKey, Vec<PendingExperienceQuestion>>,
}

/// Read the event stream; for each `ScriptQuestion` made under an experience (and
/// not a money-caution request, which keeps the caution card in
/// [`crate::script_permission`]), record it against its experience id and fetch the
/// experience metadata — so [`resolve_experience_questions`] can raise the card
/// once the name / scope resolve.
fn ingest_experience_questions(
    mut events: MessageReader<SlEvent>,
    mut pending: ResMut<PendingExperienceQuestions>,
    mut sl: MessageWriter<SlCommand>,
) {
    let mut to_fetch: HashSet<ExperienceKey> = HashSet::new();
    for event in events.read() {
        let SlSessionEvent::ScriptPermissionRequest(request) = &event.0 else {
            continue;
        };
        let Some(experience_id) = request.experience_id else {
            continue;
        };
        // A money-caution request keeps the caution card (the reference shows
        // `ScriptQuestionCaution` first even under an experience); only a
        // non-caution experience request is this host's.
        if is_caution(request.permissions) {
            continue;
        }
        pending
            .by_experience
            .entry(experience_id)
            .or_default()
            .push(PendingExperienceQuestion {
                task_id: request.task_id,
                item_id: request.item_id,
                object_name: request.object_name.clone(),
                object_owner: request.object_owner.clone(),
                permissions: request.permissions,
            });
        to_fetch.insert(experience_id);
    }
    if !to_fetch.is_empty() {
        sl.write(SlCommand(Command::RequestExperienceInfo {
            experience_ids: to_fetch.into_iter().collect(),
        }));
    }
}

/// Read the event stream; for each arriving [`ExperienceInfo`], raise the deferred
/// card for every request that was waiting on that experience id — so an experience
/// permission request stacks, orders and overflow-cycles alongside the catalogue
/// notifications and the sibling cards ([`crate::notification_host`]).
fn resolve_experience_questions(
    mut events: MessageReader<SlEvent>,
    mut pending: ResMut<PendingExperienceQuestions>,
    channel: Option<Res<NotificationChannelRoot>>,
    mut manager: ResMut<NotificationManager>,
    translator: Translator,
    mut commands: Commands,
) {
    let Some(channel) = channel else {
        return;
    };
    // The last metadata seen for each id this frame (a later reply supersedes).
    let mut infos: HashMap<ExperienceKey, ExperienceInfo> = HashMap::new();
    for event in events.read() {
        if let SlSessionEvent::ExperienceInfo(list) = &event.0 {
            for info in list {
                let _previous = infos.insert(info.public_id, info.clone());
            }
        }
    }
    for (experience_id, info) in infos {
        let Some(questions) = pending.by_experience.remove(&experience_id) else {
            continue;
        };
        for question in questions {
            spawn_experience_card(
                &mut commands,
                &channel,
                &mut manager,
                &translator,
                experience_id,
                &question,
                &info,
            );
        }
    }
}

/// The resolved content of one experience card, ready to render — the live path
/// resolves the decoded request + metadata + i18n into this; the gallery specimen
/// builds it from literals, so both render through the one
/// [`build_experience_card`] (the registry rule, [`crate::ui_element`]).
struct ExperienceContent {
    /// The intro line naming the object, owner and experience scope.
    intro: String,
    /// The experience name (its own accent line).
    experience_name: String,
    /// The experience's id, so the name renders as a clickable
    /// `secondlife:///app/experience/<id>/profile` link (the reference
    /// `[EXPERIENCE]` SLURL). `None` renders the name as plain text.
    experience_id: Option<ExperienceKey>,
    /// The note that acceptance is remembered until revoked.
    once_note: String,
    /// The header preceding the permission lines.
    scripts_intro: String,
    /// One line per recognised requested permission (bulleted).
    permission_lines: Vec<String>,
    /// The trailing confirm line ("Is this OK?").
    confirm: String,
    /// The accept (Yes) button label.
    yes_label: String,
    /// The decline (No) button label.
    no_label: String,
    /// The Block-Experience button label.
    block_experience_label: String,
    /// The Block-Object (mute) button label.
    block_object_label: String,
}

/// The entities [`build_experience_card`] produced that a caller wires: the card
/// root, the four action boxes and the close box.
struct ExperienceCard {
    /// The card root node (left with no parent — the caller adopts / parents it).
    root: Entity,
    /// The Yes (accept + admit experience) button box.
    yes: Entity,
    /// The No (decline) button box.
    no: Entity,
    /// The Block Experience (decline + block experience) button box.
    block_experience: Entity,
    /// The Block Object (decline + mute object) button box.
    block_object: Entity,
    /// The close (×) button box.
    close: Entity,
}

/// Build an experience card's node tree from resolved [`ExperienceContent`],
/// returning the entities a caller wires. The **root is left with no parent**: the
/// live host adopts it into the shared toast channel via [`adopt_toast`], the
/// gallery specimen parents it under its cell.
fn build_experience_card(commands: &mut Commands, content: &ExperienceContent) -> ExperienceCard {
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
            Name::new("experience-permission-card"),
        ))
        .id();

    // Close (×), top-trailing — the early-dismiss (conservative deny) affordance.
    let close = spawn_close_button(commands, root);

    // The intro (primary lead), the experience name (accent lead), the once-note
    // and the scripts header (dim), then each requested-permission line (dim,
    // bulleted) and the confirm line (primary) — each a width-bounded box so a
    // long paragraph wraps within the card.
    spawn_bounded_text(commands, root, &content.intro, LEAD_FONT_SIZE, TEXT_COLOR);
    // The experience name is a link to its profile
    // (`secondlife:///app/experience/<id>/profile`, the reference `[EXPERIENCE]`
    // SLURL) — viewer-experience-permission-body-links. It keeps the experience
    // accent as its link colour so the card's identity holds.
    spawn_experience_name(
        commands,
        root,
        content.experience_id,
        &content.experience_name,
    );
    spawn_bounded_text(
        commands,
        root,
        &content.once_note,
        FONT_SIZE,
        DIM_TEXT_COLOR,
    );
    spawn_bounded_text(
        commands,
        root,
        &content.scripts_intro,
        FONT_SIZE,
        DIM_TEXT_COLOR,
    );
    for line in &content.permission_lines {
        spawn_bounded_text(
            commands,
            root,
            &format!("{BULLET_GLYPH}{line}"),
            FONT_SIZE,
            DIM_TEXT_COLOR,
        );
    }
    spawn_bounded_text(commands, root, &content.confirm, FONT_SIZE, TEXT_COLOR);

    // The bottom action row: Yes (the accent default), No, Block Experience, Block
    // Object, trailing-aligned and wrapping when the four labels overflow.
    let action_row = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(BUTTON_GAP),
                justify_content: JustifyContent::End,
                ..row(Val::Px(BUTTON_GAP))
            },
            Name::new("experience-permission-actions"),
            ChildOf(root),
        ))
        .id();
    let yes = spawn_action_button(commands, action_row, &content.yes_label, ACCENT_COLOR, 1);
    let no = spawn_action_button(commands, action_row, &content.no_label, BUTTON_BORDER, 2);
    let block_experience = spawn_action_button(
        commands,
        action_row,
        &content.block_experience_label,
        BUTTON_BORDER,
        3,
    );
    let block_object = spawn_action_button(
        commands,
        action_row,
        &content.block_object_label,
        BUTTON_BORDER,
        4,
    );

    ExperienceCard {
        root,
        yes,
        no,
        block_experience,
        block_object,
        close,
    }
}

/// Build one experience card from a deferred [`PendingExperienceQuestion`] plus its
/// resolved [`ExperienceInfo`], adopt it into the shared toast channel, and wire
/// the live actions. The card is an [`Alert`](NotificationKind::Alert): it
/// **sticks** and only leaves when the user answers it. Not persisted: the
/// outstanding request does not survive a relog.
fn spawn_experience_card(
    commands: &mut Commands,
    channel: &NotificationChannelRoot,
    manager: &mut NotificationManager,
    translator: &Translator,
    experience_id: ExperienceKey,
    question: &PendingExperienceQuestion,
    info: &ExperienceInfo,
) -> NotificationId {
    let name = experience_display_name(translator, info);
    let content = experience_content(translator, question, info, experience_id, &name);
    let card = build_experience_card(commands, &content);

    // The history line names the experience, so the notification well shows what
    // was asked at a glance.
    let history = translator.format(
        "experience-permission-history",
        &TransArgs::new().text("experience", &name),
    );
    let id = adopt_toast(
        commands,
        manager,
        channel,
        card.root,
        NotificationKind::Alert,
        NotificationPriority::Normal,
        EXPERIENCE_TEMPLATE,
        None,
        history,
    );

    let root = card.root;
    let task_id = question.task_id;
    let item_id = question.item_id;
    let granted = ScriptPermissions(recognized_mask(question.permissions));

    // Yes: admit the experience and grant the recognised requested permissions.
    commands.entity(card.yes).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            set_experience_permission(&mut sl, experience_id, ExperiencePermission::Allow);
            answer_permissions(&mut sl, task_id, item_id, granted, experience_id);
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // No: deny (an empty grant), leaving the experience unset.
    commands.entity(card.no).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Block Experience: deny (an empty grant) and block the experience.
    commands.entity(card.block_experience).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            set_experience_permission(&mut sl, experience_id, ExperiencePermission::Block);
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );

    // Block Object: deny (an empty grant) and mute the holding object.
    let object_id: ObjectKey = question.task_id;
    let object_name = question.object_name.clone();
    commands.entity(card.block_object).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut blocks: MessageWriter<RequestBlock>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
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

    // Close ×: deny conservatively — dismissing must never leave a silent accept.
    commands.entity(card.close).observe(
        move |_activate: On<Activate>,
              mut sl: MessageWriter<SlCommand>,
              mut resolves: MessageWriter<ResolveNotification>| {
            answer_permissions(
                &mut sl,
                task_id,
                item_id,
                ScriptPermissions::default(),
                experience_id,
            );
            resolves.write(ResolveNotification {
                toast: root,
                button: None,
            });
        },
    );
    id
}

/// Queue a `ScriptAnswerYes` granting `permissions` (an empty set denies) to the
/// script `item_id` in object `task_id`, carrying the `experience_id` the request
/// was made under so the session's grant mirror records it.
fn answer_permissions(
    sl: &mut MessageWriter<SlCommand>,
    task_id: ObjectKey,
    item_id: InventoryKey,
    permissions: ScriptPermissions,
    experience_id: ExperienceKey,
) {
    sl.write(SlCommand(Command::AnswerScriptPermissions {
        task_id,
        item_id,
        permissions,
        experience_id: Some(experience_id),
    }));
}

/// Queue an `ExperiencePreferences` write setting the agent's preference for one
/// experience (`Allow` on accept, `Block` on Block Experience).
fn set_experience_permission(
    sl: &mut MessageWriter<SlCommand>,
    experience_id: ExperienceKey,
    permission: ExperiencePermission,
) {
    sl.write(SlCommand(Command::SetExperiencePermission {
        experience_id,
        permission,
    }));
}

/// Resolve the deferred request + metadata + i18n into the card content.
fn experience_content(
    translator: &Translator,
    question: &PendingExperienceQuestion,
    info: &ExperienceInfo,
    experience_id: ExperienceKey,
    name: &str,
) -> ExperienceContent {
    let permission_lines = other_permission_keys(question.permissions)
        .into_iter()
        .map(|key| translator.get(key))
        .collect::<Vec<_>>();
    ExperienceContent {
        intro: translator.format(
            "experience-permission-intro",
            &TransArgs::new()
                .text("object", &question.object_name)
                .text("owner", &question.object_owner)
                .text("scope", &translator.get(scope_key(info))),
        ),
        experience_name: name.to_owned(),
        experience_id: Some(experience_id),
        once_note: translator.get("experience-permission-once"),
        scripts_intro: translator.get("experience-permission-scripts"),
        permission_lines,
        confirm: translator.get("experience-permission-confirm"),
        yes_label: translator.get("experience-permission-button-yes"),
        no_label: translator.get("experience-permission-button-no"),
        block_experience_label: translator.get("experience-permission-button-block-experience"),
        block_object_label: translator.get("experience-permission-button-block-object"),
    }
}

/// The i18n key for an experience's scope word — grid-wide when
/// [`ExperienceProperties::is_grid`], otherwise land-scoped — matching the
/// reference `Grid-Scope` / `Land-Scope` substitution.
const fn scope_key(info: &ExperienceInfo) -> &'static str {
    if info.properties.is_grid() {
        "experience-scope-grid"
    } else {
        "experience-scope-land"
    }
}

/// The experience name to show: the resolved metadata name, or a placeholder when
/// the grid could not resolve the id ([`ExperienceInfo::missing`]) or returned an
/// empty name — so the card never renders a blank experience line.
fn experience_display_name(translator: &Translator, info: &ExperienceInfo) -> String {
    match resolved_name(info) {
        Some(name) => name.to_owned(),
        None => translator.get("experience-permission-unknown-name"),
    }
}

/// The metadata's usable name, or `None` when the record is a `missing`
/// placeholder or carries an empty name (either of which routes to the placeholder
/// label). Split out so the fallback rule is unit-testable.
fn resolved_name(info: &ExperienceInfo) -> Option<&str> {
    if info.missing || info.name.is_empty() {
        None
    } else {
        Some(&info.name)
    }
}

/// Spawn one bottom-row action button, bordered in the given accent (the Yes
/// default wears the card accent, the rest the neutral button border). Returns the
/// clickable box for the caller to wire onto.
fn spawn_action_button(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    border: Color,
    tab: i32,
) -> Entity {
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
            Name::new(format!("experience-permission-action:{label}")),
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
            Name::new("experience-permission-close-row"),
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
            Name::new("experience-permission-close"),
            ChildOf(close_row),
        ))
        .with_child((
            Text::new(CLOSE_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
        ))
        .id()
}

/// A width-bounded text line: the caller's text as the sole child of a
/// decoration-free box, so it wraps within the card (the
/// `viewer-text-node-padding-measure` constraint). An empty string spawns nothing.
fn spawn_bounded_text(
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
    commands.spawn((
        Text::new(text.to_owned()),
        UiFont::Sans.at(font_size),
        TextColor(color),
        ClassList::new_with_classes([TEXT_CLASS]),
        Pickable::IGNORE,
        ChildOf(box_entity),
    ));
}

/// Render the experience-name line: when the experience id is known, as a
/// clickable `secondlife:///app/experience/<id>/profile` link carrying the name
/// (a labelled link fed through the shared widget), tinted in the experience
/// accent; otherwise as plain accent text. An empty name spawns nothing.
fn spawn_experience_name(
    commands: &mut Commands,
    parent: Entity,
    experience_id: Option<ExperienceKey>,
    name: &str,
) {
    if name.is_empty() {
        return;
    }
    let Some(experience_id) = experience_id else {
        spawn_bounded_text(commands, parent, name, LEAD_FONT_SIZE, ACCENT_COLOR);
        return;
    };
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
    // A labelled link `[url  name]`: the widget shows the name and targets the
    // experience-profile SLURL. The accent is kept as the link colour.
    let linked = format!(
        "[secondlife:///app/experience/{}/profile {name}]",
        experience_id.uuid()
    );
    let mut style = LinkTextStyle::at(LEAD_FONT_SIZE);
    style.link_color = ACCENT_COLOR;
    spawn_linkified_text(commands, box_entity, &linked, style);
}

/// The gallery / `ui_test` specimen: a static experience card, so the intro /
/// name / note / permission-line / action layout is swept login-free (a live card
/// needs a scripted object running under an experience). Registered in
/// `crate::ui_element::ELEMENTS`; its buttons report an inert [`UiAction`].
pub fn spawn_experience_specimen(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let content = ExperienceContent {
        intro: cx.text(
            "'Race Gate', an object owned by Track Owner Resident, requests your participation \
             in the grid-wide experience:",
        ),
        experience_name: cx.text("Neon Speedway"),
        experience_id: Some(ExperienceKey::from(Uuid::from_u128(0xE29E_5A11))),
        once_note: cx.text(
            "Once permission is granted you will not see this message again for this experience \
             unless it is revoked from the experience profile.",
        ),
        scripts_intro: cx.text(
            "Scripts associated with this experience will be able to do the following on regions \
             where the experience is active:",
        ),
        permission_lines: vec![
            cx.text("\u{2022}\u{00a0}Act on your control inputs"),
            cx.text("\u{2022}\u{00a0}Animate your avatar"),
            cx.text("\u{2022}\u{00a0}Teleport you"),
        ],
        confirm: cx.text("Is this OK?"),
        yes_label: cx.text("Yes"),
        no_label: cx.text("No"),
        block_experience_label: cx.text("Block Experience"),
        block_object_label: cx.text("Block Object"),
    };
    let card = build_experience_card(commands, &content);
    commands.entity(card.root).insert(ChildOf(parent));
    wire_specimen_actions(commands, &card);
    card.root
}

/// Wire a specimen card's buttons to inert [`UiAction`]s (the registry rule: a
/// specimen reaches no session).
fn wire_specimen_actions(commands: &mut Commands, card: &ExperienceCard) {
    for (button, action) in [
        (card.yes, "yes"),
        (card.no, "no"),
        (card.block_experience, "block-experience"),
        (card.block_object, "block-object"),
        (card.close, "close"),
    ] {
        commands.entity(button).observe(
            move |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
                actions.write(UiAction {
                    element: EXPERIENCE_ELEMENT,
                    action,
                });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{resolved_name, scope_key};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ExperienceInfo, ExperienceProperties};
    use sl_types::experience::PROPERTY_GRID;

    /// A grid experience takes the grid-wide scope key; a land experience takes the
    /// land-scoped one.
    #[test]
    fn scope_key_tracks_the_grid_property() {
        let grid = ExperienceInfo {
            properties: ExperienceProperties(PROPERTY_GRID),
            ..ExperienceInfo::default()
        };
        assert_eq!(scope_key(&grid), "experience-scope-grid");

        let land = ExperienceInfo::default();
        assert_eq!(scope_key(&land), "experience-scope-land");
    }

    /// A resolved name is used verbatim; a `missing` placeholder or an empty name
    /// falls back (so the card never shows a blank experience line).
    #[test]
    fn resolved_name_falls_back_for_missing_or_empty() {
        let named = ExperienceInfo {
            name: "Neon Speedway".to_owned(),
            ..ExperienceInfo::default()
        };
        assert_eq!(resolved_name(&named), Some("Neon Speedway"));

        // An empty name falls back.
        let empty = ExperienceInfo::default();
        assert_eq!(resolved_name(&empty), None);

        // A missing placeholder falls back even with a name.
        let missing = ExperienceInfo {
            name: "Stale".to_owned(),
            missing: true,
            ..ExperienceInfo::default()
        };
        assert_eq!(resolved_name(&missing), None);
    }
}
