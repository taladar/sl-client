//! The **RLVa console** (`rlv_console`): type `@`-commands at your own viewer
//! and watch what each one does.
//!
//! This is the debugging and authoring tool, and it is the only place in the
//! viewer where the *agent itself* is the issuing object. Everything a worn
//! attachment can say, a person can say here — which is exactly how the RLV
//! engine is meant to be exercised, and how the reference's own console works.
//!
//! # What a line does
//!
//! The input takes one chat line, in the form an object would `llOwnerSay`:
//! `@detach=n`, or several commands comma-separated, `@detach=n,fly=n`. Each
//! command is fed to the one state machine
//! ([`sl_viewer_world_api::rlv::RlvSession`]) with the **agent's own key** as
//! the issuer, and its outcome is reported the way the reference reports it:
//!
//! - `INFO:` — accepted, and the held set changed;
//! - `ERR:` — refused (a keyword the dictionary does not know, an option that
//!   does not parse, a `=force` action, a duplicate);
//! - `RET:` — retained, held back until the viewer knows more.
//!
//! Because the issuer is the agent, closing the window lifts everything typed
//! into it — the reference clears the agent's own restrictions in `onClose` for
//! exactly this reason, so an experiment cannot leave the viewer restrained
//! after the window that made it is gone.
//!
//! # Queries are accepted but not answered yet
//!
//! `@getoutfit=2222` and its family are *questions about the viewer*, and the
//! answer is built from facts the state machine does not hold: what is worn on
//! which point, what the shared `#RLV` folder tree looks like, where the camera
//! is. Wiring that up is the query layer's own integration
//! (`viewer-rlv-queries` against a live session); until it lands, a query
//! command is reported as accepted-but-unanswered rather than being given a
//! made-up answer, because a wrong `@getattach` reply is worse than none.
//!
//! The **extension** commands are the exception, and are answered here:
//! `@getdebug_<setting>` reads one of the allowlisted debug settings and its
//! answer is shown on the reply stream, `@setdebug_<setting>:<value>=force`
//! writes one, and `@setrot:<radians>=force` turns the avatar. They are asked
//! of [`RlvState::run_extension`] only *after* the state machine has handed the
//! command back, so nothing in the dictionary can be shadowed by them.
//!
//! Reference (Firestorm, read-only): `rlvfloaters.cpp` (`RlvFloaterConsole`),
//! `floater_rlv_console.xml`.

use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui_widgets::Activate;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::SlIdentity;
use sl_rlv::{
    RLV_PREFIX, RlvCommand, RlvEnvCommand, RlvEnvSource, RlvExtCommand, RlvExtSource, RlvOutcome,
    RlvParam, RlvReply, RlvState, parse_chat_line,
};
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::UiPanelShown;
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_element::{ContentMayOverflow, TextMayClip};
use sl_viewer_ui_core::ui_ellipsis::{RevealEllipsis, spawn_ellipsis_marker};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, amend_row_node, layout_virtual_lists,
    spawn_specimen_row, spawn_virtual_scrollbar,
};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_panel,
    floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_tab::DEFAULT_ELLIPSIS;
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::rlv::{
    RlvConsoleKind, RlvSession, SETTING_DEBUG_HIDE_UNSET_DUPLICATE, ViewerRlvExt, rlv_flag,
    rlv_is_enabled,
};
use uuid::Uuid;

use crate::style::{DIM_LABEL_COLOR, FONT_SIZE, LABEL_COLOR, ROW_HEIGHT, spawn_action_button};
use sl_viewer_ui_core::skin::{
    CONSOLE_ERROR_CLASS, CONSOLE_INFO_CLASS, CONSOLE_REPLY_CLASS, LIST_SURFACE_CLASS, TEXT_CLASS,
    set_state_class, set_state_class_on, text_role,
};

/// The floater's stable id.
pub const CONSOLE_FLOATER_ID: &str = "rlv-console";

/// The width of the input field, in `"0"` advances. It fills its row, so this
/// is only the intrinsic width the fill overrides.
const INPUT_WIDTH_GLYPHS: f32 = 48.0;

/// Why a transcript line's clip may slice the line it holds: a virtualized
/// list's rows are one uniform height, so a line longer than the window is
/// drawn on its one row and cut behind a revealed `…` rather than wrapped into
/// the rows below it.
const LINE_CLIP_REASON: &str =
    "a console line is one uniform-height row, cut at the row's end behind its revealed `…` marker";

/// The height of a transcript row whose line is drawn at `font_size`:
/// [`ROW_HEIGHT`] at [`FONT_SIZE`], and in proportion above it, so a larger
/// font's line stays inside its row instead of spilling into its neighbours.
/// Never less than [`ROW_HEIGHT`].
fn console_row_height(font_size: f32) -> f32 {
    ROW_HEIGHT.max((font_size * ROW_HEIGHT / FONT_SIZE).ceil())
}

// --- Pure command handling ------------------------------------------------

