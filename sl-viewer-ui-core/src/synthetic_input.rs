//! **Synthetic input** for a running app: real input, queued, one step a frame.
//!
//! The viewer's input readers are many and varied — `bevy_picking`'s pointer,
//! the widget state machines, `ButtonInput<KeyCode>` for the world bindings,
//! `Window::cursor_position()` for the GPU pick and the camera, the focused
//! text field's editor. A driver that wants to be honest about all of them
//! writes what winit writes live: the typed `bevy_input` / `bevy_window`
//! messages **plus their [`WindowEvent`] wrappers**, from which Bevy's picking
//! input plugin derives `PointerInput` exactly as it does under a real window.
//! One source of truth, so the readers can never disagree.
//!
//! The testkit's pointer and keyboard driver was the first to do this, as an
//! `&mut App` API that stepped the frames itself. That only works where the
//! caller owns the loop; a live viewer, or one behind a remote transport, runs
//! its own. So the writing lives here, as a queue a system drains:
//!
//! - an [`InputAction`] is a list of [`InputStep`]s, built from the gesture
//!   constructors ([`InputAction::click`], [`InputAction::type_text`],
//!   [`InputAction::chord`], [`InputAction::hold_key`], …) and joined with
//!   [`InputAction::then`];
//! - [`SyntheticInput::enqueue`] queues one and returns its [`InputActionId`];
//! - the injector applies **one step per frame**, in [`First`] (after the
//!   message buffers swap and before picking's input systems, which run in
//!   `First` too, turn window events into pointer events), and in [`Last`]
//!   marks an action whose steps are spent as done **at that frame**, which
//!   [`SyntheticInput::status`] reports.
//!
//! # The frame protocol
//!
//! Picking hits use the **previous** frame's layout (`ui_picking` runs in
//! `PreUpdate` against the `ComputedNode`s of the last `PostUpdate`), and the
//! widget state machines read `just_pressed` one frame and `pressed` the next.
//! So every pointer step is its own frame: a click is move, press, release and
//! one settling frame for the widgets' observers — four frames — and a drag
//! steps the cursor one frame at a time. A click also pins the multi-click
//! interval to zero from its first frame to the end of its last, because the
//! click counter is wall-clock and two synthetic clicks in consecutive frames
//! would otherwise read as a double click. A double click pins it the other
//! way, to forever, for the same reason turned round: its two presses are two
//! frames apart, and on a machine busy enough to draw five frames a second
//! that is longer than any interval a double click is allowed. Actions run one
//! after another, never interleaved, so an action's frames are contiguous.

use std::collections::{BTreeMap, VecDeque};

use bevy::ecs::message::MessageUpdateSystems;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::picking::{PickingSettings, PickingSystems};
use bevy::prelude::*;
use bevy::time::TimeSystems;
use bevy::window::{PrimaryWindow, WindowEvent};

/// How many finished actions keep their completion frame. Older ones report
/// [`ActionStatus::Expired`]: an agent driving a viewer for hours must not
/// grow this without bound, and nothing waits on an action that long gone.
const REMEMBERED_COMPLETIONS: usize = 1024;

/// One frame's worth of input: what the injector writes in a single frame.
#[derive(Debug, Clone, PartialEq)]
pub enum InputStep {
    /// Move the pointer to this point, in logical pixels: the window's cursor,
    /// the typed `CursorMoved` and — from the second move on — `MouseMotion`.
    Move(Vec2),
    /// Press a mouse button where the pointer is.
    Press(MouseButton),
    /// Release a mouse button where the pointer is.
    Release(MouseButton),
    /// Scroll by these lines where the pointer is (vertical positive = away
    /// from the user).
    Wheel(Vec2),
    /// Raw relative mouse motion with **no** cursor move — what mouselook
    /// reads.
    Motion(Vec2),
    /// A key goes down.
    KeyDown {
        /// The physical key, which `ButtonInput<KeyCode>` (and so every world
        /// binding) reads.
        key_code: KeyCode,
        /// Its logical meaning, which the text editor and its modifier flags
        /// read.
        logical: Key,
        /// The text it produces, if any — what a text field inserts.
        text: Option<String>,
    },
    /// A key comes up.
    KeyUp {
        /// The physical key.
        key_code: KeyCode,
        /// Its logical meaning.
        logical: Key,
    },
    /// An IME event for the focused field.
    Ime(ImeStep),
    /// A frame in which nothing is written: a settle, or a held key held.
    Idle,
}

