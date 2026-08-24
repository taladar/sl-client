//! The **Experiences floater** (`viewer-experience-permission-dialog`): the surface
//! that manages the agent's already-decided experiences, mirroring the reference
//! `LLFloaterExperiences` Allowed / Blocked tabs and the *forget* that takes an
//! experience back off either list.
//!
//! # What it shows
//!
//! Two lists — the experiences the agent has **allowed** (accepted, so their
//! scripts run without prompting, [`crate::experience_permission`]) and those it
//! has **blocked** — each row a name with a **Forget** button that clears the
//! preference ([`ExperiencePermission::Forget`]), so a forgotten experience prompts
//! again next time. A **Refresh** re-reads the lists.
//!
//! # Reading the lists is a GET; a forget is a fire-and-forget
//!
//! Opening the floater (or Refresh) issues [`Command::RequestExperiencePermissions`]
//! (the `GetExperiences` cap), whose reply is the authoritative full
//! `allowed` / `blocked` id lists ([`SlSessionEvent::ExperiencePermissions`]).
//! Experience **names** are not in that reply, so each unknown id is resolved with
//! [`Command::RequestExperienceInfo`] and folded in as
//! [`SlSessionEvent::ExperienceInfo`] arrives — the same request-if-unknown / fold
//! -in-on-reply shape the group-name cache uses.
//!
//! A **forget** writes [`Command::SetExperiencePermission`] `Forget` and updates
//! the list **optimistically** (the row leaves at once). It does **not** wait for a
//! reply, because on the live grid the `ExperiencePreferences` PUT / DELETE reply
//! carries only the single edited experience — which `sl-proto` collapses into the
//! *same* [`SlSessionEvent::ExperiencePermissions`] event as the GET reply but with
//! empty lists. So this floater treats an `ExperiencePermissions` event as an
//! authoritative full list **only** while it is expecting a GET reply it issued
//! (tracked by `ExperiencesState::pending_full_list`); a mutation reply, which
//! arrives with no GET outstanding, is ignored (the optimistic update already
//! stands). The accept-prompt companion — the `ScriptQuestionExperience` toast a
//! script pops to *join* an experience — is [`crate::experience_permission`].

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;
use std::collections::BTreeMap;

use sl_client_bevy::{
    Command, ExperienceKey, ExperiencePermission, SlCommand, SlEvent, SlSessionEvent,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::{Translated, Translator};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;

/// The floater's id (its geometry-persistence key and menu target).
pub const EXPERIENCES_FLOATER_ID: &str = "experiences";

/// The element id the gallery specimen and its inert actions report under.
const EXPERIENCES_ELEMENT: &str = "experiences-floater";

/// The skin class a Forget / Refresh button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The list / body text size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// The section-heading text size, in logical pixels.
const HEADING_FONT_SIZE: f32 = 15.0;

/// Each list column's fixed height, in logical pixels (the rows clip past it).
const LIST_HEIGHT: f32 = 150.0;

/// The floater content's fixed width, in logical pixels.
const CONTENT_WIDTH: f32 = 280.0;

/// The primary body text colour.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (the empty-list note and the short-id fallback).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The heading accent — the same emerald the experience toast wears, so the
/// experience surfaces read as one family.
const HEADING_COLOR: Color = Color::srgb(0.42, 0.82, 0.60);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A list column's background tint behind its rows.
const LIST_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.25);

/// The number of leading hex characters of an experience id shown as a fallback
/// label while its name is still resolving.
const SHORT_ID_LEN: usize = 8;

/// The plugin owning the Experiences floater.
#[derive(Debug)]
pub struct ExperiencesPlugin;

impl Plugin for ExperiencesPlugin {
    /// Register the state and systems, and spawn the (hidden) floater.
    fn build(&self, app: &mut App) {
        app.init_resource::<ExperiencesState>()
            .add_systems(
                Startup,
                spawn_experiences_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    request_permissions_on_show,
                    ingest_experience_permissions,
                    ingest_experience_names,
                    rebuild_lists,
                )
                    .chain(),
            );
    }
}

/// The floater's data: the current allowed / blocked id lists, the resolved-name
/// cache, the outstanding-GET count that disambiguates a full-list reply from a
/// single-edit reply, and a revision the rebuild watches.
#[derive(Resource, Debug, Default)]
struct ExperiencesState {
    /// The experiences the agent has allowed (accepted).
    allowed: Vec<ExperienceKey>,
    /// The experiences the agent has blocked.
    blocked: Vec<ExperienceKey>,
    /// Resolved experience names, folded in as [`SlSessionEvent::ExperienceInfo`]
    /// arrives.
    names: BTreeMap<ExperienceKey, String>,
    /// The number of [`Command::RequestExperiencePermissions`] GETs whose reply is
    /// still outstanding. An `ExperiencePermissions` event is treated as an
    /// authoritative full list only while this is non-zero (see the module docs).
    pending_full_list: u32,
    /// Bumped on any change the list rebuild must react to.
    revision: u64,
}