/// What the console decided about one submitted line, before it touched the
/// state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleVerdict {
    /// Nothing was typed; nothing to do and nothing to say.
    Empty,
    /// RLV is switched off, so the line is not run.
    Disabled,
    /// The line does not start with `@`, or carries no command after it.
    NotACommand,
    /// The commands to run, in the order they were written.
    Run(Vec<String>),
}

/// Classify a submitted line the way the reference's `onInput` does: an empty
/// field does nothing, a viewer with RLV off says so, a line that is not an
/// `@`-command says so, and anything else is split on commas into the commands
/// to run.
///
/// The reference also requires more than three characters, which is its way of
/// rejecting `@` alone and `@x`; splitting and dropping the empty pieces says
/// the same thing without a magic length, and correctly accepts the shortest
/// real command there is (`@fly=n`).
#[must_use]
pub fn classify(line: &str, rlv_enabled: bool) -> ConsoleVerdict {
    let line = line.trim();
    if line.is_empty() {
        return ConsoleVerdict::Empty;
    }
    if !rlv_enabled {
        return ConsoleVerdict::Disabled;
    }
    let Some(body) = line.strip_prefix(RLV_PREFIX) else {
        return ConsoleVerdict::NotACommand;
    };
    let commands: Vec<String> = body
        .split(',')
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect();
    if commands.is_empty() {
        return ConsoleVerdict::NotACommand;
    }
    ConsoleVerdict::Run(commands)
}

/// Which console stream an outcome is reported on — the reference's INFO / ERR
/// split. (Its third stream, `RET:`, has no outcome here that could fill it;
/// see [`RlvConsoleKind`].)
#[must_use]
pub const fn outcome_stream(outcome: RlvOutcome) -> RlvConsoleKind {
    if outcome.succeeded() {
        RlvConsoleKind::Info
    } else {
        RlvConsoleKind::Error
    }
}

/// The short word appended to a reported command, naming what happened to it —
/// the reference's `getStringFromReturnCode`. `None` for a plain success, which
/// needs no explaining.
///
/// The wildcard arm is not laziness: [`RlvOutcome`] is `#[non_exhaustive]`, so
/// a variant added to the engine later must still report *something* rather
/// than fail to compile a window that has nothing to say about it.
#[must_use]
pub const fn outcome_suffix(outcome: RlvOutcome) -> Option<&'static str> {
    match outcome {
        RlvOutcome::Success => None,
        RlvOutcome::SuccessDuplicate => Some("already set"),
        RlvOutcome::SuccessUnset => Some("was not set"),
        RlvOutcome::SuccessDeprecated => Some("deprecated spelling"),
        RlvOutcome::NotAStateChange => Some("not a restriction"),
        RlvOutcome::FailedParam => Some("unknown command"),
        RlvOutcome::FailedOption => Some("bad option"),
        RlvOutcome::FailedLock => Some("already held by another object"),
        RlvOutcome::FailedUnheldBehaviour => Some("behaviour not held"),
        RlvOutcome::FailedNoSharedRoot => Some("no #RLV folder"),
        RlvOutcome::FailedDisabled => Some("turned off in your settings"),
        _ => Some("failed"),
    }
}

/// How one command is written on its console line: the command as typed, with
/// the outcome's word in brackets where there is one.
#[must_use]
pub fn report_line(command: &str, outcome: RlvOutcome) -> String {
    match outcome_suffix(outcome) {
        Some(suffix) => format!("{RLV_PREFIX}{command} ({suffix})"),
        None => format!("{RLV_PREFIX}{command}"),
    }
}

/// Whether this outcome is one the `RLVaDebugHideUnsetDuplicate` setting hides:
/// a command that set nothing new, or lifted nothing that was held.
#[must_use]
pub const fn is_unset_or_duplicate(outcome: RlvOutcome) -> bool {
    matches!(
        outcome,
        RlvOutcome::SuccessDuplicate | RlvOutcome::SuccessUnset
    )
}

/// What running one console line asked of the viewer beyond the state machine.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ConsoleRun {
    /// Whether the held set changed, so the floaters watching it rebuild.
    pub changed: bool,
    /// The heading `@setrot` asked the avatar to face, if the line carried one.
    pub rotate_to: Option<f32>,
}

/// What one of the two extension handlers made of a command, in the shape the
/// console reports: an outcome, the answer it built, whether it *owed* one, and
/// the heading only `@setrot` produces.
struct Handled {
    /// How the handler says it went.
    outcome: RlvOutcome,
    /// The line a script would have heard, if any.
    reply: Option<RlvReply>,
    /// Whether this was a **read** — one that owed an answer, so the absence of
    /// one is worth saying out loud rather than passing over.
    is_read: bool,
    /// The heading `@setrot` asks the avatar to face.
    rotate_to: Option<f32>,
}