/// The IME events a step can deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeStep {
    /// The platform IME has become active for the focused field.
    Enable,
    /// The composing text the candidate window shows, with an optional
    /// `(anchor, focus)` cursor range in it. Not part of the field's value
    /// until it is committed.
    Preedit {
        /// The composing text.
        value: String,
        /// The cursor range within it, in bytes.
        cursor: Option<(usize, usize)>,
    },
    /// The composition the user accepted: the point the text enters the
    /// buffer.
    Commit(String),
    /// The platform IME was force-disabled, cancelling any composition.
    Disable,
}

/// The multi-click interval a synthetic double click runs under: long enough
/// that no frame time separates its presses.
pub const FOREVER: core::time::Duration = core::time::Duration::MAX;

/// A gesture: the steps it takes, one per frame, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputAction {
    /// The steps, first first.
    steps: Vec<InputStep>,
    /// What the multi-click interval is pinned to while it runs: zero for
    /// clicks that must stay singles, [`FOREVER`] for ones that must count as
    /// one multi-click however long the frames between them took. `None`
    /// leaves the app's own.
    pinned_interval: Option<core::time::Duration>,
}

impl InputAction {
    /// An action of exactly these steps.
    #[must_use]
    pub const fn from_steps(steps: Vec<InputStep>) -> Self {
        Self {
            steps,
            pinned_interval: None,
        }
    }

    /// The steps it takes, one per frame.
    #[must_use]
    pub fn steps(&self) -> &[InputStep] {
        &self.steps
    }

    /// How many frames it takes.
    #[must_use]
    pub const fn frames(&self) -> usize {
        self.steps.len()
    }

    /// This action, then `next` — one action, so nothing is queued between
    /// them. A pin of the multi-click interval holds for the whole if either
    /// half asked for one, the first half's if both did.
    #[must_use]
    pub fn then(mut self, next: Self) -> Self {
        self.steps.extend(next.steps);
        self.pinned_interval = self.pinned_interval.or(next.pinned_interval);
        self
    }

    /// Move the pointer to `at` (logical pixels).
    #[must_use]
    pub fn move_to(at: Vec2) -> Self {
        Self::from_steps(vec![InputStep::Move(at)])
    }

    /// Press `button` where the pointer is.
    #[must_use]
    pub fn press(button: MouseButton) -> Self {
        Self::from_steps(vec![InputStep::Press(button)])
    }

    /// Release `button` where the pointer is.
    #[must_use]
    pub fn release(button: MouseButton) -> Self {
        Self::from_steps(vec![InputStep::Release(button)])
    }

    /// One single click at `at`: move, press, release, settle — with the
    /// multi-click interval pinned to zero, so two clicks in consecutive
    /// frames are two singles, not a double.
    #[must_use]
    pub fn click(at: Vec2, button: MouseButton) -> Self {
        Self {
            steps: vec![
                InputStep::Move(at),
                InputStep::Press(button),
                InputStep::Release(button),
                InputStep::Idle,
            ],
            pinned_interval: Some(core::time::Duration::ZERO),
        }
    }

    /// Two clicks at `at` that count as one double click — the second
    /// carries `count == 2`, which is what the widgets read — with the
    /// multi-click interval pinned to [`FOREVER`], so they still do when the
    /// two frames between the presses took longer than the app's interval.
    #[must_use]
    pub fn double_click(at: Vec2, button: MouseButton) -> Self {
        Self {
            steps: vec![
                InputStep::Move(at),
                InputStep::Press(button),
                InputStep::Release(button),
                InputStep::Press(button),
                InputStep::Release(button),
                InputStep::Idle,
            ],
            pinned_interval: Some(FOREVER),
        }
    }

    /// Press at `from`, step the pointer to `to` across `steps` frames (at
    /// least one), release, settle — the shape every drag reader (title bars,
    /// gizmos, sliders) consumes.
    #[must_use]
    pub fn drag(from: Vec2, to: Vec2, steps: u32, button: MouseButton) -> Self {
        let count = steps.max(1);
        let mut all = vec![InputStep::Move(from), InputStep::Press(button)];
        all.extend((1..=count).map(|step| InputStep::Move(from.lerp(to, fraction(step, count)))));
        all.push(InputStep::Release(button));
        all.push(InputStep::Idle);
        Self::from_steps(all)
    }

