//! The scaffold every asset editor in this crate is built on: the chrome its
//! windows are made of, and the machinery that keeps unsaved work from
//! disappearing without being asked about.
//!
//! # Why there is a scaffold at all
//!
//! There were three editors here and three copies of the same window: the same
//! status line, the same read-only note, the same multi-line body field, the
//! same Save button, each spawned by a helper that was byte-identical to its
//! siblings apart from one string. Copies drift, and these had: the third copy
//! reported a save that had not happened and claimed another editor's result,
//! because the correlation the first two grew was never copied over. So the
//! parts that have nothing to do with *which* asset is being edited live here
//! once, and each editor module is left holding only what is genuinely its own —
//! how its asset decodes, what its body looks like, and which capability its
//! save goes out over.
//!
//! # Unsaved work
//!
//! A close is otherwise irreversible: a keyed editor window is **despawned** by
//! the floater manager's close pass, so by the time anything could ask "save
//! first?" the text is gone. An `UnsavedWork` component arms the manager's
//! [`FloaterCloseGuard`] while a window is holding edits, turning its close into
//! a [`FloaterCloseRequested`] this module answers with the reference's
//! `SaveChanges` prompt (Save / Don't Save / Cancel — the reference
//! `LLPreviewNotecard::handleSaveChangesDialog`). **Don't Save** answers with
//! the unrefusable [`FloaterOp::CloseNow`], so the guard never has to be taken
//! down to let a deliberate discard through.
//!
//! **Save** and the Save *button* go the same way round: both write
//! [`SaveEditorWindow`], so the confirmation cannot save differently from the
//! button, and a window whose save is still in flight closes itself only once
//! the save has actually landed.

use crate::skin_palette::SkinPalette;
use bevy::prelude::*;
use bevy::text::EditableText;

use crate::floater::{
    Floater, FloaterCloseGuard, FloaterCloseRequested, FloaterCommand, FloaterOp, FloaterSystems,
    host_floater,
};
use crate::i18n::Translated;
use crate::notifications::{NotificationResponse, ShowNotification};
use crate::ui_font::UiFont;
use crate::ui_spawn::{self, ButtonSpec, UiLabel};
use sl_viewer_ui_core::skin::text_role;

/// The editors' text font size, in logical pixels.
pub(crate) const FONT_SIZE: f32 = 14.0;

/// A general-purpose light label colour.
pub(crate) const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// A dimmer colour for secondary text (a read-only note, a status line).
pub(crate) const DIM_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// A red-tinted colour for a failure (a refused save, a compile error).
pub(crate) const ERROR_COLOR: Color = Color::srgb(0.92, 0.55, 0.50);

/// A chrome control's border colour.
pub(crate) const CONTROL_BORDER: Color = Color::srgb(0.32, 0.36, 0.44);

/// A chrome control's background.
pub(crate) const CONTROL_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// The catalogue template asked before unsaved work is thrown away — the
/// reference's own `SaveChanges` (Save / Don't Save / Cancel).
const SAVE_CHANGES: &str = "SaveChanges";

// ---------------------------------------------------------------------------
// Chrome.
// ---------------------------------------------------------------------------

/// Despawn every child of `parent`.
pub(crate) fn tear_down(commands: &mut Commands, children: &Query<&Children>, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Spawn a fresh status line under `parent`, driven by a Fluent key.
pub(crate) fn spawn_status(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Text::default(),
            Translated::new(key),
            UiFont::Sans.at(FONT_SIZE),
            text_role(color),
            ChildOf(parent),
        ))
        .id()
}

/// Repoint an existing status node at a new Fluent key and colour.
pub(crate) fn set_status(commands: &mut Commands, status: Entity, key: &'static str, color: Color) {
    commands
        .entity(status)
        .insert((Translated::new(key), text_role(color)));
}

/// Spawn the note shown above a no-modify asset's body.
pub(crate) fn spawn_note(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    font_size: f32,
) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(font_size),
        text_role(DIM_COLOR),
        ChildOf(parent),
    ));
}

