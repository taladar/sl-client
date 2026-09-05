//! The estate half of a client's edits: the Region/Estate floater.
//!
//! The third family of `test-fake-grid-edit-surfaces`, and the one that does
//! not look like the other two. An object edit and a parcel edit each have a
//! message of their own; an estate command is **one** message — an
//! `EstateOwnerMessage` — carrying a method name and a list of byte parameters,
//! so the whole floater (its region tab, its terrain tab, its access lists and
//! its covenant) is a switch on a string. `SimSession` decodes the two methods
//! with a shape of their own (`telehub` and `terrain`) into typed events and
//! surfaces the rest as [`ServerEvent::EstateOwnerRequest`]; this module is
//! that switch's other end.
//!
//! Three things are worth knowing before reading it.
//!
//! **Every command is gated.** OpenSim answers an estate command from an agent
//! without the power by returning without a word, so the difference between an
//! estate manager and a resident is an answer against silence — which is what
//! makes the gate testable at all. The fake grid does the same, off the
//! account's [`AgentPolicy::estate_manager`].
//!
//! **The estate is stored per region.** An estate spans regions on a real grid,
//! and a ban set in one region holds in the next; the fake grid's regions are
//! independent worlds with no store above them, so each holds its own. Nothing
//! reads an estate from two regions yet, and the day something does, this is
//! the thing to move rather than to work around.
//!
//! **Nothing here is enforced.** A banned agent may still log in and a manager
//! is still only a flag on an account: the estate is a *record* the floater
//! reads and writes, not a rule the grid applies. That is the honest shape for
//! a grid whose whole purpose is to be logged into, and it is what makes the
//! access lists testable without a second account.

use std::time::Instant;

use sl_proto::{
    EstateAccessDelta, EstateAccessKind, EstateCovenant, EstateInfo, RegionIdentity, ServerEvent,
    SimSession,
};

use crate::agent_requests::AgentPolicy;
use crate::world::SceneFixtures;

/// The estate command a viewer's "regenerate the map tile" nudge issues.
const REFRESH_MAP_VISIBILITY: &str = "refreshmapvisibility";

/// The alert OpenSim's `refreshmapvisibility` handler answers with once the
/// tile has been regenerated.
const MAP_REGENERATED: &str = "Terrain map generated";

/// The estate command the Region/Estate floater opens with: give me everything.
const GET_INFO: &str = "getinfo";

/// The estate command that renames the estate and moves its sun.
const CHANGE_INFO: &str = "estatechangeinfo";

/// The estate command that saves the floater's **Region** tab.
const SET_REGION_INFO: &str = "setregioninfo";

/// The estate command that saves the floater's **Terrain** tab.
const SET_REGION_TERRAIN: &str = "setregionterrain";

/// The estate command that sets one corner's terrain detail texture.
const TEXTURE_DETAIL: &str = "texturedetail";

/// The estate command that sets one corner's terrain blend heights.
const TEXTURE_HEIGHTS: &str = "textureheights";

/// The estate command that applies the staged terrain textures and heights.
const TEXTURE_COMMIT: &str = "texturecommit";

/// The estate command that adds to or removes from one of the four lists.
const ACCESS_DELTA: &str = "estateaccessdelta";

/// The estate command that points the estate at a new covenant notecard.
const CHANGE_COVENANT: &str = "estatechangecovenantid";

/// How many corners a region's terrain texture set has: the four the estate
/// floater shows, blended by height across the region.
const TERRAIN_CORNERS: usize = 4;

/// One region's estate record: what the Region/Estate floater reads and
/// writes, and what a `getinfo` answers with.
///
/// A fixture rather than a service: the fake grid runs no estate service, so
/// this is the whole of it, and every field starts at the value a fresh
/// standalone region reports.
#[expect(
    clippy::module_name_repetitions,
    reason = "the name is read at its use sites -- `SceneFixtures::estate`'s \
              type and the crate's own re-export -- where a bare `Fixture` \
              would not say what it is a fixture of"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstateFixture {
    /// The estate's name.
    pub name: String,
    /// The raw estate-flags bitfield.
    pub flags: u32,
    /// The estate's fixed sun position, when it uses one.
    pub sun_position: u32,
    /// The covenant notecard's asset id, or [`None`] for an estate with no
    /// covenant — which is what a fresh region has.
    pub covenant: Option<uuid::Uuid>,
    /// When the covenant was last changed (a Unix timestamp), or zero when
    /// there has never been one.
    pub covenant_timestamp: u32,
    /// The address abuse reports about this estate go to.
    pub abuse_email: String,
    /// The agents allowed in when the estate is closed.
    pub allowed_agents: Vec<uuid::Uuid>,
    /// The groups allowed in when the estate is closed.
    pub allowed_groups: Vec<uuid::Uuid>,
    /// The agents refused entry.
    pub banned_agents: Vec<uuid::Uuid>,
    /// The agents who may issue this estate's commands.
    ///
    /// A record, not the gate: whether *this session* may issue a command is
    /// [`AgentPolicy::estate_manager`], which comes off the account the grid
    /// was built with. The two are deliberately separate — the list is what the
    /// floater shows, and an account can hold the power without being on it,
    /// exactly as an estate *owner* does.
    pub managers: Vec<uuid::Uuid>,
}