impl ExperiencesState {
    /// Bump the revision so the list rebuild reacts.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Replace both lists from an authoritative GET reply and bump the revision.
    fn set_lists(&mut self, allowed: Vec<ExperienceKey>, blocked: Vec<ExperienceKey>) {
        self.allowed = allowed;
        self.blocked = blocked;
        self.touch();
    }

    /// Fold a resolved name into the cache and bump the revision.
    fn note_name(&mut self, id: ExperienceKey, name: String) {
        let _previous = self.names.insert(id, name);
        self.touch();
    }

    /// Optimistically drop an experience from both lists (a forget), bumping the
    /// revision so its row leaves at once.
    fn forget(&mut self, id: ExperienceKey) {
        self.allowed.retain(|other| *other != id);
        self.blocked.retain(|other| *other != id);
        self.touch();
    }

    /// The resolved name for an experience id, if known.
    fn name(&self, id: ExperienceKey) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }
}

/// The floater's entities the systems act on: its root (toggled shown/hidden) and
/// the two list columns the rebuild fills.
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct ExperiencesUi {
    /// The floater root entity (carries [`UiPanelShown`]).
    pub(crate) panel: Entity,
    /// The allowed-experiences list column.
    allowed_list: Entity,
    /// The blocked-experiences list column.
    blocked_list: Entity,
}

