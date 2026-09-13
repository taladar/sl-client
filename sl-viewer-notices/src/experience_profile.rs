//! The **Experience profile** window (`viewer-experiences-floater`): one
//! experience's metadata, the agent's own permission for it, and — for an
//! experience the agent administers — the editable half of that metadata.
//!
//! # One window per experience
//!
//! The reference registers this floater by experience id
//! (`LLFloaterReg::showInstance("experience_profile", id)`), so comparing two
//! experiences is two windows. It is therefore a **keyed** floater
//! ([`FloaterKey::subject`]): every piece of per-window state is a component on
//! the window root, the open path goes through [`KeyedFloaters::open`], and
//! closing one ends it without disturbing the other. The experience's name goes
//! in the **title bar**, because two of these windows are otherwise identical
//! strips.
//!
//! # What it shows, and where each field comes from
//!
//! All of it is one `GetExperienceInfo` record ([`ExperienceInfo`], the reply to
//! [`Command::RequestExperienceInfo`]): the name, description, content rating,
//! owner, home-location SLURL and the [`ExperienceProperties`] bitfield — which
//! is where "grid-wide vs land-scoped", "privileged", "private" and "disabled"
//! come from. Two further replies decide what the window *offers*:
//!
//! - [`Command::RequestExperienceAdmin`] → [`SlSessionEvent::ExperienceAdminStatus`]
//!   is what reveals the **Edit** button, exactly as the reference's
//!   `experienceIsAdmin` callback does.
//! - [`Command::RequestExperiencePermissions`] →
//!   [`SlSessionEvent::ExperiencePermissions`] says whether this agent has
//!   allowed, blocked or forgotten the experience, which is what greys the one
//!   of Allow / Forget / Block that would be a no-op. A **privileged**
//!   experience is exempt — the reference does not even ask
//!   (`refreshExperience`), because the agent cannot decline it.
//!
//! # Editing
//!
//! Edit swaps the read-only column for a field column (name, description,
//! rating, home location, and the Enable / Private toggles) and offers Save /
//! Cancel. Save sends [`Command::UpdateExperience`] with an
//! [`ExperienceUpdate`], and the grid's [`SlSessionEvent::ExperienceUpdated`]
//! reply repaints the window from what was actually stored — so a field the
//! grid refused visibly reverts rather than appearing to have been saved.
//!
//! The Enable and Private toggles are the reference's two editable property
//! bits and are written the same way round: Enable **clears**
//! `PROPERTY_DISABLED`, Private **sets** `PROPERTY_PRIVATE`, and every other
//! bit in the record is carried through untouched. So is
//! [`ExperienceInfo::extended_metadata`] — the opaque LLSD-XML blob holding the
//! reference's marketplace link and logo texture. This window neither shows nor
//! edits those two (see the divergence below), and precisely because it does
//! not, it must send the blob back **verbatim**: an update that dropped it would
//! silently delete an experience's store link.
//!
//! # Deliberate divergences from the reference
//!
//! - **No marketplace link, no logo texture, no group re-assignment.** The first
//!   two live inside `extended_metadata` as LLSD-XML, which no viewer-side
//!   decoder is wired up for yet, and the third needs the group picker. All
//!   three are filed as [[viewer-experience-profile-extended-metadata]]; the
//!   blob and the owner are preserved unchanged in the meantime.
//! - The rating list runs **General → Moderate → Adult**, where the reference's
//!   runs the other way. Every other rating control in this viewer is ascending
//!   ([`crate::experiences_floater`], About Region), and a single window
//!   disagreeing with the rest is worse than disagreeing with the reference.
//!
//! Reference (Firestorm, read-only): `llfloaterexperienceprofile`,
//! `floater_experienceprofile.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button};
use bevy_flair::style::components::ClassList;

use sl_client_bevy::{
    Command, ExperienceInfo, ExperienceKey, ExperiencePermission, ExperienceProperties,
    ExperienceUpdate, SlCommand, SlCurrentRegion, SlEvent, SlRegionIdentity, SlSessionEvent,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters, host_floater,
};
use crate::i18n::{Translated, Translator};
use crate::ui::{column, row};
use crate::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use crate::ui_font::UiFont;
use crate::ui_name_link::{NameLink, NameLinkSpec, NameTarget, set_name_link, spawn_name_link};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::world_api::AgentRegionPosition;

/// The profile window's stable
/// [`Floater::id`](sl_viewer_ui_widgets::floater::Floater::id) — the **kind**;
/// which experience an instance shows is its [`FloaterKey`].
pub const EXPERIENCE_PROFILE_FLOATER_ID: &str = "experience-profile";

/// The body text size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// The experience-name heading size, in logical pixels.
const HEADING_FONT_SIZE: f32 = 16.0;

/// The window's content width, in logical pixels.
const CONTENT_WIDTH: f32 = 360.0;

/// The primary body text colour.
const TEXT_COLOR: Color = Color::srgb(0.90, 0.93, 0.97);

/// A dimmer secondary text colour (field captions, the empty-location note).
const DIM_TEXT_COLOR: Color = Color::srgb(0.64, 0.68, 0.76);

/// The heading accent — the emerald every experience surface wears.
const HEADING_COLOR: Color = Color::srgb(0.42, 0.82, 0.60);

/// A button's fallback background — the skin's `.sk-button` overrides it.
const BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// A button's fallback border — the skin's `.sk-button` overrides it.
const BUTTON_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);

/// A disabled button's label colour, so a press that would do nothing reads as
/// inert.
const DISABLED_TEXT_COLOR: Color = Color::srgb(0.45, 0.47, 0.52);

/// The skin class a button wears (`.sk-button`).
const BUTTON_CLASS: &str = "sk-button";

/// The glyph a ticked toggle shows.
const CHECKED_GLYPH: &str = "\u{2611}";

/// The glyph an unticked toggle shows.
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The description field's visible line count in edit mode.
const DESCRIPTION_LINES: f32 = 4.0;

