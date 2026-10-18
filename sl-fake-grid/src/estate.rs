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
    AgentKey, AlertInfo, EstateAccessDelta, EstateAccessKind, EstateAccessLists, EstateCovenant,
    EstateInfo, GroupKey, REGION_RESTART_MINUTES, REGION_RESTART_SECONDS, RegionIdentity,
    ServerEvent, SimSession,
};

use crate::agent_requests::AgentPolicy;
use crate::imitates::{EstatePolicy, RestartNoticeForm};
use crate::world::{AvatarIdentity, RegionChange, SceneFixtures};

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

/// The estate command that saves the floater's **Debug** tab.
const SET_REGION_DEBUG: &str = "setregiondebug";

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

/// The estate command that schedules (or, with `-1`, cancels) a restart.
const RESTART: &str = "restart";

/// The estate command that messages everyone in the region.
const REGION_MESSAGE: &str = "simulatormessage";

/// The estate command that messages everyone in the estate.
const ESTATE_MESSAGE: &str = "instantmessage";

/// An estate manager's notice to everyone in the region, as each avatar is
/// sent it ([`RegionChange::Notice`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A non-modal agent alert with this text: OpenSim's region and estate
    /// messages and its restart notice (measured by the `estate` census,
    /// 2026-10-17), slash and all.
    AgentAlert(String),
    /// An `AlertMessage`: Second Life's keyed restart notice.
    Alert {
        /// The alert's text.
        message: String,
        /// Its keys and their parameters.
        info: Vec<AlertInfo>,
    },
}

/// Sends `notice` to the session `sim` of the avatar `agent`.
///
/// # Errors
///
/// As the [`SimSession`] send it makes.
pub(crate) fn send_notice(
    sim: &mut SimSession,
    agent: AgentKey,
    notice: &Notice,
    now: Instant,
) -> Result<(), sl_proto::Error> {
    match notice {
        Notice::AgentAlert(text) => sim.send_agent_alert_message(agent, false, text, now),
        Notice::Alert { message, info } => sim.send_alert_message(message, info, &[], now),
    }
}

/// The notice a restart in `seconds` is announced with, in `form`, for the
/// region `region_name`.
#[must_use]
pub fn restart_notice(form: RestartNoticeForm, seconds: u32, region_name: &str) -> Notice {
    match form {
        RestartNoticeForm::Spelled => {
            Notice::AgentAlert(format!("/Region will restart in {}", spelled(seconds)))
        }
        RestartNoticeForm::Keyed => {
            let (key, unit, count) = if seconds >= 60 {
                (REGION_RESTART_MINUTES, "MINUTES", seconds / 60)
            } else {
                (REGION_RESTART_SECONDS, "SECONDS", seconds)
            };
            let name = region_name
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            Notice::Alert {
                message: String::new(),
                info: vec![AlertInfo {
                    message: key.to_owned(),
                    extra_params: format!(
                        "<? LLSD/XML ?>\n<llsd><map><key>{unit}</key><integer>{count}</integer>\
                         <key>NAME</key><string>{name}</string></map></llsd>\n"
                    ),
                }],
            }
        }
    }
}

/// A duration as OpenSim's `RestartModule` spells it: `1 minute`, `2 minutes
/// and 30 seconds`, `15 seconds`.
fn spelled(seconds: u32) -> String {
    let (minutes, rest) = (seconds / 60, seconds % 60);
    let mut parts = Vec::new();
    match minutes {
        0 => {}
        1 => parts.push("1 minute".to_owned()),
        many => parts.push(format!("{many} minutes")),
    }
    match rest {
        0 => {}
        1 => parts.push("1 second".to_owned()),
        many => parts.push(format!("{many} seconds")),
    }
    parts.join(" and ")
}