    /// Scroll `lines` at `at`: move there, then the wheel.
    #[must_use]
    pub fn scroll(at: Vec2, lines: Vec2) -> Self {
        Self::from_steps(vec![InputStep::Move(at), InputStep::Wheel(lines)])
    }

    /// One frame of raw relative motion with no cursor move.
    #[must_use]
    pub fn mouse_motion(delta: Vec2) -> Self {
        Self::from_steps(vec![InputStep::Motion(delta)])
    }

    /// Mouselook: `total` raw motion spread evenly over `frames` frames (at
    /// least one), the way a hand moves the mouse rather than a teleport of
    /// the view.
    #[must_use]
    pub fn mouse_look(total: Vec2, frames: u32) -> Self {
        let count = frames.max(1);
        let share = fraction(1, count);
        let per_frame = Vec2::new(total.x * share, total.y * share);
        Self::from_steps(
            core::iter::repeat_n(InputStep::Motion(per_frame), usize_from(count)).collect(),
        )
    }

    /// `key_code` goes down, with its logical meaning and optional text.
    #[must_use]
    pub fn key_down(key_code: KeyCode, logical: Key, text: Option<&str>) -> Self {
        Self::from_steps(vec![InputStep::KeyDown {
            key_code,
            logical,
            text: text.map(str::to_owned),
        }])
    }

    /// `key_code` comes up.
    #[must_use]
    pub fn key_up(key_code: KeyCode, logical: Key) -> Self {
        Self::from_steps(vec![InputStep::KeyUp { key_code, logical }])
    }

    /// Tap a key: down, up — two frames.
    #[must_use]
    pub fn tap(key_code: KeyCode, logical: Key) -> Self {
        Self::key_down(key_code, logical.clone(), None).then(Self::key_up(key_code, logical))
    }

    /// Hold a key down for `frames` frames (at least one) and let it go — a
    /// held movement key walks, flies or turns for that long. The key reads as
    /// pressed on each of those frames and released on the one after.
    #[must_use]
    pub fn hold_key(key_code: KeyCode, logical: Key, frames: u32) -> Self {
        let held = usize_from(frames.max(1)).saturating_sub(1);
        let mut all = vec![InputStep::KeyDown {
            key_code,
            logical: logical.clone(),
            text: None,
        }];
        all.extend(core::iter::repeat_n(InputStep::Idle, held));
        all.push(InputStep::KeyUp { key_code, logical });
        Self::from_steps(all)
    }

    /// A chord — a menu accelerator like `Ctrl+Shift+S`: each modifier down in
    /// order, the key tapped, the modifiers up in reverse. The logical keys are
    /// what the text editor's modifier flags read; the physical ones what the
    /// viewer's binding profiles read, so both are written.
    #[must_use]
    pub fn chord(modifiers: &[(KeyCode, Key)], key_code: KeyCode, logical: Key) -> Self {
        let mut all: Vec<InputStep> = modifiers
            .iter()
            .map(|(code, key)| InputStep::KeyDown {
                key_code: *code,
                logical: key.clone(),
                text: None,
            })
            .collect();
        all.extend(Self::tap(key_code, logical).steps);
        all.extend(modifiers.iter().rev().map(|(code, key)| InputStep::KeyUp {
            key_code: *code,
            logical: key.clone(),
        }));
        Self::from_steps(all)
    }

    /// Type `text`, one character key per frame pair, as an IME-less keyboard
    /// delivers it: the logical key and its text (what a text field inserts)
    /// over the **physical** key that carries the character on a US layout
    /// (what `ButtonInput<KeyCode>` — and so every world key binding — reads).
    ///
    /// Both halves matter and they are read by different code. A driver that
    /// typed every character on one placeholder key would drive the field
    /// perfectly while silently telling the world that no letter was ever
    /// pressed — which is precisely the coincidence a focus-routing test exists
    /// to rule out. A character with no US-layout key ([`key_code_for`] returns
    /// `None`) is typed on [`KeyCode::F35`], a key no profile binds, so the
    /// logical half still works.
    #[must_use]
    pub fn type_text(text: &str) -> Self {
        let steps = text
            .chars()
            .flat_map(|character| {
                let typed = character.to_string();
                let logical = Key::Character(typed.as_str().into());
                let key_code = key_code_for(character).unwrap_or(KeyCode::F35);
                [
                    InputStep::KeyDown {
                        key_code,
                        logical: logical.clone(),
                        text: Some(typed),
                    },
                    InputStep::KeyUp { key_code, logical },
                ]
            })
            .collect();
        Self::from_steps(steps)
    }