/// Run one command line against `state` as if `issuer` had said it, appending
/// each command's report to `lines`.
///
/// A command the dictionary does not claim is offered to the two extension
/// handlers before it is reported unknown, in the reference's order: the debug
/// window (`ext`) — `@getdebug_*`, `@setdebug_*` and `@setrot` — and then the
/// environment (`env`) — `@getenv_*` and `@setenv_*`.
///
/// Split out from the system so the whole decision — parse, apply, classify,
/// report — is testable without an app.
pub fn run_line(
    state: &mut RlvState,
    issuer: Uuid,
    line: &str,
    hide_unset_duplicate: bool,
    ext: &mut impl RlvExtSource,
    env: &mut impl RlvEnvSource,
    lines: &mut Vec<(RlvConsoleKind, String)>,
) -> ConsoleRun {
    let mut run = ConsoleRun::default();
    let Some(parsed) = parse_chat_line(line) else {
        lines.push((RlvConsoleKind::Error, "not an RLV command line".to_owned()));
        return run;
    };
    for command in parsed {
        match command {
            Err(error) => lines.push((RlvConsoleKind::Error, error.to_string())),
            Ok(command) => {
                let text = command_text(&command);
                let is_query = matches!(command.param, RlvParam::Reply { .. });
                let applied = state.apply(issuer, &command);
                // The state machine hands back every `=force` action and every
                // query; one of those may still be an extension command, which
                // is the last place a keyword can be recognised.
                let mut handled = None;
                if applied == RlvOutcome::NotAStateChange {
                    if let Some(result) = state.run_extension(issuer, &command, ext) {
                        handled = Some(Handled {
                            outcome: result.outcome,
                            reply: result.reply,
                            is_read: matches!(
                                RlvExtCommand::classify(&command),
                                Some(RlvExtCommand::GetDebug { .. })
                            ),
                            rotate_to: result.rotate_to,
                        });
                    } else if let Some(result) = state.run_environment(issuer, &command, env) {
                        handled = Some(Handled {
                            outcome: result.outcome,
                            reply: result.reply,
                            is_read: matches!(
                                RlvEnvCommand::classify(&command),
                                Some(RlvEnvCommand::GetEnv { .. })
                            ),
                            rotate_to: None,
                        });
                    }
                }
                // Only what the *state machine* applied can have changed the
                // held set. An extension command answers a question, writes a
                // setting, turns the avatar or repaints the sky — none of them a
                // restriction, so none may wake the floaters watching the
                // revision.
                if applied.succeeded() {
                    run.changed = true;
                }
                let mut outcome = applied;
                if let Some(ref result) = handled {
                    outcome = result.outcome;
                    if let Some(heading) = result.rotate_to {
                        run.rotate_to = Some(heading);
                    }
                }
                if hide_unset_duplicate && is_unset_or_duplicate(outcome) {
                    continue;
                }
                lines.push((outcome_stream(outcome), report_line(&text, outcome)));
                match handled {
                    // An extension read: the answer, as the asking script would
                    // have heard it. An empty one is shown as an empty one.
                    Some(result) => match result.reply {
                        Some(reply) => lines.push((
                            RlvConsoleKind::Reply,
                            format!("{}: {}", reply.channel, reply.message),
                        )),
                        // A *read* with no reply is one whose channel no reply
                        // may go on. The reference drops it silently; in a
                        // debugging console that is worth saying out loud.
                        // `@setrot` asked as a query is not a read and answers
                        // nothing by design, so it says nothing here either.
                        None if result.is_read => {
                            lines.push((
                                RlvConsoleKind::Error,
                                "no reply may be chatted on that channel".to_owned(),
                            ));
                        }
                        None => {}
                    },
                    // See the module documentation: the answer needs facts the
                    // state machine does not hold, and a wrong answer is worse
                    // than an honest silence.
                    None if is_query => lines.push((
                        RlvConsoleKind::Reply,
                        "(queries are not answered yet — the query source is not wired up)"
                            .to_owned(),
                    )),
                    None => {}
                }
            }
        }
    }
    run
}

/// One command written back the way it arrived —
/// `keyword[:option]=param` — which is what the console echoes so a typo is
/// visible in the report line, not normalised away.
#[must_use]
pub fn command_text(command: &RlvCommand) -> String {
    let option = command
        .option
        .as_ref()
        .map(|option| format!(":{option}"))
        .unwrap_or_default();
    if command.param_text.is_empty() {
        format!("{}{option}", command.keyword)
    } else {
        format!("{}{option}={}", command.keyword, command.param_text)
    }
}

// --- Resources ------------------------------------------------------------

/// The floater's retained entities.
#[derive(Resource, Debug)]
struct ConsoleUi {
    /// The transcript's virtualized viewport.
    viewport: Entity,
    /// The input field's `EditableText` entity.
    input: Entity,
}

/// What the transcript view was last built against, so a rebuild only happens
/// when a line was appended.
#[derive(Resource, Debug, Default)]
struct ConsoleView {
    /// The console revision the rows were bound at.
    built_revision: u64,
    /// Whether the rows have been bound at least once.
    built: bool,
    /// Whether the floater was open on the previous frame, so a close can be
    /// noticed and the agent's own restrictions lifted.
    was_open: bool,
}

/// Marks a pooled transcript row's text node, so the binder can find it.
#[derive(Component, Debug, Clone, Copy)]
struct ConsoleRowText(Entity);

// --- Plugin ---------------------------------------------------------------

/// Registers the RLVa console and its systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct RlvConsolePlugin;

