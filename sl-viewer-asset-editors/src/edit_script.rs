//! The **LSL script editor** floater (`viewer-lsl-editor-save-compile`): open a
//! script from agent inventory or a prim's contents, read it, edit its source
//! when permitted, and **save it back — which is the compile**.
//!
//! A script asset is plain UTF-8 source; unlike a notecard there is no embedded
//! container to decode. What makes a script editor more than "a text box" is the
//! grid round-trip: Second Life has *no* compile-without-save, so a Save uploads
//! the source over the `UpdateScriptAgent` / `UpdateScriptTask` capability and the
//! **simulator compiles it**, returning the result (a `compiled` flag plus any
//! [`ScriptCompileError`]s) as [`SlSessionEvent::ScriptUploaded`]. This module
//! surfaces that outcome as a status line and a diagnostics list.
//!
//! # What this surface does today, and what waits on the rich-text widget
//!
//! The reference viewer's script editor is **syntax-highlighted** with a gutter,
//! line numbers, brace matching and a clickable error list that jumps the caret
//! to the offending line. All of that needs the per-range-coloured, undo-capable
//! text widget tracked by `viewer-lsl-editor-widget` (a `parley::PlainEditor`
//! fork), which is not yet built — Bevy 0.19's editable text takes **one style
//! for the whole buffer**. Until it lands, this editor:
//!
//! - edits the source in the reusable multi-line field ([`crate::ui_text_input`],
//!   a stock `EditableText`) — no colour, no gutter, no go-to-line yet;
//! - renders the compile diagnostics as a **listed** error report (each entry's
//!   line / column / message) below the body rather than as caret jumps;
//! - saves back to **agent** inventory over `UpdateScriptAgent` or, for a script
//!   opened from a prim's contents, to that object's **task** inventory over
//!   `UpdateScriptTask` — one [`Command::UploadScript`] whose [`ScriptSource`]
//!   picks the capability.
//!
//! Deferred to their own tasks: syntax colour, brace match, folding and the
//! states/events outline (`viewer-lsl-editor-highlight`, which drives the
//! widget's per-range colour from the LSL lexer); a monospace editing font,
//! line-number gutter and clickable go-to-line (`viewer-lsl-editor-widget`);
//! and the Firestorm preprocessor (`viewer-lsl-editor-save-compile` keeps it
//! out of v1).
//!
//! # Read-only when you cannot modify
//!
//! Editability is gated on the item's owner mask carrying `MODIFY` (the
//! reference's `LLPreviewLSL::canModify`). For a script inside a prim it is the
//! two-level rule — the object's modify **and** the item's own modify bit. A
//! no-modify script opens as a read-only text block with a note and no Save
//! button, so its source is never presented as editable when a save would be
//! refused.
//!
//! # Not silently starting or stopping a running script
//!
//! A task script carries a run state. Saving it *is* a recompile, which resets
//! the script; the upload must carry `is_script_running` so the save does not
//! silently start or stop it. This editor queries the current run state
//! (`RequestScriptRunning`) on open and shows a **Running** checkbox reflecting
//! it, whose value the Save carries through — the reference's Running toggle.
//!
//! # One window per script
//!
//! A script editor is a **keyed floater** ([`FloaterKey`]): opening a second
//! script opens a second window rather than re-pointing the first — the
//! reference happily has several open at once (`LLPreviewLSL` per item id) —
//! and each window's **unsaved source**, run state and compile diagnostics are
//! its own. A task script is keyed by its object *and* item, because two rezzed
//! copies of one object carry the same item ids. Re-opening a script already up
//! only raises its window: a re-fetch would replace what the resident has typed
//! with the grid's copy.
//!
//! Reference (Firestorm, read-only): `llpreviewscript`, `llscripteditor`,
//! `llfloaterscriptdebug`.

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    AssetKey, AssetType, Command, InventoryKey, ObjectKey, ScriptCompileError, ScriptTarget,
    SlCommand, SlEvent, SlSessionEvent, Uuid,
};

