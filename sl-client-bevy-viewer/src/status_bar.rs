//! The viewer's **status area** (`viewer-ui-status-bar`): the read-outs that
//! fill the top menu bar's row to its trailing edge — the parcel permission
//! icons, the region name, the agent's coordinates, the parcel name, the L$
//! balance, the grid time, and the frame rate.
//!
//! Split out of [`crate::menu_bar`], which shipped the bar and the menu names
//! but deliberately left this for its own pass: it is a distinct concern with
//! its own data sources (parcel flags, the money read-model, the region
//! identity, the frame diagnostics), not part of the menu mechanism.
//!
//! # Layout
//!
//! [`spawn_status_area`] hangs the read-outs off the (now full-width) menu bar,
//! after the menu-search field, as one flex item that grows to fill the row — so
//! the top row reads as one continuous bar spanning the whole window, the
//! reference viewer's arrangement. Its children run in the reference order
//! (`panel_status_bar.xml`): the parcel permission icons, the region name, the
//! coordinates, then the **flexible** parcel name (which absorbs the row's slack
//! and so pushes the rest to the trailing edge), then the balance, the time and
//! the FPS. Every element but the parcel name is **fixed-width**, so a value's
//! text length changing never shifts its neighbours (the row does not jitter as
//! the clock or FPS ticks). It reflows under a right-to-left locale (the flex
//! direction follows the writing mode) and a font-size change. Every number,
//! currency amount and timestamp is formatted through the locale-aware
//! [`Translator`](crate::i18n::Translator), never a bare `to_string`.
//!
//! The menu-search field stays where [`crate::menu_bar`] put it — inline in the
//! menu bar, after the last menu — rather than moving to the trailing edge as
//! the reference viewer does; only these read-outs live here.
//!
//! # Parcel permission icons
//!
//! The permission logic mirrors `LLStatusBar::updateParcelIcons` /
//! `LLViewerParcelMgr::allowAgent*`: a permission is "in force" when the ability
//! is **denied** on the current parcel (voice / fly / push / build / scripts /
//! see-avatars), except **damage**, which is in force when damage is **enabled**
//! (the hazard). The parcel comes from [`SlAgentParcel`], mirrored from the
//! CAPS/UDP `ParcelProperties` the session ingests (see the
//! `parcelproperties-via-caps-eventqueue` memory), combined with the current
//! region's [`RegionFlags`].
//!
//! Each permission is a bundled icon (`assets/icons/parcel/*.png`, sources beside
//! them) — an original glyph rather than the reference viewer's own art: a
//! slashed microphone, an up-arrow, an arrow into a wall, a cube, a document,
//! an eye, and a heart. Each icon is **shown only while its restriction is in
//! force** (the reference viewer's semantics — [`update_parcel_icons`] toggles
//! its [`Visibility`]), but its slot is always laid out, so the icon bar keeps a
//! constant width whether nothing or everything is restricted. The glyphs are
//! white-on-transparent masks tinted the skin's "loss" colour
//! ([`ImageNode::color`] via `-bevy-image-color`). A skin can override the tint,
//! or replace / re-tint an individual glyph through its per-icon class
//! ([`ParcelIcon::class`], e.g. `.sk-parcel-icon--voice`). The pathfinding-dirty
//! / -disabled icons the reference also shows are Second Life navmesh state the
//! viewer does not track yet, so they are omitted.
//!
//! # Time is always SLT
//!
//! The clock shows Second Life Time — US Pacific — regardless of the user's own
//! zone, exactly as the reference viewer does. [`slt`] converts the current UTC
//! instant into the Pacific wall-clock components (with the US daylight-saving
//! rules) that the locale formatter then renders.

use core::time::Duration;

use bevy::diagnostic::{Diagnostic, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    Command, LindenBalance, ParcelFlags, RegionFlags, SlAgentParcel, SlCommand, SlCurrentRegion,
    SlEvent, SlIdentity, SlRegionIdentity, SlSessionEvent, Vector,
};
use sl_l10n::{DateTimeLength, DateTimeStyle};
use sl_settings::SettingValue;

use crate::i18n::{TransArgs, Translator};
use crate::settings::ViewerSettings;
use crate::slt;
use crate::ui_font::UiFont;
use crate::ui_perf::FixedSlotContentSize;
use crate::world_api::{AgentRegionPosition, SHOW_COORDINATES_KEY};

/// The read-out font size, in logical pixels.
const STATUS_FONT_SIZE: f32 = 14.0;

/// The gap between adjacent read-outs, in logical pixels.
const STATUS_GAP: f32 = 12.0;

/// The gap between adjacent parcel icons, in logical pixels.
const ICON_GAP: f32 = 3.0;

/// The fixed width of the region-name read-out, in logical pixels. Long names
/// clip rather than push their neighbours (every element but the parcel name is
/// fixed-width so the row never jitters as a value's text length changes).
const REGION_WIDTH: f32 = 150.0;

/// The fixed width of the coordinate read-out, in logical pixels.
const COORDS_WIDTH: f32 = 120.0;

/// The fixed width of the balance read-out, in logical pixels.
const BALANCE_WIDTH: f32 = 84.0;

/// The fixed width of the time read-out, in logical pixels.
const TIME_WIDTH: f32 = 116.0;

/// The fixed width of the FPS read-out, in logical pixels.
const FPS_WIDTH: f32 = 60.0;

/// The side length of a parcel-permission icon, in logical pixels.
const ICON_SIZE: f32 = 16.0;