/// The name field's width in edit mode, in `"0"`-glyph advances.
const NAME_FIELD_GLYPHS: f32 = 30.0;

// ---------------------------------------------------------------------------
// Content ratings.
// ---------------------------------------------------------------------------

/// The `sim_access` code for a General-rated experience.
pub const MATURITY_GENERAL: i32 = 13;

/// The `sim_access` code for a Moderate-rated experience.
pub const MATURITY_MODERATE: i32 = 21;

/// The `sim_access` code for an Adult-rated experience.
pub const MATURITY_ADULT: i32 = 42;

/// The rating option keys, ascending — the order both this window's combo and
/// the search tab's filter use (see the module docs' divergence note).
pub const MATURITY_KEYS: [&str; 3] = [
    "experience-rating-general",
    "experience-rating-moderate",
    "experience-rating-adult",
];

/// The combo index for a `sim_access` rating code, by the reference's
/// thresholds (`LLFloaterExperienceProfile::setMaturityString`): at or below
/// General is General, at or below Moderate is Moderate, anything above is
/// Adult.
#[must_use]
pub const fn maturity_index(maturity: i32) -> usize {
    if maturity <= MATURITY_GENERAL {
        0
    } else if maturity <= MATURITY_MODERATE {
        1
    } else {
        2
    }
}

/// The `sim_access` rating code for a combo index — the inverse of
/// [`maturity_index`] for the three codes it can produce.
#[must_use]
pub const fn maturity_from_index(index: usize) -> i32 {
    match index {
        0 => MATURITY_GENERAL,
        1 => MATURITY_MODERATE,
        _other => MATURITY_ADULT,
    }
}

/// The Fluent key naming a rating code.
#[must_use]
pub fn maturity_key(maturity: i32) -> &'static str {
    MATURITY_KEYS
        .get(maturity_index(maturity))
        .copied()
        .unwrap_or("experience-rating-adult")
}

// ---------------------------------------------------------------------------
// Opening.
// ---------------------------------------------------------------------------

/// Open (or raise) the profile window for one experience — written by the
/// experiences floater's lists and search results, and by anything else that
/// wants to show an experience.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenExperienceProfile {
    /// The experience whose profile to show.
    pub experience: ExperienceKey,
}

/// The [`FloaterKey`] of the window showing `experience`.
///
/// A [`FloaterKey`]: instances are told apart by the
/// experience id and none of them persists geometry, which is what keeps a
/// settings entry from accruing per experience ever looked at.
fn profile_key(experience: ExperienceKey) -> FloaterKey {
    FloaterKey::subject(&experience)
}

/// The profile window's [`FloaterSpec`] — shared with the `FLOATERS` registry,
/// so the swept window is the one the viewer spawns.
#[must_use]
pub fn experience_profile_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: EXPERIENCE_PROFILE_FLOATER_ID,
        title: "Experience Profile".to_owned(),
        position: Vec2::new(420.0, 180.0),
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

// ---------------------------------------------------------------------------
// Per-window state.
// ---------------------------------------------------------------------------

/// The half of an in-progress edit that is not a text field: the two property
/// toggles, the chosen rating, and the home location (which is *pressed* rather
/// than typed, so it has nowhere else to live).
#[derive(Debug, Default)]
struct EditBuffer {
    /// The Enable toggle (the inverse of [`PROPERTY_DISABLED`]).
    enabled: bool,
    /// The Private toggle ([`PROPERTY_PRIVATE`]).
    private: bool,
    /// The chosen rating, as a combo index.
    maturity: usize,
    /// The home location, or `None` for the reference's cleared SLURL.
    location: Option<url::Url>,
    /// Raised whenever the two typed fields should be re-seeded from the record
    /// — entering edit mode, or a record arriving while not editing. Consumed by
    /// the paint pass, which otherwise never writes them (see
    /// [`paint_profile_windows`]).
    seed: bool,
}

/// One profile window's state — a component on the window root, so two open
/// profiles never read each other's ([`KeyedFloaters`]).
#[derive(Component, Debug)]
pub struct ExperienceProfileState {
    /// The experience this window shows.
    experience: ExperienceKey,
    /// The last metadata record the grid sent, or `None` until it arrives.
    info: Option<ExperienceInfo>,
    /// Whether the agent administers this experience (the `IsExperienceAdmin`
    /// reply) — what reveals Edit.
    can_edit: bool,
    /// The agent's own preference: `Some(Allow)`, `Some(Block)`, or `None` for
    /// forgotten. Meaningless until [`permission_known`](Self::permission_known).
    permission: Option<ExperiencePermission>,
    /// Whether a `GetExperiences` reply has landed, so the three permission
    /// buttons can be gated on the real preference rather than on a guess.
    permission_known: bool,
    /// Whether the window is in edit mode.
    editing: bool,
    /// What an in-progress edit would save, for the parts that are not simply
    /// read back out of a field on Save.
    edit: EditBuffer,
    /// A Fluent key for the status line under the buttons, or `None` for none.
    status: Option<&'static str>,
    /// Bumped whenever the painted view must be redone.
    revision: u64,
    /// The revision the paint pass last rendered.
    painted: Option<u64>,
}

impl ExperienceProfileState {
    /// A fresh window's state for `experience`, before any reply has arrived.
    fn new(experience: ExperienceKey) -> Self {
        Self {
            experience,
            info: None,
            can_edit: false,
            permission: None,
            permission_known: false,
            editing: false,
            edit: EditBuffer::default(),
            status: None,
            revision: 0,
            painted: None,
        }
    }

