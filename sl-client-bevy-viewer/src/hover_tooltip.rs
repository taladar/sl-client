//! In-world **hover tooltips** — rest the pointer on something in the world for
//! a moment (~0.5 s) and a small info box appears at the cursor (the reference's
//! `LLToolTipMgr` / `lltoolpie.cpp` hover tips):
//!
//! - **object**: name, description, owner, and affordance hints (Touch, and a
//!   Buy price when it is for sale) — resolved by firing
//!   [`Command::RequestObjectPropertiesFamily`] for the hovered root on a
//!   debounce and reading the [`ObjectPropertiesFamily`] reply (the command and
//!   reply are already on the wire; [`crate::object_menu`] uses the same pair).
//! - **avatar**: the resolved display / legacy name — the same
//!   [`crate::world_api::AvatarState`] the name tags read.
//! - **land**: the parcel name / owner when nothing pickable is hit, gated
//!   behind [`SETTING_SHOW_LAND_TIPS`] (the reference's "Show land tooltips",
//!   off by default), from the held [`sl_client_bevy::SlAgentParcel`].
//!
//! The pick reuses the same cursor + occlusion arbitration the right-click
//! world menu does ([`crate::avatar_menu`]): the name-tag rect test wins first,
//! then UI / HUD occlusion suppress, then the **GPU ID-buffer pick**
//! ([`crate::gpu_pick`]) resolves what is drawn under the cursor — avatar,
//! object face or bare land — with the depth test doing the nearest-wins
//! arbitration the old `MeshRayCast` distance comparisons did on the CPU. It
//! runs on a dwell timer that resets on cursor motion, so the tip only appears
//! once the pointer has settled — and never while a mouse button is held (a
//! drag / click gesture). While dwelt, the pick view refreshes at
//! ~[`crate::gpu_pick::PICK_HZ`] Hz; the 1–2 frame readback latency is
//! invisible under the 0.5 s dwell.

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::picking::hover::HoverMap;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use std::collections::HashSet as StdHashSet;

use sl_client_bevy::{
    AgentKey, Command, LindenAmount, ObjectKey, ObjectPropertiesFamily, OwnerKey, ScopedObjectId,
    SlAgentParcel, SlCommand, SlEvent, SlIdentity, SlSessionEvent, Uuid,
};

use crate::gpu_pick::{GpuPickResolved, GpuPicker, PICK_HZ, PickPurpose, PickResolution};
use crate::hud::HudCamera;
use crate::hud_pick::{pointer_over_blocking_ui, pointer_over_hud};
use crate::i18n::Translator;
use crate::name_tag_billboard::NameTagHitTest;
use crate::objects::ObjectSlMotion;
use crate::world_api::AvatarState;
use crate::world_api::GroupsModel;
use crate::world_api::ObjectState;

/// Master toggle: show in-world hover tooltips (object + avatar). Default on.
pub(crate) const SETTING_SHOW_HOVER_TIPS: &str = "ShowHoverTips";

/// Show the land tooltip when nothing pickable is under the cursor (the
/// reference's "Show land tooltips"; default off).
pub(crate) const SETTING_SHOW_LAND_TIPS: &str = "ShowLandTips";

/// The settings section hover-tip toggles live in.
const HOVER_TIP_SECTION: &[&str] = &["hovertips"];

/// The dwell time, seconds, the pointer must rest before a tip appears (the
/// reference's `ToolTipDelay`, ~0.5 s).
const DWELL_SECS: f32 = 0.5;

/// Cursor motion, logical px this frame, above which the dwell resets (any real
/// pointer movement dismisses a shown tip and restarts the timer).
const MOTION_RESET_PX: f32 = 1.5;

/// The tip box's screen offset from the cursor, logical px (down-right, the
/// reference's tooltip placement).
const TIP_CURSOR_OFFSET_PX: f32 = 16.0;

/// The tip text's maximum width, logical px, before it wraps.
const TIP_MAX_WIDTH_PX: f32 = 400.0;