/// The CSS class on every parcel-permission icon, tinting it the skin's "loss"
/// colour (theme-driven rather than a hard-coded hue). Each icon's slot is
/// always laid out; only its [`Visibility`] toggles, so the icon shows only when
/// its restriction is in force while the icon bar keeps a constant width.
const ICON_CLASS: &str = "sk-parcel-icon";

/// Which parcel permission an icon reflects, in the reference viewer's
/// left-to-right order.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ParcelIcon {
    /// Voice chat is not allowed here.
    Voice,
    /// Flying is not allowed here.
    Fly,
    /// Pushing (`llPushObject`) against avatars is restricted here.
    Push,
    /// Building (object rez) is not allowed here.
    Build,
    /// Other people's scripts do not run here.
    Scripts,
    /// Avatars on the parcel are hidden from outside it.
    SeeAvatars,
    /// Damage (health / combat) is enabled here — a hazard, not a restriction.
    Damage,
}

impl ParcelIcon {
    /// Every icon, in display order.
    const ALL: [Self; 7] = [
        Self::Voice,
        Self::Fly,
        Self::Push,
        Self::Build,
        Self::Scripts,
        Self::SeeAvatars,
        Self::Damage,
    ];

    /// This icon's default bundled image asset path (relative to the viewer's
    /// asset root). Each is a white-on-transparent glyph mask, tinted at runtime
    /// by the skin ([`ICON_CLASS`]); the sources are the `.svg` files beside them.
    /// A skin can replace the glyph by setting `-bevy-image` on this icon's
    /// per-icon class ([`class`](Self::class)).
    const fn asset_path(self) -> &'static str {
        match self {
            Self::Voice => "icons/parcel/voice.png",
            Self::Fly => "icons/parcel/fly.png",
            Self::Push => "icons/parcel/push.png",
            Self::Build => "icons/parcel/build.png",
            Self::Scripts => "icons/parcel/scripts.png",
            Self::SeeAvatars => "icons/parcel/avatars.png",
            Self::Damage => "icons/parcel/damage.png",
        }
    }

    /// This icon's per-icon CSS class, so a skin can target one glyph — e.g.
    /// `.sk-parcel-icon--voice { -bevy-image: url("…"); }` to swap its art or
    /// re-tint just it. Every icon also carries the shared [`ICON_CLASS`].
    const fn class(self) -> &'static str {
        match self {
            Self::Voice => "sk-parcel-icon--voice",
            Self::Fly => "sk-parcel-icon--fly",
            Self::Push => "sk-parcel-icon--push",
            Self::Build => "sk-parcel-icon--build",
            Self::Scripts => "sk-parcel-icon--scripts",
            Self::SeeAvatars => "sk-parcel-icon--see-avatars",
            Self::Damage => "sk-parcel-icon--damage",
        }
    }
}

/// The current parcel + region context each parcel icon's visibility is computed
/// from — the client-side mirror of `LLViewerParcelMgr::allowAgent*`. Held as the
/// raw flags rather than a bool per icon (which the `struct_excessive_bools` lint
/// forbids, and which would duplicate the flag meanings anyway); [`shown`](Self::shown)
/// derives each icon on demand.
///
/// Each rule combines the region + parcel halves the viewer has: voice's region
/// "voice enabled" half is unavailable (so a region with voice wholly disabled
/// reads the same as a voice grid), making the voice icon a close, not bit-exact,
/// reproduction; the rest match the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParcelIcons {
    /// The current parcel's flags.
    parcel_flags: ParcelFlags,
    /// The parcel's `SeeAVs` state, or `None` when the UDP path omits it
    /// (treated as visible).
    see_avs: Option<bool>,
    /// The current region's flags.
    region: RegionFlags,
    /// Whether the agent may fly here — the region block-fly bit and the parcel
    /// allow-fly bit already combined ([`SlAgentParcel::can_fly`], the reference
    /// viewer's `LLAgent::canFly`).
    can_fly: bool,
}

impl ParcelIcons {
    /// Whether `icon` should be shown for this parcel + region.
    fn shown(&self, icon: ParcelIcon) -> bool {
        match icon {
            ParcelIcon::Voice => !self.parcel_flags.contains(ParcelFlags::ALLOW_VOICE),
            ParcelIcon::Fly => !self.can_fly,
            ParcelIcon::Push => {
                self.parcel_flags.contains(ParcelFlags::RESTRICT_PUSHOBJECT)
                    || self.region.contains(RegionFlags::RESTRICT_PUSHOBJECT)
            }
            ParcelIcon::Build => !self.parcel_flags.contains(ParcelFlags::CREATE_OBJECTS),
            ParcelIcon::Scripts => {
                !self.parcel_flags.contains(ParcelFlags::ALLOW_OTHER_SCRIPTS)
                    || self.region.contains(RegionFlags::SKIP_SCRIPTS)
                    || self.region.contains(RegionFlags::ESTATE_SKIP_SCRIPTS)
            }
            // `SeeAVs` defaults visible; only an explicit `false` hides avatars.
            ParcelIcon::SeeAvatars => self.see_avs == Some(false),
            ParcelIcon::Damage => {
                self.parcel_flags.contains(ParcelFlags::ALLOW_DAMAGE)
                    || self.region.contains(RegionFlags::ALLOW_DAMAGE)
            }
        }
    }
}

/// The agent's last-known L$ balance, folded from the money read-model
/// ([`SlSessionEvent::MoneyBalance`]). `None` until the first reply arrives; the
/// balance is requested on region entry and pushed by the simulator after a
/// transaction. On OpenSim the grid reports a hardcoded `0`; aditi / Second Life
/// is where it reads real.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct AgentBalance {
    /// The current balance, or `None` before the first reply.
    balance: Option<LindenBalance>,
}