    /// Mark the window for a repaint.
    const fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Seed the edit buffer from the current record — what pressing Edit (and a
    /// Cancel that reverts) does.
    fn reset_edit_buffer(&mut self) {
        let properties = self
            .info
            .as_ref()
            .map_or_else(ExperienceProperties::default, |info| info.properties);
        self.edit = EditBuffer {
            enabled: !properties.is_disabled(),
            private: properties.is_private(),
            maturity: self
                .info
                .as_ref()
                .map_or(0, |info| maturity_index(info.maturity)),
            location: self.info.as_ref().and_then(|info| info.slurl.clone()),
            seed: true,
        };
    }

    /// Whether the three permission buttons apply at all: a **privileged**
    /// experience cannot be declined, so the reference shows a note in their
    /// place rather than buttons that would be refused.
    fn permission_applies(&self) -> bool {
        !self
            .info
            .as_ref()
            .is_some_and(|info| info.properties.is_privileged())
    }

    /// The [`ExperienceUpdate`] a Save would send, from the record plus the edit
    /// buffer and the two typed fields. `None` before any record has arrived —
    /// there is nothing to base an update on, and inventing the untouched fields
    /// would overwrite them.
    fn update_from(&self, name: String, description: String) -> Option<ExperienceUpdate> {
        let info = self.info.as_ref()?;
        let mut properties = info.properties.0;
        if self.edit.enabled {
            properties &= !PROPERTY_DISABLED;
        } else {
            properties |= PROPERTY_DISABLED;
        }
        if self.edit.private {
            properties |= PROPERTY_PRIVATE;
        } else {
            properties &= !PROPERTY_PRIVATE;
        }
        Some(ExperienceUpdate {
            public_id: info.public_id,
            name,
            description,
            maturity: maturity_from_index(self.edit.maturity),
            properties,
            slurl: self.edit.location.clone(),
            // Carried through untouched: this window does not decode the blob,
            // so it must not be the thing that deletes it (see the module docs).
            extended_metadata: info.extended_metadata.clone(),
        })
    }
}

/// The `PROPERTY_DISABLED` bit, mirrored locally because the `PROPERTY_*`
/// constants are not part of the viewer runtime's re-export surface — only the
/// [`ExperienceProperties`] accessors are, and an *edit* needs to set bits, not
/// just read them.
const PROPERTY_DISABLED: i32 = sl_types::experience::PROPERTY_DISABLED;

/// The `PROPERTY_PRIVATE` bit — see [`PROPERTY_DISABLED`].
const PROPERTY_PRIVATE: i32 = sl_types::experience::PROPERTY_PRIVATE;

/// One profile window's widget handles, a component beside its state.
#[derive(Component, Debug)]
struct ExperienceProfileUi {
    /// The read-only column (shown while not editing).
    view_panel: Entity,
    /// The edit column (shown while editing).
    edit_panel: Entity,
    /// The experience-name heading in the view column.
    name_text: Entity,
    /// The description paragraph.
    description_text: Entity,
    /// The content-rating value.
    rating_text: Entity,
    /// The owner name link.
    owner_link: Entity,
    /// The home-location value (a SLURL or the "not set" note).
    location_text: Entity,
    /// The grid-wide / land-scoped line.
    scope_text: Entity,
    /// The "this experience is privileged" note — shown only when it is.
    privileged_text: Entity,
    /// The Allow button and its label (greyed when the press would be a no-op).
    allow_button: ActionHandle,
    /// The Forget button and its label.
    forget_button: ActionHandle,
    /// The Block button and its label.
    block_button: ActionHandle,
    /// The Edit button — shown only to an administrator.
    edit_button: Entity,
    /// The status line under the actions.
    status_text: Entity,
    /// The edit column's name field.
    name_field: Entity,
    /// The edit column's description field.
    description_field: Entity,
    /// The edit column's rating combo.
    maturity_combo: Entity,
    /// The edit column's home-location value.
    edit_location_text: Entity,
    /// The Enable toggle's glyph.
    enable_glyph: Entity,
    /// The Private toggle's glyph.
    private_glyph: Entity,
}

/// A spawned action button: the clickable box and the label inside it. Both are
/// needed, because "show / hide" is the box's and "grey out" is the label's.
#[derive(Debug, Clone, Copy)]
struct ActionHandle {
    /// The clickable box.
    button: Entity,
    /// The label text node inside it.
    label: Entity,
}

/// Which of the window's buttons a node is, so one observer serves them all and
/// the press can find its own window with [`host_floater`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileButton {
    /// Set the agent's preference to Allow.
    Allow,
    /// Clear the agent's preference.
    Forget,
    /// Set the agent's preference to Block.
    Block,
    /// Enter edit mode.
    Edit,
    /// Leave edit mode, discarding the edits.
    Cancel,
    /// Send the edits.
    Save,
    /// Set the home location to where the agent stands.
    SetLocation,
    /// Clear the home location.
    ClearLocation,
    /// Toggle the Enable property bit.
    ToggleEnabled,
    /// Toggle the Private property bit.
    TogglePrivate,
}

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin owning the Experience profile windows.
#[derive(Debug)]
pub struct ExperienceProfilePlugin;

impl Plugin for ExperienceProfilePlugin {
    /// Register the open path and the per-window systems.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenExperienceProfile>().add_systems(
            Update,
            (
                // After the manager's command pass: the click that opens a
                // profile usually raises the window it landed in too, and the
                // later raise wins (`FloaterSystems`).
                open_experience_profile.after(FloaterSystems::Commands),
                (
                    ingest_profile_events,
                    track_profile_maturity_combo,
                    paint_profile_windows,
                )
                    .chain()
                    .run_if(any_with_component::<ExperienceProfileState>),
            )
                .chain(),
        );
    }
}

/// Open the window for each requested experience, building its content on the
/// first open and asking the grid for everything it shows.
fn open_experience_profile(
    mut opens: MessageReader<OpenExperienceProfile>,
    mut floaters: KeyedFloaters,
    mut states: Query<&mut ExperienceProfileState>,
    mut commands: Commands,
    mut sl: MessageWriter<SlCommand>,
) {
    for open in opens.read().copied() {
        let experience = open.experience;
        let opened = floaters.open(experience_profile_floater_spec(), profile_key(experience));
        let KeyedFloaterOpen::Spawned(handle) = opened else {
            // Already up: repaint from what it holds, and re-ask, so an open
            // after an edit elsewhere shows the edit.
            if let Ok(mut state) = states.get_mut(opened.root()) {
                state.touch();
            }
            request_profile(experience, &mut sl);
            continue;
        };
        build_profile_content(&mut commands, &handle, experience);
        request_profile(experience, &mut sl);
    }
}