/// Spawn the Experiences floater (hidden): a Refresh row, then the Allowed and
/// Blocked headed list columns the rebuild fills.
fn spawn_experiences_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: EXPERIENCES_FLOATER_ID,
            title: "Experiences".to_owned(),
            position: Vec2::new(360.0, 140.0),
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
        .insert(Translated::new("experiences-title"));
    let content = commands
        .spawn((
            Node {
                width: Val::Px(CONTENT_WIDTH),
                ..column(Val::Px(6.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    // The Refresh row.
    let refresh_row = commands
        .spawn((
            Node {
                justify_content: JustifyContent::End,
                ..row(Val::Px(6.0))
            },
            ChildOf(content),
        ))
        .id();
    let refresh = spawn_text_button(&mut commands, refresh_row, "experiences-refresh", 1);
    commands.entity(refresh).observe(
        |_activate: On<Activate>,
         mut state: ResMut<ExperiencesState>,
         mut sl: MessageWriter<SlCommand>| {
            request_permissions(&mut state, &mut sl);
        },
    );

    let allowed_list = spawn_section(&mut commands, content, "experiences-allowed-heading");
    let blocked_list = spawn_section(&mut commands, content, "experiences-blocked-heading");

    commands.insert_resource(ExperiencesUi {
        panel: handle.root,
        allowed_list,
        blocked_list,
    });
}

/// Spawn one headed list section (a heading label above a fixed-height clipped
/// column), returning the list column the rebuild fills.
fn spawn_section(commands: &mut Commands, parent: Entity, heading_key: &'static str) -> Entity {
    commands.spawn((
        Text::default(),
        Translated::new(heading_key),
        UiFont::Sans.at(HEADING_FONT_SIZE),
        TextColor(HEADING_COLOR),
        ChildOf(parent),
    ));
    commands
        .spawn((
            Node {
                height: Val::Px(LIST_HEIGHT),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            BackgroundColor(LIST_BACKGROUND),
            ChildOf(parent),
        ))
        .id()
}

/// Issue a fresh full-list GET, counting it so its reply is accepted as
/// authoritative (see `ExperiencesState::pending_full_list`).
fn request_permissions(state: &mut ExperiencesState, sl: &mut MessageWriter<SlCommand>) {
    state.pending_full_list = state.pending_full_list.saturating_add(1);
    sl.write(SlCommand(Command::RequestExperiencePermissions));
}

/// When the floater becomes visible, read the current experience preferences — so
/// the lists are fresh each time it is opened. Fires on the hidden→shown **edge**
/// (tracked in a `Local`), not merely while shown, so it issues one GET per open.
fn request_permissions_on_show(
    ui: Option<Res<ExperiencesUi>>,
    panels: Query<&UiPanelShown>,
    mut was_shown: Local<bool>,
    mut state: ResMut<ExperiencesState>,
    mut sl: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    let shown = panels.get(ui.panel).is_ok_and(|panel| panel.0);
    if shown && !*was_shown {
        request_permissions(&mut state, &mut sl);
    }
    *was_shown = shown;
}

/// Fold each arriving experience-preferences reply into the state — but only while
/// a GET we issued is outstanding (a single-edit reply, which arrives with no GET
/// pending, is ignored; the optimistic update already stands). Fetch names for any
/// ids not yet cached.
fn ingest_experience_permissions(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ExperiencesState>,
    mut sl: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        let SlSessionEvent::ExperiencePermissions { allowed, blocked } = &event.0 else {
            continue;
        };
        if state.pending_full_list == 0 {
            continue;
        }
        state.pending_full_list = state.pending_full_list.saturating_sub(1);
        state.set_lists(allowed.clone(), blocked.clone());
        let unknown: Vec<ExperienceKey> = state
            .allowed
            .iter()
            .chain(state.blocked.iter())
            .filter(|id| !state.names.contains_key(id))
            .copied()
            .collect();
        if !unknown.is_empty() {
            sl.write(SlCommand(Command::RequestExperienceInfo {
                experience_ids: unknown,
            }));
        }
    }
}

/// Fold each arriving experience metadata's name into the cache (skipping a
/// `missing` placeholder or an empty name), so the rows show names in place.
fn ingest_experience_names(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<ExperiencesState>,
) {
    for event in events.read() {
        let SlSessionEvent::ExperienceInfo(list) = &event.0 else {
            continue;
        };
        for info in list {
            if !info.missing && !info.name.is_empty() {
                state.note_name(info.public_id, info.name.clone());
            }
        }
    }
}

/// Rebuild both list columns whenever the state's revision moved: despawn the old
/// rows and spawn one row (name + Forget) per experience, or an empty note.
fn rebuild_lists(
    ui: Option<Res<ExperiencesUi>>,
    state: Res<ExperiencesState>,
    translator: Translator,
    mut last_revision: Local<Option<u64>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    let Some(ui) = ui else {
        return;
    };
    if *last_revision == Some(state.revision) {
        return;
    }
    *last_revision = Some(state.revision);
    rebuild_one(
        &mut commands,
        &children,
        &translator,
        ui.allowed_list,
        &state.allowed,
        &state,
    );
    rebuild_one(
        &mut commands,
        &children,
        &translator,
        ui.blocked_list,
        &state.blocked,
        &state,
    );
}

/// Rebuild one list column: despawn its rows and spawn one row per id (or the
/// empty note when the list is empty). Each row's Forget button forgets that
/// experience — writing `Forget` and dropping the row optimistically.
fn rebuild_one(
    commands: &mut Commands,
    children: &Query<&Children>,
    translator: &Translator,
    list: Entity,
    ids: &[ExperienceKey],
    state: &ExperiencesState,
) {
    if let Ok(existing) = children.get(list) {
        for child in existing {
            commands.entity(*child).despawn();
        }
    }
    if ids.is_empty() {
        commands.spawn((
            Text::new(translator.get("experiences-empty")),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            ChildOf(list),
        ));
        return;
    }
    let forget_label = translator.get("experiences-forget");
    for id in ids {
        let label = experience_label(state, *id);
        let forget = build_experience_row(commands, list, &label, &forget_label);
        let id = *id;
        commands.entity(forget).observe(
            move |_activate: On<Activate>,
                  mut state: ResMut<ExperiencesState>,
                  mut sl: MessageWriter<SlCommand>| {
                sl.write(SlCommand(Command::SetExperiencePermission {
                    experience_id: id,
                    permission: ExperiencePermission::Forget,
                }));
                state.forget(id);
            },
        );
    }
}

/// Build one list row — an experience name and a trailing Forget button — under
/// `parent`, returning the Forget button entity for the caller to wire. Shared by
/// the live rebuild and the gallery specimen (the registry rule,
/// [`crate::ui_element`]).
fn build_experience_row(
    commands: &mut Commands,
    parent: Entity,
    name: &str,
    forget_label: &str,
) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                ..row(Val::Px(6.0))
            },
            Name::new("experiences-row"),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::new(name.to_owned()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    spawn_button(commands, row_entity, forget_label, 0)
}

/// The row label for an experience: its resolved name, or the leading hex of its id
/// as a stable fallback while the name is still resolving.
fn experience_label(state: &ExperiencesState, id: ExperienceKey) -> String {
    state
        .name(id)
        .map_or_else(|| short_experience_id(id), str::to_owned)
}

/// The leading [`SHORT_ID_LEN`] hex characters of an experience id, an ellipsis
/// appended — the stable fallback shown until the name resolves.
fn short_experience_id(id: ExperienceKey) -> String {
    let hex = id.uuid().simple().to_string();
    let head: String = hex.chars().take(SHORT_ID_LEN).collect();
    format!("{head}\u{2026}")
}

/// Spawn a translated-label text button (Refresh), returning its clickable box.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
) -> Entity {
    let button = spawn_button_shell(commands, parent, tab);
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a literal-label button (a row's Forget), returning its clickable box.
fn spawn_button(commands: &mut Commands, parent: Entity, label: &str, tab: i32) -> Entity {
    let button = spawn_button_shell(commands, parent, tab);
    commands.spawn((
        Text::new(label.to_owned()),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

/// Spawn a button shell (the bordered, skinnable box) with no label yet, returning
/// it for a caller to fill and wire.
fn spawn_button_shell(commands: &mut Commands, parent: Entity, tab: i32) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BUTTON_BACKGROUND),
            BorderColor::all(BUTTON_BORDER),
            ClassList::new_with_classes([BUTTON_CLASS]),
            Name::new("experiences-button"),
            ChildOf(parent),
        ))
        .id()
}

/// The gallery / `ui_test` specimen: a static Allowed / Blocked layout with a few
/// rows, so the heading / row / Forget layout is swept login-free (the live floater
/// needs a session). Registered in `crate::ui_element::ELEMENTS`; its buttons
/// report an inert [`UiAction`].
pub fn spawn_experiences_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let root = commands
        .spawn((
            Node {
                width: Val::Px(CONTENT_WIDTH),
                ..column(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    let forget_label = cx.text("Forget");
    // Allowed section: a heading and two rows.
    spawn_specimen_heading(commands, root, &cx.text("Allowed experiences"));
    for name in [cx.text("Neon Speedway"), cx.text("Beachside Games")] {
        let forget = build_experience_row(commands, root, &name, &forget_label);
        wire_specimen_forget(commands, forget);
    }
    // Blocked section: a heading and one row.
    spawn_specimen_heading(commands, root, &cx.text("Blocked experiences"));
    let forget = build_experience_row(commands, root, &cx.text("Spam Kiosk"), &forget_label);
    wire_specimen_forget(commands, forget);
    root
}

/// Spawn a specimen section heading (no `Translated`, so the sweep shows the
/// transformed sample text directly).
fn spawn_specimen_heading(commands: &mut Commands, parent: Entity, text: &str) {
    commands.spawn((
        Text::new(text.to_owned()),
        UiFont::Sans.at(HEADING_FONT_SIZE),
        TextColor(HEADING_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// Wire a specimen row's Forget button to an inert [`UiAction`] (the registry rule:
/// a specimen reaches no session).
fn wire_specimen_forget(commands: &mut Commands, forget: Entity) {
    commands.entity(forget).observe(
        |_activate: On<Activate>, mut actions: MessageWriter<UiAction>| {
            actions.write(UiAction {
                element: EXPERIENCES_ELEMENT,
                action: "forget",
            });
        },
    );
}

#[cfg(test)]
mod tests {
    use super::{ExperiencesState, SHORT_ID_LEN, experience_label, short_experience_id};
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_client_bevy::{ExperienceKey, Uuid};

    /// The short id is the leading hex of the (dash-free) uuid with an ellipsis.
    #[test]
    fn short_id_is_leading_hex_with_ellipsis() {
        let id = ExperienceKey::from(Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0));
        let short = short_experience_id(id);
        assert_eq!(short, "12345678\u{2026}");
        // The head is exactly SHORT_ID_LEN characters before the ellipsis.
        assert_eq!(short.chars().count(), SHORT_ID_LEN + 1);
    }

    /// A row shows the resolved name once known, and the short-id fallback until
    /// then.
    #[test]
    fn label_prefers_the_resolved_name() {
        let id = ExperienceKey::from(Uuid::from_u128(0xabcd));
        let mut state = ExperiencesState::default();
        // Before resolution: the short id.
        assert_eq!(experience_label(&state, id), short_experience_id(id));
        // After resolution: the name.
        state.note_name(id, "Neon Speedway".to_owned());
        assert_eq!(experience_label(&state, id), "Neon Speedway");
    }

    /// A forget drops the experience from whichever list held it and bumps the
    /// revision so the row rebuild reacts.
    #[test]
    fn forget_drops_from_both_lists_and_touches() {
        let allowed = ExperienceKey::from(Uuid::from_u128(0x1));
        let blocked = ExperienceKey::from(Uuid::from_u128(0x2));
        let mut state = ExperiencesState::default();
        state.set_lists(vec![allowed], vec![blocked]);
        let before = state.revision;

        state.forget(allowed);
        assert!(state.allowed.is_empty());
        assert_eq!(state.blocked, vec![blocked]);
        assert_ne!(state.revision, before);
    }

    /// A single-edit reply (no GET outstanding) must not be mistaken for a full
    /// list: the pending-GET counter is what gates acceptance.
    #[test]
    fn pending_counter_gates_full_list_acceptance() {
        let mut state = ExperiencesState::default();
        // No GET outstanding: nothing to consume.
        assert_eq!(state.pending_full_list, 0);
        // One GET issued, one reply consumes it.
        state.pending_full_list = 1;
        assert_eq!(state.pending_full_list, 1);
        state.pending_full_list = state.pending_full_list.saturating_sub(1);
        assert_eq!(state.pending_full_list, 0);
    }
}