/// Register the hover-tip settings.
pub(crate) fn register_settings(settings: &mut crate::settings::ViewerSettings) {
    settings.register_in(
        HOVER_TIP_SECTION,
        SETTING_SHOW_HOVER_TIPS,
        sl_settings::SettingValue::Bool(true),
        "Show hover tooltips over objects and avatars",
    );
    settings.register_in(
        HOVER_TIP_SECTION,
        SETTING_SHOW_LAND_TIPS,
        sl_settings::SettingValue::Bool(false),
        "Show a hover tooltip over land when nothing else is under the cursor",
    );
}

/// Marker on the cursor-anchored tooltip box (a `Text` node with a dark
/// backdrop that [`update_hover_tooltip`] positions and rewrites).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct HoverTooltip;

/// The hover-tooltip runtime state: the dwell timer, the cached
/// properties-family replies, and the request de-dup guards.
#[derive(Resource, Debug, Default)]
pub(crate) struct HoverTooltipState {
    /// Seconds the pointer has rested since the last motion.
    idle_secs: f32,
    /// Seconds since the last GPU pick request (drives the ~15 Hz refresh
    /// while dwelt).
    since_pick: f32,
    /// What the latest resolved GPU pick found under the cursor (`None` =
    /// nothing / not yet resolved), written by [`ingest_hover_picks`].
    target: Option<HoverTarget>,
    /// What the box should show this frame (`None` = hidden). The resolve
    /// system ([`update_hover_tooltip`]) writes it; the apply system
    /// ([`apply_hover_tooltip`]) renders it — split so the pick machinery's
    /// `Visibility` reads never share a system with the box's `Visibility`
    /// write (a Bevy query conflict, B0001).
    render: Option<TooltipRender>,
    /// Cached condensed properties, keyed by object root — filled by the
    /// [`ObjectPropertiesFamily`] reply.
    properties: HashMap<ObjectKey, CachedObjectInfo>,
    /// Objects whose properties-family request is outstanding (so it is fired
    /// once, not every dwell frame).
    requested: HashSet<ObjectKey>,
    /// Owner / avatar ids whose name request has been fired (same de-dup).
    requested_names: StdHashSet<Uuid>,
}

/// What the tip box should show: its lines and cursor-anchored position.
#[derive(Debug, Clone, PartialEq)]
struct TooltipRender {
    /// The box's text lines (joined with newlines).
    lines: Vec<String>,
    /// The box's top-left position, logical px.
    position: Vec2,
}

/// The condensed object info a tooltip shows, cached from the properties-family
/// reply so a re-hover reads it without a fresh round trip.
#[derive(Debug, Clone)]
struct CachedObjectInfo {
    /// The object's name.
    name: String,
    /// The object's description.
    description: String,
    /// The owner (agent or group).
    owner: OwnerKey,
    /// The asking price when the object is for sale, else `None`.
    sale_price: Option<LindenAmount>,
}

impl From<&ObjectPropertiesFamily> for CachedObjectInfo {
    fn from(properties: &ObjectPropertiesFamily) -> Self {
        Self {
            name: properties.name.clone(),
            description: properties.description.clone(),
            owner: properties.owner,
            sale_price: properties.sale_price.clone(),
        }
    }
}

/// Fold every [`ObjectPropertiesFamily`] reply into the tooltip cache (the same
/// reply [`crate::object_menu`] reads for the Mute name — a shared, harmless
/// duplicate read).
pub(crate) fn ingest_object_properties_family(
    mut events: MessageReader<SlEvent>,
    mut state: ResMut<HoverTooltipState>,
) {
    for event in events.read() {
        if let SlSessionEvent::ObjectPropertiesFamily { properties } = &event.0 {
            state.requested.remove(&properties.object_id);
            state
                .properties
                .insert(properties.object_id, CachedObjectInfo::from(properties));
        }
    }
}