impl Default for EstateFixture {
    fn default() -> Self {
        Self {
            name: DEFAULT_ESTATE_NAME.to_owned(),
            flags: 0,
            sun_position: 0,
            covenant: None,
            covenant_timestamp: 0,
            abuse_email: String::new(),
            allowed_agents: Vec::new(),
            allowed_groups: Vec::new(),
            banned_agents: Vec::new(),
            managers: Vec::new(),
        }
    }
}

/// The estate name a fake region reports.
const DEFAULT_ESTATE_NAME: &str = "Fake Estate";

impl EstateFixture {
    /// The list `kind` names, to read.
    #[must_use]
    pub fn list(&self, kind: EstateAccessKind) -> &[uuid::Uuid] {
        match kind {
            EstateAccessKind::AllowedGroups => &self.allowed_groups,
            EstateAccessKind::BannedAgents => &self.banned_agents,
            EstateAccessKind::Managers => &self.managers,
            _ => &self.allowed_agents,
        }
    }

    /// The list `kind` names, to change.
    pub const fn list_mut(&mut self, kind: EstateAccessKind) -> &mut Vec<uuid::Uuid> {
        match kind {
            EstateAccessKind::AllowedGroups => &mut self.allowed_groups,
            EstateAccessKind::BannedAgents => &mut self.banned_agents,
            EstateAccessKind::Managers => &mut self.managers,
            _ => &mut self.allowed_agents,
        }
    }

    /// This record as the `estateupdateinfo` reply for `identity`'s region: the
    /// estate's own fields, over the region's owner and estate id.
    #[must_use]
    pub fn info(&self, identity: &RegionIdentity, estate_id: u32) -> EstateInfo {
        EstateInfo {
            estate_name: self.name.clone(),
            estate_owner: identity.sim_owner,
            estate_id,
            estate_flags: self.flags,
            sun_position: self.sun_position,
            parent_estate: estate_id,
            covenant_id: self.covenant,
            covenant_timestamp: self.covenant_timestamp,
            abuse_email: self.abuse_email.clone(),
        }
    }

    /// This record as the `EstateCovenantReply` for `identity`'s region.
    #[must_use]
    pub fn covenant_reply(&self, identity: &RegionIdentity) -> EstateCovenant {
        EstateCovenant {
            covenant_id: self.covenant,
            covenant_timestamp: self.covenant_timestamp,
            estate_name: self.name.clone(),
            estate_owner_id: identity.sim_owner,
        }
    }
}