/// Which read-out a status text node carries, so one update system can rewrite
/// every text node from a single `Query<(&StatusReadout, &mut Text)>` (several
/// `Query<&mut Text, With<_>>` in one system would be a conflicting access). In
/// the reference viewer's left-to-right order (after the parcel icons): region
/// name, coordinates, then the flexible parcel name, then the trailing balance /
/// time / FPS.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum StatusReadout {
    /// The region name.
    Region,
    /// The agent's region-local `⟨x, y, z⟩` coordinates.
    Coords,
    /// The parcel name — the flexible middle that absorbs the row's slack.
    ParcelName,
    /// The L$ balance.
    Balance,
    /// The grid (SLT) time of day.
    Time,
    /// The frame rate.
    Fps,
}

/// Marker on the L$ balance read-out text node, so the snapshot floater can find
/// and blank it for a shot without knowing this module's private
/// [`StatusReadout`] enum (`viewer-snapshot-hide-balance`). Its slot keeps its
/// fixed width, so hiding the text via [`Visibility::Hidden`] blanks the balance
/// without shifting the row — the reference viewer's "hide L$ balance in
/// snapshots" behaviour.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct BalanceReadout;

/// The status area's runtime: keep the balance / position read-models current
/// and rewrite the read-outs each frame. The row itself is spawned by
/// [`spawn_status_area`], invoked from [`crate::menu_bar`] so the read-outs share
/// the (full-width) menu bar's row.
pub(crate) struct StatusBarPlugin;

impl Plugin for StatusBarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AgentBalance>()
            .init_resource::<AgentRegionPosition>()
            .add_systems(Startup, register_status_bar_settings)
            .add_systems(
                Update,
                (
                    request_balance_on_entry,
                    track_balance,
                    track_agent_position,
                    // The read-outs are human-status text (region / coords /
                    // balance / time / FPS): a ~10 Hz refresh is imperceptible,
                    // but running the rewrite every frame re-dirties the FPS
                    // slot's `Text` on almost every frame (the FPS integer keeps
                    // changing), forcing bevy_ui to re-shape and re-measure that
                    // text (`text_system` / `measure_text_system`) each frame.
                    // Throttling to 10 Hz leaves most frames with no status-text
                    // change at all.
                    update_status_readouts.run_if(on_timer(Duration::from_millis(100))),
                    update_parcel_icons.run_if(parcel_icon_inputs_changed),
                ),
            );
    }
}

/// Startup system: declare the coordinate-display setting so a stored override
/// coerces against it. Defaults on, matching the reference viewer.
fn register_status_bar_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_in(
        &["statusbar"],
        SHOW_COORDINATES_KEY,
        SettingValue::Bool(true),
        "Show the agent's region-local coordinates in the status area's location \
         read-out",
    );
}

/// Spawn the status area as the trailing part of the top menu bar's row (called
/// from [`crate::menu_bar`], after the menu-search field).
///
/// The area is one flex item that grows to fill the row after the menus and
/// search ([`flex_grow`](Node::flex_grow) `1`), holding — in the reference
/// viewer's order — the parcel permission icons, the region name, the
/// coordinates, the **flexible** parcel name (which absorbs the row's slack and
/// so pushes the rest to the trailing edge), then the fixed-width balance, time
/// and FPS. Every element but the parcel name is fixed-width, so a value's text
/// length changing never shifts its neighbours.
pub(crate) fn spawn_status_area(commands: &mut Commands, asset_server: &AssetServer, bar: Entity) {
    let area = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(STATUS_GAP),
                overflow: Overflow::clip(),
                ..default()
            },
            Name::new("status-area"),
            ChildOf(bar),
        ))
        .id();

    // The parcel-permission icons run first (leading), grouped in one row so they
    // read as a block.
    let icons = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(ICON_GAP),
                flex_shrink: 0.0,
                ..default()
            },
            Name::new("status-parcel-icons"),
            ChildOf(area),
        ))
        .id();
    for icon in ParcelIcon::ALL {
        spawn_parcel_icon(commands, asset_server, icons, icon);
    }

    // The read-outs, in reference order. Region and coordinates are fixed-width
    // and leading-aligned; the parcel name is the flexible middle; balance / time
    // / FPS are fixed-width and trailing-aligned.
    spawn_readout(
        commands,
        area,
        StatusReadout::Region,
        Some(REGION_WIDTH),
        false,
    );
    spawn_readout(
        commands,
        area,
        StatusReadout::Coords,
        Some(COORDS_WIDTH),
        false,
    );
    spawn_readout(commands, area, StatusReadout::ParcelName, None, false);
    spawn_readout(
        commands,
        area,
        StatusReadout::Balance,
        Some(BALANCE_WIDTH),
        true,
    );
    spawn_readout(commands, area, StatusReadout::Time, Some(TIME_WIDTH), true);
    spawn_readout(commands, area, StatusReadout::Fps, Some(FPS_WIDTH), true);
}