impl Plugin for RlvConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleView>()
            .add_systems(
                Startup,
                spawn_console_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                submit_console_line
                    .before(layout_virtual_lists)
                    .run_if(floater_shown(CONSOLE_FLOATER_ID)),
            )
            .add_systems(
                Update,
                (size_console_list, bind_console_rows)
                    .chain()
                    .after(layout_virtual_lists)
                    .run_if(floater_shown(CONSOLE_FLOATER_ID)),
            )
            // Not gated: the whole point is to notice the frame the window
            // *stopped* being shown.
            .add_systems(Update, clear_console_restrictions_on_close);
    }
}

// --- Floater --------------------------------------------------------------

/// The RLVa console's [`FloaterSpec`].
#[must_use]
pub fn rlv_console_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: CONSOLE_FLOATER_ID,
        title: "RLVa Console".to_owned(),
        position: Vec2::new(300.0, 140.0),
        default_size: Some(Vec2::new(560.0, 320.0)),
        min_size: Some(Vec2::new(360.0, 200.0)),
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
fn spawn_console_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, rlv_console_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("rlv-console-title"));
    let builder = commands.register_system(build_console_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the transcript above, the input and Clear below.
fn build_console_content(In(handle): In<FloaterHandle>, mut commands: Commands) {
    let ui = spawn_console_content(&mut commands, handle.content, FONT_SIZE);
    commands.insert_resource(ui);
}

/// Build the console's content into `parent` at `font_size`: the transcript
/// viewport, the prompt, the input and the Clear button. Shared by the live
/// floater's first-open build and its specimen.
fn spawn_console_content(commands: &mut Commands, parent: Entity, font_size: f32) -> ConsoleUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("rlv-console-content"),
            ChildOf(parent),
        ))
        .id();

    let viewport = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            // The rows' face, from the skin (`--list-bg`): the scrim this
            // used to paint by hand, plus the field-family text roles the
            // class re-roots for a light list. See `LIST_SURFACE_CLASS`.
            ClassList::new_with_classes([LIST_SURFACE_CLASS]),
            VirtualList::new(console_row_height(font_size)),
            VirtualViewport,
            TabIndex(1),
            Pickable::default(),
            Name::new("rlv-console-transcript"),
            ChildOf(content),
        ))
        .id();
    spawn_virtual_scrollbar(commands, viewport);

    let input_row = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                ..row(Val::Px(6.0))
            },
            Name::new("rlv-console-input-row"),
            ChildOf(content),
        ))
        .id();
    commands.spawn((
        Text::new(RlvConsoleKind::Input.prefix().to_owned()),
        UiFont::Mono.at(font_size),
        text_role(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        Name::new("rlv-console-prompt"),
        ChildOf(input_row),
    ));
    let input = spawn_text_input(
        commands,
        input_row,
        &TextInputSpec {
            tab_index: 0,
            font_size,
            width_glyphs: INPUT_WIDTH_GLYPHS,
            fill: true,
            ..TextInputSpec::new("rlv-console-input", TextInputKind::Line)
        },
    );
    spawn_clear_button(commands, input_row, font_size);

    ConsoleUi { viewport, input }
}

/// The Clear button: empty the transcript.
///
/// The session is optional so a press in a host that has none (the gallery's
/// specimen) is a no-op rather than a failed observer.
fn spawn_clear_button(commands: &mut Commands, parent: Entity, font_size: f32) {
    let button = spawn_action_button(commands, parent, "rlv-console-clear", 2, font_size);
    commands.entity(button).observe(
        move |_activate: On<Activate>, session: Option<ResMut<RlvSession>>| {
            if let Some(mut session) = session {
                session.clear_console();
            }
        },
    );
}

// --- Gallery specimen -----------------------------------------------------

/// The lines the console specimen types, in order: a restriction pair that
/// lands, an unknown keyword that is refused, an extension read that is
/// answered, and a query that is accepted but unanswered — one of each thing a
/// transcript line can be.
const SPECIMEN_LINES: [&str; 4] = [
    "@detach=n,fly=n",
    "@notacommand=n",
    "@getdebug_avatarsex=2222",
    "@getoutfit=2222",
];

/// The RLVa console's gallery / `ui_test` specimen: the live content, built by
/// the same `spawn_console_content` the floater is, with a transcript made by
/// running `SPECIMEN_LINES` through the live [`run_line`] against a fresh
/// state and drawn by the live row builder and binder.
pub fn spawn_rlv_console_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let ui = spawn_console_content(commands, parent, cx.font_size);
    let mut state = RlvState::new();
    let mut ext = ViewerRlvExt {
        settings: None,
        facts: sl_viewer_world_api::rlv::RlvExtFacts {
            aspect_ratio: Some(1.5),
            avatar_is_male: Some(false),
        },
    };
    let mut environment = sl_viewer_world_api::rlv::RlvEnvironmentSlot::default();
    let mut transcript = Vec::new();
    for line in SPECIMEN_LINES {
        // Echoed first, the way `submit_console_line` logs what was typed.
        transcript.push((RlvConsoleKind::Input, line.to_owned()));
        let _run = run_line(
            &mut state,
            Uuid::nil(),
            line,
            false,
            &mut ext,
            &mut environment,
            &mut transcript,
        );
    }
    for (index, (kind, text)) in transcript.iter().enumerate() {
        let row = spawn_specimen_row(
            commands,
            ui.viewport,
            index,
            console_row_height(cx.font_size),
        );
        let holder = spawn_console_row(commands, row, cx.font_size);
        commands
            .entity(holder)
            .insert(Text::new(cx.text(&console_line_text(*kind, text))));
        let kind = *kind;
        commands
            .entity(holder)
            .entry::<ClassList>()
            .and_modify(move |mut classes| {
                for (class, of) in CONSOLE_KIND_CLASSES {
                    set_state_class(&mut classes, class, kind == of);
                }
            });
    }
    let count = transcript.len();
    commands
        .entity(ui.viewport)
        .entry::<VirtualList>()
        .and_modify(move |mut list| list.item_count = count);
    parent
}

