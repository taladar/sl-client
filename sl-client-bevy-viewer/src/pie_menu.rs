//! The radial (**pie**) menu widget (`viewer-ui-radial-menu`): the general
//! mechanism for putting a pie on screen and picking an entry from it — not any
//! particular menu's entries, which are per-domain and belong with the domain
//! (`viewer-object-context-menu`).
//!
//! Nothing upstream has one: `bevy_ui_widgets` ships `MenuPopup` / `MenuItem` /
//! `MenuButton`, all of which assume a **line** layout. So this is ours, built on
//! `bevy_ui` and [`crate::ui`]'s scaffold.
//!
//! # Angular stability is the invariant
//!
//! A pie's whole advantage over a line menu is that you learn it with your hand:
//! "touch is a flick north" becomes muscle memory and you stop reading the menu.
//! That holds only if a slice's compass position is a property of **the entry**,
//! never of its index in whatever subset happens to be showing.
//!
//! Everything here is arranged around that one claim:
//!
//! - An entry **declares** its position ([`PieEntry::at`]). Nothing anywhere
//!   assigns a position from list order — [`resolve_slots`] writes each entry
//!   into the slot it named and returns the eight slots, so an absent entry
//!   leaves its slice **empty** rather than letting a neighbour rotate into the
//!   gap.
//! - An entry's full address is its [`PieAddress`] — the path of compass points
//!   from the root pie — and it is a **static** property of the declaration:
//!   [`addresses`] computes it without consulting any state, because there is no
//!   state that could move it. [`tests::every_action_keeps_its_declared_address`]
//!   pins every address in the fixture menu against a hard-coded table, so moving
//!   a function is a loud diff rather than a silent re-teach.
//! - [`tests::no_condition_can_move_an_entry`] then sweeps **every subset** of
//!   the live conditions and asserts that each entry either lands at its declared
//!   compass point or is absent. Never elsewhere. That is the muscle-memory claim
//!   as an executable statement.
//!
//! ## Every real menu needs a committed address test — this is not optional
//!
//! The tests above pin the *fixture* menu. That is the mechanism; the obligation
//! it demonstrates falls on **every actual pie in the viewer** — the object,
//! avatar, land and attachment menus of [[viewer-object-context-menu]], and any
//! future one. Each must ship a regression test that pins **every action's
//! address against a committed table**, exactly like
//! [`tests::every_action_keeps_its_declared_address`] does here.
//!
//! The reason is the whole point of a pie, and it is easy to lose: a developer
//! who has never heard of angular stability will one day reorder a menu's entries
//! to "tidy it up" or slot a new action in "where it fits", and every user who had
//! learned that menu with their hand is silently re-taught it. Nothing about the
//! change looks wrong in review — the menu still works, the labels are all still
//! there. Only a test that fails on a *moved address* catches it, and only if the
//! failure forces a **deliberate** update of the committed record: the diff that
//! moves an option and the diff that blesses the move must be the same, reviewed,
//! intentional commit. A menu whose positions are not pinned this way is one
//! refactor away from betraying the muscle memory it exists to build. So the rule
//! for the domain menus is: no pie ships without its address table pinned, and
//! moving an entry is a conscious edit to that table, never a side effect.
//!
//! # Where we beat the reference, and why
//!
//! The reference's autohide chain — one slot holding "Sit" or "Stand" depending
//! on state, so the angle does not move either way — is a good idea with a broken
//! implementation. Its losing members `continue` **without** incrementing the
//! slot counter (`num++` sits at the bottom of the loop body, `piemenu.cpp:474`),
//! so the number of slots a chain occupies depends on run-time state — one when a
//! member wins, several when none does — and **every entry after it rotates**.
//! That is precisely the breakage autohide exists to prevent.
//!
//! Ours cannot have that bug, because there is no counter to get wrong: a chain
//! ([`PieContent::Chain`]) is one entry at one declared compass point that
//! resolves to **at most one** of its members, and a chain with no winner leaves
//! its slot empty. Position is declared, never derived.
//!
//! The reference is also right that an ordinary hidden slice keeps its slot
//! (*"pie slices never really disappear"*), and we keep that too.
//!
//! # `More >` is ruled out by construction
//!
//! Eight slots is a hard budget and the reference spends the overflow on a slice
//! literally labelled `More >` that opens another pie — which itself has a
//! `More >`. `menu_pie_object.xml` chains **three**, so some object actions sit
//! four pies deep.
//!
//! That is the angular-stability problem one level up: which page an entry lands
//! on is a function of how many entries happen to exist, so adding one anywhere
//! pushes everything after it to a different depth. It is also unlearnable by
//! construction — a slice that says `More` tells your hand nothing.
//!
//! Nesting itself is unavoidable (eight will not be enough for every target), so
//! the rule is **nest by meaning, never by overflow**: a slice reading `Land >`
//! or `Manage >` is a stable, learnable grouping whose contents a user can
//! predict. The type system carries the rule: a sub-pie is a
//! [`PieMenuDef`], whose `label` is **not optional**, so there is nowhere to put
//! a nameless overflow bucket. If a grouping cannot be given an honest name, that
//! is the sign it is overflow rather than structure.
//!
//! # Selection is by angle, never by hit-test
//!
//! [`pick`] takes the *angle* of the cursor from the centre, not a rectangle
//! test. That is the other half of what makes a pie fast: every slice is an
//! equal-sized angular target and flicking in a direction is enough, at any
//! distance. A **dead zone** ([`PieGeometry::dead_zone`]) around the centre
//! selects nothing, so opening the menu and releasing without moving cancels.
//!
//! A consequence worth stating: because selection is angular, it does not matter
//! that the labels are drawn *outside* the ring. A label sits in its entry's
//! direction, so pointing at it picks that entry — the label is an affordance
//! telling you what lies that way, not a click target.
//!
//! # Placement, and the pointer problem — the task's one real unknown
//!
//! A line menu needs clearance in one quadrant and can flip or slide when it runs
//! out. A pie is **centred** on the spawn point and needs clearance in *every*
//! direction, so a click near an edge — or in a corner, where two edges bite at
//! once — has nowhere to put the circle. Clipping is not an option: a clipped
//! slice is an unreachable entry.
//!
//! The reference **clamps the centre** inward until the circle fits and then
//! **warps the mouse pointer to the clamped centre** (`PieMenu::show` →
//! `LLUI::setMousePositionLocal`). The warp is not a nicety: selection is by angle
//! *from the centre*, so a centre that is not under the pointer makes every angle
//! a lie and the menu opens with a slice already "chosen" in the direction of the
//! offset.
//!
//! **We clamp, and we cannot warp.** [`clamp_centre`] does the first half, and
//! does it better than the reference: it clamps by the menu's **measured box**
//! rather than by a fixed radius, because the labels are content-sized and how
//! much clearance the menu needs is not a constant anyone could have written down
//! in English.
//!
//! The second half does not port. Wayland — the primary desktop here — permits no
//! unconstrained pointer warp, and `winit`'s `set_cursor_position` fails there.
//! The task offered a way out: take a **pointer lock** for the menu's lifetime and
//! drive a virtual cursor from relative motion, which is what a locked pointer
//! gives anyway. That was built, and then **measured, and it does not work**: on
//! this desktop `CursorGrabMode::Locked` is refused, the real pointer stays
//! visible, and the result is two cursors disagreeing with each other — strictly
//! worse than either honest option. A design that needs a permission the platform
//! will not give is not a design.
//!
//! So the deliberate decision, taken with the measurement in hand:
//! **the real pointer is the only cursor, and there is no jump.** Nothing is
//! drawn, nothing is grabbed, and [`PieMenu::cursor`] is a *reading* of where the
//! pointer is rather than a thing the widget moves.
//!
//! What that buys, and what it costs, plainly:
//!
//! - The pointer and the highlight can never disagree, because there is one
//!   pointer and the highlight is computed from it. The failure mode of the
//!   virtual-cursor design — a drawn dot drifting away from the real arrow — is
//!   gone by construction.
//! - **Away from the edges, nothing is given up at all.** The centre is not
//!   clamped, so the centre *is* where you clicked, the pointer *is* on the
//!   centre, and the dead zone means nothing is selected until you move. That is
//!   the reference's behaviour exactly, and it is the overwhelmingly common case.
//! - **Near an edge, the muscle memory shifts.** The centre clamps inward, the
//!   pointer does not follow it, and the menu opens with a slice already
//!   highlighted in the direction of the offset. This is the real cost, it is the
//!   thing the reference's warp exists to prevent, and it is accepted knowingly
//!   rather than papered over. Reconsider it the day a pointer lock becomes
//!   reliable here, or an unconstrained warp does.
//! - **Descending has the same shape.** The pointer is out at the slice just
//!   picked, so a sub-pie opens with whatever it holds in that direction
//!   highlighted. [`PieMenu::drop_highlight`] at least stops the *parent's*
//!   highlight surviving into the child. The reference behaves the same way — it
//!   warps only on `show`, never on descent.
//!
//!   The fix, if this proves annoying in use, is the inverse of the one we cannot
//!   have: rather than moving the pointer to the centre, **move the centre to the
//!   pointer** — open each sub-pie centred where the pointer already is. It needs
//!   no platform permission and restores the property exactly. It is left out for
//!   now because it makes the menu hop as you descend, which is a change to the
//!   feel that wants trying before it is committed to.
//!
//! # The two interaction modes — and how "click outside to abort" coexists with
//! "a flick at any distance is enough"
//!
//! Those two requirements look contradictory: if every direction selects at any
//! distance, there is no "outside" left to click. The reference resolves it with
//! two modes, and the resolution is good, so it is reproduced here
//! ([`PieInteraction`], its `mFirstClick`):
//!
//! - [`PieInteraction::Flick`] — the button is still held from opening the menu.
//!   **No outer bound**: flick any distance and release to commit. Release in the
//!   dead zone commits nothing and pins the menu open instead.
//! - [`PieInteraction::Pinned`] — the menu is open as an ordinary menu. The outer
//!   bound applies, so a click beyond it selects nothing and **aborts**.
//!
//! The bound is the menu's **measured box**, not the ring, because the labels
//! live outside the ring and clicking a label must pick it rather than abort.
//! `Escape` aborts in either mode, as does the window losing focus — a menu the
//! user has alt-tabbed away from is not a menu they are choosing from.
//!
//! # Mouse-only, and why
//!
//! The pie is driven by the pointer alone — no keyboard selection, no tab focus.
//! The scaffold ([`crate::ui`]) makes keyboard reach the spine of the *panel* UI,
//! and the roadmap task asked for it here too, but it does not fit this widget: a
//! pie opens **on an in-world object** the pointer is over, and there is no
//! keyboard way to pick that object, so a keyboard way to pick *within* the menu
//! would open onto nothing. The reference is mouse-only for the same reason, and
//! this follows it. Selection is entirely angular — [`commit_pie_selection`] reads
//! the mouse release and picks by direction, for either button — so the labels are
//! neither buttons nor focus targets, just pictures on their wedges.
//!
//! # Direction-neutrality
//!
//! Per the scaffold's conventions the widget is direction-neutral **by
//! construction** — a circle has no leading side — but its labels are not, and
//! the two must not be confused. The slice text lays out through the same bidi
//! text stack as everything else (a `Text` node shapes its own runs), while the
//! *compass* is screen geometry: north-east must stay top-right in an RTL locale,
//! because the angle maths and the muscle memory are both physical. That falls
//! out for free from the polar placement ([`fit_pie_layout`]): a label's position
//! is computed from its compass *angle*, which no locale flips, so nothing has to
//! be exempted from the UI's mirroring — the labels never mirrored to begin with.
//!
//! Reference (Firestorm, read-only): `newview/piemenu.{h,cpp}` (the widget, the
//! angle maths, `PIE_MAX_SLICES = 8`, `PIE_OUTER_SIZE = 96`), `newview/pieslice.*`,
//! `newview/pieseparator.*`, `newview/pieautohide.*`, and the `PieMenu*` settings
//! in `newview/app_settings/settings.xml`. Note the pie is a **Firestorm
//! re-addition** — Linden Lab's viewer 2 dropped it — so upstream LL sources will
//! not have it.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::{PrimaryWindow, WindowFocused};

use crate::ui::{UiRoot, column};
use crate::ui_element::{ElementCx, RadialCentre, RadialPlacement, UiAction};
use crate::ui_font::UiFont;

/// The internal handle the pie shader (`pie_menu.wgsl`) is loaded under.
const PIE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2a7f5c31-9d84-4e62-b1a7-3c05e9f8d264");

/// The number of slices, mirroring the reference's `PIE_MAX_SLICES`.
///
/// Eight is not arbitrary and is not a tuning knob: it is the largest number of
/// angular targets a hand can hit reliably without looking, and it puts every
/// slice centre on a compass point a person already has a word for. Raising it
/// would shrink the targets and cost the muscle memory the whole widget exists
/// for; the answer to "eight is not enough" is a **named sub-pie**, never a
/// ninth slice.
pub(crate) const PIE_SLICES: usize = 8;

/// The name tying the labels' declared directions to the ring they are measured
/// from. One group: a matrix cell spawns one element, and a viewer only ever has
/// one pie open.
const PIE_RADIAL_GROUP: &str = "pie";

/// The ring's outer radius in logical pixels, mirroring the reference's
/// `PIE_OUTER_SIZE`.
const PIE_OUTER_RADIUS: f32 = 96.0;

/// The widest a slice label may be, in logical pixels — **a bound, and the one
/// place the pie needs one**.
///
/// The scaffold's convention-2 rule is that a container of text must never be
/// pinned to a measurement taken in one language, and this is a bound rather than
/// a width: a label is as wide as it needs up to here, and wraps beyond it.
///
/// It exists because a pie has a constraint an ordinary panel does not. A label
/// is placed on a wedge at a radius, and [`fit_pie_layout`] pushes that radius out
/// as the labels grow so they never overlap — so an unbounded label would make an
/// unbounded wheel. A single very long word with no break opportunity is the acute
/// case: without a bound it widens without limit, and the menu grows with it. The
/// bound caps the label's width; past it the text wraps to another line instead,
/// which the layout handles by pushing the radius out a little, not without limit.
/// `tests::a_pie_with_long_labels_keeps_every_label_in_its_own_slice` holds this
/// to account in every script.
///
/// **The ceiling this implies is real, and it is a constraint on the menu rather
/// than a bug in the widget.** Give all eight slices genuine *prose* (the
/// harness's ~170-character bidi sample) and each label wraps many lines tall; the
/// wheel that keeps them from overlapping grows past any screen, and there is
/// nowhere legal to put it. No radial layout can fix that — eight paragraphs
/// around a ring do not fit — and the answer is not to make this bound cleverer
/// but to not write a paragraph in a pie slice. It is the same authoring rule the
/// module states for `More >`: if the entry cannot be named briefly and honestly,
/// the problem is the entry.
const PIE_LABEL_MAX_WIDTH: f32 = PIE_OUTER_RADIUS * 1.6;

/// The dead zone's radius in logical pixels, mirroring the reference's
/// `PIE_INNER_SIZE`.
const PIE_INNER_RADIUS: f32 = 20.0;

/// The radius, in logical pixels, the label *centres* sit at — inside the ring,
/// out on the wedge, clear of the dead zone. The reference places its labels at
/// ≈ 0.7 of the outer radius (`PIE_X` / `PIE_Y` against `PIE_OUTER_SIZE`); this is
/// that, so a label reads as belonging to its slice rather than floating past the
/// rim.
const PIE_LABEL_RING_RADIUS: f32 = 66.0;

/// The gap, in logical pixels, kept between a label's far corner and the ring's
/// rim, so a label never touches the edge it sits inside.
const PIE_LABEL_RIM_MARGIN: f32 = 6.0;