/// What the cursor is resting on, resolved from the GPU ID-buffer pick by
/// [`ingest_hover_picks`] (or the synchronous name-tag rect test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoverTarget {
    /// An avatar (its name-tag rect, or its posed body / a worn attachment's
    /// wearer).
    Avatar(AgentKey),
    /// An in-world object (or a worn attachment), by its linkset root, plus the
    /// union of the picked prim's and the root's `PrimFlags` (for the behaviour
    /// flag line — the reference's advanced object tooltip).
    Object {
        /// The linkset root — what the properties-family / object-cost requests
        /// query.
        root: ObjectKey,
        /// The linkset root's scoped id — for the prim count, position and
        /// distance lookups.
        root_scoped: ScopedObjectId,
        /// The picked object's `PrimFlags` bits.
        flags: u32,
    },
    /// Bare land (the ray's first hit is terrain).
    Land,
}

/// The `PrimFlags` bits the reference's object tooltip surfaces
/// (`object_flags.h`, `lltoolpie.cpp` `handleTooltipObject`), paired with the
/// Fluent key of the word it shows, in the reference's line order.
const TOOLTIP_FLAGS: &[(u32, &str)] = &[
    (1 << 6, "hovertip-flag-script"),          // FLAGS_SCRIPTED
    (1 << 0, "hovertip-flag-physics"),         // FLAGS_USE_PHYSICS
    (1 << 7, "hovertip-flag-touch"),           // FLAGS_HANDLE_TOUCH
    (1 << 9, "hovertip-flag-money"),           // FLAGS_TAKES_MONEY
    (1 << 16, "hovertip-flag-drop-inventory"), // FLAGS_ALLOW_INVENTORY_DROP
    (1 << 10, "hovertip-flag-phantom"),        // FLAGS_PHANTOM
    (1 << 29, "hovertip-flag-temporary"),      // FLAGS_TEMPORARY_ON_REZ
];

/// The cursor-occlusion machinery bundled as one system param — the HUD
/// camera, render layers, the name-tag rect test, and the UI-occlusion
/// inputs — so [`update_hover_tooltip`] stays within Bevy's per-system
/// parameter limit. The world resolution itself is the asynchronous GPU
/// ID-buffer pick ([`crate::gpu_pick`]); only the HUD-occlusion ray (the
/// orthographic HUD test [`crate::hud_pick`] owns) still casts.
#[derive(SystemParam)]
pub(crate) struct HoverPick<'w, 's> {
    /// The HUD camera, for the HUD-occlusion test.
    hud_camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<HudCamera>>,
    /// Every entity's render layers, to gather the HUD subtree.
    layers: Query<'w, 's, (Entity, &'static RenderLayers)>,
    /// The pointer hover map, for the UI-occlusion test.
    hover_map: Res<'w, HoverMap>,
    /// Pickable flags, for the UI-occlusion test.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Computed node sizes, for the UI-occlusion test.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// The name-tag rect test (tags are custom billboard meshes no picking
    /// backend covers; the 2D rect test is exact and cheap, so it stays CPU).
    tag_hit: NameTagHitTest<'w, 's>,
    /// The HUD-occlusion ray caster (HUD picking stays on the orthographic
    /// CPU test by design).
    ray_cast: MeshRayCast<'w, 's>,
}

impl HoverPick<'_, '_> {
    /// Whether a blocking UI surface or a HUD attachment is under the cursor (a
    /// tip must not appear over a floater / the agent's own HUD).
    fn occluded(&mut self, cursor: Vec2) -> bool {
        pointer_over_blocking_ui(&self.hover_map, &self.pickables, &self.node_sizes)
            || pointer_over_hud(cursor, &self.hud_camera, &self.layers, &mut self.ray_cast)
    }
}

/// Fold every resolved hover pick into [`HoverTooltipState::target`]: the GPU
/// ID buffer names an avatar (its posed pixels, worn rigged submeshes
/// included), an object face (resolved to its linkset summary), bare terrain,
/// or nothing — the depth test already arbitrated nearest-wins.
pub(crate) fn ingest_hover_picks(
    mut picks: MessageReader<GpuPickResolved>,
    objects: Res<ObjectState>,
    mut state: ResMut<HoverTooltipState>,
) {
    for pick in picks.read() {
        if pick.purpose != PickPurpose::Hover {
            continue;
        }
        state.target = pick.hit.as_ref().and_then(|hit| match hit.resolution {
            // A worn rigged submesh hovers as its wearer, exactly as the old
            // CPU avatar pick reported it.
            PickResolution::Avatar { agent, worn: _ } => Some(HoverTarget::Avatar(agent)),
            PickResolution::ObjectFace { scoped, .. } => {
                objects
                    .pick_summary(scoped)
                    .map(|summary| HoverTarget::Object {
                        root: summary.root_full,
                        root_scoped: summary.root_scoped,
                        flags: summary.flags,
                    })
            }
            PickResolution::Terrain => Some(HoverTarget::Land),
            PickResolution::Water => None,
        });
    }
}

/// Spawn the tooltip box (hidden) — a dark-backed, cursor-anchored `Text` node
/// drawn over everything and never itself pickable.
pub(crate) fn setup_hover_tooltip(mut commands: Commands) {
    commands.spawn((
        Text::new(String::new()),
        crate::ui_font::UiFont::Sans.at(14.0),
        TextColor(Color::srgb(0.95, 0.95, 0.95)),
        Node {
            position_type: PositionType::Absolute,
            max_width: Val::Px(TIP_MAX_WIDTH_PX),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.75)),
        // The tip must never occlude what it describes, and must draw over
        // every other UI layer.
        Pickable::IGNORE,
        GlobalZIndex(i32::MAX),
        Visibility::Hidden,
        HoverTooltip,
        Name::new("hover-tooltip"),
    ));
}