/// Spawn the editable multi-line body field, returning its entity.
///
/// `element` names the field for the skin and the layout sweeps;
/// `visible_lines` is the field's height, which is an *intrinsic* control size
/// and therefore what sizes the window rather than the other way round (see
/// `ui_text_input`'s `fill`).
pub(crate) fn spawn_body_field(
    commands: &mut Commands,
    parent: Entity,
    text: &str,
    element: &'static str,
    visible_lines: f32,
    font_size: f32,
) -> Entity {
    crate::ui_text_input::spawn_text_input(
        commands,
        parent,
        &crate::ui_text_input::TextInputSpec {
            initial: text.to_owned(),
            font_size,
            visible_lines,
            tab_index: 1,
            // An editor's body is the part of its window worth making bigger,
            // so it takes the room a resized floater gives its content slot.
            fill: true,
            ..crate::ui_text_input::TextInputSpec::new(
                element,
                crate::ui_text_input::TextInputKind::Multiline,
            )
        },
    )
}

/// Spawn an editor's Save button: a chrome button that writes
/// [`SaveEditorWindow`] for the floater it sits in.
///
/// The press is resolved to a window here rather than in each editor, and the
/// save itself is a system rather than an observer closure, so the Save button
/// and the "Save" answer to the unsaved-work confirmation take the *same* path —
/// they cannot diverge into saving two different things.
pub(crate) fn spawn_save_button(
    commands: &mut Commands,
    parent: Entity,
    element: &'static str,
    label_key: &'static str,
    font_size: f32,
) -> Entity {
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::key(label_key), element)
            .tab_index(2)
            .colors(CONTROL_BACKGROUND, CONTROL_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(font_size),
    )
    .button;
    commands.entity(button).observe(on_save_pressed);
    button
}

/// Turn a Save press into a [`SaveEditorWindow`] naming the window it came from.
fn on_save_pressed(
    press: On<Pointer<Press>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut saves: MessageWriter<SaveEditorWindow>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // Save *this* window's asset: the button is inside it.
    if let Some(window) = host_floater(press.entity, &parents, &floaters) {
        saves.write(SaveEditorWindow { window });
    }
}

// ---------------------------------------------------------------------------
// Unsaved work.
// ---------------------------------------------------------------------------

/// Save one editor window's asset, whatever kind it holds.
///
/// Written by an editor's Save button and by a "Save" answer to the
/// unsaved-work confirmation; read by whichever editor owns the named window. A
/// window this crate does not own is simply not matched by any of them.
#[derive(Message, Debug, Clone, Copy)]
pub struct SaveEditorWindow {
    /// The editor window (the floater root) to save.
    pub window: Entity,
}

/// The unsaved work an editor window is holding.
///
/// A window carrying this is guarded: while [`dirty`](Self::dirty) is set, the
/// floater manager turns a close into a question instead of carrying it out.
/// The editor that owns the window is what decides when it is dirty — for a text
/// editor that is [`track_edited_text`] reading the field, for the appearance
/// editor it is its own edit handlers, because a slider drag is not a buffer.
#[derive(Component, Debug, Default)]
pub(crate) struct UnsavedWork {
    /// Whether the window is holding edits that a close would throw away.
    pub dirty: bool,
    /// Set while a "Save" answer is waiting for that save to land: the window
    /// closes itself the moment the work stops being unsaved, and stays open
    /// (showing the failure) if the save is refused.
    pub close_when_saved: bool,
}

/// The live text buffer an editor window's dirtiness is measured against.
///
/// Two strings and an entity rather than a flag, because the resident can type a
/// character and take it back again, and a flag would then insist there is
/// something to lose when there is not.
#[derive(Component, Debug)]
pub(crate) struct EditedText {
    /// The multi-line field the window edits in.
    pub field: Entity,
    /// The text as loaded, or as of the last save that landed.
    pub saved: String,
}

/// The window whose close is waiting on the resident's answer.
///
/// One slot: the confirmation is modal, the response carries the template name
/// rather than the raise's own id, and this is what tells our `SaveChanges` from
/// anybody else's. A second close arriving while one is pending is dropped — the
/// question on screen is about a different window, and answering it must not
/// close this one.
#[derive(Resource, Debug, Default)]
pub(crate) struct PendingDiscard(Option<Entity>);

/// The scaffold's systems: the close guard, its confirmation, and the text
/// dirtiness the guard is armed from.
#[derive(Debug)]
pub struct AssetEditorScaffoldPlugin;