use crate::floater::{
    Floater, FloaterCaps, FloaterHandle, FloaterKey, FloaterSpec, FloaterSystems, KeyedFloaterOpen,
    KeyedFloaters, host_floater,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::ui::{column, row};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;
use crate::world_api::{OpenScript, ScriptSource};

/// The editor's text font size, in logical pixels.
const FONT_SIZE: f32 = 14.0;

/// A general-purpose light label colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A dimmer colour for secondary text (the read-only note, warnings).
const DIM_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// A green-tinted colour for a checked Running box.
const CHECK_COLOR: Color = Color::srgb(0.55, 0.85, 0.60);

/// A red-tinted colour for a failed save / compile error.
const ERROR_COLOR: Color = Color::srgb(0.92, 0.55, 0.50);

/// The check glyph for a ticked toggle (`☑`).
const CHECKED_GLYPH: &str = "\u{2611}";

/// The empty-box glyph for an unticked toggle (`☐`).
const UNCHECKED_GLYPH: &str = "\u{2610}";

/// The body field's height, in visible text lines. The window is sized by this
/// (it is content-driven), not the other way round — a field's height is its
/// intrinsic control size and cannot be flexed; see `ui_text_input`'s `fill`.
const BODY_VISIBLE_LINES: f32 = 22.0;

/// The tallest the compile-diagnostics list grows before it scrolls, in logical
/// pixels — about five rows, enough to read the first errors without the list
/// taking the window away from the script it is complaining about.
const DIAGNOSTICS_HEIGHT: f32 = 96.0;

/// The read-only body block's viewport height, in logical pixels.
const READONLY_BODY_HEIGHT: f32 = 380.0;

/// The read-only body block's width bound, in logical pixels.
const READONLY_BODY_WIDTH: f32 = 520.0;

// ---------------------------------------------------------------------------
// Messages.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Plugin, resources.
// ---------------------------------------------------------------------------

/// The plugin owning the script editor floater.
#[derive(Debug)]
pub struct EditScriptPlugin;

impl Plugin for EditScriptPlugin {
    /// Register the open message and the open / ingest / report systems.
    ///
    /// Nothing spawns at `Startup`: a script window exists only while that
    /// script is open, so `open_script` spawns the instance and builds its
    /// content. The per-window systems are gated on there being a window.
    fn build(&self, app: &mut App) {
        app.add_message::<OpenScript>().add_systems(
            Update,
            (
                // After the manager's command pass — see `FloaterSystems`:
                // the click that opens a script (an object-contents row)
                // also raises the window it landed in, and the later raise
                // wins.
                open_script.after(FloaterSystems::Commands),
                (
                    ingest_script_asset,
                    report_script_running,
                    report_script_save,
                )
                    .chain()
                    .run_if(any_with_component::<ScriptEditorState>),
            )
                .chain(),
        );
    }
}

/// One open script window's entities and live state — a **component on the
/// window**, since script editors open per script ([`FloaterKey`]). The
/// reference opens several at once, and each one's unsaved source is its own.
#[derive(Component, Debug)]
struct ScriptEditorState {
    /// The content column this window's editor is built under.
    content: Entity,
    /// Where this window's script lives (Save target), fixed for its life.
    source: ScriptSource,
    /// Whether this window's script is editable, fixed for its life.
    editable: bool,
    /// The compile backend to request for this window's script.
    target: ScriptTarget,
    /// The asset id awaited (`FetchAsset` sent), matched on `AssetReceived`.
    pending_load: Option<Uuid>,
    /// Whether a save is in flight (matched on the next `ScriptUploaded` /
    /// `AssetUploadFailed`). A save is user-triggered one at a time.
    saving: bool,
    /// The editable body field, when the script is modifiable.
    body_field: Option<Entity>,
    /// The status text node (loading / saving / result), when present.
    status: Option<Entity>,
    /// The container the compile diagnostics list is rebuilt under.
    errors: Option<Entity>,
    /// The current run state of a task script (queried on open); `None` for an
    /// agent script or before the query is answered.
    running: Option<bool>,
    /// The Running toggle's glyph node, repainted when the query answers or the
    /// user toggles it.
    running_glyph: Option<Entity>,
    /// The `(object, item)` a `RequestScriptRunning` awaits a reply for.
    pending_running: Option<(ObjectKey, InventoryKey)>,
}

/// The script editor floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn script_editor_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: "script-editor",
        title: "Script".to_owned(),
        position: Vec2::new(360.0, 90.0),
        // Content-driven, for the reason the notecard editor is: the window
        // used to open at a rect measured against the read-only block, the
        // editable body is taller than that, and the content slot clips — so
        // the Save & Compile row and the diagnostics under it were cut off the
        // bottom of the window. A field's height is intrinsic and cannot be
        // flexed into a rect (`ui_text_input`'s `fill`), so the window follows
        // the field. The grip resizes it from there.
        default_size: None,
        min_size: Some(Vec2::new(300.0, 200.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// The [`FloaterKey`] of the window showing the script at `source`.
///
/// A task-held script is keyed by its **object and item**, not the item alone:
/// two rezzed copies of one object carry the same item ids, and they are two
/// different scripts.
fn script_key(source: ScriptSource) -> FloaterKey {
    match source {
        ScriptSource::Agent { item_id } => FloaterKey::subject(&item_id),
        ScriptSource::Task { task_id, item_id } => {
            FloaterKey::subject(&format!("{task_id}/{item_id}"))
        }
    }
}

/// Open a script **per script** (`viewer-keyed-floater-audit`): raise this
/// script's window when it is already up, and otherwise spawn one, hang its
/// state off it, fetch the source and (for a task script) query its run state.
///
/// Every open of the frame is honoured, not just the last. A window already up
/// is only raised — never re-fetched — because a re-fetch would replace the
/// resident's unsaved source with the grid's copy, which is the very thing
/// keying these windows is for.
fn open_script(
    mut opens: MessageReader<OpenScript>,
    mut floaters: KeyedFloaters,
    mut commands: Commands,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    for open in opens.read().cloned() {
        let opened = floaters.open(script_editor_floater_spec(), script_key(open.source));
        let KeyedFloaterOpen::Spawned(handle) = opened else {
            continue;
        };
        build_script_window(&mut commands, handle, &open);
        sl_commands.write(SlCommand(Command::FetchAsset {
            asset_id: AssetKey::from(open.asset_id),
            asset_type: AssetType::ScriptText,
            byte_range: None,
        }));
        // A task script has a run state the save must preserve; query it so the
        // Running checkbox reflects reality rather than a guess.
        if let ScriptSource::Task { task_id, item_id } = open.source
            && open.editable
        {
            sl_commands.write(SlCommand(Command::RequestScriptRunning {
                object_id: task_id,
                item_id,
            }));
        }
    }
}

/// Furnish a freshly spawned script window: its title, a loading line, and the
/// [`ScriptEditorState`] the per-window systems find it by.
fn build_script_window(commands: &mut Commands, handle: FloaterHandle, open: &OpenScript) {
    // The title is the script's name (the title node carries no Fluent key, so
    // a direct text set is not fought by the translator).
    commands
        .entity(handle.title_text)
        .insert(Text::new(open.name.clone()));
    let status = spawn_status(commands, handle.content, "script-status-loading", DIM_COLOR);
    let pending_running = match open.source {
        ScriptSource::Task { task_id, item_id } if open.editable => Some((task_id, item_id)),
        ScriptSource::Task { .. } | ScriptSource::Agent { .. } => None,
    };
    commands.entity(handle.root).insert(ScriptEditorState {
        content: handle.content,
        source: open.source,
        editable: open.editable,
        target: open.target,
        pending_load: Some(open.asset_id),
        saving: false,
        body_field: None,
        status: Some(status),
        errors: None,
        running: None,
        running_glyph: None,
        pending_running,
    });
}

// ---------------------------------------------------------------------------
// Asset received → build the editor.
// ---------------------------------------------------------------------------

/// Fold the fetched script source into the editor once it arrives: decode the
/// UTF-8 text, then build the read-only or editable body.
fn ingest_script_asset(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut ScriptEditorState>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    // Collected once and replayed per window: a reader is consumed by the first
    // pass over it, so with two scripts open the second would see nothing.
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut state in &mut windows {
        for event in &frame {
            let SlSessionEvent::AssetReceived(asset) = &event.0 else {
                continue;
            };
            if state.pending_load != Some(asset.id) {
                continue;
            }
            state.pending_load = None;
            let (content, source) = (state.content, state.source);

            // Script source is plain UTF-8 text; a non-UTF-8 byte (a corrupt asset)
            // is shown lossily rather than refused, so the resident sees what is
            // there instead of an opaque error.
            let text = String::from_utf8_lossy(&asset.data).into_owned();

            let editable = state.editable;
            let running = state.running.unwrap_or(true);
            tear_down(&mut commands, &children, content);
            let built = populate_editor(
                &mut commands,
                content,
                &text,
                editable,
                source,
                running,
                FONT_SIZE,
                true,
            );
            state.body_field = built.body_field;
            state.status = built.status;
            state.errors = built.errors;
            state.running_glyph = built.running_glyph;
        }
    }
}

/// The entities [`populate_editor`] hands back to the live state.
#[derive(Debug, Default, Clone, Copy)]
struct BuiltEditor {
    /// The editable body field, when the script is modifiable.
    body_field: Option<Entity>,
    /// The status text node, when the editor offers a Save button.
    status: Option<Entity>,
    /// The compile-diagnostics container, when the editor offers a Save button.
    errors: Option<Entity>,
    /// The Running toggle's glyph node, when a task script is editable.
    running_glyph: Option<Entity>,
}

/// Build the editor's content under `content`: the body (editable field or
/// read-only block), the Running toggle (task scripts), a Save & Compile button
/// with a status line, and an empty diagnostics container.
///
/// `live` is `true` for the real floater (the Save button and Running toggle are
/// wired to the session) and `false` for a specimen (they are shown for layout
/// but do nothing).
#[expect(
    clippy::too_many_arguments,
    reason = "the editor's shape is its content, permission gate, save target, run \
              state, font size and live/specimen flag — all genuinely independent"
)]
fn populate_editor(
    commands: &mut Commands,
    content: Entity,
    text: &str,
    editable: bool,
    source: ScriptSource,
    running: bool,
    font_size: f32,
    live: bool,
) -> BuiltEditor {
    if !editable {
        spawn_note(commands, content, "script-readonly-note", font_size);
    }

    let body_field = if editable {
        Some(spawn_body_field(commands, content, text, font_size))
    } else {
        spawn_readonly_body(commands, content, text, font_size);
        None
    };

    let running_glyph = (editable && source.is_task())
        .then(|| spawn_running_toggle(commands, content, running, font_size, live));

    let (status, errors) = if editable {
        let bar = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..row(Val::Px(8.0))
                },
                ChildOf(content),
            ))
            .id();
        let save = spawn_save_button(commands, bar, font_size);
        let status = commands
            .spawn((
                Text::default(),
                UiFont::Sans.at(font_size),
                TextColor(DIM_COLOR),
                ChildOf(bar),
            ))
            .id();
        // The diagnostics list is rebuilt under this column on every compile.
        // Bounded and scrollable, because a failed compile hands back as many
        // rows as the script has errors: unbounded, the twentieth diagnostic
        // would push the list past the window's clipping content slot (and
        // squeeze the body field, which shrinks to fit, into nothing) — the
        // window's height is fixed, so this list's growth has to stop somewhere
        // reachable.
        let errors = commands
            .spawn((
                Node {
                    max_height: Val::Px(DIAGNOSTICS_HEIGHT),
                    overflow: Overflow::scroll_y(),
                    ..column(Val::Px(2.0))
                },
                ScrollPosition::default(),
                Pickable::default(),
                Name::new("script-diagnostics"),
                ChildOf(content),
            ))
            .id();
        commands.entity(errors).observe(on_diagnostics_scroll);
        if live && let Some(field) = body_field {
            attach_save(commands, save, source, field);
        }
        (Some(status), Some(errors))
    } else {
        (None, None)
    };

    BuiltEditor {
        body_field,
        status,
        errors,
        running_glyph,
    }
}