/// Answers one drained [`ServerEvent`] that reads or writes the estate,
/// returning `true` when it was one — so the driver knows the event has been
/// dealt with.
///
/// Every command is refused in silence when `policy` says this agent holds no
/// estate power, which is what OpenSim does and the only thing that makes the
/// gate observable.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per estate method, each a handful of lines; the switch is \
              the shape of the protocol and splitting it would only hide that"
)]
pub(crate) fn answer_estate_request(
    world: &mut SceneFixtures,
    identity: &RegionIdentity,
    policy: AgentPolicy,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> bool {
    let (method, invoice, params) = match event {
        // The covenant is the one estate read with a message of its own, and
        // the one command a resident may issue: a covenant is what somebody is
        // shown *before* they buy land, so gating it would be gating the sale.
        ServerEvent::RequestEstateCovenant => {
            let covenant = world.estate.covenant_reply(identity);
            if let Err(error) = sim.send_estate_covenant_reply(&covenant, now) {
                tracing::warn!("answering an estate covenant request failed: {error}");
            }
            return true;
        }
        ServerEvent::EstateOwnerRequest {
            method,
            invoice,
            params,
        } => (method.as_str(), *invoice, params.as_slice()),
        _other => return false,
    };
    if !policy.estate_manager {
        tracing::debug!("the estate command {method} came from an agent with no estate powers");
        return true;
    }
    match method {
        // The floater's opening round trip: the estate's configuration, then
        // one message per access list. All four go out even when empty — an
        // empty list is an answer, and a viewer that receives nothing for one
        // cannot tell it from a reply that was lost.
        GET_INFO => {
            let estate_id = world.limits(identity).estate_id;
            let info = world.estate.info(identity, estate_id);
            if let Err(error) = sim.send_estate_info(&info, invoice, now) {
                tracing::warn!("answering an estate getinfo failed: {error}");
            }
            push_access_lists(world, estate_id, invoice, sim, now);
        }
        // The estate's name and sun. The viewer sends the name in parameter 0
        // and the two numbers after it.
        CHANGE_INFO => {
            if let Some(name) = params.first() {
                world.estate.name.clone_from(name);
            }
            if let Some(flags) = number(params, 1) {
                world.estate.flags = flags;
            }
            if let Some(sun) = number(params, 2) {
                world.estate.sun_position = sun;
            }
            let estate_id = world.limits(identity).estate_id;
            let info = world.estate.info(identity, estate_id);
            if let Err(error) = sim.send_estate_info(&info, invoice, now) {
                tracing::warn!("answering an estate change failed: {error}");
            }
        }
        // The floater's Region tab. The nine parameters are positional and
        // three of them are the region's own limits rather than the estate's.
        SET_REGION_INFO => {
            {
                let limits = world.limits_mut(identity);
                if let Some(agents) = number(params, 4) {
                    limits.max_agents = agents;
                }
                if let Some(bonus) = decimal(params, 5) {
                    limits.object_bonus_factor = bonus;
                }
                if let Some(access) = number(params, 6) {
                    limits.maturity =
                        sl_proto::Maturity::from_sim_access(u8::try_from(access).unwrap_or(0));
                }
            }
            let limits = world.limits(identity);
            if let Err(error) = sim.send_region_info(&limits, now) {
                tracing::warn!("answering a region info save failed: {error}");
            }
        }
        // The floater's Terrain tab: the water line, how far a terraform may
        // move the ground, and the sun.
        SET_REGION_TERRAIN => {
            {
                let limits = world.limits_mut(identity);
                if let Some(water) = decimal(params, 0) {
                    limits.water_height = water;
                }
                if let Some(raise) = decimal(params, 1) {
                    limits.terrain_raise_limit = raise;
                }
                if let Some(lower) = decimal(params, 2) {
                    limits.terrain_lower_limit = lower;
                }
                if let Some(estate_sun) = boolean(params, 3) {
                    limits.use_estate_sun = estate_sun;
                }
                if let Some(hour) = decimal(params, 5) {
                    limits.sun_hour = hour;
                }
            }
            let limits = world.limits(identity);
            if let Err(error) = sim.send_region_info(&limits, now) {
                tracing::warn!("answering a region terrain save failed: {error}");
            }
        }
        // One parameter per corner being changed, each `"<corner> <uuid>"`.
        TEXTURE_DETAIL => {
            let composition = world.terrain_composition_mut(identity);
            for (corner, value) in params.iter().filter_map(|parameter| pair(parameter)) {
                let Ok(texture) = value.parse() else {
                    tracing::debug!("a texturedetail corner named the unparsable id {value}");
                    continue;
                };
                if let Some(slot) = composition.detail_textures.get_mut(corner) {
                    *slot = texture;
                }
            }
        }
        // One parameter per corner, each `"<corner> <low> <high>"`.
        TEXTURE_HEIGHTS => {
            let composition = world.terrain_composition_mut(identity);
            for parameter in params {
                let mut fields = parameter.split_whitespace();
                let Some((corner, low, high)) = fields
                    .next()
                    .and_then(|corner| corner.parse::<usize>().ok())
                    .zip(fields.next().and_then(|low| low.parse::<f32>().ok()))
                    .zip(fields.next().and_then(|high| high.parse::<f32>().ok()))
                    .map(|((corner, low), high)| (corner, low, high))
                else {
                    tracing::debug!("a textureheights corner read as {parameter}");
                    continue;
                };
                if corner >= TERRAIN_CORNERS {
                    continue;
                }
                if let Some(slot) = composition.start_heights.get_mut(corner) {
                    *slot = low;
                }
                if let Some(slot) = composition.height_ranges.get_mut(corner) {
                    *slot = high;
                }
            }
        }
        // "Apply". The textures and heights are already stored — this is when
        // every viewer is told, and a `RegionHandshake` is the only message
        // they travel in.
        TEXTURE_COMMIT => {
            let mut updated = identity.clone();
            updated.terrain = world.terrain_composition(identity);
            if let Err(error) = sim.send_region_handshake(&updated, now) {
                tracing::warn!("re-handshaking after a terrain texture commit failed: {error}");
            }
        }
        // One add or remove against one of the four lists. The viewer sends its
        // own id first, the change second and the target third.
        ACCESS_DELTA => {
            let Some(delta) = number(params, 1).and_then(EstateAccessDelta::from_u32) else {
                tracing::debug!("an estateaccessdelta named no change this grid knows");
                return true;
            };
            let Some(target) = params
                .get(2)
                .and_then(|value| value.parse::<uuid::Uuid>().ok())
            else {
                tracing::debug!("an estateaccessdelta named an unparsable target");
                return true;
            };
            let kind = delta.list();
            let list = world.estate.list_mut(kind);
            list.retain(|held| *held != target);
            if delta.is_add() {
                list.push(target);
            }
            let estate_id = world.limits(identity).estate_id;
            let members = world.estate.list(kind).to_vec();
            if let Err(error) = sim.send_estate_access_list(estate_id, kind, &members, invoice, now)
            {
                tracing::warn!("answering an estate access change failed: {error}");
            }
        }
        // A new covenant notecard. The timestamp is the estate's own record of
        // when it last changed, and a fake grid has no wall clock it may read
        // (a seeded grid has to mint the same run twice), so it counts changes
        // instead — which is all a viewer does with it: compare it to the one
        // it last saw.
        CHANGE_COVENANT => {
            let covenant = params
                .first()
                .and_then(|value| value.parse::<uuid::Uuid>().ok())
                .filter(|id| !id.is_nil());
            world.estate.covenant = covenant;
            world.estate.covenant_timestamp = world.estate.covenant_timestamp.saturating_add(1);
            let reply = world.estate.covenant_reply(identity);
            if let Err(error) = sim.send_estate_covenant_reply(&reply, now) {
                tracing::warn!("answering a covenant change failed: {error}");
            }
        }
        // The viewer's "regenerate the map tile" nudge. The fake grid's tiles
        // are static, so there is nothing to regenerate and the answer is the
        // success one; the cool-down and generator-unavailable branches OpenSim
        // also has are states this grid cannot be in.
        REFRESH_MAP_VISIBILITY => {
            if let Err(error) = sim.send_alert_message(MAP_REGENERATED, &[], &[], now) {
                tracing::warn!("answering an estate map regeneration failed: {error}");
            }
        }
        other => {
            tracing::debug!("the estate command {other} has no answer on this grid");
            return false;
        }
    }
    true
}

/// Sends all four of the estate's access lists, which is what a `getinfo`
/// trails.
fn push_access_lists(
    world: &SceneFixtures,
    estate_id: u32,
    invoice: uuid::Uuid,
    sim: &mut SimSession,
    now: Instant,
) {
    for kind in [
        EstateAccessKind::Managers,
        EstateAccessKind::AllowedAgents,
        EstateAccessKind::AllowedGroups,
        EstateAccessKind::BannedAgents,
    ] {
        let members = world.estate.list(kind);
        if let Err(error) = sim.send_estate_access_list(estate_id, kind, members, invoice, now) {
            tracing::warn!("sending an estate access list failed: {error}");
        }
    }
}

/// Parameter `index` as an unsigned number, or [`None`] when it is absent or
/// says something else.
fn number(params: &[String], index: usize) -> Option<u32> {
    params.get(index)?.trim().parse().ok()
}

/// Parameter `index` as a decimal, or [`None`] when it is absent or says
/// something else.
fn decimal(params: &[String], index: usize) -> Option<f32> {
    params.get(index)?.trim().parse().ok()
}

/// Parameter `index` as the boolean the estate channel spells in longhand:
/// OpenSim reads `1`, `y`, `yes`, `t` and `true` as set and everything else as
/// clear, and the viewer sends `"Y"` / `"N"`.
fn boolean(params: &[String], index: usize) -> Option<bool> {
    let value = params.get(index)?.trim();
    if value.is_empty() {
        return None;
    }
    Some(matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "y" | "yes" | "t" | "true"
    ))
}

/// A `"<corner> <value>"` parameter split into its two halves, or [`None`] when
/// it is not one — the shape the terrain-texture commands use.
fn pair(parameter: &str) -> Option<(usize, &str)> {
    let (corner, value) = parameter.split_once(' ')?;
    let corner: usize = corner.trim().parse().ok()?;
    (corner < TERRAIN_CORNERS).then_some((corner, value.trim()))
}