/// One of the eight positions an entry can be pinned to.
///
/// **This is an entry's identity, not its index.** The variants are ordered as
/// the reference's slice numbering (`PIE_X` / `PIE_Y` in `piemenu.cpp`): slot 0
/// is east and they run counter-clockwise, which is the order the angle maths
/// falls out in.
///
/// Named for the compass rather than for the screen (`Up`, `TopRight`) because
/// that is how a hand learns it, and because a compass is unambiguous in a
/// mirrored layout where "right" is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Compass {
    /// Due east — slot 0, angle 0.
    East,
    /// North-east — slot 1.
    NorthEast,
    /// Due north — slot 2.
    North,
    /// North-west — slot 3.
    NorthWest,
    /// Due west — slot 4.
    West,
    /// South-west — slot 5.
    SouthWest,
    /// Due south — slot 6.
    South,
    /// South-east — slot 7.
    SouthEast,
}

impl Compass {
    /// Every compass point, in slot order.
    pub(crate) const ALL: [Self; PIE_SLICES] = [
        Self::East,
        Self::NorthEast,
        Self::North,
        Self::NorthWest,
        Self::West,
        Self::SouthWest,
        Self::South,
        Self::SouthEast,
    ];

    /// This point's slot number — its index into the eight slices.
    ///
    /// Written out rather than derived from the enum's discriminant so that the
    /// mapping is a stated fact rather than an accident of declaration order that
    /// a later reorder could silently change. Slot numbers are the wire between
    /// the shader, the tab order and the tests.
    ///
    /// A `u8` because it is genuinely 0..8, and because a small integer converts
    /// to `f32` losslessly and infallibly ([`f32::from`]) where a `usize` would
    /// need a cast the workspace forbids — [`Self::slot`] is the `usize` view for
    /// the callers that index with it.
    pub(crate) const fn slot_index(self) -> u8 {
        match self {
            Self::East => 0,
            Self::NorthEast => 1,
            Self::North => 2,
            Self::NorthWest => 3,
            Self::West => 4,
            Self::SouthWest => 5,
            Self::South => 6,
            Self::SouthEast => 7,
        }
    }

    /// This point's slot number, for indexing the eight slots.
    pub(crate) fn slot(self) -> usize {
        usize::from(self.slot_index())
    }

    /// This point's centre angle, in radians counter-clockwise from due east, in
    /// a **y-up** frame.
    pub(crate) fn centre_angle(self) -> f32 {
        let slice = core::f32::consts::TAU / 8.0;
        f32::from(self.slot_index()) * slice
    }

    /// This point's name, for a failure message and a debug overlay.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::East => "east",
            Self::NorthEast => "north-east",
            Self::North => "north",
            Self::NorthWest => "north-west",
            Self::West => "west",
            Self::SouthWest => "south-west",
            Self::South => "south",
            Self::SouthEast => "south-east",
        }
    }

    /// The compass point a direction falls in: the slice whose centre is nearest
    /// this angle.
    ///
    /// The reference rotates the angle by half a slice and floors the division
    /// (`piemenu.cpp` `handleHover`). This is the same partition stated the other
    /// way round — nearest centre wins — which is both self-evidently what
    /// "aligned to the compass points" means and free of the float-to-integer
    /// conversion the workspace lints forbid. The two agree exactly, which
    /// [`tests::the_partition_matches_the_reference_formula`] holds them to.
    pub(crate) fn from_angle(angle: f32) -> Self {
        let mut best = Self::East;
        let mut best_distance = f32::INFINITY;
        for point in Self::ALL {
            let distance = angular_distance(angle, point.centre_angle());
            if distance < best_distance {
                best_distance = distance;
                best = point;
            }
        }
        best
    }

    /// The unit vector pointing this way, in `bevy_ui`'s **y-down** screen frame —
    /// so a label placed along it lands in this slice's wedge.
    ///
    /// The y is negated from [`Self::centre_angle`]'s y-up convention exactly
    /// once, here, so a caller placing a label works in screen coordinates
    /// throughout.
    fn screen_direction(self) -> Vec2 {
        let angle = self.centre_angle();
        Vec2::new(angle.cos(), -angle.sin())
    }
}

/// The absolute angular difference between two angles, wrapped into `0..=PI`.
///
/// Plain `f32` arithmetic throughout, per the convention the rest of the crate
/// follows: the workspace's `arithmetic_side_effects` lint fires on `glam`'s
/// overloaded operators but not on floating-point arithmetic.
fn angular_distance(left: f32, right: f32) -> f32 {
    let raw = (left - right).rem_euclid(core::f32::consts::TAU);
    if raw > core::f32::consts::PI {
        core::f32::consts::TAU - raw
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// The declaration. A menu is data, and every position in it is written down.
// ---------------------------------------------------------------------------

/// One thing a pie can do, at one position.
///
/// The `when` condition is what makes a menu state-aware **without** letting
/// state move anything: see [`PieContent`] for the two things it means, neither
/// of which is "shuffle up to fill a gap".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PieAction {
    /// The slice's text. Laid out through the ordinary bidi text stack.
    pub(crate) label: &'static str,
    /// What this emits when picked — the `action` of the [`UiAction`] the widget
    /// writes, and the name the address table pins.
    pub(crate) action: &'static str,
    /// The condition that must hold, or `None` for unconditionally available.
    ///
    /// A plain, named key rather than a closure over a session, because the
    /// registry's rule is that an element is **constructible without its
    /// wiring** ([`crate::ui_element`]): the live viewer fills
    /// [`PieConditions`] from the world, the gallery leaves it empty, and a test
    /// sets exactly the subset it wants to interrogate.
    pub(crate) when: Option<&'static str>,
}

/// What lives at one position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PieContent {
    /// A single action. If its `when` fails it is **disabled** — faded, not
    /// pickable, and still occupying its slot, because the position belongs to
    /// the entry and not to whether it is available this second.
    Action(PieAction),
    /// A named sub-pie, opened in place. The mechanism is recursive
    /// (`PieMenu::appendContextSubMenu`).
    SubPie(&'static PieMenuDef),
    /// An **autohide chain**: mutually exclusive candidates for this one
    /// position, of which at most one shows — the reference's own example is a
    /// Sit / Stand toggle, where the point is that the angle does not move either
    /// way.
    ///
    /// The first member whose `when` holds takes the slot; a member with no
    /// `when` is an unconditional fallback. If none holds the slot stays
    /// **empty** — it never collapses, and nothing after it shifts, which is the
    /// bug in the reference's counter-driven version.
    ///
    /// Note `when` means something different here than on a bare
    /// [`PieContent::Action`]: in a chain it decides *which member holds the
    /// slot* (the winner is then enabled), rather than whether a fixed entry is
    /// enabled. Both readings leave the position untouched, which is the only
    /// thing that has to be true of it.
    Chain(&'static [PieAction]),
}

/// One entry: a declared position, and what sits there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PieEntry {
    /// **The position, declared.** Never inferred from this entry's index in the
    /// list — assigning slices in list order is the obvious implementation and it
    /// is the wrong one.
    pub(crate) at: Compass,
    /// What lives there.
    pub(crate) content: PieContent,
}

/// A pie: a name, and its entries.
///
/// The `label` is **not** optional, and that is load-bearing rather than tidy: a
/// sub-pie slice shows this text, so there is nowhere to write a nameless
/// overflow bucket. `More >` cannot be expressed without lying in the `label`
/// field, which a reviewer can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PieMenuDef {
    /// What a slice opening this pie reads, and what names it in the address
    /// table. An honest name for the grouping — if there is not one, the grouping
    /// is overflow rather than structure.
    pub(crate) label: &'static str,
    /// The entries, in any order: order is presentation, position is
    /// [`PieEntry::at`].
    pub(crate) entries: &'static [PieEntry],
}

impl PieMenuDef {
    /// The entry declared at `at`, if any.
    ///
    /// A scan rather than an index, because the entry list is *not* slot-indexed:
    /// that is the whole point. Eight is small enough that the scan is free, and
    /// the alternative — storing entries in slot order — would quietly re-admit
    /// the positional coupling this design exists to remove.
    fn entry_at(&self, at: Compass) -> Option<&'static PieEntry> {
        self.entries.iter().find(|entry| entry.at == at)
    }

    /// The sub-pie at `at`, if that position holds one.
    fn sub_pie_at(&self, at: Compass) -> Option<&'static Self> {
        match self.entry_at(at).map(|entry| entry.content) {
            Some(PieContent::SubPie(menu)) => Some(menu),
            Some(PieContent::Action(_) | PieContent::Chain(_)) | None => None,
        }
    }

    /// Follow a path of compass points from this pie, returning the pie it lands
    /// on — the root itself for an empty path.
    ///
    /// Returns `None` if any step does not hold a sub-pie, which is how a stale
    /// path (a menu rebuilt under a changed declaration) fails closed rather than
    /// silently showing the root.
    fn follow(&'static self, path: &[Compass]) -> Option<&'static Self> {
        let mut menu = self;
        for step in path {
            menu = menu.sub_pie_at(*step)?;
        }
        Some(menu)
    }
}

/// The **address** of a function in a menu tree: the compass points to flick,
/// from the root.
///
/// This is the muscle memory, written down. It is a static property of the
/// declaration — no condition, no session state and no ordering can change it —
/// which is exactly what makes it testable, and what
/// [`tests::every_action_keeps_its_declared_address`] pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PieAddress(pub(crate) Vec<Compass>);

impl core::fmt::Display for PieAddress {
    /// `north > east`, so a failure message reads as the gesture it describes.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let names: Vec<&str> = self.0.iter().map(|point| point.name()).collect();
        write!(f, "{}", names.join(" > "))
    }
}

/// Every action in a menu tree, with its address, depth-first.
///
/// Every member of an autohide chain reports the **same** address, because they
/// share the one position — that is what a chain is, and a test that read them as
/// separate addresses would be describing a different widget.
pub(crate) fn addresses(menu: &'static PieMenuDef) -> Vec<(&'static str, PieAddress)> {
    let mut found = Vec::new();
    collect_addresses(menu, &mut Vec::new(), &mut found);
    found
}

/// [`addresses`]' recursion: walk `menu`, tracking the path taken to reach it.
fn collect_addresses(
    menu: &'static PieMenuDef,
    path: &mut Vec<Compass>,
    found: &mut Vec<(&'static str, PieAddress)>,
) {
    // Walked in compass order rather than declaration order, so the table a test
    // pins is itself independent of how the entries happen to be written down.
    for point in Compass::ALL {
        let Some(entry) = menu.entry_at(point) else {
            continue;
        };
        path.push(point);
        match entry.content {
            PieContent::Action(action) => {
                found.push((action.action, PieAddress(path.clone())));
            }
            PieContent::Chain(members) => {
                for member in members {
                    found.push((member.action, PieAddress(path.clone())));
                }
            }
            PieContent::SubPie(sub) => collect_addresses(sub, path, found),
        }
        path.pop();
    }
}

/// The conditions that currently hold, by name.
///
/// A component rather than a resource, so two pies can be open (or under test) in
/// one world without sharing a truth. The live viewer fills it from the session;
/// the gallery leaves it empty and every conditional entry simply reads as
/// unavailable, which is a *true* rendering of "no session", not a stub.
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct PieConditions(pub(crate) Vec<&'static str>);

impl PieConditions {
    /// A set of conditions.
    pub(crate) fn new(conditions: impl IntoIterator<Item = &'static str>) -> Self {
        Self(conditions.into_iter().collect())
    }

    /// Whether `condition` holds. `None` — an entry with no condition — always
    /// holds.
    fn holds(&self, condition: Option<&'static str>) -> bool {
        match condition {
            None => true,
            Some(name) => self.0.contains(&name),
        }
    }
}

/// What a slot renders and does, once the declaration has been resolved against
/// the live conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedSlot {
    /// The text to draw.
    pub(crate) label: &'static str,
    /// What picking it does.
    pub(crate) outcome: SlotOutcome,
    /// Whether it can be picked at all. A disabled slot keeps its position and
    /// its label, and simply cannot be committed.
    pub(crate) enabled: bool,
}

/// What picking a slot does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOutcome {
    /// Emit this action.
    Action(&'static str),
    /// Descend into this sub-pie.
    SubPie(&'static PieMenuDef),
}

/// A slot's state as the shader reads it — four bits per slot, packed by
/// [`pack_slot_states`]. The discriminants are shared with `pie_menu.wgsl` and
/// must be kept in step with its `STATE_*` constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    /// Nothing lives here. The slice still renders — an absent entry leaves its
    /// slice empty — but dimmer.
    Empty,
    /// A pickable action.
    Action,
    /// An entry that is here but not available.
    Disabled,
    /// A sub-pie.
    SubPie,
}

impl SlotState {
    /// This state's four bits, as `pie_menu.wgsl`'s `STATE_*` constants.
    ///
    /// A match rather than a discriminant cast: the numbers are a **wire format**
    /// shared with a file the compiler cannot see, so they are written down on
    /// purpose rather than inherited from declaration order — which a reorder
    /// would silently change, and nothing on this side would notice.
    const fn bits(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Action => 1,
            Self::Disabled => 2,
            Self::SubPie => 3,
        }
    }
}

/// **Resolve a pie into its eight slots.**
///
/// The one function the whole angular-stability claim rests on, and it is
/// deliberately dull: each entry is written to `slots[entry.at.slot()]`. There is
/// no counter, no cursor into the entry list and no way for one entry's
/// availability to be observed by another — so there is nothing that *could*
/// move a position, which is a stronger property than testing that nothing does.
///
/// Returned slot-indexed (not as a list), because a list is what invites the bug.
pub(crate) fn resolve_slots(
    menu: &PieMenuDef,
    conditions: &PieConditions,
) -> [Option<ResolvedSlot>; PIE_SLICES] {
    let mut slots = [None; PIE_SLICES];
    for entry in menu.entries {
        let resolved = match entry.content {
            PieContent::Action(action) => Some(ResolvedSlot {
                label: action.label,
                outcome: SlotOutcome::Action(action.action),
                enabled: conditions.holds(action.when),
            }),
            PieContent::SubPie(sub) => Some(ResolvedSlot {
                label: sub.label,
                outcome: SlotOutcome::SubPie(sub),
                enabled: !sub.entries.is_empty(),
            }),
            // The chain: the first member whose condition holds takes the slot,
            // and if none does the slot stays `None` — empty, not collapsed.
            PieContent::Chain(members) => members
                .iter()
                .find(|member| conditions.holds(member.when))
                .map(|member| ResolvedSlot {
                    label: member.label,
                    outcome: SlotOutcome::Action(member.action),
                    enabled: true,
                }),
        };
        if let Some(slot) = slots.get_mut(entry.at.slot()) {
            *slot = resolved;
        }
    }
    slots
}

/// Pack the eight slots' states into the `u32` the shader unpacks, four bits
/// each, slot 0 in the low nibble.
fn pack_slot_states(slots: &[Option<ResolvedSlot>; PIE_SLICES]) -> u32 {
    let mut packed: u32 = 0;
    for (index, slot) in slots.iter().enumerate() {
        let state = match slot {
            None => SlotState::Empty,
            Some(slot) if !slot.enabled => SlotState::Disabled,
            Some(slot) => match slot.outcome {
                SlotOutcome::Action(_) => SlotState::Action,
                SlotOutcome::SubPie(_) => SlotState::SubPie,
            },
        };
        let Ok(shift) = u32::try_from(index) else {
            continue;
        };
        packed |= state.bits().wrapping_shl(shift.wrapping_mul(4));
    }
    packed
}

// ---------------------------------------------------------------------------
// Geometry: picking by angle, and placing the circle on a screen with edges.
// ---------------------------------------------------------------------------

/// The pie's measurements, in logical pixels.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PieGeometry {
    /// The dead zone's radius: inside this, nothing is selected.
    pub(crate) dead_zone: f32,
    /// The ring's outer radius — what is drawn, not what bounds selection.
    pub(crate) outer: f32,
}

impl Default for PieGeometry {
    fn default() -> Self {
        Self {
            dead_zone: PIE_INNER_RADIUS,
            outer: PIE_OUTER_RADIUS,
        }
    }
}