/// The name resolvers a tooltip reads — the same sources the name tags and the
/// about-land floater use.
#[derive(SystemParam)]
pub(crate) struct HoverNames<'w> {
    /// Avatar name records (display / legacy / provisional).
    avatars: Res<'w, AvatarState>,
    /// Group names (for a group-owned object or parcel).
    groups: Option<Res<'w, GroupsModel>>,
    /// The UI translator for the static labels.
    translator: Translator<'w>,
}

impl HoverNames<'_> {
    /// Resolve an owner (agent or group) to a display string, firing a name
    /// request (de-duplicated through `requested`) when it has not resolved yet.
    fn owner_name(
        &self,
        owner: OwnerKey,
        requested: &mut StdHashSet<Uuid>,
        commands: &mut MessageWriter<SlCommand>,
    ) -> String {
        match owner {
            OwnerKey::Agent(agent) => {
                if let Some(name) = self.avatars.name_of(agent) {
                    name.to_owned()
                } else {
                    if requested.insert(agent.uuid()) {
                        commands.write(SlCommand(Command::RequestAvatarNames(vec![agent])));
                    }
                    format!("({})", short_id(agent.uuid()))
                }
            }
            OwnerKey::Group(group) => {
                if let Some(name) = self
                    .groups
                    .as_ref()
                    .and_then(|groups| groups.group_name(group))
                {
                    name.to_owned()
                } else {
                    if requested.insert(group.uuid())
                        && let Some(groups) = self.groups.as_ref()
                    {
                        groups.request_name(group, commands);
                    }
                    format!("({})", short_id(group.uuid()))
                }
            }
        }
    }
}