    /// One IME event.
    #[must_use]
    pub fn ime(step: ImeStep) -> Self {
        Self::from_steps(vec![InputStep::Ime(step)])
    }
}

/// `numerator / denominator` as an `f32`, for the interpolation steps. Both are
/// small frame counts; one past `u16` saturates rather than casting.
fn fraction(numerator: u32, denominator: u32) -> f32 {
    f32::from(u16::try_from(numerator).unwrap_or(u16::MAX))
        / f32::from(u16::try_from(denominator).unwrap_or(u16::MAX))
}

/// A frame count as a `usize`, saturating on a platform too narrow for it.
fn usize_from(count: u32) -> usize {
    usize::try_from(count).unwrap_or(usize::MAX)
}

/// The physical key a US-layout keyboard puts `character` on, or `None` when
/// this table has no entry for it.
///
/// Deliberately shallow: the letters, the digit row, and the punctuation the
/// viewer's own fields actually take (a numeric field's `-` and `.`, a chat
/// bar's space and comma). It is a *physical* mapping, so an upper-case letter
/// resolves to the same key as its lower-case twin — the shift state a real
/// keyboard would also be holding is the caller's to press if it matters.
#[must_use]
pub const fn key_code_for(character: char) -> Option<KeyCode> {
    let key = match character.to_ascii_lowercase() {
        'a' => KeyCode::KeyA,
        'b' => KeyCode::KeyB,
        'c' => KeyCode::KeyC,
        'd' => KeyCode::KeyD,
        'e' => KeyCode::KeyE,
        'f' => KeyCode::KeyF,
        'g' => KeyCode::KeyG,
        'h' => KeyCode::KeyH,
        'i' => KeyCode::KeyI,
        'j' => KeyCode::KeyJ,
        'k' => KeyCode::KeyK,
        'l' => KeyCode::KeyL,
        'm' => KeyCode::KeyM,
        'n' => KeyCode::KeyN,
        'o' => KeyCode::KeyO,
        'p' => KeyCode::KeyP,
        'q' => KeyCode::KeyQ,
        'r' => KeyCode::KeyR,
        's' => KeyCode::KeyS,
        't' => KeyCode::KeyT,
        'u' => KeyCode::KeyU,
        'v' => KeyCode::KeyV,
        'w' => KeyCode::KeyW,
        'x' => KeyCode::KeyX,
        'y' => KeyCode::KeyY,
        'z' => KeyCode::KeyZ,
        '0' => KeyCode::Digit0,
        '1' => KeyCode::Digit1,
        '2' => KeyCode::Digit2,
        '3' => KeyCode::Digit3,
        '4' => KeyCode::Digit4,
        '5' => KeyCode::Digit5,
        '6' => KeyCode::Digit6,
        '7' => KeyCode::Digit7,
        '8' => KeyCode::Digit8,
        '9' => KeyCode::Digit9,
        ' ' => KeyCode::Space,
        '-' => KeyCode::Minus,
        '.' => KeyCode::Period,
        ',' => KeyCode::Comma,
        '/' => KeyCode::Slash,
        _other => return None,
    };
    Some(key)
}

/// Which queued action this is, in the order they were queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputActionId(u64);

/// Where a queued action is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStatus {
    /// Waiting behind the actions queued before it.
    Queued,
    /// Its steps are being applied.
    Running,
    /// Its last step ran in this injector frame (see
    /// [`SyntheticInput::frame`]), and the frame has finished.
    Done {
        /// The injector frame of its last step.
        frame: u64,
    },
    /// Done so long ago that its frame is forgotten.
    Expired,
    /// No action was ever queued under this id — one from another app.
    Unknown,
}

/// The action being applied.
#[derive(Debug)]
struct Running {
    /// Its id.
    id: InputActionId,
    /// The steps still to apply, next first.
    steps: VecDeque<InputStep>,
    /// The app's multi-click interval before the action pinned it, to put
    /// back when it is done. `None` when it pins nothing, or the app has no
    /// picking settings.
    saved_interval: Option<core::time::Duration>,
}