/// **Pick a slot from a cursor offset — by angle, never by hit-test.**
///
/// `offset` is the cursor relative to the ring centre in a **y-up** frame (see
/// [`ui_offset`] for the one conversion from `bevy_ui`'s y-down space).
///
/// `bound` is the outer limit beyond which nothing is selected, or `None` for no
/// limit at all. That is the whole of the [`PieInteraction`] distinction: a flick
/// has no limit, so a fast gesture in roughly the right direction lands whatever
/// distance it travels, while a pinned menu has one, so there is an "outside" to
/// click for abort.
pub(crate) fn pick(offset: Vec2, geometry: PieGeometry, bound: Option<f32>) -> Option<Compass> {
    let distance = offset.length();
    if distance <= geometry.dead_zone {
        return None;
    }
    if bound.is_some_and(|bound| distance > bound) {
        return None;
    }
    Some(Compass::from_angle(offset.to_angle()))
}

/// Convert a `bevy_ui` offset (**y down**) into the y-up frame the compass is
/// reasoned about in.
///
/// The single conversion, mirroring the one in `pie_menu.wgsl`. Every angle in
/// this module is y-up; every screen coordinate that reaches it comes through
/// here.
pub(crate) const fn ui_offset(offset: Vec2) -> Vec2 {
    Vec2::new(offset.x, -offset.y)
}

/// **Clamp a pie's centre so the whole menu clears the viewport edges.**
///
/// The reference clamps by the ring's radius. Ours clamps by the menu's
/// **measured box**, which is a different and better thing: the labels live
/// outside the ring and are content-sized, so a long translation makes the menu
/// genuinely wider and the amount of clearance it needs is not a constant anyone
/// could have written down in English.
///
/// `ring_offset` is where the ring's centre sits relative to the box's centre —
/// read from the laid-out tree rather than assumed to be zero, because the label
/// columns are content-sized and need not be symmetric.
///
/// A menu larger than the viewport on an axis cannot be placed legally at all; it
/// is centred on that axis, which at least loses the same amount at both ends
/// rather than all of it at one.
pub(crate) fn clamp_centre(requested: Vec2, ring_offset: Vec2, size: Vec2, viewport: Vec2) -> Vec2 {
    Vec2::new(
        clamp_axis(requested.x, ring_offset.x, size.x, viewport.x),
        clamp_axis(requested.y, ring_offset.y, size.y, viewport.y),
    )
}

/// [`clamp_centre`] on one axis.
fn clamp_axis(requested: f32, ring_offset: f32, size: f32, viewport: f32) -> f32 {
    let half = size / 2.0;
    // The box centre is `requested - ring_offset`, and must lie within
    // `half..=viewport - half`; rearranged, these are the bounds on the ring
    // centre itself.
    let low = half + ring_offset;
    let high = viewport - half + ring_offset;
    if low > high {
        // Wider than the viewport: centre it and lose the same at both ends.
        return f32::midpoint(low, high);
    }
    requested.clamp(low, high)
}

// ---------------------------------------------------------------------------
// The widget.
// ---------------------------------------------------------------------------

/// Which of the two interaction modes an open pie is in — the reference's
/// `mFirstClick`, named for what it means rather than for when it happens.
///
/// See the [module documentation](self) for why both exist: they are what lets
/// "a flick in a direction is enough, at any distance" and "click outside to
/// abort" both be true without contradicting each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PieInteraction {
    /// The button that opened the menu is still down. **No outer bound**: any
    /// distance beyond the dead zone selects. Releasing commits the highlighted
    /// slot, or — in the dead zone — commits nothing and pins the menu open.
    Flick,
    /// The menu is open on its own. The outer bound applies, so a click beyond
    /// the menu's box selects nothing and aborts.
    Pinned,
}

/// An open pie menu, on its root node.
#[derive(Component, Debug, Clone)]
pub(crate) struct PieMenu {
    /// The declaration this pie renders. Static: a menu is data.
    pub(crate) menu: &'static PieMenuDef,
    /// The path from `menu` to the pie currently showing — one compass point per
    /// level descended. Empty at the root.
    pub(crate) path: Vec<Compass>,
    /// Where the pointer is, in logical pixels from the ring centre, y-down (in
    /// `bevy_ui`'s own frame; converted to y-up at the two points that need an
    /// angle).
    ///
    /// A *reading*, not a thing the widget owns: [`drive_pie_cursor`] recomputes
    /// it from the real pointer every frame. See the module's placement section.
    pub(crate) cursor: Vec2,
    /// The slot the cursor currently picks, if any.
    pub(crate) highlighted: Option<Compass>,
    /// Which interaction mode this pie is in.
    pub(crate) interaction: PieInteraction,
    /// The `element` this pie's [`UiAction`]s are attributed to.
    pub(crate) element: &'static str,
}

impl PieMenu {
    /// The pie currently showing — the root, or whatever sub-pie the path has
    /// descended into.
    pub(crate) fn current(&self) -> Option<&'static PieMenuDef> {
        self.menu.follow(&self.path)
    }

    /// Drop the current highlight, so nothing is selected until the next frame
    /// has read the pointer again.
    ///
    /// All a level change *can* do about the pointer, since the pointer is not
    /// ours to move — see the module's placement section. It is honest rather than
    /// sufficient: the pointer is still out at the slice just picked, so
    /// [`drive_pie_cursor`] will immediately re-highlight whatever the sub-pie
    /// holds in that same direction. Clearing it at least means the *parent's*
    /// highlight never survives into the child.
    pub(crate) const fn drop_highlight(&mut self) {
        self.highlighted = None;
    }
}

/// A marker on the ring node — the square the shader draws into, and the node
/// whose centre is the pie's origin for every angle.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PieRing;

/// A marker on one slot's label node, carrying which slot it is.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PieLabel {
    /// The compass point this label sits at. Fixed at spawn: a label never moves.
    pub(crate) at: Compass,
}

/// The path whose labels are **currently spawned** under a pie root.
///
/// Compared against [`PieMenu::path`] by [`update_pie_labels`] so the labels are
/// rebuilt only when the pie actually descends or ascends a level — never on the
/// per-frame cursor updates, which would otherwise churn the labels faster than
/// the layout can place them.
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct DisplayedPiePath(Vec<Compass>);

/// Where a live pie asked to be placed, and what became of the request.
///
/// **Also the mark of a pie that was actually opened**, which is a distinction
/// the widget needs and gets for free here. A pie spawned *in flow* — the
/// gallery's card, a headless test fixture — is a **specimen**: it renders, it
/// resolves, its labels emit their actions when clicked (so the registry's
/// no-wiring contract is exercised for real), but it does not take the cursor and
/// does not close itself, because there is nothing to close. It was never opened.
///
/// Only a pie carrying this is *live*, and only a live pie is placed, grabs the
/// pointer, or can be aborted.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PiePlacement {
    /// Where the caller asked for the ring's centre, in logical pixels. Updated on
    /// a descent so the sub-pie re-centres on the pointer — the inverse of the
    /// reference's pointer warp, which we cannot do.
    pub(crate) requested: Vec2,
    /// Whether the menu has been **revealed** yet. It is spawned hidden and shown
    /// only once its layout has settled — see [`place_pie_menu`] for why the first
    /// measured frame is too early, and what the visible flicker looked like.
    pub(crate) placed: bool,
    /// The previous frame's applied top-left, in logical pixels — the settle
    /// detector. The menu grows over a frame or two (the labels are measured, then
    /// [`fit_pie_layout`] sizes the ring around them), so its top-left keeps moving
    /// until the size stops changing; when two consecutive frames agree, the layout
    /// has converged and it is safe to reveal.
    settled_at: Option<Vec2>,
}

/// The uniform `pie_menu.wgsl` reads. Field order and padding mirror the WGSL
/// `PieParams` exactly, so the std140 packing lines up.
#[derive(Clone, Copy, Debug, ShaderType)]
struct PieParams {
    /// The ring's resting fill.
    background: Vec4,
    /// The dividers and edge rings.
    line: Vec4,
    /// The highlighted slot's fill.
    selected: Vec4,
    /// The dead zone's radius, in physical pixels.
    inner_radius: f32,
    /// The ring's outer radius, in physical pixels.
    outer_radius: f32,
    /// Eight slot states, four bits each — see [`pack_slot_states`].
    slot_states: u32,
    /// The highlighted slot, or `-1`.
    highlighted: i32,
}

/// The pie's ring material: one node, one draw, the geometry in the shader.
#[derive(AsBindGroup, Asset, TypePath, Debug, Clone)]
pub(crate) struct PieMenuMaterial {
    /// Everything the shader needs.
    #[uniform(0)]
    params: PieParams,
}

impl UiMaterial for PieMenuMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PIE_SHADER_HANDLE)
    }
}

/// The ring's resting fill — the reference's `PieMenuBgColor`
/// (`0.24 0.24 0.24 0.8`): a neutral, fairly transparent grey rather than the
/// darker, more opaque tint this used to carry.
const PIE_BACKGROUND: Color = Color::srgba(0.24, 0.24, 0.24, 0.8);

/// The dividers and the edge rings — the reference's `PieMenuLineColor`
/// (`0 0 0 0.5`), a soft black.
const PIE_LINE: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);

/// The highlighted slot's colour — the reference's `PieMenuSelectedColor`,
/// `EmphasisColor_35` (`0.950 0.412 0.173 0.35`): a semi-transparent orange. The
/// shader draws it as a **radial gradient**, full at the inner edge and fading to
/// nothing at the rim, matching `gl_washer_segment_2d(…, selectedColor,
/// borderColor)`.
const PIE_SELECTED: Color = Color::srgba(0.95, 0.412, 0.173, 0.35);

/// A slice label's text colour.
const PIE_LABEL: Color = Color::srgb(0.93, 0.95, 0.98);

/// A disabled slice label's text colour — faded well down so "here but
/// unavailable" reads at a glance (the reference fades a disabled item to 0.3
/// alpha; this goes a little further).
const PIE_LABEL_DISABLED: Color = Color::srgba(0.93, 0.95, 0.98, 0.22);

/// A sub-pie slice's label colour, so "this opens another pie" reads without the
/// label having to say `>`.
const PIE_LABEL_SUB_PIE: Color = Color::srgb(0.65, 0.86, 1.0);

/// The plugin: the shader, the material, and the systems that drive a live pie.
///
/// The widget itself needs none of this to *lay out* — [`spawn_pie_menu`] builds
/// a tree out of ordinary nodes — which is what lets the headless harness check
/// it with no renderer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PieMenuPlugin;

impl Plugin for PieMenuPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, PIE_SHADER_HANDLE, "pie_menu.wgsl", Shader::from_wgsl);
        app.add_plugins(UiMaterialPlugin::<PieMenuMaterial>::default())
            .add_message::<OpenPieMenu>()
            .add_systems(
                Update,
                (
                    open_pie_menus,
                    // Ordered: the cursor moves, the highlight follows it, and a
                    // release acts on the highlight the user last saw — not on one
                    // computed from a frame they never had a chance to look at.
                    drive_pie_cursor,
                    commit_pie_selection,
                    abort_pie_on_focus_loss,
                    update_pie_labels,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                // After layout, because these need the menu's *measured* size:
                // `fit_pie_layout` reads each label's size to place it and grow the
                // ring, the placement clamps by the resulting box, and the material
                // converts to physical pixels through the computed scale factor.
                // `recenter_pie_on_pointer` runs first so a descent's new centre is
                // in `requested` before `place_pie_menu` reads it. Chained so the
                // placement clamps by the size the fit produced.
                (
                    recenter_pie_on_pointer,
                    fit_pie_layout,
                    place_pie_menu,
                    drive_pie_material,
                )
                    .chain()
                    .after(bevy::ui::UiSystems::Layout),
            );
    }
}

/// Ask for a pie menu at a screen position — the widget's whole inward API.
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenPieMenu {
    /// The declaration to show.
    pub(crate) menu: &'static PieMenuDef,
    /// Where to centre it, in logical pixels. Clamped inward if the menu will not
    /// fit there.
    pub(crate) at: Vec2,
    /// The `element` its actions are attributed to.
    pub(crate) element: &'static str,
    /// The conditions that hold right now. Snapshotted at open: a menu whose
    /// entries changed availability *while it was open* would be re-teaching the
    /// user's hand mid-gesture.
    pub(crate) conditions: &'static [&'static str],
}