/// The leading fragment of a UUID, for a not-yet-resolved name fallback.
fn short_id(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

/// One object's tooltip extras — the reference's advanced-tooltip prim count,
/// region position and own-avatar distance lines.
struct ObjectExtras {
    /// The linkset's prim count (`ObjectState::linkset_prim_count`).
    prim_count: usize,
    /// The root's region-local position (Second Life coordinates), if resolved.
    position: Option<Vec3>,
    /// The own-avatar → object distance, metres, when both are placed.
    distance: Option<f32>,
}

/// The object-side data the tooltip reads for the prim-count / position /
/// distance lines: the tracked-object store, objects' Second Life motion and
/// world transforms, and the own-agent identity (to place the own avatar).
#[derive(SystemParam)]
pub(crate) struct HoverObjectData<'w, 's> {
    /// The tracked-object store (linkset prim count + root-entity lookup).
    state: Res<'w, ObjectState>,
    /// Objects' Second Life motion (the region position the tip shows).
    motions: Query<'w, 's, &'static ObjectSlMotion>,
    /// World transforms (the object + own-avatar positions for the distance).
    globals: Query<'w, 's, &'static GlobalTransform>,
    /// The own-agent id, to find the own avatar's anchor for the distance.
    identity: Option<Res<'w, SlIdentity>>,
}

impl HoverObjectData<'_, '_> {
    /// Resolve the extras for a linkset root.
    fn extras(&self, root_scoped: ScopedObjectId, avatars: &AvatarState) -> ObjectExtras {
        let entity = self.state.entity_by_scoped(&root_scoped);
        let position = entity
            .and_then(|entity| self.motions.get(entity).ok())
            .map(|motion| Vec3::new(motion.position.x, motion.position.y, motion.position.z));
        let object_world = entity
            .and_then(|entity| self.globals.get(entity).ok())
            .map(GlobalTransform::translation);
        let distance = match (object_world, self.own_avatar_world(avatars)) {
            (Some(object), Some(own)) => Some(object.distance(own)),
            _other => None,
        };
        ObjectExtras {
            prim_count: self.state.linkset_prim_count(&root_scoped),
            position,
            distance,
        }
    }

    /// The own avatar's world position, when it is placed.
    fn own_avatar_world(&self, avatars: &AvatarState) -> Option<Vec3> {
        let own = self.identity.as_ref()?.agent_id?;
        let anchor = avatars
            .labelled_avatars()
            .find(|(agent, _label, _tag)| *agent == own)
            .map(|(_agent, anchor, _tag)| anchor)?;
        Some(self.globals.get(anchor).ok()?.translation())
    }
}

/// Whether hover tips (and land tips) are enabled, from the settings store.
fn tip_toggles(settings: Option<&crate::settings::ViewerSettings>) -> (bool, bool) {
    let get = |name: &str, default: bool| {
        settings
            .and_then(|settings| settings.store().get_bool(name).ok())
            .unwrap_or(default)
    };
    (
        get(SETTING_SHOW_HOVER_TIPS, true),
        get(SETTING_SHOW_LAND_TIPS, false),
    )
}

/// Resolve the hover tooltip: accumulate dwell while the pointer rests, then
/// keep a ~[`PICK_HZ`] Hz GPU pick refreshed at the cursor and stash the box's
/// desired content in [`HoverTooltipState::render`] (or clear it when the
/// pointer moves, a button is held, or nothing is under it). Deliberately
/// holds **no** overlay query — [`apply_hover_tooltip`] does the box write, so
/// the pick machinery's `Visibility` reads never conflict with the box's
/// `Visibility` write.
#[expect(
    clippy::too_many_arguments,
    reason = "the resolve fuses the cursor / dwell inputs, the occlusion machinery, the \
              GPU pick queue, the name resolvers, the held parcel and the settings"
)]
pub(crate) fn update_hover_tooltip(
    windows: Query<&Window>,
    motion: Res<AccumulatedMouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut pick: HoverPick,
    mut picker: ResMut<GpuPicker>,
    names: HoverNames,
    object_data: HoverObjectData,
    mut costs: ResMut<crate::object_cost::ObjectCostModel>,
    parcel: Option<Res<SlAgentParcel>>,
    settings: Option<Res<crate::settings::ViewerSettings>>,
    mut state: ResMut<HoverTooltipState>,
    mut sl_commands: MessageWriter<SlCommand>,
) {
    let (show_tips, show_land) = tip_toggles(settings.as_deref());
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    // No tips while a button is held (a drag / click gesture), in mouselook (no
    // cursor), when disabled, or on real pointer motion (which also resets the
    // dwell and dismisses a shown tip).
    let dismiss = !show_tips
        || buttons.pressed(MouseButton::Left)
        || buttons.pressed(MouseButton::Right)
        || cursor.is_none()
        || motion.delta.length() > MOTION_RESET_PX;
    if dismiss {
        state.idle_secs = 0.0;
        state.since_pick = f32::MAX;
        state.target = None;
        state.render = None;
        return;
    }
    let Some(cursor) = cursor else {
        state.render = None;
        return;
    };
    state.idle_secs += time.delta_secs();
    if state.idle_secs < DWELL_SECS {
        return;
    }

    // A blocking UI surface or HUD attachment under the cursor suppresses the
    // tip (checked only once dwelt, so the raycast is not run every frame).
    if pick.occluded(cursor) {
        state.render = None;
        return;
    }

    // Keep the GPU pick fresh at ~PICK_HZ while dwelt on world content; the
    // resolved answer arrives 1–2 frames later via `ingest_hover_picks`.
    // (`f32::MAX + dt` stays `f32::MAX`, so the post-dismiss sentinel simply
    // requests immediately on the first dwelt frame.)
    state.since_pick += time.delta_secs();
    if state.since_pick >= 1.0 / PICK_HZ {
        picker.request(cursor, PickPurpose::Hover);
        state.since_pick = 0.0;
    }

    // The name-tag rect test wins over the world pick (the reference's tag →
    // world order); it is synchronous and exact, so it stays CPU.
    let target = match pick.tag_hit.agent_at(cursor) {
        Some(agent) => Some(HoverTarget::Avatar(agent)),
        None => state.target,
    };

    let lines = match target {
        Some(HoverTarget::Avatar(agent)) => Some(vec![names.avatars.label_text(agent)]),
        Some(HoverTarget::Object {
            root,
            root_scoped,
            flags,
        }) => {
            let extras = object_data.extras(root_scoped, &names.avatars);
            Some(object_lines(
                root,
                flags,
                &extras,
                &names,
                &mut state,
                &mut costs,
                &mut sl_commands,
            ))
        }
        Some(HoverTarget::Land) if show_land => {
            land_lines(parcel.as_deref(), &names, &mut state, &mut sl_commands)
        }
        _ => None,
    };

    state.render = lines
        .filter(|lines| !lines.is_empty())
        .map(|lines| TooltipRender {
            lines,
            position: Vec2::new(
                cursor.x + TIP_CURSOR_OFFSET_PX,
                cursor.y + TIP_CURSOR_OFFSET_PX,
            ),
        });
}