impl Plugin for AssetEditorScaffoldPlugin {
    /// Register the save channel, the pending-confirmation slot and the guard
    /// systems.
    fn build(&self, app: &mut App) {
        app.add_message::<SaveEditorWindow>()
            .init_resource::<PendingDiscard>()
            .add_systems(
                Update,
                (
                    track_edited_text,
                    arm_close_guards,
                    // Before the manager carries a close out, so the guard it
                    // reads is this frame's.
                    close_saved_windows.before(FloaterSystems::Commands),
                    // After it, so the request this frame's close raised is
                    // answered this frame rather than the next.
                    ask_before_discarding.after(FloaterSystems::Commands),
                    answer_discard,
                )
                    .chain(),
            );
    }
}

/// Measure a text editor window's dirtiness: the buffer differing from what was
/// loaded or last saved *is* the unsaved work.
fn track_edited_text(
    mut windows: Query<(&EditedText, &mut UnsavedWork)>,
    fields: Query<&EditableText>,
) {
    for (edited, mut unsaved) in &mut windows {
        let Ok(field) = fields.get(edited.field) else {
            continue;
        };
        let dirty = field.value() != edited.saved.as_str();
        if unsaved.dirty != dirty {
            unsaved.dirty = dirty;
        }
    }
}

/// Arm (or disarm) each guarded window's [`FloaterCloseGuard`] from whether it
/// is holding unsaved work.
///
/// Only on a change: a guard rewritten every frame would mark the component
/// changed every frame for every open editor, and the manager reads it on a
/// close, not on a schedule.
fn arm_close_guards(
    windows: Query<(Entity, &UnsavedWork), Changed<UnsavedWork>>,
    mut commands: Commands,
) {
    for (window, unsaved) in &windows {
        commands
            .entity(window)
            .insert(FloaterCloseGuard::new(unsaved.dirty));
    }
}

/// Ask before throwing an editor window's unsaved work away.
fn ask_before_discarding(
    mut requests: MessageReader<FloaterCloseRequested>,
    windows: Query<&UnsavedWork>,
    mut pending: ResMut<PendingDiscard>,
    mut notify: MessageWriter<ShowNotification>,
) {
    for request in requests.read() {
        if !windows.get(request.floater).is_ok_and(|work| work.dirty) {
            continue;
        }
        if pending.0.is_some() {
            // Somebody else's question is on screen. Dropping this one leaves
            // the window open and its work intact, which is the safe half of
            // the two answers; asking twice at once would let one answer close
            // the other's window.
            continue;
        }
        pending.0 = Some(request.floater);
        notify.write(ShowNotification::new(SAVE_CHANGES));
    }
}

/// Carry out the answer: save and then close, close and lose the edits, or stay.
fn answer_discard(
    mut responses: MessageReader<NotificationResponse>,
    mut pending: ResMut<PendingDiscard>,
    mut windows: Query<&mut UnsavedWork>,
    mut saves: MessageWriter<SaveEditorWindow>,
    mut floater_commands: MessageWriter<FloaterCommand>,
) {
    for response in responses.read() {
        if response.template != SAVE_CHANGES {
            continue;
        }
        let Some(window) = pending.0.take() else {
            continue;
        };
        let Ok(mut work) = windows.get_mut(window) else {
            continue;
        };
        match response.button {
            // Save: the close waits for the save to land, so a refused save
            // leaves the window up with its failure on screen rather than
            // closing over work that was never stored.
            Some("Yes") => {
                work.close_when_saved = true;
                saves.write(SaveEditorWindow { window });
            }
            // Don't Save: the work is given up deliberately, so the close goes
            // out **unrefusable**. Taking the guard down instead would not
            // work — the work has not stopped being unsaved, and the tracker
            // would re-arm the guard in time to ask the same question again.
            Some("No") => {
                work.close_when_saved = false;
                floater_commands.write(FloaterCommand {
                    floater: window,
                    op: FloaterOp::CloseNow,
                });
            }
            // Cancel, or a dismissal with no choice: nothing happens, which is
            // exactly what "cancel" means for a close.
            _other => {}
        }
    }
}