/// Spawn a pie's node tree under `parent`, showing `menu`'s root.
///
/// The whole widget, and it takes no session, no window and no material — per the
/// registry's rule ([`crate::ui_element`]) an element must be constructible
/// without its wiring. The ring's material is attached separately (and only if
/// the app has one), so a headless layout test gets the same tree the viewer
/// does, minus the pixels.
pub(crate) fn spawn_pie_menu(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
    menu: &'static PieMenuDef,
    element: &'static str,
    conditions: PieConditions,
) -> Entity {
    let geometry = PieGeometry::default();
    let diameter = geometry.outer * 2.0;
    let root = commands
        .spawn((
            Node {
                // A square the ring fills, with every child placed **by polar
                // coordinate** inside it (`fit_pie_layout`). Not a grid, and not
                // by flow: a compass rose is polar, and the eight positions are an
                // angle and a radius rather than cells in a table.
                //
                // The size is a starting guess. `fit_pie_layout` measures the
                // labels each frame and grows it until each one fits inside its
                // own wedge, so the ring is content-driven — the pie is as big as
                // its labels need and no bigger.
                width: Val::Px(diameter),
                height: Val::Px(diameter),
                ..default()
            },
            PieMenu {
                menu,
                path: Vec::new(),
                cursor: Vec2::ZERO,
                highlighted: None,
                // `Pinned` is the resting mode, and `open_pie_menus` moves a
                // live pie to `Flick`. That way round because `Flick` means
                // exactly one thing — *a mouse button is being held right now* —
                // which is true of a menu someone just opened and never true of a
                // specimen sitting in a gallery card.
                interaction: PieInteraction::Pinned,
                element,
            },
            conditions,
            geometry,
            // The root's labels are spawned below for the empty (root) path, so the
            // displayed path starts empty and matches — `update_pie_labels` then
            // leaves them alone until the pie descends.
            DisplayedPiePath::default(),
            Name::new("pie-menu"),
            ChildOf(parent),
        ))
        // **Swallow pointer presses that land on the menu**, so they do not bubble
        // up to an ancestor's open-observer (the gallery hangs one on `UiRoot`).
        // `Pickable::should_block_lower` on the ring stops entities *behind* the
        // menu from being picked, but a `Pointer<Press>` still propagates *up* the
        // hierarchy — so a right-click in the dead zone would reach the world's
        // open-observer and reopen the menu the same frame `commit_pie_selection`
        // closed it. Left-click did not show this only because that observer is
        // secondary-button-only. Stopping propagation here makes a click on the
        // menu the menu's alone; a click off it never reaches this node and still
        // opens a new pie.
        .observe(swallow_pie_press)
        .id();

    commands.spawn((
        Node {
            // Fills the root, so the ring's centre is the root's centre and every
            // angle in the widget is measured from one point.
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        PieRing,
        // **Block picking over the whole menu**, so a click on a slice or in the
        // dead zone is handled by the menu (`commit_pie_selection` selects, or
        // closes in the dead zone) and never leaks through to the world's
        // open-observer behind it — which would open a *second* menu on top of a
        // click the user meant for the first. A click *off* the menu misses this
        // node and reaches the world, opening a new menu there, which is the
        // reference's rule. (The block is the ring's square, marginally larger than
        // the visible circle; a click in a bare corner closes rather than opening
        // anew — a sliver, and closing is the safe default there.)
        Pickable {
            should_block_lower: true,
            is_hoverable: true,
        },
        // Spawned before the labels, so it draws under them: `bevy_ui` paints
        // children in order, and the labels sit *inside* the ring.
        RadialCentre {
            group: PIE_RADIAL_GROUP,
        },
        Name::new("pie-ring"),
        ChildOf(root),
    ));
    // The material, only if this app renders. `Commands` cannot reach `Assets`,
    // so it is queued — and in a headless layout test the collection is simply
    // absent and the ring stays an ordinary (invisible) node of the right size,
    // which is all the layout checks need it to be.
    commands.queue(move |world: &mut World| {
        attach_pie_material(world);
    });

    rebuild_pie_labels(commands, root, cx, menu, &PieConditions::default());
    root
}

/// Give every ring node that has not got one a [`PieMenuMaterial`].
///
/// A world command rather than a system, so a pie spawned by
/// [`spawn_pie_menu`] is drawable in the same frame it appears rather than one
/// frame later — a menu that flickers into existence blank is a menu that reads
/// as broken.
fn attach_pie_material(world: &mut World) {
    let Some(mut materials) = world.get_resource_mut::<Assets<PieMenuMaterial>>() else {
        // No renderer (the headless harness): nothing to attach, and nothing
        // wrong with that.
        return;
    };
    let handle = materials.add(PieMenuMaterial {
        params: PieParams {
            background: LinearRgba::from(PIE_BACKGROUND).to_vec4(),
            line: LinearRgba::from(PIE_LINE).to_vec4(),
            selected: LinearRgba::from(PIE_SELECTED).to_vec4(),
            inner_radius: PIE_INNER_RADIUS,
            outer_radius: PIE_OUTER_RADIUS,
            slot_states: 0,
            highlighted: -1,
        },
    });
    let mut rings =
        world.query_filtered::<Entity, (With<PieRing>, Without<MaterialNode<PieMenuMaterial>>)>();
    let targets: Vec<Entity> = rings.iter(world).collect();
    for ring in targets {
        world.entity_mut(ring).insert(MaterialNode(handle.clone()));
    }
}

/// Rebuild a pie's eight labels for the pie currently showing.
///
/// Despawned and respawned on a path change rather than patched, mirroring the
/// gallery's own reasoning: an element's strings are baked in at construction, as
/// a real panel's are, and patching them in place would exercise a path the
/// viewer does not have.
fn rebuild_pie_labels(
    commands: &mut Commands,
    root: Entity,
    cx: ElementCx,
    menu: &PieMenuDef,
    conditions: &PieConditions,
) {
    let slots = resolve_slots(menu, conditions);
    for point in Compass::ALL {
        let Some(slot) = slots.get(point.slot()).copied().flatten() else {
            // An empty slot spawns no label — and, crucially, nothing else moves
            // into its direction. The wedge is still drawn and still empty, which
            // is what "pie slices never really disappear" looks like when the
            // position is a property of the entry rather than of a list.
            continue;
        };
        let color = match (slot.enabled, slot.outcome) {
            (false, _) => PIE_LABEL_DISABLED,
            (true, SlotOutcome::SubPie(_)) => PIE_LABEL_SUB_PIE,
            (true, SlotOutcome::Action(_)) => PIE_LABEL,
        };
        commands
            .spawn((
                // A label is a **picture, not a click target**. The pointer has one
                // selection path and it is angular: [`commit_pie_selection`] reads the
                // mouse release, for either button, and picks by *direction* — so a
                // right-click on a slice selects it exactly like a left-click, matching
                // the reference. The menu is mouse-only (no keyboard reach: there is no
                // keyboard way to pick the in-world object a pie opens on, so a keyboard
                // way to pick within it would reach nothing), which is why the labels
                // are neither `Button`s nor `TabIndex` focus targets.
                Node {
                    // Placed by `fit_pie_layout`, which centres it on its wedge at a
                    // polar offset from the ring's centre. Absolute rather than in
                    // flow: the position is an angle, and no flow can express one.
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    // A bound, not a width: a long translation wraps rather than
                    // pushing the ring wider than it needs to be. See the constant.
                    max_width: Val::Px(PIE_LABEL_MAX_WIDTH),
                    ..default()
                },
                // **The claim the box checks cannot make**: this label must still
                // *mean* its slice. Selection is by angle from the ring's centre, so a
                // label that drifts more than half a slice away from its own compass
                // point sits in a neighbour's sector — and pointing at it would pick
                // something other than what it says. `fit_pie_layout` places it at
                // exactly its own angle, so this is the arithmetic's guard rather than
                // the layout's; it costs nothing and it is the claim, written down.
                RadialPlacement {
                    group: PIE_RADIAL_GROUP,
                    angle: point.centre_angle(),
                    // Half a slice: the widget resolves a direction to the nearest
                    // slice centre, so this is not a fudge factor, it is the exact
                    // width of the claim.
                    tolerance: core::f32::consts::TAU / 16.0,
                },
                PieLabel { at: point },
                Name::new(format!("pie-label:{}", point.name())),
                ChildOf(root),
            ))
            .with_child((
                Text::new(cx.text(slot.label)),
                // A label is bounded (see `max_width` above), so a word wider than
                // the bound has nowhere to go and would simply overflow — measured
                // at 163 px against a 153 px box, in Cyrillic at 22 px.
                // `WordOrCharacter` breaks such a word rather than letting it hang
                // out, and leaves every ordinary label — which fits — untouched.
                TextLayout {
                    linebreak: LineBreak::WordOrCharacter,
                    ..default()
                },
                cx.font(UiFont::Sans),
                // A sub-pie's label is tinted (and the shader draws a rim chevron);
                // neither writes a `>` into the string, so there is no bidi arrow to
                // mirror and no width added to the text.
                TextColor(color),
                Name::new(format!("pie-label-text:{}", point.name())),
            ));
    }
}

/// **Place the labels by polar coordinate, and grow the ring to hold them.**
///
/// This is the layout, and it runs every frame after `bevy_ui` has *measured* the
/// labels but before it commits their transforms. The order matters: a label's
/// size is what decides how far out it has to sit, and its size is only known once
/// the text has been shaped.
///
/// The geometry, per label:
///
/// - It is centred on its wedge — at its compass direction, at a radius `r` from
///   the ring's centre. The reference does the same (`PIE_X` / `PIE_Y`), and it is
///   why the labels read as *belonging* to the slices: they sit **inside** the
///   ring, on the wedge, not floating out beyond the rim.
/// - `r` is the label ring radius — far enough out to clear the dead zone, near
///   enough in to stay well inside the rim. The reference's is ≈ 0.7 of the outer
///   radius.
/// - The ring must then be at least large enough that the label's *far* corner —
///   its centre plus its half-extent along the outward direction — is still inside
///   the rim. So the required outer radius is `r + (the label's reach outward)`,
///   maxed over all eight labels, and the root grows to twice that. A pie with one
///   long label is a bigger circle, not a label hanging off the edge of a fixed
///   one.
///
/// The growth is the content-driven half of the scaffold's convention, in polar
/// form: the pie is exactly as big as its labels need and no bigger, so a longer
/// translation makes a larger wheel rather than an overrun.
pub(crate) fn fit_pie_layout(
    mut pies: Query<(&mut Node, &mut PieGeometry, &Children), With<PieMenu>>,
    mut labels: Query<(&PieLabel, &mut Node, &ComputedNode), Without<PieMenu>>,
) {
    // The tangent of half a slice — how wide a slice's sector is, per unit of
    // radius, measured from its centre line. A label whose *tangential* extent
    // (the part across the wedge, not along it) exceeds this at the label radius
    // has spilled into a neighbour's sector, which is where adjacent labels start
    // to overlap.
    let half_slice_tan = (core::f32::consts::TAU / 16.0).tan();

    for (mut root_node, mut geometry, children) in &mut pies {
        // First pass: the two radii the labels demand.
        //
        // - The **label ring** must be large enough that no label's tangential
        //   extent overflows its 45° sector, or adjacent labels collide. A wide
        //   label on a diagonal is the worst case: its width is almost entirely
        //   tangential there. This is what a fixed radius got wrong — long labels
        //   at a fixed distance simply ran into each other.
        // - The **outer ring** must then clear each label's *radial* reach, so the
        //   whole label sits inside the rim.
        //
        // Both start at the reference's base size and only ever grow, which is the
        // **minimum**: the menu never shrinks below `PIE_OUTER_SIZE`, so a language
        // with single-character labels (Japanese, say) still gets the full,
        // comfortable base wheel rather than a tiny one that is hard to aim at. The
        // size adapts *up* for longer labels, never *down* past what a hand needs.
        let mut label_radius = PIE_LABEL_RING_RADIUS;
        for child in children.iter() {
            let Ok((label, _node, computed)) = labels.get(child) else {
                continue;
            };
            let scale = computed.inverse_scale_factor;
            let half = Vec2::new(computed.size.x * scale / 2.0, computed.size.y * scale / 2.0);
            let direction = label.at.screen_direction();
            // The support of the box along the tangent (perpendicular to the
            // radial direction): `|perp·(hx,0)| + |perp·(0,hy)|`.
            let tangent = Vec2::new(-direction.y, direction.x);
            let tangential = tangent.x.abs() * half.x + tangent.y.abs() * half.y;
            // The radius at which that tangential extent just fills the sector.
            let demanded = (tangential + PIE_LABEL_RIM_MARGIN) / half_slice_tan;
            if demanded > label_radius {
                label_radius = demanded;
            }
        }

        let mut required_outer = PIE_OUTER_RADIUS;
        for child in children.iter() {
            let Ok((label, _node, computed)) = labels.get(child) else {
                continue;
            };
            let scale = computed.inverse_scale_factor;
            let half = Vec2::new(computed.size.x * scale / 2.0, computed.size.y * scale / 2.0);
            let direction = label.at.screen_direction();
            // The radial reach: how far the box extends from its own centre in the
            // outward direction, `|dx|·hx + |dy|·hy`.
            let reach = direction.x.abs() * half.x + direction.y.abs() * half.y;
            let demanded = label_radius + reach + PIE_LABEL_RIM_MARGIN;
            if demanded > required_outer {
                required_outer = demanded;
            }
        }
        if (geometry.outer - required_outer).abs() > 0.5 {
            geometry.outer = required_outer;
        }
        let diameter = geometry.outer * 2.0;
        if root_node.width != Val::Px(diameter) {
            root_node.width = Val::Px(diameter);
        }
        if root_node.height != Val::Px(diameter) {
            root_node.height = Val::Px(diameter);
        }

        // Second pass: place each label. Its centre sits at the label ring radius
        // along its direction, and `left` / `top` are the top-left corner, so the
        // node is offset back by its own half-extent.
        let centre = geometry.outer;
        for child in children.iter() {
            let Ok((label, mut node, computed)) = labels.get_mut(child) else {
                continue;
            };
            let scale = computed.inverse_scale_factor;
            let half = Vec2::new(computed.size.x * scale / 2.0, computed.size.y * scale / 2.0);
            let direction = label.at.screen_direction();
            let label_centre = Vec2::new(
                centre + direction.x * label_radius,
                centre + direction.y * label_radius,
            );
            let left = label_centre.x - half.x;
            let top = label_centre.y - half.y;
            if node.left != Val::Px(left) {
                node.left = Val::Px(left);
            }
            if node.top != Val::Px(top) {
                node.top = Val::Px(top);
            }
        }
    }
}

/// Stop a pointer press that landed on the menu from bubbling to an ancestor's
/// open-observer. See the `.observe` call in [`spawn_pie_menu`] for the full why.
fn swallow_pie_press(mut press: On<Pointer<Press>>) {
    press.propagate(false);
}

/// Open a pie on request.
///
/// A request is only written for a click that lands **off** any open menu — the
/// menu blocks picking over its own area (see [`spawn_pie_menu`]'s ring) and
/// swallows the press ([`swallow_pie_press`]), so a click on a slice or the dead
/// zone never reaches the open-observer and is handled by the menu itself
/// ([`commit_pie_selection`]) instead. That is the reference's rule: a right-click
/// on the menu selects (or, in the dead zone, closes), while a right-click
/// elsewhere opens a new menu there.
fn open_pie_menus(
    mut commands: Commands,
    mut requests: MessageReader<OpenPieMenu>,
    root: Option<Res<UiRoot>>,
    // **Only *live* pies** (those carrying a `PiePlacement`), never a specimen a
    // test spawns in flow, which must survive a live pie opening.
    open: Query<Entity, (With<PieMenu>, With<PiePlacement>)>,
) {
    let Some(root) = root else {
        return;
    };
    for request in requests.read() {
        // One live pie at a time: opening a menu elsewhere replaces the last, which
        // is the reference's behaviour — a fresh right-click off the menu opens a
        // new one for that location and the old one goes away.
        for existing in &open {
            commands.entity(existing).despawn();
        }
        let pie = spawn_pie_menu(
            &mut commands,
            root.0,
            ElementCx::new(),
            request.menu,
            request.element,
            PieConditions::new(request.conditions.iter().copied()),
        );
        commands.entity(pie).insert((
            // Hidden until placed: the first layout is what *measures* the menu,
            // and the measurement is what the clamp needs. One frame of a menu in
            // the wrong place is a flicker; hiding it costs nothing.
            Visibility::Hidden,
            PiePlacement {
                requested: request.at,
                placed: false,
                settled_at: None,
            },
        ));
        // NOTE: `place_pie_menu` writes `position_type` / `left` / `top` onto the
        // pie's *existing* `Node`, field by field. Do not insert a fresh
        // `Node { position_type, ..default() }` here or there — a component insert
        // **replaces**, so it would wipe the root's size and every child's
        // placement, and the menu would collapse. It fails only for a *live* pie,
        // so a gallery card and every layout test would go on looking perfect.
    }
}

/// Place a live pie: clamp its centre so the whole menu clears the viewport, and
/// reveal it.
///
/// Runs after layout because it needs the menu's **measured** box — the labels
/// are content-sized, so how much clearance the menu needs is not a constant.
fn place_pie_menu(
    mut pies: Query<(
        &mut Node,
        &mut Visibility,
        &mut PiePlacement,
        &mut PieMenu,
        &ComputedNode,
        &UiGlobalTransform,
        &Children,
    )>,
    rings: Query<&UiGlobalTransform, With<PieRing>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let viewport = Vec2::new(window.width(), window.height());
    for (mut node, mut visibility, mut placement, mut pie, computed, transform, children) in
        &mut pies
    {
        let scale = computed.inverse_scale_factor;
        let size = Vec2::new(computed.size.x * scale, computed.size.y * scale);
        if size.x <= 0.0 || size.y <= 0.0 {
            // Not measured yet.
            continue;
        }
        // Where the ring's centre sits relative to the box's centre. Read from the
        // laid-out tree rather than assumed zero: the label tracks are
        // content-sized, and a design that assumed symmetry would bend every angle
        // the day one of them was not.
        let ring_offset = children
            .iter()
            .find_map(|child| rings.get(child).ok())
            .map_or(Vec2::ZERO, |ring| {
                Vec2::new(
                    (ring.translation.x - transform.translation.x) * scale,
                    (ring.translation.y - transform.translation.y) * scale,
                )
            });
        let centre = clamp_centre(placement.requested, ring_offset, size, viewport);
        let box_centre = Vec2::new(centre.x - ring_offset.x, centre.y - ring_offset.y);
        let left = box_centre.x - size.x / 2.0;
        let top = box_centre.y - size.y / 2.0;
        // Written field by field onto the *existing* `Node`, never as a fresh one:
        // an insert replaces the component, taking the root's size and every
        // child's placement with it. Repositioned every frame, not once, so a
        // descent that moves `requested` (re-centring on the pointer) is followed.
        if node.position_type != PositionType::Absolute {
            node.position_type = PositionType::Absolute;
        }
        if node.left != Val::Px(left) {
            node.left = Val::Px(left);
        }
        if node.top != Val::Px(top) {
            node.top = Val::Px(top);
        }
        // **Reveal only once the layout has settled.** The menu is spawned hidden
        // and grows over a frame or two as its labels are measured and
        // `fit_pie_layout` sizes the ring around them, so its top-left keeps
        // moving. Revealing on the first measured frame showed the menu at a
        // half-formed size and position, which then visibly jumped as it settled —
        // the flicker "down and to the right of the cursor, then it snaps back".
        // Waiting for two consecutive frames to agree costs a frame or two of an
        // already-hidden menu and removes the jump.
        if !placement.placed {
            let here = Vec2::new(left, top);
            let settled = placement
                .settled_at
                .is_some_and(|last| last.abs_diff_eq(here, 0.5));
            if settled {
                placement.placed = true;
                *visibility = Visibility::Inherited;
                // A live pie is opened with the button still held, so the gesture
                // is already under way.
                pie.interaction = PieInteraction::Flick;
            }
            placement.settled_at = Some(here);
        }
    }
}

/// Track the real pointer relative to each pie's ring centre, and pick the
/// highlighted slot from it.
///
/// **The real pointer is the only cursor**, and this is the whole of the
/// consequence: [`PieMenu::cursor`] is not a thing the widget *owns* and moves,
/// it is a reading of where the pointer is, recomputed every frame. See the
/// module's placement section for the decision behind that and what it costs.
///
/// The ring's centre — not the menu box's — because that is what every angle is
/// measured from, and the two are only the same while the label columns happen to
/// be symmetric.
fn drive_pie_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    rings: Query<&UiGlobalTransform, With<PieRing>>,
    mut pies: Query<(&mut PieMenu, &PieGeometry, &ComputedNode, &Children)>,
) {
    let Some(pointer) = windows.iter().next().and_then(Window::cursor_position) else {
        // The pointer has left the window. Nothing is under it, so nothing is
        // highlighted — and a click cannot arrive here anyway.
        for (mut pie, _geometry, _computed, _children) in &mut pies {
            if pie.highlighted.is_some() {
                pie.highlighted = None;
            }
        }
        return;
    };
    for (mut pie, geometry, computed, children) in &mut pies {
        let Some(centre) = children
            .iter()
            .find_map(|child| rings.get(child).ok())
            .map(|ring| ring.translation)
        else {
            continue;
        };
        // `UiGlobalTransform` is physical; the window's cursor position is
        // logical. One conversion, here.
        let scale = computed.inverse_scale_factor;
        pie.cursor = Vec2::new(pointer.x - centre.x * scale, pointer.y - centre.y * scale);

        // The outer bound: none while flicking (a gesture lands whatever distance
        // it travels), the menu's measured half-extent once pinned (so there is an
        // outside to click). Measured, so the labels are inside it — pointing at a
        // label picks its slot rather than aborting.
        let bound = match pie.interaction {
            PieInteraction::Flick => None,
            PieInteraction::Pinned => Some(f32::max(
                computed.size.x * scale / 2.0,
                computed.size.y * scale / 2.0,
            )),
        };
        let picked = pick(ui_offset(pie.cursor), *geometry, bound);
        if pie.highlighted != picked {
            pie.highlighted = picked;
        }
    }
}