/// Ask the grid for everything one profile window shows: the metadata, whether
/// the agent administers it, and the agent's own preferences.
fn request_profile(experience: ExperienceKey, sl: &mut MessageWriter<SlCommand>) {
    sl.write(SlCommand(Command::RequestExperienceInfo {
        experience_ids: vec![experience],
    }));
    sl.write(SlCommand(Command::RequestExperienceAdmin {
        experience_id: experience,
    }));
    sl.write(SlCommand(Command::RequestExperiencePermissions));
}

// ---------------------------------------------------------------------------
// Content.
// ---------------------------------------------------------------------------

/// Build a fresh window's content and insert its per-window state.
fn build_profile_content(
    commands: &mut Commands,
    handle: &FloaterHandle,
    experience: ExperienceKey,
) {
    commands
        .entity(handle.title_text)
        .insert(Translated::new("experience-profile-title"));
    let content = commands
        .spawn((
            Node {
                width: Val::Px(CONTENT_WIDTH),
                ..column(Val::Px(6.0))
            },
            Name::new("experience-profile-content"),
            ChildOf(handle.content),
        ))
        .id();

    let view = build_view_panel(commands, content);
    let edit = build_edit_panel(commands, content);

    let mut state = ExperienceProfileState::new(experience);
    state.touch();
    commands.entity(handle.root).insert((
        state,
        ExperienceProfileUi {
            view_panel: view.panel,
            edit_panel: edit.panel,
            name_text: view.name_text,
            description_text: view.description_text,
            rating_text: view.rating_text,
            owner_link: view.owner_link,
            location_text: view.location_text,
            scope_text: view.scope_text,
            privileged_text: view.privileged_text,
            allow_button: view.allow_button,
            forget_button: view.forget_button,
            block_button: view.block_button,
            edit_button: view.edit_button.button,
            status_text: view.status_text,
            name_field: edit.name_field,
            description_field: edit.description_field,
            maturity_combo: edit.maturity_combo,
            edit_location_text: edit.location_text,
            enable_glyph: edit.enable_glyph,
            private_glyph: edit.private_glyph,
        },
    ));
}

/// The read-only column's handles, returned by [`build_view_panel`].
struct ViewPanel {
    /// The column itself.
    panel: Entity,
    /// The name heading.
    name_text: Entity,
    /// The description paragraph.
    description_text: Entity,
    /// The rating value.
    rating_text: Entity,
    /// The owner name link.
    owner_link: Entity,
    /// The home-location value.
    location_text: Entity,
    /// The scope line.
    scope_text: Entity,
    /// The privileged note.
    privileged_text: Entity,
    /// The Allow button.
    allow_button: ActionHandle,
    /// The Forget button.
    forget_button: ActionHandle,
    /// The Block button.
    block_button: ActionHandle,
    /// The Edit button.
    edit_button: ActionHandle,
    /// The status line.
    status_text: Entity,
}

/// Build the read-only column: the name heading, the fields, the three
/// permission buttons, Edit, and the status line.
fn build_view_panel(commands: &mut Commands, parent: Entity) -> ViewPanel {
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..column(Val::Px(5.0))
            },
            Name::new("experience-profile-view"),
            ChildOf(parent),
        ))
        .id();
    let name_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(HEADING_FONT_SIZE),
            TextColor(HEADING_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-name"),
            ChildOf(panel),
        ))
        .id();
    let description_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-description"),
            ChildOf(panel),
        ))
        .id();
    let rating_text = spawn_field_row(commands, panel, "experience-profile-rating");
    let owner_row = spawn_caption_row(commands, panel, "experience-profile-owner");
    let owner_link = spawn_name_link(
        commands,
        owner_row,
        NameLinkSpec::new(
            "experience-profile-owner-loading",
            "experience-profile-owner-none",
        ),
    );
    let location_text = spawn_field_row(commands, panel, "experience-profile-location");
    let scope_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-scope"),
            ChildOf(panel),
        ))
        .id();
    let privileged_text = commands
        .spawn((
            Text::default(),
            Translated::new("experience-profile-privileged"),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-privileged"),
            ChildOf(panel),
        ))
        .id();

    let actions = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                ..row(Val::Px(6.0))
            },
            Name::new("experience-profile-actions"),
            ChildOf(panel),
        ))
        .id();
    let allow_button = spawn_action(
        commands,
        actions,
        "experience-profile-allow",
        ProfileButton::Allow,
        1,
    );
    let forget_button = spawn_action(
        commands,
        actions,
        "experience-profile-forget",
        ProfileButton::Forget,
        2,
    );
    let block_button = spawn_action(
        commands,
        actions,
        "experience-profile-block",
        ProfileButton::Block,
        3,
    );
    let edit_button = spawn_action(
        commands,
        actions,
        "experience-profile-edit",
        ProfileButton::Edit,
        4,
    );
    let status_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-status"),
            ChildOf(panel),
        ))
        .id();

    ViewPanel {
        panel,
        name_text,
        description_text,
        rating_text,
        owner_link,
        location_text,
        scope_text,
        privileged_text,
        allow_button,
        forget_button,
        block_button,
        edit_button,
        status_text,
    }
}

/// The edit column's handles, returned by [`build_edit_panel`].
struct EditPanel {
    /// The column itself.
    panel: Entity,
    /// The name field.
    name_field: Entity,
    /// The description field.
    description_field: Entity,
    /// The rating combo.
    maturity_combo: Entity,
    /// The home-location value.
    location_text: Entity,
    /// The Enable toggle's glyph.
    enable_glyph: Entity,
    /// The Private toggle's glyph.
    private_glyph: Entity,
}