// --- Systems --------------------------------------------------------------

/// Run a line when the input field is committed with `Enter`.
///
/// The single-line field deliberately leaves `Enter` for its consumer (the chat
/// bar relies on that too), so the console owns the key: on a press while the
/// input has focus, take the text, clear the field, and run it. The press is
/// consumed so nothing downstream sees it as a second submit.
fn submit_console_line(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Option<Res<ConsoleUi>>,
    run: crate::intake::RlvRun,
    mut fields: Query<&mut EditableText>,
    mut contexts: (ResMut<FontCx>, ResMut<LayoutCx>),
) {
    let crate::intake::RlvRun {
        identity,
        mut settings,
        facts,
        mut controls,
        mut environment,
        mut session,
    } = run;
    let Some(ui) = ui else {
        return;
    };
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    if focus.get() != Some(ui.input) {
        return;
    }
    let Ok(mut field) = fields.get_mut(ui.input) else {
        return;
    };
    let typed = field.value().to_string();
    if typed.trim().is_empty() {
        return;
    }
    // Clear the field for the next line, caret at the start — the chat bar's
    // own reset, so the two behave the same.
    field.editor_mut().set_text("");
    let mut driver = field.editor_mut().driver(&mut contexts.0, &mut contexts.1);
    driver.refresh_layout();
    driver.move_to_text_start();
    keyboard.clear_just_pressed(KeyCode::Enter);

    {
        let enabled = rlv_is_enabled(settings.as_deref());
        let hide = rlv_flag(settings.as_deref(), SETTING_DEBUG_HIDE_UNSET_DUPLICATE);
        session.log(RlvConsoleKind::Input, typed.clone());
        match classify(&typed, enabled) {
            ConsoleVerdict::Empty => {}
            ConsoleVerdict::Disabled => {
                session.log(RlvConsoleKind::Error, "RLVa is disabled");
            }
            ConsoleVerdict::NotACommand => {
                session.log(RlvConsoleKind::Error, "Invalid command");
            }
            ConsoleVerdict::Run(_commands) => {
                // The agent is the issuer, so what is typed here can be lifted
                // by closing the window — see the module documentation.
                let issuer = identity
                    .as_deref()
                    .and_then(|identity| identity.agent_id)
                    .map_or_else(Uuid::nil, |agent| agent.uuid());
                let mut ext = ViewerRlvExt {
                    settings: settings.as_deref_mut(),
                    facts: facts.as_deref().copied().unwrap_or_default(),
                };
                let mut lines = Vec::new();
                let run = run_line(
                    session.state_mut(),
                    issuer,
                    &typed,
                    hide,
                    &mut ext,
                    &mut *environment,
                    &mut lines,
                );
                for (kind, text) in lines {
                    session.log(kind, text);
                }
                if run.changed {
                    session.bump();
                }
                // `@setrot` is the one extension command that moves something:
                // the movement driver picks the heading up on its next frame.
                if let (Some(heading), Some(controls)) = (run.rotate_to, controls.as_mut()) {
                    controls.forced_heading = Some(heading);
                }
            }
        }
    }
}

/// Keep the transcript's item count in step, and pin it to the newest line.
fn size_console_list(
    session: Res<RlvSession>,
    ui: Option<Res<ConsoleUi>>,
    mut view: ResMut<ConsoleView>,
    mut lists: Query<&mut VirtualList>,
) {
    let Some(ui) = ui else {
        return;
    };
    if view.built && view.built_revision == session.console_revision() {
        return;
    }
    view.built = true;
    view.built_revision = session.console_revision();
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        let count = session.console().len();
        list.item_count = count;
        // A console reads from the bottom: a new line should be the one on
        // screen, not one the user has to scroll to. The layout pass clamps
        // this against the live viewport height, so asking for one row past the
        // end is exactly "scroll to the bottom".
        list.scroll_to_index(count);
    }
}