/// Wire a Save button to upload the edited source over the source's
/// `UpdateScript*` capability and have the simulator compile it. The current
/// Running toggle state is carried through for a task script.
fn attach_save(commands: &mut Commands, button: Entity, source: ScriptSource, body_field: Entity) {
    commands.entity(button).observe(
        move |press: On<Pointer<Press>>,
              fields: Query<&EditableText>,
              parents: Query<&ChildOf>,
              floaters: Query<(Entity, &Floater)>,
              mut windows: Query<&mut ScriptEditorState>,
              children: Query<&Children>,
              mut sl_commands: MessageWriter<SlCommand>,
              mut commands: Commands| {
            if press.button != PointerButton::Primary {
                return;
            }
            // Compile *this* window's source: the button is inside it.
            let Some(window) = host_floater(press.entity, &parents, &floaters) else {
                return;
            };
            let Ok(mut state) = windows.get_mut(window) else {
                return;
            };
            let Ok(field) = fields.get(body_field) else {
                return;
            };
            let running = state.running.unwrap_or(true);
            sl_commands.write(SlCommand(Command::UploadScript {
                location: source.location(running),
                target: state.target,
                source: field.value().to_string().into_bytes(),
            }));
            state.saving = true;
            // A fresh compile supersedes the previous run's diagnostics.
            if let Some(errors) = state.errors {
                tear_down(&mut commands, &children, errors);
            }
            if let Some(status) = state.status {
                set_status(&mut commands, status, "script-status-saving", DIM_COLOR);
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Run-state query reply.
// ---------------------------------------------------------------------------

/// Reflect a `RequestScriptRunning` reply into the Running toggle: record the run
/// state and repaint the checkbox glyph.
fn report_script_running(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut ScriptEditorState>,
    mut commands: Commands,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut state in &mut windows {
        for event in &frame {
            let SlSessionEvent::ScriptRunning {
                object_id,
                item_id,
                running,
            } = &event.0
            else {
                continue;
            };
            if state.pending_running != Some((*object_id, *item_id)) {
                continue;
            }
            state.running = Some(*running);
            repaint_running_glyph(&mut commands, state.running_glyph, *running);
        }
    }
}

// ---------------------------------------------------------------------------
// Save / compile result.
// ---------------------------------------------------------------------------

/// Report the outcome of an in-flight script save. A save is user-triggered one
/// at a time, so the next terminal upload event while `saving` is set is treated
/// as its result. A [`SlSessionEvent::ScriptUploaded`] carries the compile
/// outcome; an [`SlSessionEvent::AssetUploadFailed`] is a transport failure.
fn report_script_save(
    mut events: MessageReader<SlEvent>,
    mut windows: Query<&mut ScriptEditorState>,
    mut inventory: Option<ResMut<crate::inventory::InventoryModel>>,
    children: Query<&Children>,
    translator: Translator,
    mut commands: Commands,
) {
    let frame: Vec<&SlEvent> = events.read().collect();
    if frame.is_empty() {
        return;
    }
    for mut state in &mut windows {
        for event in &frame {
            if !state.saving {
                continue;
            }
            match &event.0 {
                SlSessionEvent::ScriptUploaded {
                    new_asset,
                    compiled,
                    errors,
                    ..
                } => {
                    state.saving = false;
                    // Follow the item onto the asset the save wrote, or the
                    // next open of this script fetches the source as it was
                    // *before* — see the notecard editor for the same
                    // rebinding and why the reply's item id is not relied on.
                    if let (Some(new_asset), ScriptSource::Agent { item_id }) =
                        (new_asset, state.source)
                        && let Some(model) = inventory.as_mut()
                    {
                        let _folder = model.rebind_asset(item_id, *new_asset);
                    }
                    let (key, color) = if *compiled {
                        ("script-status-saved", DIM_COLOR)
                    } else {
                        ("script-status-compile-failed", ERROR_COLOR)
                    };
                    if let Some(status) = state.status {
                        set_status(&mut commands, status, key, color);
                    }
                    if let Some(container) = state.errors {
                        tear_down(&mut commands, &children, container);
                        for error in errors {
                            spawn_error_row(
                                &mut commands,
                                container,
                                &translator,
                                error,
                                FONT_SIZE,
                            );
                        }
                    }
                }
                SlSessionEvent::AssetUploadFailed { reason } => {
                    warn!("script save failed: {reason}");
                    state.saving = false;
                    if let Some(status) = state.status {
                        set_status(
                            &mut commands,
                            status,
                            "script-status-save-failed",
                            ERROR_COLOR,
                        );
                    }
                }
                _other => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Content builders.
// ---------------------------------------------------------------------------

/// Despawn every child of `parent`.
fn tear_down(commands: &mut Commands, children: &Query<&Children>, parent: Entity) {
    if let Ok(existing) = children.get(parent) {
        for child in existing.iter().collect::<Vec<_>>() {
            commands.entity(child).despawn();
        }
    }
}

/// Spawn a fresh status line under `parent`, driven by a Fluent key.
fn spawn_status(
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
            TextColor(color),
            ChildOf(parent),
        ))
        .id()
}

/// Repoint an existing status node at a new Fluent key and colour.
fn set_status(commands: &mut Commands, status: Entity, key: &'static str, color: Color) {
    commands
        .entity(status)
        .insert((Translated::new(key), TextColor(color)));
}

/// Spawn the read-only note shown above a no-modify script's source.
fn spawn_note(commands: &mut Commands, parent: Entity, key: &'static str, font_size: f32) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(font_size),
        TextColor(DIM_COLOR),
        ChildOf(parent),
    ));
}

/// Spawn the editable multi-line body field, returning its entity.
fn spawn_body_field(commands: &mut Commands, parent: Entity, text: &str, font_size: f32) -> Entity {
    crate::ui_text_input::spawn_text_input(
        commands,
        parent,
        &crate::ui_text_input::TextInputSpec {
            initial: text.to_owned(),
            font_size,
            visible_lines: BODY_VISIBLE_LINES,
            tab_index: 1,
            ..crate::ui_text_input::TextInputSpec::new(
                "script-body",
                crate::ui_text_input::TextInputKind::Multiline,
            )
        },
    )
}

/// Spawn the read-only body: a bounded, clipped block showing the script source
/// in a monospace font (no caret, no edit).
fn spawn_readonly_body(commands: &mut Commands, parent: Entity, text: &str, font_size: f32) {
    commands
        .spawn((
            Node {
                max_width: Val::Px(READONLY_BODY_WIDTH),
                max_height: Val::Px(READONLY_BODY_HEIGHT),
                overflow: Overflow::clip(),
                ..column(Val::Px(2.0))
            },
            ChildOf(parent),
        ))
        .with_child((
            Text::new(text.to_owned()),
            UiFont::Mono.at(font_size),
            TextColor(LABEL_COLOR),
        ));
}

/// Spawn the Running toggle for a task script: a check glyph plus a label. When
/// `live`, clicking it flips the run state carried into the next Save. Returns
/// the glyph node so the run-state query can repaint it.
fn spawn_running_toggle(
    commands: &mut Commands,
    parent: Entity,
    running: bool,
    font_size: f32,
    live: bool,
) -> Entity {
    let mut row_entity = commands.spawn((
        Button,
        Node {
            align_items: AlignItems::Center,
            ..row(Val::Px(4.0))
        },
        Pickable::default(),
        Name::new("script-running-toggle"),
        ChildOf(parent),
    ));
    if live {
        row_entity.observe(on_running_toggle);
    }
    let host = row_entity.id();
    let glyph = commands
        .spawn((
            Text::new(if running {
                CHECKED_GLYPH
            } else {
                UNCHECKED_GLYPH
            }),
            UiFont::Sans.at(font_size),
            TextColor(if running { CHECK_COLOR } else { DIM_COLOR }),
            Pickable::IGNORE,
            ChildOf(host),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new("script-running"),
        UiFont::Sans.at(font_size),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(host),
    ));
    glyph
}

/// The Running toggle was clicked: flip the run state the next Save carries, and
/// repaint the glyph. The state is applied on Save (a save *is* a recompile), so
/// this does not send a separate `SetScriptRunning`.
fn on_running_toggle(
    press: On<Pointer<Press>>,
    parents: Query<&ChildOf>,
    floaters: Query<(Entity, &Floater)>,
    mut windows: Query<&mut ScriptEditorState>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    // The toggle belongs to the window it sits in — with two scripts open,
    // this must not flip the other one's run state.
    let Some(window) = host_floater(press.entity, &parents, &floaters) else {
        return;
    };
    let Ok(mut state) = windows.get_mut(window) else {
        return;
    };
    let running = !state.running.unwrap_or(true);
    state.running = Some(running);
    repaint_running_glyph(&mut commands, state.running_glyph, running);
}

/// Repaint the Running toggle's glyph to reflect `running`.
fn repaint_running_glyph(commands: &mut Commands, glyph: Option<Entity>, running: bool) {
    if let Some(glyph) = glyph {
        commands.entity(glyph).insert((
            Text::new(if running {
                CHECKED_GLYPH
            } else {
                UNCHECKED_GLYPH
            }),
            TextColor(if running { CHECK_COLOR } else { DIM_COLOR }),
        ));
    }
}

/// Spawn one compile-diagnostic row: the position (when the grid gave one) plus
/// the message, formatted for the active locale.
fn spawn_error_row(
    commands: &mut Commands,
    parent: Entity,
    translator: &Translator,
    error: &ScriptCompileError,
    font_size: f32,
) {
    let line = match (error.line, error.column) {
        (Some(line), Some(column)) => translator.format(
            "script-error-at",
            &TransArgs::new()
                .int("line", i64::from(line))
                .int("column", i64::from(column))
                .text("message", &error.message),
        ),
        _ => translator.format(
            "script-error-nopos",
            &TransArgs::new().text("message", &error.message),
        ),
    };
    commands.spawn((
        Text::new(line),
        UiFont::Mono.at(font_size),
        TextColor(ERROR_COLOR),
        ChildOf(parent),
    ));
}

/// Scroll the bounded diagnostics list with the mouse wheel (its own
/// [`ScrollPosition`]), matching the notecard reader and the result lists.
fn on_diagnostics_scroll(
    mut event: On<Pointer<Scroll>>,
    mut positions: Query<&mut ScrollPosition>,
) {
    /// Logical pixels one wheel notch scrolls.
    const LINE_SCROLL_PIXELS: f32 = 24.0;
    if let Ok(mut position) = positions.get_mut(event.entity) {
        position.0.y = (position.0.y - event.y * LINE_SCROLL_PIXELS).max(0.0);
    }
    event.propagate(false);
}

/// Spawn the Save & Compile button, returning its entity for the caller to wire.
fn spawn_save_button(commands: &mut Commands, parent: Entity, font_size: f32) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(2),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.32, 0.36, 0.44)),
            BackgroundColor(Color::srgb(0.13, 0.15, 0.20)),
            Pickable::default(),
            Name::new("script-save"),
            ChildOf(parent),
        ))
        .with_child((
            Text::default(),
            Translated::new("script-save"),
            UiFont::Sans.at(font_size),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The sample LSL source the specimen's body shows.
const SPECIMEN_TEXT: &str = "default\n{\n    state_entry()\n    {\n        \
                             llSay(0, \"Hello, Avatar!\");\n    }\n}\n";

/// Spawn the script editor's content specimen: an editable body, a Running
/// toggle, a Save & Compile button and one sample compile-error row, built with
/// no floater / session so `crate::ui_test` sweeps its layout across every
/// script, scale and font.
pub fn spawn_script_editor_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    let col = commands
        .spawn((
            Node {
                ..column(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    // A task source so the Running toggle is swept alongside the body.
    let source = ScriptSource::Task {
        task_id: ObjectKey::from(Uuid::from_u128(0)),
        item_id: InventoryKey::from(Uuid::from_u128(0)),
    };
    let built = populate_editor(
        commands,
        col,
        &cx.text(SPECIMEN_TEXT),
        true,
        source,
        true,
        cx.font_size,
        false,
    );
    // A representative diagnostic so the error-list layout is swept too.
    if let Some(errors) = built.errors {
        commands.spawn((
            Text::new(cx.text("Line 5, column 9: syntax error")),
            UiFont::Mono.at(cx.font_size),
            TextColor(ERROR_COLOR),
            ChildOf(errors),
        ));
    }
    col
}

#[cfg(test)]
mod tests {
    use crate::world_api::ScriptSource;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::ScriptUploadLocation;
    use sl_client_bevy::{InventoryKey, ObjectKey, Uuid};

    /// A task upload carries the run state through as `is_script_running`, and no
    /// experience is set; an agent upload has neither.
    #[test]
    fn task_location_carries_run_state() {
        let item_id = InventoryKey::from(Uuid::from_u128(1));
        let task_id = ObjectKey::from(Uuid::from_u128(2));

        let agent = ScriptSource::Agent { item_id }.location(true);
        assert_eq!(agent, ScriptUploadLocation::AgentInventory { item_id });

        let task = ScriptSource::Task { task_id, item_id }.location(false);
        assert_eq!(
            task,
            ScriptUploadLocation::TaskInventory {
                task_id,
                item_id,
                running: false,
                experience: None,
            }
        );
    }

    /// **One window per script** (`viewer-keyed-floater-audit`): the open path,
    /// driven by the message an object-contents row writes.
    ///
    /// The reference opens several scripts at once, and each window's unsaved
    /// source is its own — which the singleton editor could not hold.
    mod instances {
        use super::super::{ScriptEditorState, open_script, script_key};
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin};
        use crate::ui::UiRoot;
        use crate::world_api::{OpenScript, ScriptSource};
        use bevy::prelude::*;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{Command, InventoryKey, ObjectKey, ScriptTarget, SlCommand, Uuid};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// Two scripts: one in the agent's inventory, one inside an object.
        fn scripts() -> (ScriptSource, ScriptSource) {
            (
                ScriptSource::Agent {
                    item_id: InventoryKey::from(Uuid::from_u128(0xC3)),
                },
                ScriptSource::Task {
                    task_id: ObjectKey::from(Uuid::from_u128(0xD4)),
                    item_id: InventoryKey::from(Uuid::from_u128(0xE5)),
                },
            )
        }

        /// An app with the floater manager and this module's open path.
        fn editor_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<OpenScript>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_plugins(FloaterPlugin)
                .add_systems(Update, open_script);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open a script the way every caller does — by writing the message.
        fn open(app: &mut App, source: ScriptSource, asset: u128) {
            app.world_mut().write_message(OpenScript {
                name: "A script".to_owned(),
                asset_id: Uuid::from_u128(asset),
                editable: true,
                source,
                target: ScriptTarget::Mono,
            });
            app.update();
        }

        /// Every live script window, as (entity, source) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, ScriptSource)> {
            app.world_mut()
                .query::<(Entity, &ScriptEditorState)>()
                .iter(app.world())
                .map(|(entity, state)| (entity, state.source))
                .collect()
        }

        /// How many asset fetches the session was asked for this frame.
        fn fetches(app: &App) -> usize {
            app.world()
                .resource::<Messages<SlCommand>>()
                .iter_current_update_messages()
                .filter(|command| matches!(command.0, Command::FetchAsset { .. }))
                .count()
        }

        /// Two scripts are two windows, each with its own source and content
        /// column — including a task script, which is keyed by object *and*
        /// item so two rezzed copies of one object stay apart.
        #[test]
        fn two_scripts_open_two_windows() -> Result<(), TestError> {
            let (agent_script, task_script) = scripts();
            let mut app = editor_app();
            open(&mut app, agent_script, 0xC3_00);
            open(&mut app, task_script, 0xE5_00);

            let open_windows = windows(&mut app);
            assert_eq!(
                open_windows.len(),
                2,
                "the second script reused the first window"
            );
            let world = app.world();
            let contents: Vec<Entity> = open_windows
                .iter()
                .filter_map(|(window, _source)| world.get::<ScriptEditorState>(*window))
                .map(|state| state.content)
                .collect();
            assert!(
                contents.first() != contents.get(1),
                "both windows build into one content column"
            );
            let keys: Vec<Option<&crate::floater::FloaterKey>> = open_windows
                .iter()
                .map(|(window, _source)| world.get::<Floater>(*window).and_then(Floater::key))
                .collect();
            assert!(keys.contains(&Some(&script_key(agent_script))));
            assert!(keys.contains(&Some(&script_key(task_script))));
            // The task key names the object as well as the item.
            assert!(
                script_key(task_script).as_str().contains('/'),
                "a task script must be keyed by its object and item"
            );
            Ok(())
        }

        /// **Re-opening a script that is already up never re-fetches it** — the
        /// unsaved-source guarantee.
        #[test]
        fn reopening_a_script_does_not_refetch_it() -> Result<(), TestError> {
            let (agent_script, _task) = scripts();
            let mut app = editor_app();
            open(&mut app, agent_script, 0xC3_00);
            assert_eq!(fetches(&app), 1, "the first open must fetch the source");

            open(&mut app, agent_script, 0xC3_00);
            assert_eq!(
                fetches(&app),
                0,
                "a re-open re-fetched the source, which would discard unsaved edits"
            );
            assert_eq!(windows(&mut app).len(), 1);
            Ok(())
        }

        /// Closing one script ends that window and leaves the other open.
        #[test]
        fn closing_one_script_leaves_the_other() -> Result<(), TestError> {
            let (agent_script, task_script) = scripts();
            let mut app = editor_app();
            open(&mut app, agent_script, 0xC3_00);
            open(&mut app, task_script, 0xE5_00);
            let target = windows(&mut app)
                .into_iter()
                .find_map(|(window, source)| (source == agent_script).then_some(window))
                .ok_or("the agent script has no window")?;

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
                left.first().map(|(_window, source)| *source),
                Some(task_script)
            );
            Ok(())
        }
    }
}