/// Build the edit column: the three typed / chosen fields, the two location
/// buttons, the two property toggles, and Save / Cancel. Starts hidden.
fn build_edit_panel(commands: &mut Commands, parent: Entity) -> EditPanel {
    let panel = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                display: Display::None,
                ..column(Val::Px(5.0))
            },
            Name::new("experience-profile-edit"),
            ChildOf(parent),
        ))
        .id();
    spawn_caption(commands, panel, "experience-profile-name-caption");
    let name_field = spawn_text_input(
        commands,
        panel,
        &TextInputSpec {
            font_size: FONT_SIZE,
            width_glyphs: NAME_FIELD_GLYPHS,
            tab_index: 5,
            ..TextInputSpec::new("experience-profile-name-field", TextInputKind::Line)
        },
    );
    spawn_caption(commands, panel, "experience-profile-description-caption");
    let description_field = spawn_text_input(
        commands,
        panel,
        &TextInputSpec {
            font_size: FONT_SIZE,
            visible_lines: DESCRIPTION_LINES,
            tab_index: 6,
            ..TextInputSpec::new(
                "experience-profile-description-field",
                TextInputKind::Multiline,
            )
        },
    );
    let rating_row = spawn_caption_row(commands, panel, "experience-profile-rating");
    let labels: Vec<String> = MATURITY_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let maturity_combo = spawn_combo(
        commands,
        rating_row,
        &ComboSpec {
            element: "experience-profile-rating",
            labels: &labels,
            active: 0,
            tab_index: 7,
            font_size: FONT_SIZE,
            translate_labels: true,
        },
    );
    let location_row = spawn_caption_row(commands, panel, "experience-profile-location");
    let location_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
            Name::new("experience-profile-edit-location"),
            ChildOf(location_row),
        ))
        .id();
    let location_buttons = commands
        .spawn((
            Node {
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    let _set = spawn_action(
        commands,
        location_buttons,
        "experience-profile-set-location",
        ProfileButton::SetLocation,
        8,
    );
    let _clear = spawn_action(
        commands,
        location_buttons,
        "experience-profile-clear-location",
        ProfileButton::ClearLocation,
        9,
    );
    let enable_glyph = spawn_toggle(
        commands,
        panel,
        "experience-profile-enabled",
        ProfileButton::ToggleEnabled,
        10,
    );
    let private_glyph = spawn_toggle(
        commands,
        panel,
        "experience-profile-private",
        ProfileButton::TogglePrivate,
        11,
    );
    let save_row = commands
        .spawn((
            Node {
                ..row(Val::Px(6.0))
            },
            ChildOf(panel),
        ))
        .id();
    let _save = spawn_action(
        commands,
        save_row,
        "experience-profile-save",
        ProfileButton::Save,
        12,
    );
    let _cancel = spawn_action(
        commands,
        save_row,
        "experience-profile-cancel",
        ProfileButton::Cancel,
        13,
    );

    EditPanel {
        panel,
        name_field,
        description_field,
        maturity_combo,
        location_text,
        enable_glyph,
        private_glyph,
    }
}

/// Spawn a caption line on its own.
fn spawn_caption(commands: &mut Commands, parent: Entity, caption_key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(caption_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(DIM_TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// Spawn a `caption value` row and return the row, for a caller that fills the
/// value side with something other than a plain text node.
fn spawn_caption_row(commands: &mut Commands, parent: Entity, caption_key: &'static str) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    spawn_caption(commands, row_entity, caption_key);
    row_entity
}

/// Spawn a `caption value` row and return the value text node.
fn spawn_field_row(commands: &mut Commands, parent: Entity, caption_key: &'static str) -> Entity {
    let row_entity = spawn_caption_row(commands, parent, caption_key);
    commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id()
}

/// Spawn one of the window's buttons, wired to the shared press observer.
fn spawn_action(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    button: ProfileButton,
    tab: i32,
) -> ActionHandle {
    let entity = commands
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
            button,
            Name::new("experience-profile-button"),
            ChildOf(parent),
        ))
        .id();
    let label = commands
        .spawn((
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
            ChildOf(entity),
        ))
        .id();
    commands.entity(entity).observe(on_profile_button);
    ActionHandle {
        button: entity,
        label,
    }
}

/// Spawn a property toggle — a clickable glyph leading a translated label —
/// returning the glyph node the paint pass writes.
fn spawn_toggle(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    button: ProfileButton,
    tab: i32,
) -> Entity {
    let row_entity = commands
        .spawn((
            Button,
            TabIndex(tab),
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(5.0))
            },
            button,
            Pickable::default(),
            Name::new("experience-profile-toggle"),
            ChildOf(parent),
        ))
        .id();
    let glyph = commands
        .spawn((
            Text::new(UNCHECKED_GLYPH),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(TEXT_COLOR),
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT_SIZE),
        TextColor(TEXT_COLOR),
        Pickable::IGNORE,
        ChildOf(row_entity),
    ));
    commands.entity(row_entity).observe(on_profile_button);
    glyph
}

// ---------------------------------------------------------------------------
// Interaction.
// ---------------------------------------------------------------------------