/// Who an estate request came from, and how the grid answers it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EstateAsker<'a> {
    /// The region's identity.
    pub(crate) identity: &'a RegionIdentity,
    /// The asking agent.
    pub(crate) agent: &'a AvatarIdentity,
    /// What the agent may do.
    pub(crate) policy: AgentPolicy,
    /// How the grid answers the estate.
    pub(crate) estate: EstatePolicy,
}

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
    /// The estate's flags: its bits of the region flag space.
    pub flags: sl_proto::RegionFlags,
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
    /// Who banned each of [`banned_agents`](Self::banned_agents), where the
    /// ban came through this grid — what the `EstateAccess` capability tells
    /// beside the list. When is not kept: a fake grid has no wall clock it may
    /// read.
    pub banned_by: std::collections::BTreeMap<uuid::Uuid, uuid::Uuid>,
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
            flags: sl_proto::RegionFlags::default(),
            sun_position: 0,
            covenant: None,
            covenant_timestamp: 0,
            abuse_email: String::new(),
            allowed_agents: Vec::new(),
            allowed_groups: Vec::new(),
            banned_agents: Vec::new(),
            banned_by: std::collections::BTreeMap::new(),
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

    /// The four lists as the `EstateAccess` capability answers them: a ban
    /// with its banner where this grid knows it, and at no known time, as
    /// OpenSim answers a ban whose time it holds none for.
    #[must_use]
    pub fn access_lists(&self) -> EstateAccessLists {
        EstateAccessLists {
            allowed_agents: self
                .allowed_agents
                .iter()
                .copied()
                .map(AgentKey::from)
                .collect(),
            allowed_groups: self
                .allowed_groups
                .iter()
                .copied()
                .map(GroupKey::from)
                .collect(),
            banned_agents: self
                .banned_agents
                .iter()
                .map(|agent| sl_proto::EstateBan {
                    agent: AgentKey::from(*agent),
                    banning_agent: self.banned_by.get(agent).copied().map(AgentKey::from),
                    ban_date: None,
                    last_login_date: None,
                })
                .collect(),
            managers: self.managers.iter().copied().map(AgentKey::from).collect(),
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
/// returning the [`RegionChange`]s the region's other sessions have to be told
/// about — or [`None`] when there was nothing to answer, because the event was
/// not an estate one at all or named a method this grid has no answer for.
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
    asker: EstateAsker<'_>,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    let identity = asker.identity;
    let (method, invoice, params) = match event {
        // The covenant is the one estate read with a message of its own, and
        // the one command a resident may issue: a covenant is what somebody is
        // shown *before* they buy land, so gating it would be gating the sale.
        ServerEvent::RequestEstateCovenant => {
            let covenant = world.estate.covenant_reply(identity);
            if let Err(error) = sim.send_estate_covenant_reply(&covenant, now) {
                tracing::warn!("answering an estate covenant request failed: {error}");
            }
            return Some(Vec::new());
        }
        ServerEvent::EstateOwnerRequest {
            method,
            invoice,
            params,
        } => (method.as_str(), *invoice, params.as_slice()),
        _other => return None,
    };
    if !asker.policy.estate_manager {
        tracing::debug!("the estate command {method} came from an agent with no estate powers");
        return Some(Vec::new());
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
                world.estate.flags = sl_proto::RegionFlags::from_legacy(flags);
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
        // The floater's Region tab. The nine parameters are positional; the
        // two settings only the capability carries keep what they were.
        SET_REGION_INFO => {
            let mut update = region_info_update(&world.limits(identity));
            if let Some(flag) = boolean(params, 0) {
                update.block_terraform = flag;
            }
            if let Some(flag) = boolean(params, 1) {
                update.block_fly = flag;
            }
            if let Some(flag) = boolean(params, 2) {
                update.allow_damage = flag;
            }
            if let Some(flag) = boolean(params, 3) {
                update.allow_land_resell = flag;
            }
            if let Some(agents) = number(params, 4) {
                update.agent_limit = i32::try_from(agents).unwrap_or(i32::MAX);
            }
            if let Some(bonus) = decimal(params, 5) {
                update.object_bonus = bonus;
            }
            if let Some(access) = number(params, 6) {
                update.maturity =
                    sl_proto::Maturity::from_sim_access(u8::try_from(access).unwrap_or(0));
            }
            if let Some(flag) = boolean(params, 7) {
                update.restrict_pushobject = flag;
            }
            if let Some(flag) = boolean(params, 8) {
                update.allow_parcel_changes = flag;
            }
            return Some(apply_region_info(world, identity, &update, sim, now));
        }
        // The floater's Debug tab: scripts, collisions and physics off.
        SET_REGION_DEBUG => {
            {
                let limits = world.limits_mut(identity);
                let mut flags = limits.flags();
                for (index, flag) in [
                    sl_proto::RegionFlags::SKIP_SCRIPTS,
                    sl_proto::RegionFlags::SKIP_COLLISIONS,
                    sl_proto::RegionFlags::SKIP_PHYSICS,
                ]
                .into_iter()
                .enumerate()
                {
                    if let Some(on) = boolean(params, index) {
                        flags = flags.with(flag, on);
                    }
                }
                set_flags(limits, flags);
            }
            // Told by `SimStats` alone, the writer included: no `RegionInfo`
            // follows a Debug-tab save on either grid.
            let flags = world.limits(identity).flags();
            sim.set_region_stats_flags(flags);
            return Some(vec![RegionChange::RegionFlagsChanged(flags)]);
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
            return Some(vec![RegionChange::RegionConfigured(Box::new(limits))]);
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
            let mut changes = Vec::new();
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
                let composition = world.terrain_composition_mut(identity);
                if let Some(slot) = composition.start_heights.get_mut(corner) {
                    *slot = low;
                }
                if let Some(slot) = composition.height_ranges.get_mut(corner) {
                    *slot = high;
                }
                // OpenSim saves the region's settings for every corner and
                // tells everybody each time: a `RegionInfo`, then a handshake.
                if asker.estate.terrain_heights_told_per_corner {
                    let limits = world.limits(identity);
                    if let Err(error) = sim.send_region_info(&limits, now) {
                        tracing::warn!("telling a terrain height change failed: {error}");
                    }
                    let mut updated = identity.clone();
                    updated.terrain = world.terrain_composition(identity);
                    if let Err(error) = sim.send_region_handshake(&updated, now) {
                        tracing::warn!("re-handshaking after a terrain height failed: {error}");
                    }
                    changes.push(RegionChange::RegionConfigured(Box::new(limits)));
                    changes.push(RegionChange::TerrainRetextured(Box::new(updated.terrain)));
                }
            }
            return Some(changes);
        }
        // "Apply". The textures and heights are already stored — this is when
        // every viewer is told, and a `RegionHandshake` is the only message
        // they travel in.
        TEXTURE_COMMIT => {
            // OpenSim told everybody corner by corner already, and its commit
            // answers nothing.
            if asker.estate.terrain_heights_told_per_corner {
                return Some(Vec::new());
            }
            let composition = world.terrain_composition(identity);
            let mut updated = identity.clone();
            updated.terrain = composition;
            if let Err(error) = sim.send_region_handshake(&updated, now) {
                tracing::warn!("re-handshaking after a terrain texture commit failed: {error}");
            }
            // "Every viewer" is the whole point of the commit, and the region's
            // other sessions each need the handshake stamped with their *own*
            // identity, so the change carries the composition rather than a
            // finished message.
            return Some(vec![RegionChange::TerrainRetextured(Box::new(composition))]);
        }
        // One add or remove against one of the four lists. The viewer sends its
        // own id first, the change second and the target third.
        ACCESS_DELTA => {
            let Some(delta) = number(params, 1).and_then(EstateAccessDelta::from_u32) else {
                tracing::debug!("an estateaccessdelta named no change this grid knows");
                return Some(Vec::new());
            };
            let Some(target) = params
                .get(2)
                .and_then(|value| value.parse::<uuid::Uuid>().ok())
            else {
                tracing::debug!("an estateaccessdelta named an unparsable target");
                return Some(Vec::new());
            };
            let kind = delta.list();
            let list = world.estate.list_mut(kind);
            list.retain(|held| *held != target);
            if delta.is_add() {
                list.push(target);
            }
            if kind == EstateAccessKind::BannedAgents {
                if delta.is_add() {
                    let _previous = world
                        .estate
                        .banned_by
                        .insert(target, asker.agent.agent_id.uuid());
                } else {
                    let _removed = world.estate.banned_by.remove(&target);
                }
            }
            let estate_id = world.limits(identity).estate_id;
            // OpenSim answers a change to the allowed or the banned agents
            // with both lists, allowed first (measured by the `estate`
            // census, 2026-10-17); a change to the managers or the groups
            // with that list alone. Second Life is unmeasured and answers
            // as OpenSim does.
            let kinds: &[EstateAccessKind] = match kind {
                EstateAccessKind::AllowedAgents | EstateAccessKind::BannedAgents => &[
                    EstateAccessKind::AllowedAgents,
                    EstateAccessKind::BannedAgents,
                ],
                _ => std::slice::from_ref(&kind),
            };
            for kind in kinds {
                let members = world.estate.list(*kind).to_vec();
                if let Err(error) =
                    sim.send_estate_access_list(estate_id, *kind, &members, invoice, now)
                {
                    tracing::warn!("answering an estate access change failed: {error}");
                }
            }
        }
        // A new covenant notecard. The timestamp is the estate's own record of
        // when it last changed, and a fake grid has no wall clock it may read
        // (a seeded grid has to mint the same run twice), so it counts changes
        // instead — which is all a viewer does with it: compare it to the one
        // it last saw. Nobody is told: OpenSim answers the change with nothing
        // (measured by the `estate` census, 2026-10-17) and the reference
        // viewer shows the covenant it sent; the next `EstateCovenantRequest`
        // reads it back.
        CHANGE_COVENANT => {
            let covenant = params
                .first()
                .and_then(|value| value.parse::<uuid::Uuid>().ok())
                .filter(|id| !id.is_nil());
            world.estate.covenant = covenant;
            world.estate.covenant_timestamp = world.estate.covenant_timestamp.saturating_add(1);
        }
        // A restart: the first notice to everyone in the region, in the
        // grid's own form. The countdown's later notices and the restart
        // itself are `server-fake-grid-estate-actions`; a `-1` (the
        // reference's cancel, OpenSim's delay) with nothing pending is
        // answered with nothing, as OpenSim answers it.
        RESTART => {
            let Some(seconds) = params
                .first()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|seconds| *seconds > 0.0)
            else {
                return Some(Vec::new());
            };
            let Some(seconds) = format!("{:.0}", seconds.trunc())
                .parse::<u32>()
                .ok()
                .filter(|seconds| *seconds > 0)
            else {
                return Some(Vec::new());
            };
            let notice = restart_notice(
                asker.estate.restart_notice,
                seconds,
                &identity
                    .sim_name
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
            );
            if let Err(error) = send_notice(sim, asker.agent.agent_id, &notice, now) {
                tracing::warn!("announcing a restart failed: {error}");
            }
            return Some(vec![RegionChange::Notice(Box::new(notice))]);
        }
        // The region and the estate messages: OpenSim sends both as a
        // non-modal agent alert with the text alone, to everyone in the
        // region (measured by the `estate` census, 2026-10-17). The fake
        // grid's estate is one region, so the two are the same here.
        REGION_MESSAGE | ESTATE_MESSAGE => {
            let body = if method == REGION_MESSAGE {
                params.get(4)
            } else {
                params.get(1)
            };
            let Some(body) = body.filter(|body| !body.is_empty()) else {
                return Some(Vec::new());
            };
            let notice = Notice::AgentAlert(format!("/{body}"));
            if let Err(error) = send_notice(sim, asker.agent.agent_id, &notice, now) {
                tracing::warn!("sending an estate message failed: {error}");
            }
            return Some(vec![RegionChange::Notice(Box::new(notice))]);
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
            return None;
        }
    }
    Some(Vec::new())
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