/// Commit on the mouse button.
///
/// The two modes differ only here, and only in what a release with nothing
/// highlighted means: while flicking it pins the menu open (the reference's
/// "borderless click"), and once pinned it aborts.
fn commit_pie_selection(
    mouse: Res<ButtonInput<MouseButton>>,
    mut commands: Commands,
    mut pies: Query<(Entity, &mut PieMenu, &PieConditions), With<PiePlacement>>,
    mut actions: MessageWriter<UiAction>,
) {
    let released = mouse.any_just_released([MouseButton::Left, MouseButton::Right]);
    if !released {
        return;
    }
    for (entity, mut pie, conditions) in &mut pies {
        // Only a highlight that resolves to an **enabled** slot is a real target.
        // An empty or disabled slice has nothing to pick — it reads like the dead
        // zone — so a click on one dismisses (or, mid-flick, pins) exactly as a
        // dead-zone click does, rather than sitting there doing nothing.
        let highlighted = pie.highlighted;
        let target = highlighted.and_then(|point| {
            let current = pie.current()?;
            resolve_slots(current, conditions)
                .get(point.slot())
                .copied()
                .flatten()
                .filter(|slot| slot.enabled)
                .map(|_slot| point)
        });
        match (pie.interaction, target) {
            // A flick that landed on nothing pickable: the user opened the menu and
            // let go without choosing, which means they want to read it. Pin it.
            (PieInteraction::Flick, None) => pie.interaction = PieInteraction::Pinned,
            // A click on nothing pickable, with the menu already open: dismiss. The
            // dead zone, the empty slices, the disabled ones and the outside all
            // land here — anywhere that is not a live choice closes the menu.
            (PieInteraction::Pinned, None) => {
                commands.entity(entity).despawn();
            }
            (PieInteraction::Flick | PieInteraction::Pinned, Some(_)) => {
                apply_pie_selection(
                    &mut commands,
                    entity,
                    &mut pie,
                    conditions,
                    true,
                    &mut actions,
                );
            }
        }
    }
}

/// Act on whatever the pie currently highlights: emit an action and close, or
/// descend a level.
///
/// The one place a selection is turned into an outcome, called from
/// [`commit_pie_selection`] — so a slice means the same thing however the release
/// arrived at it.
///
/// `live` is whether this pie was *opened* (see [`PiePlacement`]) rather than
/// spawned in flow as a specimen. A specimen still emits its action — that is the
/// registry's no-wiring contract, and it is what a test reads — but it does not
/// close, because a card in the gallery was never opened and there is nothing for
/// it to close back to.
fn apply_pie_selection(
    commands: &mut Commands,
    entity: Entity,
    pie: &mut PieMenu,
    conditions: &PieConditions,
    live: bool,
    actions: &mut MessageWriter<UiAction>,
) {
    let Some(point) = pie.highlighted else {
        return;
    };
    let Some(current) = pie.current() else {
        return;
    };
    let slots = resolve_slots(current, conditions);
    let Some(slot) = slots.get(point.slot()).copied().flatten() else {
        return;
    };
    if !slot.enabled {
        return;
    }
    match slot.outcome {
        SlotOutcome::Action(action) => {
            actions.write(UiAction {
                element: pie.element,
                action,
            });
            if live {
                commands.entity(entity).despawn();
            }
        }
        SlotOutcome::SubPie(_) => {
            pie.path.push(point);
            // The parent's highlight must not survive into the child.
            pie.drop_highlight();
            // A sub-pie is read, not flicked into: the gesture that opened it has
            // ended.
            pie.interaction = PieInteraction::Pinned;
            // **Re-centre the sub-pie on the pointer.** We cannot warp the pointer
            // to the new centre (the reference does; Wayland forbids it), so we do
            // the inverse — move the centre to the pointer. Without this the
            // pointer is left out at the slice just picked and the sub-pie opens
            // with a slice already chosen in that direction. A specimen (no
            // `PiePlacement`) is not placed and so is not re-centred; the marker is
            // simply ignored there.
            commands.entity(entity).insert(PieRecenterOnPointer);
        }
    }
}

/// A one-shot marker: a pie that has just descended and must re-centre itself on
/// the current pointer. Consumed by [`recenter_pie_on_pointer`].
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PieRecenterOnPointer;

/// Move a just-descended pie's requested centre to the current pointer, so the
/// sub-pie opens *around* the pointer rather than off to one side of it.
///
/// The inverse of the reference's pointer warp, and the only half of it available
/// on a platform that will not move the cursor. It updates `requested`;
/// [`place_pie_menu`] repositions to it every frame, so the menu slides its centre
/// under the pointer. `drive_pie_cursor` then reads the cursor as ~centre next
/// frame, and the dead zone means nothing is selected until the pointer moves —
/// exactly the fresh state a newly opened pie has.
fn recenter_pie_on_pointer(
    mut commands: Commands,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut pies: Query<(Entity, &mut PiePlacement), With<PieRecenterOnPointer>>,
) {
    let pointer = windows.iter().next().and_then(Window::cursor_position);
    for (entity, mut placement) in &mut pies {
        if let Some(pointer) = pointer {
            placement.requested = pointer;
        }
        commands.entity(entity).remove::<PieRecenterOnPointer>();
    }
}

/// Close an open pie when the window loses focus.
///
/// A menu the user has alt-tabbed away from is not a menu they are choosing from,
/// and one left open behind another window is a trap; closing it on focus loss is
/// the same instinct as closing it on `Escape`.
fn abort_pie_on_focus_loss(
    mut focused: MessageReader<WindowFocused>,
    mut commands: Commands,
    pies: Query<Entity, With<PieMenu>>,
) {
    if !focused.read().any(|event| !event.focused) {
        return;
    }
    for pie in &pies {
        commands.entity(pie).despawn();
    }
}

/// Rebuild the labels when the pie descends a level.
fn update_pie_labels(
    mut commands: Commands,
    mut pies: Query<(Entity, &PieMenu, &PieConditions, &mut DisplayedPiePath), Changed<PieMenu>>,
    labels: Query<Entity, With<PieLabel>>,
    children: Query<&Children>,
) {
    for (entity, pie, conditions, mut displayed) in &mut pies {
        // **Rebuild only when the path changed**, not on every `PieMenu` change.
        // This is load-bearing: `drive_pie_cursor` writes the cursor every frame,
        // so `PieMenu` is `Changed` every frame, and rebuilding the labels each
        // time would despawn and respawn them before `fit_pie_layout`'s placement
        // could ever be consumed by layout — the labels would pile at the root's
        // origin, laid out from scratch every frame. The labels' *content* depends
        // only on which pie is showing, which is the path; the cursor moves the
        // highlight, not the menu.
        if displayed.0 == pie.path {
            continue;
        }
        let Some(current) = pie.current() else {
            continue;
        };
        // Only the labels: the ring (and its material) must survive a descent, or
        // the menu would blink at every level.
        if let Ok(existing) = children.get(entity) {
            for child in existing.iter() {
                if labels.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }
        rebuild_pie_labels(&mut commands, entity, ElementCx::new(), current, conditions);
        displayed.0.clone_from(&pie.path);
    }
}

/// Push each pie's live state into its ring material.
fn drive_pie_material(
    pies: Query<(&PieMenu, &PieConditions, &PieGeometry, &Children)>,
    rings: Query<(&MaterialNode<PieMenuMaterial>, &ComputedNode), With<PieRing>>,
    mut materials: ResMut<Assets<PieMenuMaterial>>,
) {
    for (pie, conditions, geometry, children) in &pies {
        let Some(current) = pie.current() else {
            continue;
        };
        let slots = resolve_slots(current, conditions);
        let states = pack_slot_states(&slots);
        let highlighted = pie
            .highlighted
            .and_then(|point| i32::try_from(point.slot()).ok())
            .unwrap_or(-1);
        for child in children.iter() {
            let Ok((node, computed)) = rings.get(child) else {
                continue;
            };
            let Some(mut material) = materials.get_mut(&node.0) else {
                continue;
            };
            // The shader works in physical pixels (the node's own space), while
            // everything above is logical.
            let scale = if computed.inverse_scale_factor > 0.0 {
                1.0 / computed.inverse_scale_factor
            } else {
                1.0
            };
            material.params.inner_radius = geometry.dead_zone * scale;
            material.params.outer_radius = geometry.outer * scale;
            material.params.slot_states = states;
            material.params.highlighted = highlighted;
        }
    }
}

// ---------------------------------------------------------------------------
// The fixture menu.
//
// Deliberately **not** a real viewer menu: which entries any given pie holds is
// per-domain and belongs with the domain (`viewer-object-context-menu`). This
// exists so the widget has something to be checked against — and it is chosen to
// exercise every case the mechanism claims to handle, rather than to look
// plausible: a plain action, a conditional (disabled) action, an autohide chain,
// a named sub-pie, a nested sub-pie two levels deep, and a **deliberately empty
// slot**, which is the one that proves nothing rotates.
// ---------------------------------------------------------------------------

/// The condition naming a sat-down avatar, for the fixture's autohide chain.
pub(crate) const FIXTURE_SITTING: &str = "sitting";

/// The condition naming an object the avatar may edit.
pub(crate) const FIXTURE_CAN_EDIT: &str = "can-edit";

/// The fixture's nested sub-pie, two levels from the root.
static FIXTURE_LAND_PIE: PieMenuDef = PieMenuDef {
    // Named for what it groups, not for the fact that it is extra. There is no
    // `More >` here and there is nowhere to put one.
    label: "Land",
    entries: &[
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "About Land",
                action: "about-land",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::Action(PieAction {
                label: "Buy Land",
                action: "buy-land",
                when: None,
            }),
        },
    ],
};

/// The fixture's sub-pie.
static FIXTURE_MANAGE_PIE: PieMenuDef = PieMenuDef {
    label: "Manage",
    entries: &[
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Take Copy",
                action: "take-copy",
                when: None,
            }),
        },
        PieEntry {
            at: Compass::East,
            content: PieContent::SubPie(&FIXTURE_LAND_PIE),
        },
    ],
};

/// The fixture pie the registry and the gallery show. See the section comment
/// above for why each entry is here.
pub(crate) static FIXTURE_PIE: PieMenuDef = PieMenuDef {
    label: "Object",
    entries: &[
        PieEntry {
            at: Compass::North,
            content: PieContent::Action(PieAction {
                label: "Touch",
                action: "touch",
                when: None,
            }),
        },
        // The autohide chain: one position, two mutually exclusive candidates, so
        // the angle is the same whether you are sitting or standing. This is the
        // reference's own example, and the case its counter-driven implementation
        // gets wrong.
        PieEntry {
            at: Compass::East,
            content: PieContent::Chain(&[
                PieAction {
                    label: "Stand Up",
                    action: "stand",
                    when: Some(FIXTURE_SITTING),
                },
                PieAction {
                    label: "Sit Here",
                    action: "sit",
                    when: None,
                },
            ]),
        },
        PieEntry {
            at: Compass::South,
            content: PieContent::SubPie(&FIXTURE_MANAGE_PIE),
        },
        // A conditional action: without the condition it is *disabled*, and it
        // keeps its slot either way.
        PieEntry {
            at: Compass::West,
            content: PieContent::Action(PieAction {
                label: "Edit",
                action: "edit",
                when: Some(FIXTURE_CAN_EDIT),
            }),
        },
        PieEntry {
            at: Compass::NorthWest,
            content: PieContent::Action(PieAction {
                label: "Open",
                action: "open",
                when: None,
            }),
        },
        // North-east, south-east and south-west are deliberately left empty. They
        // stay empty: no entry below them shuffles up, which is the whole claim.
    ],
};

/// The instruction on the live pie's target.
const PIE_TARGET_LABEL: &str = "Right-click anywhere on screen to open the pie menu. Flick a \
     direction and release; or release without moving to pin it open, then click a slice. Click \
     the inner circle (or right-click) to close; a slice that opens a sub-pie has a chevron on \
     its rim.";

/// The target's backdrop.
const PIE_TARGET_BACKGROUND: Color = Color::srgba(0.36, 0.72, 0.98, 0.10);

/// The target's border.
const PIE_TARGET_BORDER: Color = Color::srgb(0.36, 0.72, 0.98);