/// Every button in the window, resolved to **its own** window with
/// [`host_floater`] — so a press in one profile never edits another's.
#[expect(
    clippy::too_many_arguments,
    reason = "an observer's parameters are its injected world access: the button kind, \
              the window lookup, that window's state and fields, the region the \
              location button reads, and the two command sinks"
)]
fn on_profile_button(
    activate: On<Activate>,
    buttons: Query<&ProfileButton>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut states: Query<(&mut ExperienceProfileState, &ExperienceProfileUi)>,
    fields: Query<&EditableText>,
    position: Res<AgentRegionPosition>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut sl: MessageWriter<SlCommand>,
) {
    let Ok(button) = buttons.get(activate.entity) else {
        return;
    };
    let Some(window) = host_floater(activate.entity, &parents, &floaters) else {
        return;
    };
    let Ok((mut state, ui)) = states.get_mut(window) else {
        return;
    };
    let experience = state.experience;
    match *button {
        ProfileButton::Allow | ProfileButton::Block | ProfileButton::Forget => {
            let permission = match *button {
                ProfileButton::Allow => ExperiencePermission::Allow,
                ProfileButton::Block => ExperiencePermission::Block,
                _forget => ExperiencePermission::Forget,
            };
            sl.write(SlCommand(Command::SetExperiencePermission {
                experience_id: experience,
                permission,
            }));
            // Optimistic, like the experiences floater's Forget: the cap's
            // reply carries only the edited experience, never the full lists.
            state.permission = match permission {
                ExperiencePermission::Forget => None,
                other => Some(other),
            };
            state.permission_known = true;
            state.touch();
        }
        ProfileButton::Edit => {
            state.reset_edit_buffer();
            state.editing = true;
            state.status = None;
            state.touch();
        }
        ProfileButton::Cancel => {
            state.editing = false;
            state.status = None;
            state.touch();
        }
        ProfileButton::Save => {
            let name = field_value(&fields, ui.name_field);
            let description = field_value(&fields, ui.description_field);
            match state.update_from(name, description) {
                Some(update) => {
                    sl.write(SlCommand(Command::UpdateExperience { update }));
                    state.status = Some("experience-profile-saving");
                }
                // No record to base an update on: say so rather than sending a
                // half-invented one.
                None => state.status = Some("experience-profile-not-loaded"),
            }
            state.touch();
        }
        ProfileButton::SetLocation => {
            match current_location(&position, &regions) {
                Some(location) => {
                    state.edit.location = Some(location);
                    state.status = None;
                }
                None => state.status = Some("experience-profile-no-location"),
            }
            state.touch();
        }
        ProfileButton::ClearLocation => {
            state.edit.location = None;
            state.touch();
        }
        ProfileButton::ToggleEnabled => {
            state.edit.enabled = !state.edit.enabled;
            state.touch();
        }
        ProfileButton::TogglePrivate => {
            state.edit.private = !state.edit.private;
            state.touch();
        }
    }
}

/// One edit field's current text, or empty when the field has gone.
fn field_value(fields: &Query<&EditableText>, field: Entity) -> String {
    fields
        .get(field)
        .map(|text| text.value().to_string())
        .unwrap_or_default()
}

/// The SLURL of where the agent stands, or `None` before the region handshake
/// (or before the first position update) — the reference's `onClickLocation`,
/// which likewise does nothing without a region.
fn current_location(
    position: &AgentRegionPosition,
    regions: &Query<&SlRegionIdentity, With<SlCurrentRegion>>,
) -> Option<url::Url> {
    let sim_name = regions
        .single()
        .ok()
        .and_then(|region| region.0.sim_name.clone())?;
    let pos = position.position.as_ref()?;
    let url = sl_types::map::Location::new(
        sim_name,
        local_coord_u8(pos.x),
        local_coord_u8(pos.y),
        local_coord_u16(pos.z),
    )
    .as_maps_url();
    url::Url::parse(&url).ok()
}

/// Clamp a region-local x / y coordinate to the SLURL's `u8` range (a
/// var-region position past 255 m clamps; the classic SLURL cannot express it).
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

/// Mirror the rating combo's selection into the edit buffer of the window it
/// sits in. The combo reports by element name rather than by entity, so the
/// window is found from the combo entity the message carries.
fn track_profile_maturity_combo(
    mut changes: MessageReader<ComboChanged>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut states: Query<&mut ExperienceProfileState>,
) {
    for change in changes.read() {
        let Some(window) = host_floater(change.combo, &parents, &floaters) else {
            continue;
        };
        let Ok(mut state) = states.get_mut(window) else {
            continue;
        };
        if state.edit.maturity != change.active {
            state.edit.maturity = change.active;
            state.touch();
        }
    }
}

// ---------------------------------------------------------------------------
// Ingest.
// ---------------------------------------------------------------------------

/// Fold this frame's experience replies into **every** open profile window they
/// are about.
///
/// The frame's events are collected once and replayed per window: a reader is
/// consumed by the first pass over it, so with two profiles open the second
/// would otherwise see nothing.
fn ingest_profile_events(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut ExperienceProfileState>,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut state in &mut windows {
        for event in &frame {
            ingest_one(&mut state, &event.0);
        }
    }
}

/// Fold one event into one window's state.
fn ingest_one(state: &mut ExperienceProfileState, event: &SlSessionEvent) {
    match event {
        SlSessionEvent::ExperienceInfo(list) => {
            if let Some(info) = list
                .iter()
                .find(|info| info.public_id == state.experience && !info.missing)
            {
                note_info(state, info.clone());
            }
        }
        SlSessionEvent::ExperienceUpdated(info) => {
            if info.public_id == state.experience {
                note_info(state, info.clone());
                // The grid answered: leave edit mode showing what was stored.
                state.editing = false;
                state.status = Some("experience-profile-saved");
            }
        }
        SlSessionEvent::ExperienceAdminStatus {
            experience_id,
            is_admin,
        } => {
            if *experience_id == state.experience && state.can_edit != *is_admin {
                state.can_edit = *is_admin;
                state.touch();
            }
        }
        SlSessionEvent::ExperiencePermissions { allowed, blocked } => {
            // A single-edit reply arrives with both lists empty (the runtime
            // collapses it into this event); the optimistic update already
            // stands, so an empty pair is not evidence of a forgotten
            // experience.
            if allowed.is_empty() && blocked.is_empty() {
                return;
            }
            let permission = if allowed.contains(&state.experience) {
                Some(ExperiencePermission::Allow)
            } else if blocked.contains(&state.experience) {
                Some(ExperiencePermission::Block)
            } else {
                None
            };
            if !state.permission_known || state.permission != permission {
                state.permission = permission;
                state.permission_known = true;
                state.touch();
            }
        }
        _other => {}
    }
}