/// Spawn one parcel-permission icon under `parent`. Its slot is always laid out
/// (so the icon bar keeps a constant width), but it starts hidden and
/// [`update_parcel_icons`] shows it only while its restriction is in force. The
/// glyph is a white-on-transparent mask, so the skin's [`ImageNode::color`] tint
/// (via `ICON_CLASS`) recolours it wholesale; the per-icon [`class`](ParcelIcon::class)
/// lets a skin re-tint or replace just this glyph.
fn spawn_parcel_icon(
    commands: &mut Commands,
    asset_server: &AssetServer,
    parent: Entity,
    icon: ParcelIcon,
) {
    commands.spawn((
        ImageNode::new(asset_server.load(icon.asset_path())),
        Node {
            width: Val::Px(ICON_SIZE),
            height: Val::Px(ICON_SIZE),
            flex_shrink: 0.0,
            ..default()
        },
        // Hidden — not `Display::None` — so its slot is reserved and the bar's
        // width is the same whether or not the restriction is in force.
        Visibility::Hidden,
        ClassList::new_with_classes([ICON_CLASS, icon.class()]),
        icon,
        Name::new("status-parcel-icon"),
        ChildOf(parent),
    ));
}

/// Spawn one text read-out as a fixed- or flexible-width slot under `parent`.
///
/// `width` is the slot's fixed logical width, or `None` for the flexible parcel
/// name (which grows to fill the row). `trailing` aligns the text to the slot's
/// trailing edge (the numeric balance / time / FPS), otherwise the leading edge.
/// The text lives in a child node so the slot's fixed width and clipping hold
/// regardless of the text's measured length (the `bevy_ui` text-measure width
/// caveat — see the `sl-client-viewer-ui-gotchas` memory).
fn spawn_readout(
    commands: &mut Commands,
    parent: Entity,
    readout: StatusReadout,
    width: Option<f32>,
    trailing: bool,
) {
    let mut slot = Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        justify_content: if trailing {
            JustifyContent::FlexEnd
        } else {
            JustifyContent::FlexStart
        },
        overflow: Overflow::clip(),
        ..default()
    };
    match width {
        Some(width) => {
            slot.width = Val::Px(width);
            slot.flex_shrink = 0.0;
        }
        None => {
            // The parcel name: grow into the row's slack, shrink to nothing.
            slot.flex_grow = 1.0;
            slot.flex_basis = Val::Px(0.0);
            slot.min_width = Val::Px(0.0);
        }
    }
    let slot_entity = commands
        .spawn((slot, Name::new("status-readout-slot"), ChildOf(parent)))
        .id();
    // The text-node layout keeps the `fps` (and every other read-out's) unit
    // label from intermittently vanishing (`viewer-fps-label-intermittent`).
    //
    // The read-outs are marked [`FixedSlotContentSize`] so a value ticking (the
    // FPS at ~10 Hz) does **not** trip the layout gate into a full-tree relayout
    // (`ui_perf::ui_layout_dirty`). But `bevy_ui`'s glyph pass (`text_system`, in
    // the ungated `UiSystems::PostLayout`) still reflows on every text change, and
    // it lays the glyphs out inside the node's **last computed** `content_box` —
    // which, with layout gated off, is stale. The proportional font makes the
    // same digit count a different width ("88 fps" is wider than "11 fps"), so a
    // value wider than the one layout last sized the node for would, in the old
    // config, be **word-wrapped into that stale-narrow box** — dropping the unit
    // onto a second line the slot's clip then hid.
    //
    // The fix keeps the node's width **independent of its content** so the stale
    // box is never too narrow, and pins the value to the correct edge:
    //
    // - **Trailing** read-outs (balance / time / FPS) fill the slot
    //   (`flex_basis: 0` + `flex_grow: 1`, so the node is always the slot's full
    //   width regardless of the text) and right-justify their glyphs, so the
    //   trailing unit sits at the slot's trailing edge and any overflow clips the
    //   *leading* digits instead.
    // - **Leading** read-outs (region / coordinates / parcel name) keep their full
    //   text width (`flex_shrink: 0`) and never wrap ([`TextLayout::no_wrap`]), so
    //   an over-long value clips its trailing tail against the leading-anchored
    //   slot rather than wrapping.
    let (text_layout, text_node) = if trailing {
        (
            TextLayout::default().with_justify(Justify::Right),
            Node {
                flex_basis: Val::Px(0.0),
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
        )
    } else {
        (
            TextLayout::no_wrap(),
            Node {
                flex_shrink: 0.0,
                ..default()
            },
        )
    };
    let text = commands
        .spawn((
            Text::new(String::new()),
            UiFont::Sans.at(STATUS_FONT_SIZE),
            TextColor(Color::WHITE),
            text_layout,
            text_node,
            ClassList::new_with_classes(["sk-status-readout"]),
            readout,
            Name::new("status-readout"),
            ChildOf(slot_entity),
        ))
        .id();
    // A fixed-width read-out's text sits in a fixed-width, clipping slot, so its
    // measured length can never change the layout. Mark it so the PostUpdate
    // layout gate ignores its per-frame `ContentSize` churn — without this the
    // FPS integer re-measures at ~10 Hz and forces a full-tree relayout each
    // time (`ui_perf::FixedSlotContentSize`). The flexible parcel name is not
    // marked (it has no fixed width to absorb a change).
    if width.is_some() {
        commands.entity(text).insert(FixedSlotContentSize);
    }
    // The parcel name is a click target: pressing it opens the About Land
    // floater on the current parcel (the reference viewer's location read-out).
    if readout == StatusReadout::ParcelName {
        commands
            .entity(text)
            .insert((Button, Pickable::default()))
            .observe(on_parcel_name_press);
    }
    // Mark the balance so the snapshot floater can blank it for a shot.
    if readout == StatusReadout::Balance {
        commands.entity(text).insert(BalanceReadout);
    }
}