/// Spawn a surface that opens a **live** pie where you right-click it — the pie's
/// entry in [`crate::ui_element::ELEMENTS`].
///
/// A pie is not registered *as itself*, and that is deliberate. Almost everything
/// interesting about it is in the gesture — the placement near an edge, the dead
/// zone cancelling, the two interaction modes, descending into a sub-pie — none of
/// which a menu merely *drawn* on a gallery card can show; and a persistent
/// always-open pie sitting in a card behaves nothing like the real thing, which is
/// opened, used once and dismissed, one at a time. So the registered, gallery-shown,
/// matrix-swept element is this **target**: a plain card that opens a live pie on
/// a right-click. The pie's own layout is checked directly instead, by the tests
/// in this module that spawn it and run the harness's checks over it.
///
/// It works in the gallery and in the viewer for the same reason every other
/// element does: it reaches for no session. A right-click writes an
/// [`OpenPieMenu`], and who acts on that is the app's business.
pub(crate) fn spawn_radial_menu_target(
    commands: &mut Commands,
    parent: Entity,
    cx: ElementCx,
) -> Entity {
    commands
        .spawn((
            Node {
                // Room to open a pie in, and — deliberately — not much more: with
                // the target near the window's edge, a right-click in its corner is
                // the clamped-placement case, which is the one worth being able to
                // reach by hand.
                min_height: Val::Px(PIE_OUTER_RADIUS * 2.5),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                max_width: Val::Px(560.0),
                ..column(Val::Px(8.0))
            },
            BackgroundColor(PIE_TARGET_BACKGROUND),
            BorderColor::all(PIE_TARGET_BORDER),
            Name::new("radial-menu-target"),
            ChildOf(parent),
        ))
        .with_children(|target| {
            target.spawn((
                Text::new(cx.text(PIE_TARGET_LABEL)),
                cx.font(UiFont::Sans),
                TextColor(PIE_LABEL),
                Name::new("radial-menu-target-text"),
            ));
            // The menu's **address table**, on screen: every function, and the
            // gesture that reaches it.
            //
            // This is the thing worth being able to see. A pie's whole value is
            // that its addresses are stable, and a person cannot check that by
            // looking at the wheel — they would have to open every sub-pie and
            // remember. Printed, the contract is readable at a glance, and it is
            // the same table `tests::every_action_keeps_its_declared_address`
            // pins, from the same function.
            //
            // Not run through `cx.text`: an address is a fact about the menu, not
            // a string to translate, and the same reasoning the field grid's
            // numeric values are left literal under.
            target.spawn((
                Text::new(address_table(&FIXTURE_PIE)),
                cx.font(UiFont::Mono),
                TextColor(PIE_LABEL_SUB_PIE),
                Name::new("radial-menu-target-addresses"),
            ));
        })
        .observe(open_pie_on_right_click)
        .id()
}

/// Render a menu's address table, one function per line.
fn address_table(menu: &'static PieMenuDef) -> String {
    addresses(menu)
        .into_iter()
        .map(|(action, address)| format!("{action}: {address}"))
        .collect::<Vec<String>>()
        .join("\n")
}

/// Observer: a right-click on the target opens the fixture pie under the pointer.
///
/// The **secondary** button, which is what a context menu is bound to on every
/// desktop and in the reference viewer.
fn open_pie_on_right_click(press: On<Pointer<Press>>, mut requests: MessageWriter<OpenPieMenu>) {
    if press.button != PointerButton::Secondary {
        return;
    }
    requests.write(OpenPieMenu {
        menu: &FIXTURE_PIE,
        at: press.pointer_location.position,
        element: "radial-menu",
        // No session here, so nothing conditional holds. The live viewer's own
        // menus will fill this from the object under the pointer.
        conditions: &[],
    });
}

#[cfg(test)]
mod tests {
    use super::{
        Compass, FIXTURE_CAN_EDIT, FIXTURE_PIE, FIXTURE_SITTING, OpenPieMenu,
        PIE_LABEL_RING_RADIUS, PIE_OUTER_RADIUS, PIE_SLICES, PieAddress, PieConditions, PieContent,
        PieGeometry, PieMenu, PieMenuDef, PiePlacement, SlotOutcome, addresses, clamp_centre,
        pack_slot_states, pick, resolve_slots, ui_offset,
    };
    use crate::ui::UiDirection;
    use crate::ui_element::{ElementCx, SCRIPTS, SampleText, UiAction};
    use crate::ui_test::{LayoutTest, drain_actions, find_by_name, layout_violations, settle};
    use bevy::prelude::*;
    use bevy::window::PrimaryWindow;
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Every condition the fixture menu knows about — the axes
    /// [`no_condition_can_move_an_entry`] sweeps the powerset of.
    const FIXTURE_CONDITIONS: [&str; 2] = [FIXTURE_SITTING, FIXTURE_CAN_EDIT];

    // -----------------------------------------------------------------------
    // The address table: the muscle memory, pinned.
    // -----------------------------------------------------------------------

    /// **The regression test the whole widget exists to make possible.**
    ///
    /// Every function in the fixture menu, and the exact gesture that reaches it.
    /// This table is the contract: if a change moves a function, this test fails
    /// and names both the old address and the new one, which is precisely the
    /// review conversation that should happen. A pie that silently re-teaches
    /// every angle its users have learned is the failure mode the whole design is
    /// arranged against, and nothing but a pinned table catches it — the menu
    /// still looks perfectly reasonable afterwards.
    ///
    /// Note the two chain members share one address. That is not a quirk of the
    /// test; it is what an autohide chain *is*, and a table that gave them
    /// separate addresses would be describing a widget with a bug.
    #[test]
    fn every_action_keeps_its_declared_address() {
        // Listed in the order the walk takes: depth-first, in compass order at
        // every level — so `Manage`'s east sub-pie is exhausted before its west
        // entry. The order is the walk's, not the reader's; what is being pinned
        // is each address, and the ordering only has to be *stable*.
        let expected: Vec<(&str, Vec<Compass>)> = vec![
            ("stand", vec![Compass::East]),
            ("sit", vec![Compass::East]),
            ("touch", vec![Compass::North]),
            ("open", vec![Compass::NorthWest]),
            ("edit", vec![Compass::West]),
            (
                "about-land",
                vec![Compass::South, Compass::East, Compass::North],
            ),
            (
                "buy-land",
                vec![Compass::South, Compass::East, Compass::South],
            ),
            ("take-copy", vec![Compass::South, Compass::West]),
        ];
        let actual: Vec<(&str, Vec<Compass>)> = addresses(&FIXTURE_PIE)
            .into_iter()
            .map(|(action, PieAddress(path))| (action, path))
            .collect();
        assert_eq!(
            actual, expected,
            "a pie function moved. That is not a refactor — every user who had \
             learned this menu with their hand has just been re-taught it silently. \
             If the move is intended, change the table and say so in the commit."
        );
    }

    /// **No state can move an entry.** The claim, swept.
    ///
    /// For every subset of the live conditions, every action either sits at the
    /// compass point its declaration names, or is absent — never anywhere else.
    /// This is what the reference cannot say: its autohide chain's losing members
    /// skip the slot counter, so the number of slots a chain occupies depends on
    /// run-time state and every entry after it rotates.
    #[test]
    fn no_condition_can_move_an_entry() -> Result<(), TestError> {
        let declared = addresses(&FIXTURE_PIE);
        // The powerset of the conditions: two conditions is four worlds, and the
        // point is that *every* one of them agrees.
        for mask in 0..(1_u32 << FIXTURE_CONDITIONS.len()) {
            let held: Vec<&'static str> = FIXTURE_CONDITIONS
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    u32::try_from(*index).is_ok_and(|index| mask & (1 << index) != 0)
                })
                .map(|(_, condition)| *condition)
                .collect();
            let conditions = PieConditions::new(held.clone());

