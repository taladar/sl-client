//! The **Strings** floater (`rlv_strings`): the canned texts RLVa emits, and
//! the user's rewrites of them.
//!
//! Eight strings are customisable — the ones a *remote* party reads, plus the
//! two that stand in for a blocked IM. They are the texts where a person's own
//! wording matters: the line the other side sees when your viewer refuses their
//! IM is, socially, yours.
//!
//! The floater is the reference's: pick a string from the list, read what it is
//! for, edit it, or put the default back.
//!
//! # Two deliberate divergences from the reference
//!
//! - **The edits take effect at once.** The reference writes them to an
//!   `rlv_strings.xml` of its own, loads that file only at startup, and so has
//!   to tell the user to relog. Here they are ordinary settings
//!   ([`sl_viewer_world_api::rlv::RLV_STRINGS`]) read at the point of use, so
//!   there is nothing to relog for.
//! - **The rest of `rlva_strings.xml` is not listed.** The reference's file
//!   also holds the `hidden_*` placeholders and the `blocked_*` refusal
//!   notices; it marks them non-customisable and does not list them either.
//!   Those are viewer-voice text and live in the translated notification
//!   catalogue, which is where a rewording of them belongs.
//!
//! Reference (Firestorm, read-only): `rlvfloaters.cpp` (`RlvFloaterStrings`),
//! `floater_rlv_strings.xml`, `rlva_strings.xml`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_settings::{Scope, SettingValue};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_combo::{ComboChanged, ComboSpec, spawn_combo};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::rlv::{RLV_STRINGS, RlvStringDef, rlv_string};

use crate::style::{ACTION_BACKGROUND, DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR};

/// The floater's stable id.
pub const STRINGS_FLOATER_ID: &str = "rlv-strings";

/// The value editor's height, in visible text lines. Three fits every default
/// in the table with room to grow one.
const VALUE_LINES: f32 = 3.0;

// --- Pure view model ------------------------------------------------------

/// The string the list is showing at `index`, or `None` past the end.
#[must_use]
pub fn string_at(index: usize) -> Option<&'static RlvStringDef> {
    RLV_STRINGS.get(index)
}

/// The labels the picker lists, in the table's own (key) order.
#[must_use]
pub fn picker_labels() -> Vec<String> {
    RLV_STRINGS
        .iter()
        .map(|entry| entry.label.to_owned())
        .collect()
}

/// Whether `text` is what the setting store should hold for `entry` — an edit
/// back to the reference's own wording is a *reset*, not an override, so the
/// setting file does not accumulate lines that say nothing.
#[must_use]
pub fn is_default_text(entry: &RlvStringDef, text: &str) -> bool {
    text == entry.default
}

// --- Resources ------------------------------------------------------------

/// The floater's retained entities.
#[derive(Resource, Debug)]
struct StringsUi {
    /// The picker combo (carries `ComboSelection`).
    picker: Entity,
    /// The description line under the picker.
    description: Entity,
    /// The value editor's [`EditableText`] entity.
    value_field: Entity,
}

/// Which string the floater is showing, and the text last written into the
/// editor for it.
///
/// The second half is what stops the two directions fighting: the editor is
/// authored by the user *and* rewritten when the selection changes, so the
/// mirror only writes a setting when the field differs from what it was last
/// filled with.
#[derive(Resource, Debug, Default)]
struct StringsSelection {
    /// The index into [`RLV_STRINGS`].
    index: usize,
    /// The text the editor was last filled with.
    shown: String,
    /// Whether the editor has been filled at least once.
    filled: bool,
}

// --- Plugin ---------------------------------------------------------------

/// Registers the Strings floater, its selection state and its two-way binding.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvStringsPlugin;

impl Plugin for RlvStringsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StringsSelection>()
            .add_systems(
                Startup,
                spawn_strings_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    follow_strings_picker,
                    fill_strings_editor,
                    mirror_strings_editor,
                )
                    .chain()
                    .run_if(floater_shown(STRINGS_FLOATER_ID)),
            );
    }
}

// --- Floater --------------------------------------------------------------

/// The Strings floater's [`FloaterSpec`].
#[must_use]
pub fn rlv_strings_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: STRINGS_FLOATER_ID,
        title: "RLVa Strings".to_owned(),
        position: Vec2::new(340.0, 170.0),
        default_size: Some(Vec2::new(520.0, 300.0)),
        min_size: Some(Vec2::new(380.0, 220.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: chrome only.
fn spawn_strings_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, rlv_strings_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("rlv-strings-title"));
    let builder = commands.register_system(build_strings_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the picker, the description, the editor and the
/// Restore default button.
fn build_strings_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            Name::new("rlv-strings-content"),
            ChildOf(handle.content),
        ))
        .id();

    let labels = picker_labels();
    let picker = spawn_combo(
        &mut commands,
        content,
        &ComboSpec {
            element: "rlv-strings-picker",
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT_SIZE,
            // The labels are the reference's own English wording, carried
            // verbatim from `rlva_strings.xml`; they are data, not UI chrome,
            // and translating them would mean re-translating the reference's
            // string table rather than this window.
            translate_labels: false,
        },
    );

    let description = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(DIM_LABEL_COLOR),
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
            Name::new("rlv-strings-description"),
            ChildOf(content),
        ))
        .id();

    let value_field = spawn_text_input(
        &mut commands,
        content,
        &TextInputSpec {
            tab_index: 1,
            font_size: FONT_SIZE,
            visible_lines: VALUE_LINES,
            ..TextInputSpec::new("rlv-strings-value", TextInputKind::Multiline)
        },
    );

    let actions = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                ..row(Val::Px(6.0))
            },
            Name::new("rlv-strings-actions"),
            ChildOf(content),
        ))
        .id();
    spawn_restore_button(&mut commands, actions);

    commands.insert_resource(StringsUi {
        picker,
        description,
        value_field,
    });
}