/// Render the resolved tooltip into the box: position it, rewrite its text (only
/// on change), and show / hide it. The **only** system that writes the overlay,
/// so its `Visibility` / `Node` / `Text` writes stay clear of the pick reads.
pub(crate) fn apply_hover_tooltip(
    state: Res<HoverTooltipState>,
    mut overlay: Query<(&mut Node, &mut Text, &mut Visibility), With<HoverTooltip>>,
) {
    let Ok((mut node, mut text, mut visibility)) = overlay.single_mut() else {
        return;
    };
    match &state.render {
        Some(render) => {
            let left = Val::Px(render.position.x);
            let top = Val::Px(render.position.y);
            if node.left != left {
                node.left = left;
            }
            if node.top != top {
                node.top = top;
            }
            let joined = render.lines.join("\n");
            if text.0 != joined {
                *text = Text::new(joined);
            }
            visibility.set_if_neq(Visibility::Inherited);
        }
        None => {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Compose an object tip's lines — the reference's advanced object tooltip:
/// `[price] name`, description, owner, the behaviour-flag line, the prim count +
/// land impact, position and distance. Fires the properties-family and
/// object-cost requests on first sight.
fn object_lines(
    root: ObjectKey,
    flags: u32,
    extras: &ObjectExtras,
    names: &HoverNames,
    state: &mut HoverTooltipState,
    costs: &mut crate::object_cost::ObjectCostModel,
    commands: &mut MessageWriter<SlCommand>,
) -> Vec<String> {
    let Some(info) = state.properties.get(&root).cloned() else {
        // Not cached yet: fire the request once and show a placeholder so the
        // box appears immediately, then fills when the reply lands.
        if state.requested.insert(root) {
            commands.write(SlCommand(Command::RequestObjectPropertiesFamily {
                request_flags: 0,
                object_id: root,
            }));
        }
        return vec![names.translator.get("hovertip-loading")];
    };
    // The reference's line order: [price] name, description, owner, flag line.
    let mut name = info.name.clone();
    if let Some(price) = info.sale_price.as_ref() {
        // For-sale objects prefix the price (`TooltipPrice` "L$[AMOUNT]: ").
        name = format!("L${}: {name}", price.0);
    }
    let mut lines = vec![name];
    if !info.description.is_empty() && info.description != info.name {
        lines.push(info.description.clone());
    }
    let owner = names.owner_name(info.owner, &mut state.requested_names, commands);
    lines.push(format!(
        "{} {owner}",
        names.translator.get("hovertip-owner")
    ));

    // The behaviour-flag line (the reference's advanced tooltip), each set flag
    // rendered as its word in the reference's order, space-joined.
    let flag_words: Vec<String> = TOOLTIP_FLAGS
        .iter()
        .filter(|(bit, _key)| flags & bit != 0)
        .map(|(_bit, key)| names.translator.get(key))
        .collect();
    if !flag_words.is_empty() {
        lines.push(flag_words.join(" "));
    }

    // Prim count + land impact on one line (the reference's
    // "Prims: N, Land Impact: M"). The land impact arrives from the
    // `GetObjectCost` cap; request it once, and append it when cached (nothing
    // shows on a grid without the cap).
    let mut prims = format!(
        "{} {}",
        names.translator.get("hovertip-prims"),
        extras.prim_count
    );
    match costs.resolve(root, commands) {
        crate::object_cost::LandImpact::Known(land_impact) => {
            prims = format!(
                "{prims}{} {land_impact:.0}",
                names.translator.get("hovertip-land-impact")
            );
        }
        crate::object_cost::LandImpact::Pending => {
            prims = format!("{prims}{} …", names.translator.get("hovertip-land-impact"));
        }
        // No `GetObjectCost` cap on this grid (or not requested): show only the
        // prim count, like the reference on plain OpenSim.
        crate::object_cost::LandImpact::CapUnavailable
        | crate::object_cost::LandImpact::NotRequested => {}
    }
    lines.push(prims);

    // Position (region coordinates) and own-avatar distance.
    if let Some(position) = extras.position {
        lines.push(format!(
            "{} <{:.2}, {:.2}, {:.2}>",
            names.translator.get("hovertip-position"),
            position.x,
            position.y,
            position.z,
        ));
    }
    if let Some(distance) = extras.distance {
        lines.push(format!(
            "{} {distance:.2} m",
            names.translator.get("hovertip-distance")
        ));
    }
    lines
}

/// Compose a land tip's lines: the parcel name and owner (or a public note).
fn land_lines(
    parcel: Option<&SlAgentParcel>,
    names: &HoverNames,
    state: &mut HoverTooltipState,
    commands: &mut MessageWriter<SlCommand>,
) -> Option<Vec<String>> {
    let info = parcel?.current.as_ref()?;
    let mut lines = vec![info.name.clone()];
    let owner = names.owner_name(info.owner, &mut state.requested_names, commands);
    lines.push(format!(
        "{} {owner}",
        names.translator.get("hovertip-owner")
    ));
    Some(lines)
}

/// The hover-tooltip plugin: the runtime state, the overlay spawn, and the
/// dwell / reply systems.
#[derive(Debug, Default)]
pub(crate) struct HoverTooltipPlugin;

impl Plugin for HoverTooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverTooltipState>()
            .add_systems(Startup, setup_hover_tooltip)
            .add_systems(
                Update,
                (
                    ingest_object_properties_family,
                    ingest_hover_picks,
                    update_hover_tooltip,
                    apply_hover_tooltip,
                )
                    .chain(),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::{CachedObjectInfo, HoverTooltipState};
    use sl_client_bevy::{ObjectKey, Uuid};

    /// A properties reply caches under its object id and clears the outstanding
    /// request guard.
    #[test]
    fn cache_round_trips_and_clears_request() {
        let mut state = HoverTooltipState::default();
        let key = ObjectKey::from(Uuid::from_u128(42));
        state.requested.insert(key);
        state.properties.insert(
            key,
            CachedObjectInfo {
                name: "Vendor".to_owned(),
                description: String::new(),
                owner: sl_client_bevy::OwnerKey::Agent(Uuid::from_u128(7).into()),
                sale_price: None,
            },
        );
        state.requested.remove(&key);
        assert!(state.properties.contains_key(&key));
        assert!(!state.requested.contains(&key));
    }
}