            for (action, PieAddress(path)) in &declared {
                let Some((last, parents)) = path.split_last() else {
                    continue;
                };
                let menu = follow_for_test(&FIXTURE_PIE, parents)
                    .ok_or("the fixture's address table names a path that does not exist")?;
                let slots = resolve_slots(menu, &conditions);
                // Wherever this action shows up at all, it must be in the slot it
                // declared — and in no other slot, ever.
                for point in Compass::ALL {
                    let Some(slot) = slots.get(point.slot()).copied().flatten() else {
                        continue;
                    };
                    if slot.outcome == SlotOutcome::Action(action) {
                        assert_eq!(
                            point,
                            *last,
                            "with conditions {held:?}, `{action}` resolved to {} but is \
                             declared at {} — a condition moved an entry, which is the \
                             one thing that must never happen",
                            point.name(),
                            last.name()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// A test-local re-implementation of the path walk, so the sweep above is not
    /// checking a function against itself.
    fn follow_for_test(menu: &'static PieMenuDef, path: &[Compass]) -> Option<&'static PieMenuDef> {
        let mut current = menu;
        for step in path {
            let entry = current.entries.iter().find(|entry| entry.at == *step)?;
            match entry.content {
                PieContent::SubPie(sub) => current = sub,
                PieContent::Action(_) | PieContent::Chain(_) => return None,
            }
        }
        Some(current)
    }

    /// Two entries at one position would be a silent overwrite: one of them would
    /// simply never appear, and which one would depend on declaration order — the
    /// coupling this design removes, sneaking back in through the door.
    #[test]
    fn no_pie_declares_two_entries_at_one_position() {
        fn check(menu: &'static PieMenuDef, failures: &mut Vec<String>) {
            for point in Compass::ALL {
                let count = menu
                    .entries
                    .iter()
                    .filter(|entry| entry.at == point)
                    .count();
                if count > 1 {
                    failures.push(format!(
                        "`{}` declares {count} entries at {}",
                        menu.label,
                        point.name()
                    ));
                }
            }
            for entry in menu.entries {
                if let PieContent::SubPie(sub) = entry.content {
                    check(sub, failures);
                }
            }
        }
        let mut failures = Vec::new();
        check(&FIXTURE_PIE, &mut failures);
        assert!(failures.is_empty(), "{failures:#?}");
    }

    /// An absent entry leaves its slice **empty** — it must never shift its
    /// neighbours round to close the gap.
    ///
    /// The fixture leaves north-east, south-east and south-west empty on purpose,
    /// so this is a real reading and not a tautology over an empty list.
    #[test]
    fn an_empty_slot_stays_empty() -> Result<(), TestError> {
        let slots = resolve_slots(&FIXTURE_PIE, &PieConditions::default());
        for point in [Compass::NorthEast, Compass::SouthEast, Compass::SouthWest] {
            assert!(
                slots.get(point.slot()).copied().flatten().is_none(),
                "{} is declared empty and must stay empty",
                point.name()
            );
        }
        // ... and the entries around them are exactly where they were declared,
        // rather than having closed up.
        let north = slots
            .get(Compass::North.slot())
            .copied()
            .flatten()
            .ok_or("north lost its entry")?;
        assert_eq!(north.outcome, SlotOutcome::Action("touch"));
        Ok(())
    }

    /// The autohide chain: one position, whichever member wins, and an empty slot
    /// when none does.
    #[test]
    fn an_autohide_chain_holds_one_position_whatever_wins() -> Result<(), TestError> {
        // Standing: the conditional `Stand Up` loses, the unconditional `Sit Here`
        // takes the slot.
        let standing = resolve_slots(&FIXTURE_PIE, &PieConditions::default());
        let slot = standing
            .get(Compass::East.slot())
            .copied()
            .flatten()
            .ok_or("east lost its chain")?;
        assert_eq!(slot.outcome, SlotOutcome::Action("sit"));

        // Sitting: `Stand Up` wins — at the very same compass point, which is the
        // entire purpose of a chain.
        let sitting = resolve_slots(&FIXTURE_PIE, &PieConditions::new([FIXTURE_SITTING]));
        let slot = sitting
            .get(Compass::East.slot())
            .copied()
            .flatten()
            .ok_or("east lost its chain")?;
        assert_eq!(slot.outcome, SlotOutcome::Action("stand"));

        // And nothing else moved between the two worlds.
        for point in [Compass::North, Compass::West, Compass::NorthWest] {
            assert_eq!(
                standing
                    .get(point.slot())
                    .copied()
                    .flatten()
                    .map(|s| s.outcome),
                sitting
                    .get(point.slot())
                    .copied()
                    .flatten()
                    .map(|s| s.outcome),
                "{} moved when the chain's winner changed",
                point.name()
            );
        }
        Ok(())
    }

    /// A chain with no winner leaves its slot empty rather than collapsing —
    /// stated directly, on a chain built for the purpose, because the fixture's
    /// own chain has an unconditional fallback and so always has a winner.
    #[test]
    fn a_chain_with_no_winner_leaves_its_slot_empty() {
        static NO_WINNER: PieMenuDef = PieMenuDef {
            label: "Test",
            entries: &[
                super::PieEntry {
                    at: Compass::East,
                    content: PieContent::Chain(&[super::PieAction {
                        label: "Only If",
                        action: "only-if",
                        when: Some("never"),
                    }]),
                },
                super::PieEntry {
                    at: Compass::North,
                    content: PieContent::Action(super::PieAction {
                        label: "Always",
                        action: "always",
                        when: None,
                    }),
                },
            ],
        };
        let slots = resolve_slots(&NO_WINNER, &PieConditions::default());
        assert!(
            slots.get(Compass::East.slot()).copied().flatten().is_none(),
            "a chain with no winner must leave its slot empty"
        );
        assert_eq!(
            slots
                .get(Compass::North.slot())
                .copied()
                .flatten()
                .map(|slot| slot.outcome),
            Some(SlotOutcome::Action("always")),
            "the entry after an empty chain must not move into it"
        );
    }

    /// A disabled entry keeps its slot and its label. It is *here*, and it is not
    /// available — two different facts, and folding them together is what would
    /// make an angle depend on state.
    #[test]
    fn a_disabled_entry_keeps_its_position() -> Result<(), TestError> {
        for (conditions, enabled) in [
            (PieConditions::default(), false),
            (PieConditions::new([FIXTURE_CAN_EDIT]), true),
        ] {
            let slots = resolve_slots(&FIXTURE_PIE, &conditions);
            let slot = slots
                .get(Compass::West.slot())
                .copied()
                .flatten()
                .ok_or("`Edit` lost its slot when it was disabled — it must keep it")?;
            assert_eq!(slot.outcome, SlotOutcome::Action("edit"));
            assert_eq!(slot.enabled, enabled);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The angle maths.
    // -----------------------------------------------------------------------

    /// Our nearest-centre partition and the reference's rotate-and-floor are the
    /// same function.
    ///
    /// The reference is the specification here — this is muscle memory, so
    /// "close enough" is not a thing — and the two are written differently enough
    /// (we avoid the float-to-integer conversion the lints forbid) that agreeing
    /// by inspection is not good enough either. So: a fine sweep of the whole
    /// circle, against a literal transcription of `piemenu.cpp`'s `handleHover`.
    #[test]
    fn the_partition_matches_the_reference_formula() {
        /// `piemenu.cpp` `handleHover`, transcribed: rotate by half a slice, then
        /// floor the division.
        fn reference_slot(angle: f32) -> usize {
            let rotated = angle + core::f32::consts::PI / 8.0;
            let wrapped = rotated.rem_euclid(core::f32::consts::TAU);
            let floored = (8.0 * wrapped / core::f32::consts::TAU).floor();
            // The one line that is not a literal transcription: the reference's
            // float-to-integer cast, spelled as a search for the integer equal to
            // the floored value. The workspace forbids the cast; the two are the
            // same for a value the reference's own arithmetic keeps in `0..8`.
            let slot = (0_u8..8)
                .find(|candidate| (f32::from(*candidate) - floored).abs() < 0.5)
                .unwrap_or(0);
            usize::from(slot % 8)
        }

        // `u16`, so the counter converts to `f32` losslessly and infallibly
        // rather than through a cast.
        let steps: u16 = 4_000;
        for step in 0..steps {
            let fraction = f32::from(step) / f32::from(steps);
            let angle = fraction * core::f32::consts::TAU;
            // Skip the boundaries themselves, where the two tie and a float's last
            // bit decides: a user cannot aim at a boundary to a millionth of a
            // radian, so the tie-break is not a behaviour worth pinning.
            let slice = core::f32::consts::TAU / 8.0;
            let offset = (angle + slice / 2.0).rem_euclid(slice);
            if offset < 1e-3 || slice - offset < 1e-3 {
                continue;
            }
            assert_eq!(
                Compass::from_angle(angle).slot(),
                reference_slot(angle),
                "at {angle} rad our partition and the reference's disagree"
            );
        }
    }

    /// Each compass point's own centre resolves back to itself, and the slice
    /// centres are on the axes rather than straddling them — which is what the
    /// reference's half-slice rotation is for, and the reason "due north" is a
    /// direction a hand can hit.
    #[test]
    fn a_slice_centre_resolves_to_its_own_point() {
        for point in Compass::ALL {
            assert_eq!(
                Compass::from_angle(point.centre_angle()),
                point,
                "{} does not resolve to itself",
                point.name()
            );
        }
        // Due north, exactly, in `bevy_ui`'s y-down space.
        assert_eq!(
            Compass::from_angle(ui_offset(Vec2::new(0.0, -50.0)).to_angle()),
            Compass::North,
            "straight up the screen must be north"
        );
        assert_eq!(
            Compass::from_angle(ui_offset(Vec2::new(50.0, 0.0)).to_angle()),
            Compass::East
        );
        assert_eq!(
            Compass::from_angle(ui_offset(Vec2::new(0.0, 50.0)).to_angle()),
            Compass::South
        );
        assert_eq!(
            Compass::from_angle(ui_offset(Vec2::new(-50.0, 0.0)).to_angle()),
            Compass::West
        );
    }

    /// Slot numbers are distinct and cover 0..8 — the wire the shader, the tab
    /// order and the tests all share.
    #[test]
    fn every_compass_point_has_its_own_slot() {
        let mut slots: Vec<usize> = Compass::ALL.into_iter().map(Compass::slot).collect();
        slots.sort_unstable();
        assert_eq!(slots, (0..PIE_SLICES).collect::<Vec<usize>>());
    }

    /// The dead zone selects nothing, so opening the menu and releasing without
    /// moving cancels — and, because the virtual cursor starts at the centre, the
    /// menu never opens with a slice already chosen.
    #[test]
    fn the_dead_zone_selects_nothing() {
        let geometry = PieGeometry::default();
        assert_eq!(pick(Vec2::ZERO, geometry, None), None);
        for slot in 0_u8..8 {
            let angle = f32::from(slot) * core::f32::consts::TAU / 8.0;
            let inside = Vec2::new(
                angle.cos() * (geometry.dead_zone - 1.0),
                angle.sin() * (geometry.dead_zone - 1.0),
            );
            assert_eq!(
                pick(inside, geometry, None),
                None,
                "inside the dead zone must select nothing, in every direction"
            );
        }
    }

    /// A flick lands at **any** distance; a pinned menu has an outside to click.
    ///
    /// The two halves of the interaction model, which look contradictory until you
    /// notice they are different modes.
    #[test]
    fn a_flick_has_no_outer_bound_but_a_pinned_menu_does() {
        let geometry = PieGeometry::default();
        let far = Vec2::new(0.0, 4_000.0);
        assert_eq!(
            pick(far, geometry, None),
            Some(Compass::North),
            "a flick must land whatever distance it travels — that is the gesture"
        );
        assert_eq!(
            pick(far, geometry, Some(geometry.outer)),
            None,
            "a pinned menu must have an outside, or there is nowhere to click to abort"
        );
        // Just inside the bound still picks, so the "outside" starts exactly where
        // the menu ends.
        let near = Vec2::new(0.0, geometry.outer - 1.0);
        assert_eq!(
            pick(near, geometry, Some(geometry.outer)),
            Some(Compass::North)
        );
    }

    // -----------------------------------------------------------------------
    // Placement.
    // -----------------------------------------------------------------------

    /// A pie asked for in the middle of a roomy window is placed exactly where it
    /// was asked for — no clamp, and so no cursor jump.
    #[test]
    fn a_pie_with_room_is_not_moved() {
        let requested = Vec2::new(800.0, 600.0);
        assert_eq!(
            clamp_centre(
                requested,
                Vec2::ZERO,
                Vec2::new(300.0, 300.0),
                Vec2::new(1600.0, 1200.0)
            ),
            requested,
            "a pie with room must not move — the user clicked there"
        );
    }

    /// A pie asked for in a corner is clamped inward until the **whole menu** —
    /// labels included — is on screen. Both edges bite at once, which is the case
    /// a one-axis fix silently gets wrong.
    #[test]
    fn a_pie_in_a_corner_is_clamped_on_both_axes() {
        let size = Vec2::new(300.0, 200.0);
        let viewport = Vec2::new(1600.0, 1200.0);
        let placed = clamp_centre(Vec2::new(4.0, 2.0), Vec2::ZERO, size, viewport);
        assert_eq!(placed, Vec2::new(150.0, 100.0));
        // The box it implies is exactly on screen, flush to both edges.
        let low = Vec2::new(placed.x - size.x / 2.0, placed.y - size.y / 2.0);
        assert_eq!(low, Vec2::ZERO);

        let placed = clamp_centre(Vec2::new(1_599.0, 1_199.0), Vec2::ZERO, size, viewport);
        assert_eq!(placed, Vec2::new(1_450.0, 1_100.0));
    }

    /// The clamp respects an **off-centre ring**: the label columns are
    /// content-sized and need not be symmetric, so the ring is not necessarily at
    /// the box's centre — and it is the ring, not the box, that every angle is
    /// measured from.
    #[test]
    fn the_clamp_places_the_ring_not_the_box() {
        // The ring sits 40 px right of the box centre, so the box hangs further to
        // the left and the ring must be kept further from the left edge.
        let ring_offset = Vec2::new(40.0, 0.0);
        let size = Vec2::new(300.0, 200.0);
        let viewport = Vec2::new(1600.0, 1200.0);
        let placed = clamp_centre(Vec2::new(0.0, 600.0), ring_offset, size, viewport);
        assert!(
            (placed.x - 190.0).abs() < 0.001,
            "150 px of half-box, plus the 40 px offset, but the ring landed at {}",
            placed.x
        );
        // And the box that implies is flush to the edge, not over it.
        let box_centre = placed.x - ring_offset.x;
        assert!(
            (box_centre - size.x / 2.0).abs() < 0.001,
            "the box must sit flush to the edge, not over it"
        );
    }

    /// A menu too big for the window cannot be placed legally; it is centred, so
    /// it loses the same amount at both ends rather than all of it at one.
    #[test]
    fn a_pie_larger_than_the_window_is_centred() {
        let placed = clamp_centre(
            Vec2::new(10.0, 10.0),
            Vec2::ZERO,
            Vec2::new(900.0, 900.0),
            Vec2::new(800.0, 800.0),
        );
        assert_eq!(placed, Vec2::new(400.0, 400.0));
    }

    // -----------------------------------------------------------------------
    // The shader's wire.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // The widget in a real (headless) layout, through the shared harness.
    // -----------------------------------------------------------------------

    /// The fixture pie, spawned through the registry into a real layout.
    /// A headless app with the fixture pie spawned **in flow** and settled.
    ///
    /// The pie is spawned directly rather than through the registry: it is not a
    /// registered element (the registered one is the right-click *target* — see
    /// [`super::spawn_radial_menu_target`]), so its layout is checked here, where
    /// the fixture can be spawned and driven with nothing behind it. A pie spawned
    /// this way carries no `PiePlacement`, so `place_pie_menu` leaves it in the
    /// flow where the layout checks can measure it.
    fn pie_app(direction: UiDirection) -> Result<App, TestError> {
        let test = LayoutTest::new().with_direction(direction);
        let mut app = test.build();
        crate::ui_test::enable_action_recording(&mut app);
        app.add_systems(
            Startup,
            (|mut commands: Commands, root: Res<crate::ui::UiRoot>| {
                super::spawn_pie_menu(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    &FIXTURE_PIE,
                    "radial-menu",
                    PieConditions::default(),
                );
            })
            .after(crate::ui::UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        Ok(app)
    }

    /// A headless app with the fixture pie spawned as a **live** menu (it carries
    /// a `PiePlacement`, so the interaction systems act on it) and the mouse
    /// commit + label rebuild wired up, ready for [`commit_select`].
    ///
    /// `drive_pie_cursor` is deliberately *not* added: it would overwrite the
    /// highlight from the (absent) window pointer every frame, so the test sets the
    /// highlight itself and the commit reads it — which is exactly the state a real
    /// frame hands the commit, minus the pointer plumbing.
    fn live_pie_app() -> Result<App, TestError> {
        let mut app = LayoutTest::new().build();
        crate::ui_test::enable_action_recording(&mut app);
        app.init_resource::<ButtonInput<MouseButton>>()
            .add_systems(
                Startup,
                (|mut commands: Commands, root: Res<crate::ui::UiRoot>| {
                    let pie = super::spawn_pie_menu(
                        &mut commands,
                        root.0,
                        ElementCx::new(),
                        &FIXTURE_PIE,
                        "radial-menu",
                        PieConditions::default(),
                    );
                    commands.entity(pie).insert(PiePlacement {
                        requested: Vec2::new(400.0, 300.0),
                        placed: true,
                        settled_at: Some(Vec2::ZERO),
                    });
                })
                .after(crate::ui::UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (super::commit_pie_selection, super::update_pie_labels),
            );
        settle(&mut app);
        Ok(app)
    }

    /// Point the pie at `point` and release the mouse, as a click on that slice
    /// does: the highlight is what the pointer would have set, the release is what
    /// [`commit_pie_selection`] acts on.
    fn commit_select(app: &mut App, point: Compass) -> Result<(), TestError> {
        let pie = find_by_name(app, "pie-menu").ok_or("the pie did not spawn")?;
        {
            let mut menu = app
                .world_mut()
                .get_mut::<PieMenu>(pie)
                .ok_or("the pie lost its state")?;
            menu.highlighted = Some(point);
            menu.interaction = super::PieInteraction::Pinned;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        // A second update so `record_actions` copies the emitted message out of the
        // queue before it is drained.
        app.update();
        Ok(())
    }

    /// Where a named node's box centre landed.
    fn centre_of(app: &mut App, name: &str) -> Result<Vec2, TestError> {
        let entity = find_by_name(app, name).ok_or_else(|| format!("no node named `{name}`"))?;
        let transform = app
            .world()
            .get::<UiGlobalTransform>(entity)
            .ok_or("the node has not been laid out")?;
        Ok(transform.translation)
    }

    /// **The compass must not mirror.** North-east stays top-right in an RTL
    /// layout.
    ///
    /// It is the one claim the layout matrix cannot make: the matrix checks that
    /// boxes are legal, and a perfectly legal layout with east and west swapped
    /// would sail through it while being completely broken. The slice under the
    /// pointer is picked from an *angle*, which no locale changes — so a label
    /// placed at the wrong screen corner would put the north-east label at the
    /// top-left while the north-east slice stayed at the top-right, and the picture
    /// and the maths would disagree in the one widget whose entire value is that
    /// they agree.
    ///
    /// The polar placement makes this true for free — a label's position comes
    /// from its compass angle, which `UiDirection` never touches — but "for free"
    /// is exactly the kind of thing that quietly stops being true, so it is pinned.
    #[test]
    fn the_compass_does_not_mirror_under_rtl() -> Result<(), TestError> {
        for direction in [UiDirection::Ltr, UiDirection::Rtl] {
            let mut app = pie_app(direction)?;
            let north_west = centre_of(&mut app, "pie-label:north-west")?;
            let ring = centre_of(&mut app, "pie-ring")?;
            let east = centre_of(&mut app, "pie-label:east")?;
            let north = centre_of(&mut app, "pie-label:north")?;

            assert!(
                north_west.x < ring.x,
                "{direction:?}: the north-west label must sit west of the ring, whatever the \
                 UI direction — a compass is screen geometry, not reading order"
            );
            assert!(
                east.x > ring.x,
                "{direction:?}: the east label must sit east of the ring"
            );
            assert!(
                north.y < ring.y,
                "{direction:?}: the north label must sit above the ring"
            );
            assert!(
                north_west.y < ring.y,
                "{direction:?}: the north-west label must sit above the ring"
            );
        }
        Ok(())
    }

    /// The ring lands on the menu's centre, so that every angle is measured from
    /// the middle of the thing the user is looking at.
    ///
    /// The root is a square the ring fills, so the two centres coincide by
    /// construction — but it is worth an assertion rather than a comment, because
    /// nothing about the layout would *look* wrong if it stopped holding: the menu
    /// would simply pick the wrong slices, slightly, near the diagonals.
    #[test]
    fn the_ring_sits_at_the_menu_centre() -> Result<(), TestError> {
        let mut app = pie_app(UiDirection::Ltr)?;
        let ring = centre_of(&mut app, "pie-ring")?;
        let menu = centre_of(&mut app, "pie-menu")?;
        assert!(
            (ring.x - menu.x).abs() < 1.0 && (ring.y - menu.y).abs() < 1.0,
            "the ring must sit at the menu's centre, but the ring is at {ring} and the menu \
             at {menu}"
        );
        Ok(())
    }

    /// An empty slot spawns no label — and nothing moves into its cell.
    #[test]
    fn an_empty_slot_draws_no_label() -> Result<(), TestError> {
        let mut app = pie_app(UiDirection::Ltr)?;
        for point in [Compass::NorthEast, Compass::SouthEast, Compass::SouthWest] {
            assert!(
                find_by_name(&mut app, &format!("pie-label:{}", point.name())).is_none(),
                "{} is empty and must draw no label",
                point.name()
            );
        }
        // The occupied ones are all still there, in their declared places.
        for point in [Compass::North, Compass::East, Compass::South, Compass::West] {
            assert!(
                find_by_name(&mut app, &format!("pie-label:{}", point.name())).is_some(),
                "{} is declared and must draw a label",
                point.name()
            );
        }
        Ok(())
    }

    /// Picking a slice emits exactly its action — the registry's no-wiring rule,
    /// on the pie.
    ///
    /// Driven through the real mouse commit — the sole pointer path — with nothing
    /// behind it that could touch a session.
    #[test]
    fn picking_a_slice_emits_its_action() -> Result<(), TestError> {
        let mut app = live_pie_app()?;
        // North is `Touch`.
        commit_select(&mut app, Compass::North)?;
        assert_eq!(
            drain_actions(&mut app),
            vec![UiAction {
                element: "radial-menu",
                action: "touch",
            }],
            "picking the north slice must emit exactly the action declared there"
        );
        Ok(())
    }

    /// Clicking a **disabled** slice emits nothing and **dismisses** the menu — it
    /// is here and can be aimed at, but there is nothing to pick, so a click reads
    /// like the dead zone.
    #[test]
    fn a_disabled_slice_emits_nothing_and_dismisses() -> Result<(), TestError> {
        let mut app = live_pie_app()?;
        // `Edit` sits at west and its condition does not hold here.
        commit_select(&mut app, Compass::West)?;
        assert_eq!(
            drain_actions(&mut app),
            vec![],
            "a disabled slice must do nothing when picked"
        );
        assert!(
            find_by_name(&mut app, "pie-menu").is_none(),
            "a click on a disabled slice must dismiss the menu, like a dead-zone click"
        );
        Ok(())
    }

    /// Clicking an **empty** slice — one no entry was declared at — also dismisses
    /// and emits nothing, for the same reason: there is nothing there to pick.
    #[test]
    fn an_empty_slice_dismisses() -> Result<(), TestError> {
        let mut app = live_pie_app()?;
        // North-east is left empty by the fixture.
        commit_select(&mut app, Compass::NorthEast)?;
        assert_eq!(
            drain_actions(&mut app),
            vec![],
            "an empty slice emits nothing"
        );
        assert!(
            find_by_name(&mut app, "pie-menu").is_none(),
            "a click on an empty slice must dismiss the menu"
        );
        Ok(())
    }

    /// **Descending into a sub-pie swaps the labels for the sub-pie's own**, and
    /// drops the parent's highlight.
    ///
    /// Note what is *not* claimed: the pointer does not move to the centre. It is
    /// not ours to move (see the module's placement section), so the sub-pie opens
    /// with whatever it holds in the direction the pointer is already pointing.
    /// The reference behaves the same way. What must not happen is the parent's
    /// highlight surviving into the child, which is a different menu.
    #[test]
    fn descending_into_a_sub_pie_swaps_the_labels() -> Result<(), TestError> {
        let mut app = live_pie_app()?;
        // `Manage` is the sub-pie at south.
        commit_select(&mut app, Compass::South)?;

        let pie = find_by_name(&mut app, "pie-menu").ok_or("the pie did not spawn")?;
        let menu = app
            .world()
            .get::<PieMenu>(pie)
            .ok_or("the pie closed when it should have descended")?;
        assert_eq!(
            menu.path,
            vec![Compass::South],
            "the pie must have descended"
        );
        assert_eq!(
            menu.interaction,
            super::PieInteraction::Pinned,
            "a sub-pie is read and clicked, not flicked into: the gesture that opened it ended"
        );
        assert_eq!(
            drain_actions(&mut app),
            vec![],
            "opening a sub-pie is navigation, not an action"
        );

        // The labels are the sub-pie's now — and at the sub-pie's own declared
        // positions.
        assert!(
            find_by_name(&mut app, "pie-label:west").is_some(),
            "`Take Copy` is declared at west in the `Manage` pie"
        );
        assert!(
            find_by_name(&mut app, "pie-label:north").is_none(),
            "the root's north entry must be gone: this is a different pie now"
        );
        Ok(())
    }

    /// **Opening a live pie must not despawn a specimen.**
    ///
    /// Found in the gallery: `open_pie_menus` closes any other open pie before
    /// spawning the new one, and it closed *every* `PieMenu` — including the
    /// always-shown specimen card, which is a pie in flow, not an open menu. So a
    /// right-click that opened a live pie deleted the card the user was looking at.
    /// The fix is that the close only reaches *live* pies (those carrying a
    /// `PiePlacement`); this holds it there.
    #[test]
    fn opening_a_live_pie_leaves_a_specimen_alone() -> Result<(), TestError> {
        let mut app = pie_app(UiDirection::Ltr)?;
        app.add_message::<OpenPieMenu>()
            .add_systems(Update, super::open_pie_menus);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        settle(&mut app);

        let specimen = find_by_name(&mut app, "pie-menu").ok_or("the specimen did not spawn")?;
        assert!(
            app.world().get::<PiePlacement>(specimen).is_none(),
            "a specimen is a pie in flow, with no placement — that is what marks it not-live"
        );

        app.world_mut().write_message(OpenPieMenu {
            menu: &FIXTURE_PIE,
            at: Vec2::new(640.0, 360.0),
            element: "radial-menu",
            conditions: &[],
        });
        settle(&mut app);

        assert!(
            app.world().get::<PieMenu>(specimen).is_some(),
            "opening a live pie must not despawn the always-shown specimen card"
        );
        Ok(())
    }

    /// **Moving the cursor must not rebuild the labels.**
    ///
    /// This exists because a bug that did exactly that shipped past every other
    /// test. `drive_pie_cursor` writes the cursor every frame, which marks
    /// `PieMenu` `Changed` every frame; an earlier `update_pie_labels` rebuilt the
    /// labels on *any* `PieMenu` change, so it despawned and respawned all eight
    /// every frame — faster than `fit_pie_layout` could place them, so they piled
    /// at the root's origin in the running viewer. The whole layout was visibly
    /// broken while the suite was green, because the harness does not run
    /// `drive_pie_cursor` and so never triggered the churn.
    ///
    /// The label content depends only on *which pie is showing* — the path — not
    /// on where the cursor is. So this changes the cursor, runs the rebuild
    /// system, and asserts the very same label entities are still there: a rebuild
    /// would have despawned them and minted new ids.
    #[test]
    fn moving_the_cursor_does_not_rebuild_the_labels() -> Result<(), TestError> {
        let mut app = pie_app(UiDirection::Ltr)?;
        app.add_systems(Update, super::update_pie_labels);

        let before: Vec<Entity> = Compass::ALL
            .into_iter()
            .filter_map(|point| find_by_name(&mut app, &format!("pie-label:{}", point.name())))
            .collect();
        assert!(
            before.len() >= 4,
            "the fixture pie must have spawned its labels first: {before:?}"
        );

        let pie = find_by_name(&mut app, "pie-menu").ok_or("the pie did not spawn")?;
        // Move the cursor and the highlight, exactly as `drive_pie_cursor` does
        // every frame, marking `PieMenu` changed without touching the path.
        {
            let mut menu = app
                .world_mut()
                .get_mut::<PieMenu>(pie)
                .ok_or("the pie lost its state")?;
            menu.cursor = Vec2::new(40.0, 10.0);
            menu.highlighted = Some(Compass::East);
        }
        settle(&mut app);

        let after: Vec<Entity> = Compass::ALL
            .into_iter()
            .filter_map(|point| find_by_name(&mut app, &format!("pie-label:{}", point.name())))
            .collect();
        assert_eq!(
            after, before,
            "the labels must be the same entities after a cursor move — a rebuild here \
             churns them faster than the layout can place them"
        );
        Ok(())
    }

    /// **A pie whose every label runs as long as a label plausibly can still means
    /// what it says.**
    ///
    /// The matrix cannot ask this, and it is worth being clear why rather than
    /// trusting that it does: `SampleText` swaps a string for one of its own
    /// length class, and every label in the fixture pie is button-length
    /// ("Touch", "Sit Here"), so every cell of the matrix hands the pie *short*
    /// samples. The radial checks run there, and pass, and prove nothing about the
    /// case that would break them.
    ///
    /// This is that case: the longest label a pie slice can honestly be given,
    /// at **every** point at once, in every script, pseudolocalised, at every
    /// font size. It stresses both things a long label can break. Width: a wide
    /// label at a fixed radius runs into its neighbours, which is why
    /// [`fit_pie_layout`] pushes the label radius out as the labels grow. Height:
    /// a label that wraps tall does the same on the other axis. Get either wrong
    /// and adjacent labels overlap into an unreadable pile, or a label's centre
    /// drifts far enough that its *angle* from the ring creeps toward a neighbour
    /// and it stops meaning its own slice.
    ///
    /// Both were real, and this found them (with the labels stacked in a grid, an
    /// earlier layout, they collided). See `PIE_LABEL_MAX_WIDTH` for the third
    /// thing it found — the ceiling on what a slice label can be — and why that one
    /// is a constraint on the *menu* rather than a bug in the widget.
    #[test]
    fn a_pie_with_long_labels_keeps_every_label_in_its_own_slice() -> Result<(), TestError> {
        /// About as long as a slice label ever honestly gets: a real reference
        /// entry ("Take Copy") at the length a translation stretches it to. Kept
        /// under `SampleText::PROSE_CHARS` on purpose, so a script cell swaps it
        /// for that script's *label*-length sample rather than a paragraph — the
        /// length class a pie label actually belongs to.
        const LONG: &str = "Take a Copy of This Object";
        static LONG_PIE: PieMenuDef = PieMenuDef {
            label: "Long",
            entries: &[
                super::PieEntry {
                    at: Compass::East,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "east",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::NorthEast,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "north-east",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::North,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "north",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::NorthWest,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "north-west",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::West,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "west",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::SouthWest,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "south-west",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::South,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "south",
                        when: None,
                    }),
                },
                super::PieEntry {
                    at: Compass::SouthEast,
                    content: PieContent::Action(super::PieAction {
                        label: LONG,
                        action: "south-east",
                        when: None,
                    }),
                },
            ],
        };

        let mut failures = Vec::new();
        let cells = SCRIPTS
            .iter()
            .map(SampleText::Script)
            // Pseudolocalisation is the one that actually lengthens rather than
            // substitutes: it keeps the label and makes it ~40% longer, which is
            // the real shape of "this shipped in English and then got translated".
            .chain([SampleText::Pseudo, SampleText::Native]);
        for cell in cells {
            let direction = match cell.name() {
                "Arabic" | "Hebrew" => UiDirection::Rtl,
                _other => UiDirection::Ltr,
            };
            let test = LayoutTest::new().with_direction(direction);
            for font_size in [11.0_f32, 15.0, 22.0] {
                let cx = ElementCx {
                    text: cell,
                    font_size,
                };
                let mut app = test.build();
                app.add_message::<UiAction>();
                app.add_systems(
                    Startup,
                    (move |mut commands: Commands, root: Res<crate::ui::UiRoot>| {
                        super::spawn_pie_menu(
                            &mut commands,
                            root.0,
                            cx,
                            &LONG_PIE,
                            "long-pie",
                            PieConditions::default(),
                        );
                    })
                    .after(crate::ui::UiScaffoldSystems::SpawnRoot),
                );
                settle(&mut app);
                let violations = layout_violations(&mut app, test);
                if !violations.is_empty() {
                    failures.push(format!(
                        "long labels in {} at {font_size}px ({direction:?}): {violations:#?}",
                        cell.name()
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        Ok(())
    }

    /// **A pie with tiny labels does not shrink below the base size.**
    ///
    /// The menu grows to fit longer labels — but it must never shrink *below* a
    /// comfortable minimum, or a language with single-character labels (Japanese,
    /// say) would get a wheel too small to aim at. The floor is the reference's
    /// `PIE_OUTER_SIZE`; this holds the size there for one-glyph labels, so the
    /// content-driven growth can only ever adapt *up*.
    #[test]
    fn a_pie_with_tiny_labels_keeps_a_minimum_size() -> Result<(), TestError> {
        // One CJK glyph at every compass point — about as small as a slice label
        // ever gets. Built in the body (a `static` cannot loop) and leaked to the
        // `'static` the widget wants; a test process is short-lived, so the leak is
        // bounded and harmless.
        let entries: Vec<super::PieEntry> = [
            (Compass::East, "e"),
            (Compass::NorthEast, "ne"),
            (Compass::North, "n"),
            (Compass::NorthWest, "nw"),
            (Compass::West, "w"),
            (Compass::SouthWest, "sw"),
            (Compass::South, "s"),
            (Compass::SouthEast, "se"),
        ]
        .into_iter()
        .map(|(at, action)| super::PieEntry {
            at,
            content: PieContent::Action(super::PieAction {
                label: "字",
                action,
                when: None,
            }),
        })
        .collect();
        let entries: &'static [super::PieEntry] = Box::leak(entries.into_boxed_slice());
        let menu: &'static PieMenuDef = Box::leak(Box::new(PieMenuDef {
            label: "Tiny",
            entries,
        }));

        let test = LayoutTest::new();
        let mut app = test.build();
        app.add_message::<UiAction>();
        app.add_systems(
            Startup,
            (move |mut commands: Commands, root: Res<crate::ui::UiRoot>| {
                super::spawn_pie_menu(
                    &mut commands,
                    root.0,
                    ElementCx::new(),
                    menu,
                    "tiny",
                    PieConditions::default(),
                );
            })
            .after(crate::ui::UiScaffoldSystems::SpawnRoot),
        );
        settle(&mut app);
        settle(&mut app);

        let pie = find_by_name(&mut app, "pie-menu").ok_or("the tiny pie did not spawn")?;
        let geometry = app
            .world()
            .get::<PieGeometry>(pie)
            .ok_or("the pie lost its geometry")?;
        assert!(
            (geometry.outer - PIE_OUTER_RADIUS).abs() < 0.5,
            "single-glyph labels must keep the base size ({PIE_OUTER_RADIUS} px), but the ring \
             came out at {} px",
            geometry.outer
        );
        Ok(())
    }

    /// **A pie that was actually opened keeps its compass grid.**
    ///
    /// This exists because an earlier version passed every test while the live
    /// menu was visibly broken. Every test spawned a *specimen* (a pie in flow),
    /// and the bug was in the other path: `open_pie_menus` inserted a fresh `Node`
    /// to make the menu absolute, and a component insert **replaces** — so the
    /// whole placed layout went with it and the labels came up stacked in a line.
    ///
    /// The lesson is the test, not the fix: a widget with two spawn paths needs
    /// checks on both, and "the fixture lays out correctly" says nothing about the
    /// path a user actually takes. So this drives the real `OpenPieMenu` request
    /// through the real systems and asserts the compass came out around the ring,
    /// in the right directions — which is the thing that was wrong.
    #[test]
    fn a_live_pie_places_its_labels_around_the_ring() -> Result<(), TestError> {
        let mut app = LayoutTest::new().build();
        app.add_message::<UiAction>()
            .add_message::<OpenPieMenu>()
            .add_systems(Update, super::open_pie_menus)
            .add_systems(
                PostUpdate,
                super::place_pie_menu.after(bevy::ui::UiSystems::Layout),
            );
        // `place_pie_menu` clamps against the window, so there has to be one.
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        settle(&mut app);

        app.world_mut().write_message(OpenPieMenu {
            menu: &FIXTURE_PIE,
            at: Vec2::new(640.0, 360.0),
            element: "radial-menu",
            conditions: &[],
        });
        // Three settles: open spawns, the next frame measures the labels, the one
        // after that places them — the same measure-then-place lag `fit_pie_layout`
        // has live.
        settle(&mut app);
        settle(&mut app);
        settle(&mut app);

        let pie = find_by_name(&mut app, "pie-menu").ok_or("the live pie did not open")?;
        assert_eq!(
            app.world()
                .get::<Node>(pie)
                .ok_or("the pie lost its `Node`")?
                .position_type,
            PositionType::Absolute,
            "a live pie is placed by inset, not by flow"
        );

        // The compass came out around the ring, in the right directions — the
        // failure this exists for was every label piling at one corner.
        let ring = centre_of(&mut app, "pie-ring")?;
        let north = centre_of(&mut app, "pie-label:north")?;
        let east = centre_of(&mut app, "pie-label:east")?;
        let south = centre_of(&mut app, "pie-label:south")?;
        let north_west = centre_of(&mut app, "pie-label:north-west")?;
        assert!(
            north.y < ring.y,
            "north must sit above the ring: ring {ring}, north {north}"
        );
        assert!(
            south.y > ring.y,
            "south must sit below the ring: ring {ring}, south {south}"
        );
        assert!(
            east.x > ring.x,
            "east must sit right of the ring: ring {ring}, east {east}"
        );
        assert!(
            north_west.x < ring.x && north_west.y < ring.y,
            "north-west must sit up and to the left: ring {ring}, north-west {north_west}"
        );
        // On a single label ring — symmetric — and inside the rim, which is the
        // whole point of the polar layout: the labels belong to the wedges rather
        // than floating past the edge. The east and west labels sit opposite each
        // other, so their radii must match, and the ring must have grown to keep
        // them inside it.
        let west = centre_of(&mut app, "pie-label:west")?;
        let east_radius = (east.x - ring.x).abs();
        let west_radius = (west.x - ring.x).abs();
        assert!(
            (east_radius - west_radius).abs() < 2.0,
            "opposite labels must sit on one ring: east at {east_radius} px, west at \
             {west_radius} px"
        );
        assert!(
            east_radius >= PIE_LABEL_RING_RADIUS - 2.0,
            "a label must sit out past the dead zone, on its wedge: {east_radius} px"
        );
        let geometry = app
            .world()
            .get::<PieGeometry>(pie)
            .ok_or("the pie lost its geometry")?;
        assert!(
            east_radius < geometry.outer,
            "a label's centre must sit inside the rim ({} px), not past it at {east_radius} px",
            geometry.outer
        );
        Ok(())
    }

    /// The packed slot states say what each slot is, in the nibble the shader
    /// reads. Packed by hand here, so a change to either side of the wire has to
    /// be a deliberate one.
    #[test]
    fn slot_states_pack_into_the_nibbles_the_shader_reads() {
        let slots = resolve_slots(&FIXTURE_PIE, &PieConditions::default());
        let packed = pack_slot_states(&slots);
        let nibble = |point: Compass| {
            u32::try_from(point.slot())
                .ok()
                .map(|slot| (packed >> (slot * 4)) & 0xF)
        };
        // 0 empty, 1 action, 2 disabled, 3 sub-pie — the WGSL `STATE_*` constants.
        assert_eq!(nibble(Compass::North), Some(1), "Touch is a plain action");
        assert_eq!(
            nibble(Compass::East),
            Some(1),
            "the chain resolved to Sit Here"
        );
        assert_eq!(nibble(Compass::South), Some(3), "Manage is a sub-pie");
        assert_eq!(nibble(Compass::West), Some(2), "Edit has no condition held");
        assert_eq!(nibble(Compass::NorthEast), Some(0), "north-east is empty");
    }
}