/// Open the About Land floater on the agent's current parcel when the top-bar
/// location read-out is clicked.
fn on_parcel_name_press(
    press: On<Pointer<Press>>,
    parcel: Res<SlAgentParcel>,
    mut about_land: MessageWriter<crate::about_land::OpenAboutLand>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    if let Some(current) = parcel.current.as_ref() {
        about_land.write(crate::about_land::OpenAboutLand {
            subject: crate::about_land::AboutLandSubject::CurrentParcel(current.local_id),
            read_only: false,
        });
    }
}

/// Ask the simulator for the agent's balance the moment the region handshake
/// completes, so the read-out fills in without waiting for a transaction to push
/// it (the reference viewer requests it on entry too).
fn request_balance_on_entry(
    mut events: MessageReader<SlEvent>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        if matches!(event.0, SlSessionEvent::RegionHandshakeComplete) {
            commands.write(SlCommand(Command::RequestMoneyBalance));
        }
    }
}

/// Fold each [`SlSessionEvent::MoneyBalance`] into the [`AgentBalance`] mirror,
/// and chime the money up / down UI sound on a real balance change (not the first
/// reply, which is the initial read rather than a transaction).
fn track_balance(
    mut events: MessageReader<SlEvent>,
    mut agent_balance: ResMut<AgentBalance>,
    mut ui_sound: MessageWriter<crate::ui_sounds::PlayUiSound>,
) {
    for event in events.read() {
        if let SlSessionEvent::MoneyBalance(money) = &event.0 {
            let next = Some(LindenBalance::from(money.balance.clone()));
            if agent_balance.balance != next {
                if let (Some(before), Some(after)) = (
                    agent_balance
                        .balance
                        .as_ref()
                        .and_then(LindenBalance::to_i64),
                    next.as_ref().and_then(LindenBalance::to_i64),
                ) {
                    use crate::ui_sounds::{PlayUiSound, UiSound};
                    if after > before {
                        ui_sound.write(PlayUiSound(UiSound::MoneyUp));
                    } else if after < before {
                        ui_sound.write(PlayUiSound(UiSound::MoneyDown));
                    }
                }
                agent_balance.balance = next;
            }
        }
    }
}

/// Fold the own-avatar object's region-local position into
/// [`AgentRegionPosition`] as its updates arrive. The own avatar is the object
/// whose `full_id` equals the agent id ([`SlIdentity::agent_id`]).
fn track_agent_position(
    mut events: MessageReader<SlEvent>,
    identity: Res<SlIdentity>,
    mut position: ResMut<AgentRegionPosition>,
) {
    let Some(agent) = identity.agent_id else {
        return;
    };
    for event in events.read() {
        let (SlSessionEvent::ObjectAdded(object) | SlSessionEvent::ObjectUpdated(object)) =
            &event.0
        else {
            continue;
        };
        if object.full_id.uuid() == agent.uuid() {
            position.position = Some(object.motion.position.clone());
        }
    }
}

/// Rewrite the region / coordinates / parcel / balance / time / FPS read-outs
/// each frame from the live read-models and the frame diagnostics, formatted for
/// the active locale.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the translator, the \
              frame diagnostics, the balance / position read-models, the agent parcel, the \
              settings, the current region and the read-out text nodes"
)]
fn update_status_readouts(
    translator: Translator,
    diagnostics: Res<DiagnosticsStore>,
    balance: Res<AgentBalance>,
    position: Res<AgentRegionPosition>,
    agent_parcel: Res<SlAgentParcel>,
    settings: Option<Res<ViewerSettings>>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut readouts: Query<(&StatusReadout, &mut Text)>,
) {
    let region_name = regions
        .single()
        .ok()
        .and_then(|region| region.0.sim_name.as_ref())
        .map(|name| name.to_string());
    let parcel_name = agent_parcel
        .current
        .as_ref()
        .map(|parcel| parcel.name.clone())
        .filter(|name| !name.is_empty());
    let show_coords = settings
        .as_ref()
        .and_then(|settings| settings.store().get_bool(SHOW_COORDINATES_KEY).ok())
        .unwrap_or(true);

    for (readout, mut text) in &mut readouts {
        let next = match readout {
            StatusReadout::Region => region_name.clone().unwrap_or_else(|| {
                // Nothing to show before the region handshake.
                translator.get("status-bar-connecting")
            }),
            StatusReadout::Coords => {
                coords_text(&translator, position.position.as_ref(), show_coords)
            }
            StatusReadout::ParcelName => parcel_name.clone().unwrap_or_default(),
            StatusReadout::Balance => balance_text(&translator, balance.balance.as_ref()),
            StatusReadout::Time => time_text(&translator),
            StatusReadout::Fps => fps_text(&translator, &diagnostics),
        };
        if text.0 != next {
            text.0 = next;
        }
    }
}

/// Build the coordinate read-out `(x, y, z)` from the agent's region-local
/// position, or empty when the position is unknown or the setting is off. Zero
/// fraction digits rounds each coordinate to a whole metre with the locale's
/// digits — no float-to-int cast needed.
fn coords_text(translator: &Translator, position: Option<&Vector>, show_coords: bool) -> String {
    if !show_coords {
        return String::new();
    }
    let Some(pos) = position else {
        return String::new();
    };
    let x = translator.decimal(f64::from(pos.x), 0);
    let y = translator.decimal(f64::from(pos.y), 0);
    let z = translator.decimal(f64::from(pos.z), 0);
    format!("({x}, {y}, {z})")
}