/// The Region tab's settings as `limits` state them.
fn region_info_update(limits: &sl_proto::RegionLimits) -> sl_proto::RegionInfoUpdate {
    use sl_proto::RegionFlags;
    let flags = limits.flags();
    sl_proto::RegionInfoUpdate {
        block_terraform: flags.contains(RegionFlags::BLOCK_TERRAFORM),
        block_fly: flags.contains(RegionFlags::BLOCK_FLY),
        allow_damage: flags.contains(RegionFlags::ALLOW_DAMAGE),
        allow_land_resell: !flags.contains(RegionFlags::BLOCK_LAND_RESELL),
        agent_limit: i32::try_from(limits.max_agents).unwrap_or(i32::MAX),
        object_bonus: limits.object_bonus_factor,
        maturity: limits.maturity,
        restrict_pushobject: flags.contains(RegionFlags::RESTRICT_PUSHOBJECT),
        allow_parcel_changes: flags.contains(RegionFlags::ALLOW_PARCEL_CHANGES),
        block_fly_over: flags.contains(RegionFlags::BLOCK_FLYOVER),
        block_parcel_search: flags.contains(RegionFlags::BLOCK_PARCEL_SEARCH),
    }
}

/// Writes `flags` into both of `limits`' flag fields: the legacy one takes
/// the low half, the extended block the whole, where the region sends one.
fn set_flags(limits: &mut sl_proto::RegionLimits, flags: sl_proto::RegionFlags) {
    limits.region_flags = sl_proto::RegionFlags::from_legacy(flags.legacy_bits());
    if limits.region_flags_extended.is_some() {
        limits.region_flags_extended = Some(flags);
    }
}