/// Record a fresh metadata record, re-seeding the edit buffer when the window is
/// not mid-edit (so a refresh does not overwrite what is being typed).
fn note_info(state: &mut ExperienceProfileState, info: ExperienceInfo) {
    state.info = Some(info);
    if !state.editing {
        state.reset_edit_buffer();
    }
    state.touch();
}

// ---------------------------------------------------------------------------
// Paint.
// ---------------------------------------------------------------------------

/// Repaint every window whose revision moved (or whose locale did) — in place,
/// never by rebuilding the widgets: a floater builds its content once and
/// updates it in place.
///
/// The two typed fields are the exception to "repaint everything": they are
/// seeded **once**, on the [`seed`](EditBuffer::seed)
/// edge that entering edit mode (or a fresh record arriving outside it) raises.
/// A paint that wrote them every pass would replace what is being typed with the
/// grid's copy on the next unrelated revision bump — the bug the notecard editor
/// already paid for.
#[expect(
    clippy::too_many_arguments,
    reason = "one paint pass writes every kind of node the window holds: text, \
              colours, name links, edit fields, the combo and the two panels' \
              visibility"
)]
fn paint_profile_windows(
    translator: Translator,
    mut windows: Query<(&mut ExperienceProfileState, &ExperienceProfileUi)>,
    mut texts: Query<&mut Text>,
    mut colors: Query<&mut TextColor>,
    mut nodes: Query<&mut Node>,
    mut links: Query<&mut NameLink>,
    mut fields: Query<&mut EditableText>,
    mut combos: Query<&mut ComboSelection>,
) {
    let relocalised = translator.changed();
    for (mut state, ui) in &mut windows {
        if !relocalised && state.painted == Some(state.revision) {
            continue;
        }
        state.painted = Some(state.revision);
        let seed = core::mem::take(&mut state.edit.seed);
        let info = state.info.as_ref();
        set_text(
            &mut texts,
            ui.name_text,
            &info.map_or_else(
                || translator.get("experience-profile-loading"),
                |info| experience_display_name(info, &translator),
            ),
        );
        set_text(
            &mut texts,
            ui.description_text,
            info.map_or("", |info| info.description.as_str()),
        );
        set_text(
            &mut texts,
            ui.rating_text,
            &info.map_or_else(String::new, |info| {
                translator.get(maturity_key(info.maturity))
            }),
        );
        set_name_link(
            &mut links,
            Some(ui.owner_link),
            NameTarget::from_option(info.is_some(), info.and_then(|info| info.owner)),
        );
        set_text(
            &mut texts,
            ui.location_text,
            &location_text(info.and_then(|info| info.slurl.as_ref()), &translator),
        );
        set_text(
            &mut texts,
            ui.scope_text,
            &info.map_or_else(String::new, |info| {
                translator.get(if info.properties.is_grid() {
                    "experience-profile-scope-grid"
                } else {
                    "experience-profile-scope-land"
                })
            }),
        );
        // The privileged note and the permission buttons are mutually exclusive:
        // an experience the agent cannot decline offers nothing to press.
        let privileged = !state.permission_applies();
        show(&mut nodes, ui.privileged_text, privileged);
        for (action, permission) in [
            (ui.allow_button, Some(ExperiencePermission::Allow)),
            (ui.forget_button, None),
            (ui.block_button, Some(ExperiencePermission::Block)),
        ] {
            show(&mut nodes, action.button, !privileged);
            // The button standing for the current preference is the no-op one.
            let redundant = state.permission_known && state.permission == permission;
            set_label_enabled(&mut colors, action.label, !redundant);
        }
        show(&mut nodes, ui.edit_button, state.can_edit && !privileged);
        set_text(
            &mut texts,
            ui.status_text,
            &state
                .status
                .map_or_else(String::new, |key| translator.get(key)),
        );

        show(&mut nodes, ui.view_panel, !state.editing);
        show(&mut nodes, ui.edit_panel, state.editing);
        if seed {
            set_field(
                &mut fields,
                ui.name_field,
                info.map_or("", |info| info.name.as_str()),
            );
            set_field(
                &mut fields,
                ui.description_field,
                info.map_or("", |info| info.description.as_str()),
            );
        }
        if let Ok(mut combo) = combos.get_mut(ui.maturity_combo)
            && combo.active != state.edit.maturity
        {
            combo.active = state.edit.maturity;
        }
        set_text(
            &mut texts,
            ui.edit_location_text,
            &location_text(state.edit.location.as_ref(), &translator),
        );
        set_text(&mut texts, ui.enable_glyph, glyph_for(state.edit.enabled));
        set_text(&mut texts, ui.private_glyph, glyph_for(state.edit.private));
    }
}

/// The tick / empty-box glyph for a toggle.
const fn glyph_for(on: bool) -> &'static str {
    if on { CHECKED_GLYPH } else { UNCHECKED_GLYPH }
}

/// An experience's display name, or the reference's "(Untitled)" placeholder
/// when the grid sent an empty one (`ExperienceNameUntitled`).
fn experience_display_name(info: &ExperienceInfo, translator: &Translator) -> String {
    if info.name.is_empty() {
        translator.get("experience-name-untitled")
    } else {
        info.name.clone()
    }
}

/// A home-location SLURL rendered for display, or the "not set" note.
fn location_text(slurl: Option<&url::Url>, translator: &Translator) -> String {
    slurl.map_or_else(
        || translator.get("experience-profile-location-none"),
        url::Url::to_string,
    )
}

/// Write a text node's value, only on a real change.
fn set_text(texts: &mut Query<&mut Text>, entity: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != value
    {
        value.clone_into(&mut text.0);
    }
}

/// Write an edit field's value, only on a real change (so a repaint never moves
/// the caret of a field being typed in).
fn set_field(fields: &mut Query<&mut EditableText>, entity: Entity, value: &str) {
    if let Ok(mut field) = fields.get_mut(entity)
        && field.value() != value
    {
        field.editor_mut().set_text(value);
    }
}