/// Build a freshly-pooled row's text node, and bind every row to the line it
/// now presents.
fn bind_console_rows(
    mut commands: Commands,
    session: Res<RlvSession>,
    ui: Option<Res<ConsoleUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &ConsoleRowText)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut classes: Query<&mut ClassList>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        spawn_console_row(&mut commands, row_entity, FONT_SIZE);
    }

    let refresh_all = session.is_changed();
    for (row, child_of, holder) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !refresh_all && !row.is_changed() {
            continue;
        }
        let line = row.index.and_then(|index| session.console().get(index));
        let value = line.map_or_else(String::new, |line| console_line_text(line.kind, &line.text));
        if let Ok((mut text, _color)) = texts.get_mut(holder.0)
            && text.0 != value
        {
            text.0 = value;
        }
        // What the line *is* — a reply, an accepted command, a refused one —
        // said as a class. A typed command carries none and reads as plain
        // text, which is what the parked (empty) row also wants.
        let kind = line.map(|line| line.kind);
        for (class, of) in CONSOLE_KIND_CLASSES {
            set_state_class_on(&mut classes, holder.0, class, kind == Some(of));
        }
    }
}

/// The class each kind of transcript line carries. A typed command
/// ([`RlvConsoleKind::Input`]) is not listed: it carries none and reads as
/// plain text.
const CONSOLE_KIND_CLASSES: [(&str, RlvConsoleKind); 3] = [
    (CONSOLE_REPLY_CLASS, RlvConsoleKind::Reply),
    (CONSOLE_INFO_CLASS, RlvConsoleKind::Info),
    (CONSOLE_ERROR_CLASS, RlvConsoleKind::Error),
];

/// How a transcript line is written: its stream's prefix, then its text.
fn console_line_text(kind: RlvConsoleKind, text: &str) -> String {
    format!("{}{text}", kind.prefix())
}