/// Build the balance read-out (`L$1,234`), or a placeholder before the first
/// reply.
fn balance_text(translator: &Translator, balance: Option<&LindenBalance>) -> String {
    match balance.and_then(LindenBalance::to_i64) {
        Some(amount) => translator.currency_l(amount),
        None => translator.get("status-bar-balance-unknown"),
    }
}

/// Build the time read-out — the current Second Life Time (US Pacific), rendered
/// for the locale with an `SLT` marker.
fn time_text(translator: &Translator) -> String {
    let when = slt::current_slt(slt::now_unix());
    let clock = translator.datetime(when, DateTimeStyle::Time, DateTimeLength::Short);
    translator.format("status-bar-time", &TransArgs::new().text("time", &clock))
}

/// Build the FPS read-out from the smoothed frame-time diagnostic.
fn fps_text(translator: &Translator, diagnostics: &DiagnosticsStore) -> String {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(Diagnostic::smoothed)
        // Zero fraction digits renders a whole-number FPS with the locale's
        // digits, again avoiding any float-to-int cast.
        .map_or_else(|| "--".to_owned(), |value| translator.decimal(value, 0));
    translator.format("status-bar-fps", &TransArgs::new().text("fps", &fps))
}

/// Run condition for [`update_parcel_icons`]: one of its inputs moved this
/// frame — the agent's parcel record (written upstream only on a real
/// change), the current region's identity (its flags feed the icon
/// predicates), or a freshly spawned icon needing its initial visibility.
fn parcel_icon_inputs_changed(
    agent_parcel: Res<SlAgentParcel>,
    regions: Query<(), (With<SlCurrentRegion>, Changed<SlRegionIdentity>)>,
    icons: Query<(), Added<ParcelIcon>>,
) -> bool {
    agent_parcel.is_changed() || !regions.is_empty() || !icons.is_empty()
}