/// Close a window that was waiting on its save, now that the save has landed.
///
/// The editor clears `dirty` when a save succeeds — for a text editor by moving
/// [`EditedText::saved`] onto what was written — so "no longer dirty" is the
/// signal, and a refused save (which changes neither) simply never fires it.
///
/// The close is [`FloaterOp::CloseNow`] because the guard is disarmed by
/// [`arm_close_guards`] a frame behind the dirty flag, and a refusable close in
/// that window would ask about work that has just been saved.
fn close_saved_windows(
    mut windows: Query<(Entity, &mut UnsavedWork)>,
    mut floater_commands: MessageWriter<FloaterCommand>,
) {
    for (window, mut work) in &mut windows {
        if !work.close_when_saved || work.dirty {
            continue;
        }
        work.close_when_saved = false;
        floater_commands.write(FloaterCommand {
            floater: window,
            op: FloaterOp::CloseNow,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{AssetEditorScaffoldPlugin, PendingDiscard, SaveEditorWindow, UnsavedWork};
    use crate::floater::{FloaterCloseGuard, FloaterCommand, FloaterOp, FloaterPlugin};
    use crate::notifications::{
        NotificationId, NotificationManager, NotificationResponse, ShowNotification,
    };
    use crate::ui::{UiPanelShown, UiRoot};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` rather than the disallowed `unwrap` /
    /// `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// An app with the floater manager and the scaffold, plus one floater
    /// standing in for an editor window holding unsaved work.
    fn guarded_app() -> (App, Entity) {
        let mut app = App::new();
        app.add_message::<ShowNotification>()
            .add_message::<NotificationResponse>()
            .init_resource::<UiScale>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins((FloaterPlugin, AssetEditorScaffoldPlugin));
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.update();
        let window = {
            let mut commands = app.world_mut().commands();
            crate::floater::spawn_floater(
                &mut commands,
                root,
                crate::edit_notecard::notecard_editor_floater_spec(),
            )
            .root
        };
        app.world_mut().flush();
        // A floater is spawned hidden and opened by its feature; this one is
        // open, because a close is only interesting on a window that is up.
        app.world_mut().entity_mut(window).insert((
            UiPanelShown(true),
            UnsavedWork {
                dirty: true,
                close_when_saved: false,
            },
        ));
        app.update();
        (app, window)
    }

    /// Ask the manager to close the window, the way its close button does.
    fn close(app: &mut App, window: Entity) {
        app.world_mut()
            .resource_mut::<Messages<FloaterCommand>>()
            .write(FloaterCommand {
                floater: window,
                op: FloaterOp::Close,
            });
        app.update();
    }

    /// Any notification id — the scaffold routes on the template name, exactly
    /// as the settings editor's own confirmation does, because the response
    /// carries the name and the raise does not hand back its id.
    fn some_id() -> NotificationId {
        let mut manager = NotificationManager::default();
        manager.allocate_id()
    }

    /// Answer the confirmation with one of its buttons.
    fn answer(app: &mut App, button: &'static str) {
        app.world_mut()
            .resource_mut::<Messages<NotificationResponse>>()
            .write(NotificationResponse {
                id: some_id(),
                template: "SaveChanges",
                button: Some(button),
                ignored: false,
                input: None,
            });
        app.update();
    }

    /// Whether the window is still open.
    ///
    /// The manager hides a singleton and despawns a keyed instance, so the flag
    /// it keeps in step is the one thing that answers for both. The real editor
    /// windows are keyed, which is what makes a close that goes through ahead of
    /// its question unrecoverable.
    fn open_window(app: &App, window: Entity) -> bool {
        app.world()
            .get::<UiPanelShown>(window)
            .is_some_and(|shown| shown.0)
    }

    /// How many saves were asked for this frame.
    fn saves(app: &App) -> usize {
        app.world()
            .resource::<Messages<SaveEditorWindow>>()
            .iter_current_update_messages()
            .count()
    }

    /// **The confirmation this scaffold routes on is the one the catalogue
    /// ships**, down to the button names.
    ///
    /// A raise for a template that is not in the catalogue is dropped, and a
    /// button name that is not in its form never matches — either way the
    /// question would never be asked, or never answered, and the close it was
    /// guarding would quietly discard the work again.
    #[test]
    fn the_save_prompt_is_catalogued_with_the_buttons_we_route_on() -> Result<(), TestError> {
        let template = crate::notifications::template("SaveChanges")
            .ok_or("the reference's SaveChanges is in the catalogue")?;
        for name in ["Yes", "No", "Cancel"] {
            assert!(
                template.form.iter().any(|button| button.name == name),
                "no `{name}` arm to route on"
            );
        }
        Ok(())
    }

    /// A dirty window's close is a question, not a close: the window survives
    /// and the `SaveChanges` prompt goes up.
    #[test]
    fn closing_dirty_work_asks_first() -> Result<(), TestError> {
        let (mut app, window) = guarded_app();
        assert_eq!(
            app.world()
                .get::<FloaterCloseGuard>(window)
                .map(|guard| guard.armed),
            Some(true),
            "unsaved work must arm the manager's guard"
        );

        close(&mut app, window);
        assert!(
            open_window(&app, window),
            "the close discarded the unsaved work"
        );
        let raised: Vec<&'static str> = app
            .world()
            .resource::<Messages<ShowNotification>>()
            .iter_current_update_messages()
            .map(|show| show.template)
            .collect();
        assert_eq!(raised, vec!["SaveChanges"]);
        assert_eq!(
            app.world().resource::<PendingDiscard>().0,
            Some(window),
            "the window was not held for the answer"
        );
        Ok(())
    }

    /// **Don't Save** gives the work up and the window closes — *while the work
    /// is still unsaved*, which is the whole difficulty.
    ///
    /// The answer cannot be spelled "the work is no longer unsaved": the buffer
    /// has not changed, the tracker measures it as unsaved again on the next
    /// frame, and the guard it re-arms would turn the resident's own answer into
    /// the same question again, for ever. So the close must go through with the
    /// work still dirty, and no second prompt behind it.
    #[test]
    fn answering_dont_save_closes_the_window() -> Result<(), TestError> {
        let (mut app, window) = guarded_app();
        close(&mut app, window);
        answer(&mut app, "No");
        assert_eq!(
            app.world()
                .get::<UnsavedWork>(window)
                .map(|work| work.dirty),
            Some(true),
            "the answer must not pretend the work was saved"
        );

        // One more frame for the re-issued close to reach the manager.
        app.update();
        assert!(
            !open_window(&app, window),
            "Don't Save must close the window"
        );
        let asked_again: usize = app
            .world()
            .resource::<Messages<ShowNotification>>()
            .iter_current_update_messages()
            .filter(|show| show.template == "SaveChanges")
            .count();
        assert_eq!(
            asked_again, 0,
            "the answer was turned back into the question"
        );
        Ok(())
    }

    /// **Cancel** leaves everything exactly as it was — the window open, the
    /// work unsaved, and nothing saved behind the resident's back.
    #[test]
    fn answering_cancel_changes_nothing() -> Result<(), TestError> {
        let (mut app, window) = guarded_app();
        close(&mut app, window);
        answer(&mut app, "Cancel");
        app.update();
        assert!(
            open_window(&app, window),
            "Cancel must not close the window"
        );
        assert_eq!(saves(&app), 0, "Cancel must not save");
        assert_eq!(
            app.world()
                .get::<UnsavedWork>(window)
                .map(|work| work.dirty),
            Some(true),
            "Cancel must leave the work unsaved"
        );
        Ok(())
    }

    /// **Save** asks for the save and holds the close until it lands — a refused
    /// save (the work stays dirty) never closes the window, and the success that
    /// clears the dirt does.
    #[test]
    fn answering_save_closes_only_once_the_save_lands() -> Result<(), TestError> {
        let (mut app, window) = guarded_app();
        close(&mut app, window);
        answer(&mut app, "Yes");
        assert_eq!(saves(&app), 1, "Save must ask the editor to save");
        assert!(
            open_window(&app, window),
            "the window closed before its save landed"
        );

        // A frame in which the save has not come back: still open.
        app.update();
        assert!(
            open_window(&app, window),
            "the window closed on a pending save"
        );

        // The editor clears the dirt when the save lands.
        if let Some(mut work) = app.world_mut().get_mut::<UnsavedWork>(window) {
            work.dirty = false;
        }
        app.update();
        app.update();
        assert!(
            !open_window(&app, window),
            "a landed save must close the window"
        );
        Ok(())
    }
}