/// Show or hide a node by its `display`, only on a real change.
fn show(nodes: &mut Query<&mut Node>, entity: Entity, shown: bool) {
    let wanted = if shown { Display::Flex } else { Display::None };
    if let Ok(mut node) = nodes.get_mut(entity)
        && node.display != wanted
    {
        node.display = wanted;
    }
}

/// Grey (or un-grey) one action button's label, so a press that would be a
/// no-op reads as inert. The press itself is already a no-op — sending the
/// preference the agent already has changes nothing — so this is presentation,
/// not a gate, which is why it does not reach for
/// [`InteractionDisabled`](bevy::ui::InteractionDisabled) (advisory per
/// observer, so it is no substitute for the action itself being harmless).
fn set_label_enabled(colors: &mut Query<&mut TextColor>, label: Entity, enabled: bool) {
    let wanted = TextColor(if enabled {
        TEXT_COLOR
    } else {
        DISABLED_TEXT_COLOR
    });
    if let Ok(mut color) = colors.get_mut(label)
        && *color != wanted
    {
        *color = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExperienceProfileState, MATURITY_ADULT, MATURITY_GENERAL, MATURITY_MODERATE,
        PROPERTY_DISABLED, PROPERTY_PRIVATE, maturity_from_index, maturity_index, maturity_key,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ExperienceInfo, ExperienceKey, ExperienceProperties, Uuid};

    /// A state holding one record, so the buffer / update tests have something
    /// to edit.
    fn state_with(properties: i32, maturity: i32) -> ExperienceProfileState {
        let id = ExperienceKey::from(Uuid::from_u128(0x51));
        let mut state = ExperienceProfileState::new(id);
        state.info = Some(ExperienceInfo {
            public_id: id,
            name: "Neon Speedway".to_owned(),
            description: "Ride the rails".to_owned(),
            properties: ExperienceProperties(properties),
            maturity,
            extended_metadata: "<llsd><map/></llsd>".to_owned(),
            ..ExperienceInfo::default()
        });
        state.reset_edit_buffer();
        state
    }

    /// Every rating code maps to the combo index the reference's thresholds
    /// give it, and back to the canonical code for that band.
    #[test]
    fn maturity_maps_by_the_reference_thresholds() {
        assert_eq!(maturity_index(0), 0);
        assert_eq!(maturity_index(MATURITY_GENERAL), 0);
        assert_eq!(maturity_index(MATURITY_GENERAL + 1), 1);
        assert_eq!(maturity_index(MATURITY_MODERATE), 1);
        assert_eq!(maturity_index(MATURITY_MODERATE + 1), 2);
        assert_eq!(maturity_index(MATURITY_ADULT), 2);

        assert_eq!(maturity_from_index(0), MATURITY_GENERAL);
        assert_eq!(maturity_from_index(1), MATURITY_MODERATE);
        assert_eq!(maturity_from_index(2), MATURITY_ADULT);
        // An index past the list is the most restrictive reading, not a panic.
        assert_eq!(maturity_from_index(9), MATURITY_ADULT);

        assert_eq!(maturity_key(MATURITY_GENERAL), "experience-rating-general");
        assert_eq!(
            maturity_key(MATURITY_MODERATE),
            "experience-rating-moderate"
        );
        assert_eq!(maturity_key(MATURITY_ADULT), "experience-rating-adult");
    }

    /// The edit buffer reads the two property bits the way the reference's
    /// checkboxes do: Enable is the *inverse* of DISABLED, Private is PRIVATE.
    #[test]
    fn edit_buffer_reads_the_property_bits() {
        let state = state_with(PROPERTY_DISABLED | PROPERTY_PRIVATE, MATURITY_ADULT);
        assert!(!state.edit.enabled);
        assert!(state.edit.private);
        assert_eq!(state.edit.maturity, 2);

        let state = state_with(0, MATURITY_GENERAL);
        assert!(state.edit.enabled);
        assert!(!state.edit.private);
        assert_eq!(state.edit.maturity, 0);
    }

    /// A Save writes the two toggles back into the bitfield without disturbing
    /// any other bit, and carries the opaque extended metadata through
    /// verbatim — the thing this window must not be able to delete.
    #[test]
    fn update_preserves_other_bits_and_the_metadata_blob() -> Result<(), Box<dyn core::error::Error>>
    {
        // A bit this window knows nothing about, alongside DISABLED.
        let foreign = 0x4000;
        let mut state = state_with(foreign | PROPERTY_DISABLED, MATURITY_GENERAL);
        state.edit.enabled = true;
        state.edit.private = true;
        state.edit.maturity = 2;

        let update = state
            .update_from("Renamed".to_owned(), "New words".to_owned())
            .ok_or("a state holding a record must produce an update")?;
        assert_eq!(update.name, "Renamed");
        assert_eq!(update.description, "New words");
        assert_eq!(update.maturity, MATURITY_ADULT);
        assert_eq!(update.properties & PROPERTY_DISABLED, 0);
        assert_eq!(update.properties & PROPERTY_PRIVATE, PROPERTY_PRIVATE);
        assert_eq!(update.properties & foreign, foreign);
        assert_eq!(update.extended_metadata, "<llsd><map/></llsd>");
        Ok(())
    }

    /// With no record yet there is nothing to update *from*, so a Save is
    /// refused rather than sending a record invented from the empty fields.
    #[test]
    fn update_needs_a_record_first() {
        let state = ExperienceProfileState::new(ExperienceKey::from(Uuid::from_u128(0x52)));
        assert!(
            state
                .update_from("Name".to_owned(), String::new())
                .is_none()
        );
    }

    /// A privileged experience offers no permission buttons: the agent cannot
    /// decline it, and the reference shows a note instead.
    #[test]
    fn privileged_experiences_offer_no_permission_buttons() {
        let state = state_with(sl_types::experience::PROPERTY_PRIVILEGED, MATURITY_GENERAL);
        assert!(!state.permission_applies());

        let state = state_with(0, MATURITY_GENERAL);
        assert!(state.permission_applies());
    }
}