/// The Restore-default button: drop the override so the reference's own wording
/// comes back, and let [`fill_strings_editor`] put it in the editor.
fn spawn_restore_button(commands: &mut Commands, parent: Entity) {
    commands
        .spawn((
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(ACTION_BACKGROUND),
            TabIndex(2),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Name::new("rlv-strings-restore"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new("rlv-strings-restore"),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>,
                  mut selection: ResMut<StringsSelection>,
                  mut settings: ResMut<ViewerSettings>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                let Some(entry) = string_at(selection.index) else {
                    return;
                };
                settings.reset(Scope::Global, entry.key);
                settings.save_async();
                // Make the next fill unconditional: the stored value changed
                // under the editor, which nothing else would notice.
                selection.filled = false;
            },
        );
}

// --- View systems ---------------------------------------------------------

/// Follow the picker: a user pick selects that string and re-fills the editor.
fn follow_strings_picker(
    mut picks: MessageReader<ComboChanged>,
    ui: Option<Res<StringsUi>>,
    mut selection: ResMut<StringsSelection>,
) {
    let Some(ui) = ui else {
        return;
    };
    for pick in picks.read() {
        if pick.combo != ui.picker {
            continue;
        }
        if selection.index != pick.active {
            selection.index = pick.active;
            selection.filled = false;
        }
    }
}

/// Fill the description and the editor for the selected string, whenever the
/// selection moved (or a restore invalidated what is shown).
fn fill_strings_editor(
    ui: Option<Res<StringsUi>>,
    settings: Option<Res<ViewerSettings>>,
    mut selection: ResMut<StringsSelection>,
    mut fields: Query<&mut EditableText>,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    if selection.filled {
        return;
    }
    let Some(entry) = string_at(selection.index) else {
        return;
    };
    let value = rlv_string(settings.as_deref(), entry.key);
    if let Ok(mut field) = fields.get_mut(ui.value_field) {
        field.editor_mut().set_text(&value);
    }
    if let Ok(mut text) = texts.get_mut(ui.description)
        && text.0 != entry.description
    {
        entry.description.clone_into(&mut text.0);
    }
    selection.shown = value;
    selection.filled = true;
}

/// Mirror an edit back into the settings store: an override where the text
/// differs from the reference's, and a reset where the user typed the default
/// back.
fn mirror_strings_editor(
    ui: Option<Res<StringsUi>>,
    mut selection: ResMut<StringsSelection>,
    fields: Query<&EditableText>,
    settings: Option<ResMut<ViewerSettings>>,
) {
    let (Some(ui), Some(mut settings)) = (ui, settings) else {
        return;
    };
    if !selection.filled {
        return;
    }
    let Ok(field) = fields.get(ui.value_field) else {
        return;
    };
    let typed = field.value().to_string();
    if typed == selection.shown {
        return;
    }
    let Some(entry) = string_at(selection.index) else {
        return;
    };
    if is_default_text(entry, &typed) {
        settings.reset(Scope::Global, entry.key);
    } else {
        settings.set(
            Scope::Global,
            entry.key,
            SettingValue::String(typed.clone()),
        );
    }
    settings.save_async();
    selection.shown = typed;
}

#[cfg(test)]
mod tests {
    use super::{is_default_text, picker_labels, string_at};

    /// A `Box<dyn Error>` alias, so a test can use `?`.
    type TestError = Box<dyn core::error::Error>;
    use pretty_assertions::assert_eq;
    use sl_viewer_world_api::rlv::{RLV_STRINGS, RLV_STRINGS_SECTION};

    /// The picker lists one label per customisable string, in table order.
    #[test]
    fn the_picker_lists_every_customisable_string() {
        let labels = picker_labels();
        assert_eq!(labels.len(), RLV_STRINGS.len());
        assert_eq!(
            labels.first().map(String::as_str),
            RLV_STRINGS.first().map(|entry| entry.label)
        );
    }

    /// The index the picker reports maps back to the entry the editor writes,
    /// and an index past the end selects nothing rather than wrapping.
    #[test]
    fn an_index_maps_to_its_entry() {
        assert_eq!(
            string_at(0).map(|entry| entry.key),
            RLV_STRINGS.first().map(|entry| entry.key)
        );
        assert!(string_at(RLV_STRINGS.len()).is_none());
    }

    /// Typing the reference's own wording back is a reset, not an override —
    /// which is what keeps the settings file free of lines that say nothing.
    #[test]
    fn typing_the_default_back_is_a_reset() -> Result<(), TestError> {
        let entry = string_at(0).ok_or("the string table is empty")?;
        assert!(is_default_text(entry, entry.default));
        assert!(!is_default_text(entry, "something else"));
        Ok(())
    }

    /// Every entry carries the three texts the floater draws, so no row can
    /// render blank.
    #[test]
    fn every_entry_is_fully_described() {
        for entry in RLV_STRINGS {
            assert!(!entry.key.is_empty());
            assert!(!entry.default.is_empty(), "{} has no default", entry.key);
            assert!(!entry.label.is_empty(), "{} has no label", entry.key);
            assert!(
                !entry.description.is_empty(),
                "{} has no description",
                entry.key
            );
        }
    }

    /// The strings are registered under their own section, so the settings file
    /// keeps them together and the debug editor groups them.
    #[test]
    fn the_strings_live_in_their_own_section() {
        assert_eq!(RLV_STRINGS_SECTION, ["rlv", "strings"]);
    }
}
