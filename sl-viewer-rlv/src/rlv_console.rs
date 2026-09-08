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
use sl_client_bevy::SlIdentity;
use sl_rlv::{
    RLV_PREFIX, RlvCommand, RlvEnvCommand, RlvEnvSource, RlvExtCommand, RlvExtSource, RlvOutcome,
    RlvParam, RlvReply, RlvState, parse_chat_line,
};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::Translated;
use sl_viewer_ui_core::ui::UiPanelShown;
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, amend_row_node, layout_virtual_lists,
    spawn_virtual_scrollbar,
};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_panel,
    floater_shown, spawn_floater,
};
use sl_viewer_ui_widgets::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use sl_viewer_world_api::AvatarControls;
use sl_viewer_world_api::rlv::{
    RlvConsoleKind, RlvEnvironmentSlot, RlvExtFacts, RlvSession,
    SETTING_DEBUG_HIDE_UNSET_DUPLICATE, ViewerRlvExt, rlv_flag, rlv_is_enabled,
};
use uuid::Uuid;

use crate::style::{
    ACTION_BACKGROUND, DIM_LABEL_COLOR, ERROR_COLOR, FONT_SIZE, INFO_COLOR, LABEL_COLOR,
    LIST_BACKGROUND, ROW_HEIGHT,
};

/// The floater's stable id.
pub const CONSOLE_FLOATER_ID: &str = "rlv-console";

/// The width of the input field, in `"0"` advances. It fills its row, so this
/// is only the intrinsic width the fill overrides.
const INPUT_WIDTH_GLYPHS: f32 = 48.0;

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
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(4.0))
            },
            Name::new("rlv-console-content"),
            ChildOf(handle.content),
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
            BackgroundColor(LIST_BACKGROUND),
            VirtualList::new(ROW_HEIGHT),
            VirtualViewport,
            TabIndex(1),
            Pickable::default(),
            Name::new("rlv-console-transcript"),
            ChildOf(content),
        ))
        .id();
    spawn_virtual_scrollbar(&mut commands, viewport);

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
        UiFont::Mono.at(FONT_SIZE),
        TextColor(DIM_LABEL_COLOR),
        Pickable::IGNORE,
        Name::new("rlv-console-prompt"),
        ChildOf(input_row),
    ));
    let input = spawn_text_input(
        &mut commands,
        input_row,
        &TextInputSpec {
            tab_index: 0,
            font_size: FONT_SIZE,
            width_glyphs: INPUT_WIDTH_GLYPHS,
            fill: true,
            ..TextInputSpec::new("rlv-console-input", TextInputKind::Line)
        },
    );
    spawn_clear_button(&mut commands, input_row);

    commands.insert_resource(ConsoleUi { viewport, input });
}

/// The Clear button: empty the transcript.
fn spawn_clear_button(commands: &mut Commands, parent: Entity) {
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
            Name::new("rlv-console-clear"),
            ChildOf(parent),
        ))
        .with_child((
            Text::new(String::new()),
            UiFont::Sans.at(FONT_SIZE),
            TextColor(LABEL_COLOR),
            Translated::new("rlv-console-clear"),
            Pickable::IGNORE,
        ))
        .observe(
            move |mut press: On<Pointer<Press>>, mut session: ResMut<RlvSession>| {
                press.propagate(false);
                if press.button != PointerButton::Primary {
                    return;
                }
                session.clear_console();
            },
        );
}

// --- Systems --------------------------------------------------------------

/// Run a line when the input field is committed with `Enter`.
///
/// The single-line field deliberately leaves `Enter` for its consumer (the chat
/// bar relies on that too), so the console owns the key: on a press while the
/// input has focus, take the text, clear the field, and run it. The press is
/// consumed so nothing downstream sees it as a second submit.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the keyboard and \
              focus that decide a submit happened, the UI handles, the identity and settings the \
              run needs, the two facts the debug-setting allowlist reads, the movement controls a \
              forced rotation writes, the environment seam the sky family writes through, the \
              session it writes to, and the field plus the two text contexts clearing it requires"
)]
fn submit_console_line(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Option<Res<ConsoleUi>>,
    identity: Option<Res<SlIdentity>>,
    settings: Option<Res<ViewerSettings>>,
    facts: Option<Res<RlvExtFacts>>,
    mut controls: Option<ResMut<AvatarControls>>,
    mut environment: ResMut<RlvEnvironmentSlot>,
    mut session: ResMut<RlvSession>,
    mut fields: Query<&mut EditableText>,
    mut contexts: (ResMut<FontCx>, ResMut<LayoutCx>),
) {
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
                    settings: settings.as_deref(),
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
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        amend_row_node(&mut commands, row_entity, |node| {
            node.align_items = AlignItems::Center;
            node.padding = UiRect::horizontal(Val::Px(4.0));
        });
        let text = commands
            .spawn((
                Text::new(String::new()),
                UiFont::Mono.at(FONT_SIZE),
                TextColor(LABEL_COLOR),
                Pickable::IGNORE,
                Name::new("rlv-console-line"),
                ChildOf(row_entity),
            ))
            .id();
        commands.entity(row_entity).insert(ConsoleRowText(text));
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
        let (value, color) = line.map_or_else(
            || (String::new(), LABEL_COLOR),
            |line| {
                (
                    format!("{}{}", line.kind.prefix(), line.text),
                    line_color(line.kind),
                )
            },
        );
        if let Ok((mut text, mut text_color)) = texts.get_mut(holder.0) {
            if text.0 != value {
                text.0 = value;
            }
            if text_color.0 != color {
                text_color.0 = color;
            }
        }
    }
}

/// The colour each console stream is written in.
const fn line_color(kind: RlvConsoleKind) -> Color {
    match kind {
        RlvConsoleKind::Input => LABEL_COLOR,
        RlvConsoleKind::Info => INFO_COLOR,
        RlvConsoleKind::Error => ERROR_COLOR,
        RlvConsoleKind::Reply => DIM_LABEL_COLOR,
    }
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

    /// This viewer has no `RenderResolutionDivisor`, so the write it is asked
    /// for is refused rather than silently swallowed.
    #[test]
    fn a_write_this_viewer_cannot_do_is_refused() -> Result<(), TestError> {
        let agent = Uuid::from_u128(1);
        let mut state = RlvState::new();
        let mut lines = Vec::new();
        let mut source = ext(RlvExtFacts::default());
        assert_eq!(
            source.debug_value(RlvDebugSetting::RenderResolutionDivisor),
            None
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