/// Update each parcel-permission icon when an input changed: show it only
/// while its restriction is in force for the current parcel + region (the
/// reference viewer's semantics), hiding it otherwise. Visibility (not
/// display) toggles, so the icon bar keeps a constant width either way. An
/// unresolved parcel leaves them all hidden. Gated on
/// [`parcel_icon_inputs_changed`].
fn update_parcel_icons(
    agent_parcel: Res<SlAgentParcel>,
    regions: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    mut icons: Query<(&ParcelIcon, &mut Visibility)>,
) {
    let region_flags = regions.single().ok().map_or_else(
        || RegionFlags::from_bits(0),
        |region| RegionFlags::from_bits(region.0.region_flags),
    );
    // With no resolved parcel there is nothing in force, so every icon is hidden.
    let context = agent_parcel.current.as_ref().map(|parcel| ParcelIcons {
        parcel_flags: parcel.flags(),
        see_avs: parcel.see_avs,
        region: region_flags,
        can_fly: agent_parcel.can_fly,
    });

    for (icon, mut visibility) in &mut icons {
        let active = context.as_ref().is_some_and(|context| context.shown(*icon));
        let next = if active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        // Write through change detection only on a real change.
        if *visibility != next {
            *visibility = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ParcelIcon, ParcelIcons};
    use sl_client_bevy::{ParcelFlags, RegionFlags};

    /// A wide-open parcel's flags — voice, scripts and building all allowed.
    fn open_parcel() -> ParcelFlags {
        ParcelFlags::from_bits(0)
            .union(ParcelFlags::ALLOW_VOICE)
            .union(ParcelFlags::ALLOW_OTHER_SCRIPTS)
            .union(ParcelFlags::CREATE_OBJECTS)
    }

    /// The context for a parcel + region, with avatars visible unless overridden.
    fn context(parcel: ParcelFlags, region: RegionFlags, can_fly: bool) -> ParcelIcons {
        ParcelIcons {
            parcel_flags: parcel,
            see_avs: Some(true),
            region,
            can_fly,
        }
    }

    /// Whether any icon is shown.
    fn any_shown(icons: &ParcelIcons) -> bool {
        ParcelIcon::ALL.iter().any(|icon| icons.shown(*icon))
    }

    /// A wide-open parcel on an unrestricted region shows no icons.
    #[test]
    fn open_parcel_shows_no_icons() {
        assert!(!any_shown(&context(
            open_parcel(),
            RegionFlags::from_bits(0),
            true
        )));
    }

    /// A locked-down parcel (no voice / build / scripts, avatars hidden) lights
    /// the matching icons; damage stays off while the flag is clear.
    #[test]
    fn locked_parcel_lights_restriction_icons() {
        let icons = ParcelIcons {
            parcel_flags: ParcelFlags::from_bits(0),
            see_avs: Some(false),
            region: RegionFlags::from_bits(0),
            can_fly: false,
        };
        assert!(icons.shown(ParcelIcon::Voice), "no ALLOW_VOICE shows voice");
        assert!(icons.shown(ParcelIcon::Fly), "can_fly false shows fly");
        assert!(
            icons.shown(ParcelIcon::Build),
            "no CREATE_OBJECTS shows build"
        );
        assert!(
            icons.shown(ParcelIcon::Scripts),
            "no ALLOW_OTHER_SCRIPTS shows scripts"
        );
        assert!(
            icons.shown(ParcelIcon::SeeAvatars),
            "SeeAVs false shows see-avatars"
        );
        assert!(
            !icons.shown(ParcelIcon::Damage),
            "damage stays off while the flag is clear"
        );
    }

    /// `SeeAVs` unknown (the UDP path omits it) is treated as visible — no icon.
    #[test]
    fn unknown_see_avs_hides_the_icon() {
        let icons = ParcelIcons {
            parcel_flags: open_parcel(),
            see_avs: None,
            region: RegionFlags::from_bits(0),
            can_fly: true,
        };
        assert!(!icons.shown(ParcelIcon::SeeAvatars));
    }

    /// Damage lights from either the parcel flag or the region flag.
    #[test]
    fn damage_icon_lights_from_parcel_or_region() {
        let parcel_damage = context(
            open_parcel().union(ParcelFlags::ALLOW_DAMAGE),
            RegionFlags::from_bits(0),
            true,
        );
        assert!(parcel_damage.shown(ParcelIcon::Damage));
        let region_damage = context(open_parcel(), RegionFlags::ALLOW_DAMAGE, true);
        assert!(
            region_damage.shown(ParcelIcon::Damage),
            "region damage lights the icon even on a peaceful parcel"
        );
    }

    /// Scripts light from either the parcel flag or a region skip-scripts bit.
    #[test]
    fn scripts_icon_lights_from_region_skip() {
        assert!(
            !context(open_parcel(), RegionFlags::from_bits(0), true).shown(ParcelIcon::Scripts)
        );
        assert!(context(open_parcel(), RegionFlags::SKIP_SCRIPTS, true).shown(ParcelIcon::Scripts));
        assert!(
            context(open_parcel(), RegionFlags::ESTATE_SKIP_SCRIPTS, true)
                .shown(ParcelIcon::Scripts)
        );
    }

    /// **`viewer-fps-label-intermittent`.** A trailing read-out (balance / time /
    /// FPS) must keep its unit label whatever its value ticks to.
    ///
    /// The read-outs carry [`FixedSlotContentSize`], so a value ticking — the FPS
    /// at ~10 Hz — does **not** trip the layout gate ([`ui_perf::ui_layout_dirty`])
    /// into a full-tree relayout; that gate is the whole reason the FPS box exists
    /// as a fixed slot. But `bevy_ui`'s glyph pass (`text_system`, in the *ungated*
    /// `UiSystems::PostLayout`) still reflows on each text change, and it lays the
    /// glyphs inside the node's **last computed** `content_box` — stale while
    /// layout is gated off. In the old config the text node's width tracked its
    /// content, so a value wider than the one layout last sized the node for was
    /// word-wrapped into a too-narrow stale box, dropping the unit onto a second
    /// line the slot's clip then hid (the intermittent missing `fps`).
    ///
    /// This harness is layout-only — no glyph pass — so it cannot re-run
    /// `text_system` against a stale box. It asserts the invariant that removes the
    /// stale-narrow box altogether: **the trailing read-out's text node is the
    /// slot's full width regardless of its value**, so the glyph pass' bounds never
    /// shrink below the slot and the widest realistic value never wraps. A control
    /// built the *old* way (a content-width node) is measured alongside to prove
    /// the invariant has teeth: its width genuinely tracked the value.
    #[test]
    fn trailing_readout_width_does_not_track_its_value() -> Result<(), crate::ui_test::TestError> {
        use bevy::ecs::world::CommandQueue;
        use bevy::prelude::*;

        use super::{FPS_WIDTH, STATUS_FONT_SIZE, StatusReadout, spawn_readout};
        use crate::ui::UiRoot;
        use crate::ui_font::UiFont;
        use crate::ui_test::{LayoutTest, settle};

        /// One text line at the read-out size is ~1 line-height; two are ~double.
        const ONE_LINE_MAX: f32 = STATUS_FONT_SIZE * 1.5;
        /// The node width is the slot width to within sub-pixel rounding.
        const WIDTH_EPSILON: f32 = 1.0;

        let mut app = LayoutTest::new().build();
        settle(&mut app);
        let root = app.world().resource::<UiRoot>().0;

        // The real FPS read-out, spawned through the production path so the test
        // tracks whatever node config `spawn_readout` actually gives it.
        {
            let world = app.world_mut();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, world);
            spawn_readout(
                &mut commands,
                root,
                StatusReadout::Fps,
                Some(FPS_WIDTH),
                true,
            );
            queue.apply(world);
        }
        let fps_text = {
            let mut query = app.world_mut().query::<(Entity, &StatusReadout)>();
            query
                .iter(app.world())
                .find(|(_, readout)| **readout == StatusReadout::Fps)
                .map(|(entity, _)| entity)
                .ok_or("spawn_readout did not spawn an FPS text node")?
        };

        // A control in an identical fixed-width, clipping slot but with the *old*
        // config: a plain content-width text node (default `flex_shrink`, default
        // line-break). Its width will be shown to track its content — the stale
        // box the glyph pass used to wrap into.
        let control_slot = app
            .world_mut()
            .spawn((
                Node {
                    width: Val::Px(FPS_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let control_text = app
            .world_mut()
            .spawn((
                Text::new(String::new()),
                UiFont::Sans.at(STATUS_FONT_SIZE),
                ChildOf(control_slot),
            ))
            .id();

        let logical = |app: &App, entity: Entity| -> Vec2 {
            app.world()
                .entity(entity)
                .get::<ComputedNode>()
                .map(|computed| computed.size * computed.inverse_scale_factor)
                .unwrap_or_default()
        };

        let mut control_min = f32::MAX;
        let mut control_max = 0.0_f32;
        // One- to three-digit frame rates: every realistic value, plus enough of a
        // margin that a fast display's 100+ fps is covered too.
        for value in [1, 8, 11, 44, 60, 88, 99, 120, 240] {
            let label = format!("{value} fps");
            {
                let Some(mut text) = app.world_mut().get_mut::<Text>(fps_text) else {
                    return Err("the FPS text node vanished".into());
                };
                text.0 = label.clone();
            }
            {
                let Some(mut text) = app.world_mut().get_mut::<Text>(control_text) else {
                    return Err("the control text node vanished".into());
                };
                text.0 = label.clone();
            }
            settle(&mut app);

            let fps = logical(&app, fps_text);
            assert!(
                (fps.x - FPS_WIDTH).abs() <= WIDTH_EPSILON,
                "`{label}` sized the FPS text node to {} logical px, not the slot's {FPS_WIDTH} px \
                 — a content-tracking width lets the gated glyph pass wrap the label into a stale \
                 box",
                fps.x,
            );
            assert!(
                fps.y < ONE_LINE_MAX,
                "`{label}` wrapped in the real read-out: {} logical px tall (one line is \
                 ~{STATUS_FONT_SIZE} px) — the `fps` label would be clipped away",
                fps.y,
            );

            let control = logical(&app, control_text).x;
            control_min = control_min.min(control);
            control_max = control_max.max(control);
        }

        // Teeth: the old content-width config really did vary with the value, so
        // the invariant above is not vacuous — a narrower earlier layout is exactly
        // the stale box the glyph pass wrapped `fps` into.
        assert!(
            control_max - control_min > 2.0,
            "the control's width did not vary with its value ({control_min}..{control_max} px), so \
             the old config could not have produced a stale-narrow box — this test proves nothing"
        );
        Ok(())
    }

    /// **`viewer-fps-label-intermittent`, leading half.** A leading read-out
    /// (region / coordinates / parcel name) never wraps: an over-long value clips
    /// its trailing tail against the leading-anchored slot rather than wrapping to
    /// a second line the slot's clip would hide. Mirrors the trailing test's other
    /// branch of the [`spawn_readout`] fix (`TextLayout::no_wrap`).
    #[test]
    fn leading_readout_clips_rather_than_wrapping() -> Result<(), crate::ui_test::TestError> {
        use bevy::ecs::world::CommandQueue;
        use bevy::prelude::*;

        use super::{REGION_WIDTH, STATUS_FONT_SIZE, StatusReadout, spawn_readout};
        use crate::ui::UiRoot;
        use crate::ui_font::UiFont;
        use crate::ui_test::{LayoutTest, settle};

        /// One text line at the read-out size is ~1 line-height; two are ~double.
        const ONE_LINE_MAX: f32 = STATUS_FONT_SIZE * 1.5;
        /// A region name far wider than the fixed slot, so wrapping (if it
        /// happened) would take several lines.
        const LONG_NAME: &str = "A Very Long Region Name That Overflows The Slot By A Lot";

        let mut app = LayoutTest::new().build();
        settle(&mut app);
        let root = app.world().resource::<UiRoot>().0;

        {
            let world = app.world_mut();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, world);
            spawn_readout(
                &mut commands,
                root,
                StatusReadout::Region,
                Some(REGION_WIDTH),
                false,
            );
            queue.apply(world);
        }
        let region_text = {
            let mut query = app.world_mut().query::<(Entity, &StatusReadout)>();
            query
                .iter(app.world())
                .find(|(_, readout)| **readout == StatusReadout::Region)
                .map(|(entity, _)| entity)
                .ok_or("spawn_readout did not spawn a region text node")?
        };

        // A control in an identical fixed-width slot but with the old wrapping
        // config, to calibrate: the same name in a wrapping node takes several
        // lines, so the no-wrap assertion is not vacuous.
        let control_slot = app
            .world_mut()
            .spawn((
                Node {
                    width: Val::Px(REGION_WIDTH),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let control_text = app
            .world_mut()
            .spawn((
                Text::new(LONG_NAME),
                UiFont::Sans.at(STATUS_FONT_SIZE),
                ChildOf(control_slot),
            ))
            .id();

        {
            let Some(mut text) = app.world_mut().get_mut::<Text>(region_text) else {
                return Err("the region text node vanished".into());
            };
            text.0 = LONG_NAME.to_owned();
        }
        settle(&mut app);

        let height = |app: &App, entity: Entity| -> f32 {
            app.world()
                .entity(entity)
                .get::<ComputedNode>()
                .map(|computed| computed.size.y * computed.inverse_scale_factor)
                .unwrap_or_default()
        };
        let region_h = height(&app, region_text);
        let control_h = height(&app, control_text);
        assert!(
            region_h < ONE_LINE_MAX,
            "the leading read-out wrapped: {region_h} logical px tall (one line is \
             ~{STATUS_FONT_SIZE} px)"
        );
        assert!(
            control_h > region_h + STATUS_FONT_SIZE,
            "the wrapping control did not take more lines than the no-wrap read-out ({control_h} \
             vs {region_h} px), so the no-wrap assertion proves nothing — widen `LONG_NAME`"
        );
        Ok(())
    }

    /// Push restriction lights from either the parcel flag or the region flag.
    #[test]
    fn push_icon_lights_from_parcel_or_region() {
        assert!(!context(open_parcel(), RegionFlags::from_bits(0), true).shown(ParcelIcon::Push));
        assert!(
            context(
                open_parcel().union(ParcelFlags::RESTRICT_PUSHOBJECT),
                RegionFlags::from_bits(0),
                true,
            )
            .shown(ParcelIcon::Push)
        );
        assert!(
            context(open_parcel(), RegionFlags::RESTRICT_PUSHOBJECT, true).shown(ParcelIcon::Push)
        );
    }
}