/// Applies the Region tab's `update` to the region — from `setregioninfo` or
/// the `DispatchRegionInfo` capability alike — answers the writer with the
/// region's new record, and returns the change the region's other sessions
/// are told about.
pub(crate) fn apply_region_info(
    world: &mut SceneFixtures,
    identity: &sl_proto::RegionIdentity,
    update: &sl_proto::RegionInfoUpdate,
    sim: &mut SimSession,
    now: Instant,
) -> Vec<RegionChange> {
    use sl_proto::RegionFlags;
    {
        let limits = world.limits_mut(identity);
        let flags = limits
            .flags()
            .with(RegionFlags::BLOCK_TERRAFORM, update.block_terraform)
            .with(RegionFlags::BLOCK_FLY, update.block_fly)
            .with(RegionFlags::ALLOW_DAMAGE, update.allow_damage)
            .with(RegionFlags::BLOCK_LAND_RESELL, !update.allow_land_resell)
            .with(RegionFlags::RESTRICT_PUSHOBJECT, update.restrict_pushobject)
            .with(
                RegionFlags::ALLOW_PARCEL_CHANGES,
                update.allow_parcel_changes,
            )
            .with(RegionFlags::BLOCK_FLYOVER, update.block_fly_over)
            .with(RegionFlags::BLOCK_PARCEL_SEARCH, update.block_parcel_search);
        set_flags(limits, flags);
        limits.max_agents = u32::try_from(update.agent_limit).unwrap_or(0);
        limits.legacy_max_agents = u8::try_from(limits.max_agents).unwrap_or(u8::MAX);
        limits.object_bonus_factor = update.object_bonus;
        limits.maturity = update.maturity;
    }
    let limits = world.limits(identity);
    sim.set_region_stats_flags(limits.flags());
    if let Err(error) = sim.send_region_info(&limits, now) {
        tracing::warn!("answering a region info save failed: {error}");
    }
    vec![RegionChange::RegionConfigured(Box::new(limits))]
}

/// Parameter `index` as an unsigned number, its whole part taken if it is
/// spelled as a decimal, or [`None`] when it is absent or says something else.
fn number(params: &[String], index: usize) -> Option<u32> {
    // A decimal spelling counts. `setregioninfo` formats every numeric
    // parameter with six decimal places, so the region's agent limit arrives as
    // `"100.000000"` and a parser that took digits alone would read the field
    // as absent and leave that half of the save silently unapplied — which is
    // exactly what it did until a two-avatar test asked whether the other
    // avatar was told.
    let value = params.get(index)?.trim();
    value.split('.').next()?.parse().ok()
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