/// The queue of synthetic input, and what became of each action.
#[derive(Resource, Debug, Default)]
pub struct SyntheticInput {
    /// Actions waiting to start, first first.
    queue: VecDeque<(InputActionId, InputAction)>,
    /// The action being applied, if any.
    running: Option<Running>,
    /// The id the next queued action takes.
    next_id: u64,
    /// The injector's frame counter: how many frames it has run, counting the
    /// current one.
    frame: u64,
    /// The frame each recently finished action finished in.
    done: BTreeMap<InputActionId, u64>,
}

impl SyntheticInput {
    /// Queue `action` behind whatever is already queued.
    pub fn enqueue(&mut self, action: InputAction) -> InputActionId {
        let id = InputActionId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.queue.push_back((id, action));
        id
    }

    /// Where `id` is.
    #[must_use]
    pub fn status(&self, id: InputActionId) -> ActionStatus {
        if let Some(&frame) = self.done.get(&id) {
            return ActionStatus::Done { frame };
        }
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.id == id)
        {
            return ActionStatus::Running;
        }
        if self.queue.iter().any(|(queued, _action)| *queued == id) {
            return ActionStatus::Queued;
        }
        if id.0 < self.next_id {
            ActionStatus::Expired
        } else {
            ActionStatus::Unknown
        }
    }

    /// Whether nothing is queued or running.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.running.is_none() && self.queue.is_empty()
    }

    /// The injector's frame counter: the number of frames it has run, the
    /// current one included. What [`ActionStatus::Done`] counts in.
    #[must_use]
    pub const fn frame(&self) -> u64 {
        self.frame
    }

    /// The step to apply this frame, starting the next queued action when none
    /// is running, and — when that action has just started — the multi-click
    /// interval it asks to be pinned to. Actions with no steps are finished on
    /// the way past. `None` when there is nothing to apply.
    fn advance(&mut self) -> Option<(InputStep, Option<core::time::Duration>)> {
        let mut started_pinned = None;
        loop {
            if let Some(running) = self.running.as_mut() {
                return running.steps.pop_front().map(|step| (step, started_pinned));
            }
            let (id, action) = self.queue.pop_front()?;
            if action.steps.is_empty() {
                self.finish(id);
                continue;
            }
            started_pinned = action.pinned_interval;
            self.running = Some(Running {
                id,
                steps: action.steps.into(),
                saved_interval: None,
            });
        }
    }

    /// Record `id` as done in the current frame, forgetting the oldest record
    /// when there are too many.
    fn finish(&mut self, id: InputActionId) {
        self.done.insert(id, self.frame);
        while self.done.len() > REMEMBERED_COMPLETIONS {
            self.done.pop_first();
        }
    }
}

/// The injector: [`SyntheticInput`] and the two systems that drain it.
///
/// Needs the window plugin (for the primary window the messages are stamped
/// with) and the input plugin (for the typed message types); picking is
/// optional — without it the multi-click pin is a no-op.
#[derive(Debug, Default, Clone, Copy)]
pub struct SyntheticInputPlugin;

impl Plugin for SyntheticInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SyntheticInput>()
            .add_systems(
                First,
                // Picking turns window events into pointer events in `First`
                // too: written after it, a press would reach the pointer a
                // frame late, and a held button would step once more after its
                // release.
                apply_synthetic_input
                    .after(MessageUpdateSystems)
                    .after(TimeSystems)
                    .before(PickingSystems::Input),
            )
            .add_systems(Last, finish_synthetic_input);
    }
}

/// Apply this frame's step: start the next action if none is running (pinning
/// the multi-click interval if it asks), then write its next step's messages.
///
/// An action with no steps finishes in the frame it is reached, without taking
/// the frame from the action behind it.
fn apply_synthetic_input(world: &mut World) {
    let Some(mut input) = world.get_resource_mut::<SyntheticInput>() else {
        return;
    };
    input.frame = input.frame.saturating_add(1);
    let Some((step, pin)) = input.advance() else {
        return;
    };
    if let Some(interval) = pin {
        let saved = world
            .get_resource_mut::<PickingSettings>()
            .map(|mut settings| core::mem::replace(&mut settings.multi_click_interval, interval));
        if let Some(running) = world.resource_mut::<SyntheticInput>().running.as_mut() {
            running.saved_interval = saved;
        }
    }
    write_step(world, step);
}