/// Dress a pooled transcript row: its alignment and padding, and the one text
/// node the binder writes the line into. Returns that text node, which is also
/// recorded on the row as [`ConsoleRowText`].
///
/// The line is one row high, whatever its length (the list's rows are
/// uniform), so it does not wrap: it sits in a shrinking clip container and a
/// line longer than the window loses its tail behind the shared `…` marker —
/// the inventory row's and the table cell's arrangement.
fn spawn_console_row(commands: &mut Commands, row_entity: Entity, font_size: f32) -> Entity {
    amend_row_node(commands, row_entity, |node| {
        node.align_items = AlignItems::Center;
        node.padding = UiRect::horizontal(Val::Px(4.0));
    });
    let clip = commands
        .spawn((
            Node {
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                align_items: AlignItems::Center,
                ..default()
            },
            ContentMayOverflow {
                reason: LINE_CLIP_REASON,
            },
            TextMayClip {
                reason: LINE_CLIP_REASON,
            },
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    let text = commands
        .spawn((
            Text::new(String::new()),
            TextLayout::no_wrap(),
            UiFont::Mono.at(font_size),
            ClassList::new_with_classes([TEXT_CLASS]),
            // The line keeps its full width; the clip is what shrinks, so an
            // over-long line overflows it and reveals the marker.
            Node {
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
            Name::new("rlv-console-line"),
            ChildOf(clip),
        ))
        .id();
    let marker = spawn_ellipsis_marker(
        commands,
        row_entity,
        font_size,
        LABEL_COLOR,
        DEFAULT_ELLIPSIS,
    );
    commands.entity(clip).insert(RevealEllipsis { marker });
    commands.entity(row_entity).insert(ConsoleRowText(text));
    text
}

/// Lift everything the console put in force when the window closes.
///
/// The reference does this in `onClose` and it is not a nicety: the console
/// issues as the agent, and an agent-issued restriction has no object that can
/// go away to lift it. Without this, closing the window after `@detach=n` would
/// leave the viewer restrained with nothing to take the restriction off.
fn clear_console_restrictions_on_close(
    mut view: ResMut<ConsoleView>,
    identity: Option<Res<SlIdentity>>,
    floaters: Query<(Entity, &Floater)>,
    panels: Query<&UiPanelShown>,
    mut session: ResMut<RlvSession>,
) {
    let open = floater_panel(&floaters, CONSOLE_FLOATER_ID)
        .and_then(|panel| panels.get(panel).ok())
        .is_some_and(|shown| shown.0);
    if open == view.was_open {
        return;
    }
    view.was_open = open;
    if open {
        return;
    }
    let issuer = identity
        .as_deref()
        .and_then(|identity| identity.agent_id)
        .map_or_else(Uuid::nil, |agent| agent.uuid());
    if session.state().restrictions_of(issuer).is_empty() {
        return;
    }
    session.state_mut().clear_object(issuer);
    session.bump();
}

#[cfg(test)]
mod tests {
    use super::{
        ConsoleRun, ConsoleVerdict, classify, is_unset_or_duplicate, outcome_stream,
        outcome_suffix, report_line, run_line,
    };
    use pretty_assertions::assert_eq;
    use sl_rlv::{RlvDebugSetting, RlvDebugValue, RlvExtSource as _, RlvOutcome, RlvState};
    use sl_viewer_world_api::rlv::{RlvConsoleKind, RlvEnvironmentSlot, RlvExtFacts, ViewerRlvExt};
    use uuid::Uuid;

    /// A `Box<dyn Error>` alias, so a test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// The extension source a test line is run against: the viewer's own, with
    /// no settings store and whichever facts the test supplies.
    fn ext(facts: RlvExtFacts) -> ViewerRlvExt<'static> {
        ViewerRlvExt {
            settings: None,
            facts,
        }
    }

    /// Run one line with no extension facts, which is what every test that is
    /// not about the extension family wants.
    fn run(
        state: &mut RlvState,
        issuer: Uuid,
        line: &str,
        hide_unset_duplicate: bool,
        lines: &mut Vec<(RlvConsoleKind, String)>,
    ) -> ConsoleRun {
        run_line(
            state,
            issuer,
            line,
            hide_unset_duplicate,
            &mut ext(RlvExtFacts::default()),
            &mut RlvEnvironmentSlot::default(),
            lines,
        )
    }

    /// The four things a submitted line can be.
    #[test]
    fn a_line_is_classified_before_it_is_run() {
        assert_eq!(classify("   ", true), ConsoleVerdict::Empty);
        assert_eq!(classify("@detach=n", false), ConsoleVerdict::Disabled);
        assert_eq!(classify("hello", true), ConsoleVerdict::NotACommand);
        assert_eq!(classify("@", true), ConsoleVerdict::NotACommand);
        assert_eq!(
            classify("@detach=n,fly=n", true),
            ConsoleVerdict::Run(vec!["detach=n".to_owned(), "fly=n".to_owned()])
        );
    }

    /// A line is lower-cased and its parts trimmed, the way an object's own
    /// line is, so `@Detach=N` behaves exactly as `@detach=n` does.
    #[test]
    fn a_line_is_normalised_the_way_the_wire_form_is() {
        assert_eq!(
            classify("@Detach=N , Fly=N ", true),
            ConsoleVerdict::Run(vec!["detach=n".to_owned(), "fly=n".to_owned()])
        );
    }

    /// The shortest real command is accepted — the reference's length check
    /// would have been off by one for it.
    #[test]
    fn the_shortest_real_command_is_accepted() {
        assert_eq!(
            classify("@fly=n", true),
            ConsoleVerdict::Run(vec!["fly=n".to_owned()])
        );
    }

    /// Each outcome reports on the stream the reference puts it on.
    #[test]
    fn an_outcome_reports_on_its_own_stream() {
        assert_eq!(outcome_stream(RlvOutcome::Success), RlvConsoleKind::Info);
        assert_eq!(
            outcome_stream(RlvOutcome::SuccessDuplicate),
            RlvConsoleKind::Info
        );
        assert_eq!(
            outcome_stream(RlvOutcome::FailedParam),
            RlvConsoleKind::Error
        );
        assert_eq!(
            outcome_stream(RlvOutcome::FailedOption),
            RlvConsoleKind::Error
        );
    }

    /// A plain success needs no explaining; everything else says what happened.
    #[test]
    fn only_a_plain_success_reports_without_a_reason() {
        assert_eq!(report_line("detach=n", RlvOutcome::Success), "@detach=n");
        assert_eq!(
            report_line("detach=n", RlvOutcome::SuccessDuplicate),
            "@detach=n (already set)"
        );
        assert!(outcome_suffix(RlvOutcome::Success).is_none());
        assert!(outcome_suffix(RlvOutcome::FailedParam).is_some());
    }

    /// A keyword the *user* turned off does not read as one the viewer never
    /// had: the console is where they find out it was their own doing.
    #[test]
    fn a_disabled_keyword_says_whose_doing_it_was() {
        assert_eq!(
            report_line("setenv=n", RlvOutcome::FailedDisabled),
            "@setenv=n (turned off in your settings)"
        );
        assert_eq!(
            outcome_stream(RlvOutcome::FailedDisabled),
            RlvConsoleKind::Error
        );
    }

    /// Running a real line applies it and reports one line per command.
    #[test]
    fn a_run_line_applies_and_reports_each_command() -> Result<(), TestError> {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let run = run(&mut state, agent, "@detach=n,fly=n", false, &mut lines);

        assert!(run.changed);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines.iter().all(|(kind, _)| *kind == RlvConsoleKind::Info));
        // Both are held by the agent. `@fly` is reference-counted, so it also
        // reads as in force globally; `@detach` deliberately is not — "may this
        // come off" is the lock model's question, not a global yes/no — so the
        // held set is what proves it landed.
        assert!(state.has_behaviour(sl_rlv::RlvBehaviour::Fly));
        let held: Vec<String> = state
            .restrictions_of(agent)
            .iter()
            .map(sl_rlv::RlvHeldCommand::as_string)
            .collect();
        assert_eq!(held, vec!["detach".to_owned(), "fly".to_owned()]);
        Ok(())
    }

    /// A keyword the dictionary does not know is refused, and says so.
    #[test]
    fn an_unknown_command_is_refused() -> Result<(), TestError> {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let run = run(&mut state, agent, "@notacommand=n", false, &mut lines);

        assert!(!run.changed);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(
            lines.first().ok_or("no report line")?.0,
            RlvConsoleKind::Error
        );
        Ok(())
    }

    /// The hide-unset-duplicate setting drops exactly the two outcomes it
    /// names, and nothing else.
    #[test]
    fn hiding_unset_duplicates_drops_only_those() {
        assert!(is_unset_or_duplicate(RlvOutcome::SuccessDuplicate));
        assert!(is_unset_or_duplicate(RlvOutcome::SuccessUnset));
        assert!(!is_unset_or_duplicate(RlvOutcome::Success));
        assert!(!is_unset_or_duplicate(RlvOutcome::FailedParam));

        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let _first = run(&mut state, agent, "@detach=n", true, &mut lines);
        let before = lines.len();
        let _duplicate = run(&mut state, agent, "@detach=n", true, &mut lines);
        assert_eq!(
            lines.len(),
            before,
            "the duplicate should have been hidden: {lines:?}"
        );
    }

    /// A query is accepted but reported as unanswered, so the person typing it
    /// is told rather than left waiting for a reply that never comes.
    #[test]
    fn a_query_says_it_is_not_answered_yet() {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let _run = run(&mut state, agent, "@getoutfit=2222", false, &mut lines);

        assert!(
            lines.iter().any(|(kind, _)| *kind == RlvConsoleKind::Reply),
            "{lines:?}"
        );
    }

    /// An extension read *is* answered, on the reply stream, with the channel
    /// it would have been shouted on — the console is the only place today
    /// where a `@getdebug_*` gets a real answer.
    #[test]
    fn an_extension_read_is_answered() -> Result<(), TestError> {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let mut source = ext(RlvExtFacts {
            aspect_ratio: Some(1.5),
            avatar_is_male: Some(true),
        });
        let run = run_line(
            &mut state,
            agent,
            "@getdebug_avatarsex=2222",
            false,
            &mut source,
            &mut RlvEnvironmentSlot::default(),
            &mut lines,
        );

        assert!(
            !run.changed,
            "a read changes no restriction, so it must not wake the floaters \
             watching the revision"
        );
        let reply = lines
            .iter()
            .find(|(kind, _)| *kind == RlvConsoleKind::Reply)
            .ok_or("no reply line")?;
        assert_eq!(reply.1, "2222: 1");
        // The report line itself is a plain success, not "unknown command".
        assert_eq!(
            lines.first().ok_or("no report line")?.0,
            RlvConsoleKind::Info,
            "{lines:?}"
        );
        Ok(())
    }

    /// With no settings store behind it — which is every test here, and any
    /// harness app that never built one — the writable row reads as "not
    /// reduced" and the write is refused rather than silently swallowed.
    ///
    /// The store-backed half of this row (the write lands, and the value a
    /// script wrote is kept out of the user's settings file) is pinned where
    /// the source lives, in `sl_viewer_world_api::rlv`.
    #[test]
    fn a_write_with_no_settings_store_is_refused() -> Result<(), TestError> {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let mut source = ext(RlvExtFacts::default());
        assert_eq!(
            source.debug_value(RlvDebugSetting::RenderResolutionDivisor),
            Some(RlvDebugValue::U32(1))
        );
        assert_eq!(
            source.debug_value(RlvDebugSetting::WindLightUseAtmosShaders),
            Some(RlvDebugValue::Bool(true)),
            "this viewer always renders the atmospheric sky"
        );
        let _run = run_line(
            &mut state,
            agent,
            "@setdebug_renderresolutiondivisor:4=force",
            false,
            &mut source,
            &mut RlvEnvironmentSlot::default(),
            &mut lines,
        );
        assert_eq!(
            lines.first().ok_or("no report line")?.0,
            RlvConsoleKind::Error,
            "{lines:?}"
        );
        Ok(())
    }

    /// `@setrot` comes back as a heading for the caller to hand to the movement
    /// driver, rather than being applied here.
    #[test]
    fn setrot_hands_back_a_heading() {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let run = run(&mut state, agent, "@setrot:0=force", false, &mut lines);
        assert_eq!(run.rotate_to, Some(sl_rlv::SETROT_OFFSET));
        assert!(!run.changed, "turning the avatar is not a restriction");
        assert_eq!(
            lines.first().map(|(kind, _)| *kind),
            Some(RlvConsoleKind::Info),
            "{lines:?}"
        );
    }

    /// A *read* on a channel no reply may go on says so; `@setrot` written as a
    /// query still turns the avatar and stays quiet, because it is not a read
    /// and never had an answer to lose.
    #[test]
    fn only_a_read_complains_about_the_channel() {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();

        let mut read = Vec::new();
        let _run = run(&mut state, agent, "@getdebug_avatarsex=0", false, &mut read);
        assert!(
            read.iter()
                .any(|(kind, text)| *kind == RlvConsoleKind::Error && text.contains("channel")),
            "{read:?}"
        );

        let mut turn = Vec::new();
        let run = run(&mut state, agent, "@setrot:0=2222", false, &mut turn);
        assert_eq!(run.rotate_to, Some(sl_rlv::SETROT_OFFSET));
        assert!(
            turn.iter().all(|(kind, _)| *kind == RlvConsoleKind::Info),
            "{turn:?}"
        );
    }
}