/// End the frame: an action whose steps are spent is done, in this frame, and
/// gives back the multi-click interval it pinned.
fn finish_synthetic_input(world: &mut World) {
    let Some(mut input) = world.get_resource_mut::<SyntheticInput>() else {
        return;
    };
    if !input
        .running
        .as_ref()
        .is_some_and(|running| running.steps.is_empty())
    {
        return;
    }
    let Some(running) = input.running.take() else {
        return;
    };
    input.finish(running.id);
    if let (Some(interval), Some(mut settings)) = (
        running.saved_interval,
        world.get_resource_mut::<PickingSettings>(),
    ) {
        settings.multi_click_interval = interval;
    }
}

/// The primary window, which every message is stamped with.
/// [`Entity::PLACEHOLDER`] when the app has none.
fn primary_window(world: &mut World) -> Entity {
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    windows.single(world).unwrap_or(Entity::PLACEHOLDER)
}

/// Write one step's messages: each typed message plus its [`WindowEvent`]
/// wrapper, the shape winit writes.
fn write_step(world: &mut World, step: InputStep) {
    let window = primary_window(world);
    match step {
        InputStep::Move(at) => write_move(world, window, at),
        InputStep::Press(button) => write_button(world, window, button, ButtonState::Pressed),
        InputStep::Release(button) => write_button(world, window, button, ButtonState::Released),
        InputStep::Wheel(lines) => {
            let wheel = MouseWheel {
                unit: MouseScrollUnit::Line,
                x: lines.x,
                y: lines.y,
                window,
                phase: TouchPhase::Moved,
            };
            world.write_message(wheel);
            world.write_message(WindowEvent::MouseWheel(wheel));
        }
        InputStep::Motion(delta) => write_motion(world, delta),
        InputStep::KeyDown {
            key_code,
            logical,
            text,
        } => write_key(world, window, key_code, logical, text, ButtonState::Pressed),
        InputStep::KeyUp { key_code, logical } => {
            write_key(
                world,
                window,
                key_code,
                logical,
                None,
                ButtonState::Released,
            );
        }
        InputStep::Ime(step) => {
            let ime = match step {
                ImeStep::Enable => Ime::Enabled { window },
                ImeStep::Preedit { value, cursor } => Ime::Preedit {
                    window,
                    value,
                    cursor,
                },
                ImeStep::Commit(value) => Ime::Commit { window, value },
                ImeStep::Disable => Ime::Disabled { window },
            };
            world.write_message(ime.clone());
            world.write_message(WindowEvent::Ime(ime));
        }
        InputStep::Idle => {}
    }
}

/// Move the window's cursor to `at` (logical pixels) and write `CursorMoved`,
/// plus `MouseMotion` when there was a previous position to move from. Nothing
/// is written when the app has no primary window: there is no cursor to move.
fn write_move(world: &mut World, window: Entity, at: Vec2) {
    let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    let Ok(mut win) = windows.single_mut(world) else {
        return;
    };
    let previous = win.cursor_position();
    let scale_factor = win.scale_factor();
    // Component-wise `f32`: the arithmetic lint fires on `glam`'s operators.
    let physical = Vec2::new(at.x * scale_factor, at.y * scale_factor);
    win.set_physical_cursor_position(Some(physical.as_dvec2()));
    let delta = previous.map(|was| Vec2::new(at.x - was.x, at.y - was.y));
    let moved = CursorMoved {
        window,
        position: at,
        delta,
    };
    world.write_message(moved.clone());
    world.write_message(WindowEvent::CursorMoved(moved));
    if let Some(delta) = delta {
        write_motion(world, delta);
    }
}

/// Raw relative motion.
fn write_motion(world: &mut World, delta: Vec2) {
    let motion = MouseMotion { delta };
    world.write_message(motion);
    world.write_message(WindowEvent::MouseMotion(motion));
}

/// A mouse button changing state.
fn write_button(world: &mut World, window: Entity, button: MouseButton, state: ButtonState) {
    let input = MouseButtonInput {
        button,
        state,
        window,
    };
    world.write_message(input);
    world.write_message(WindowEvent::MouseButtonInput(input));
}

/// A key changing state.
fn write_key(
    world: &mut World,
    window: Entity,
    key_code: KeyCode,
    logical: Key,
    text: Option<String>,
    state: ButtonState,
) {
    let input = KeyboardInput {
        key_code,
        logical_key: logical,
        state,
        text: text.map(Into::into),
        repeat: false,
        window,
    };
    world.write_message(input.clone());
    world.write_message(WindowEvent::KeyboardInput(input));
}

#[cfg(test)]
mod tests;
