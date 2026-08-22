//! Pure nearby-avatar radar model — no Bevy, no I/O.
//!
//! The radar (`radar`) is the Firestorm-style presence tool: a live
//! list of who is nearby with distances, plus enter / leave alerts. This
//! module holds everything unit-testable about it: the per-sweep set diff and
//! threshold-crossing detection (the reference's `FSRadar::updateRadarList`
//! over its `mLastRadarSweep` snapshot), the per-agent bookkeeping
//! (first-seen time, last distance / region, profile age and payment info),
//! and the row formatting / filtering / sorting helpers the floater's table
//! consumes.
//!
//! Conventions, matching the reference (Firestorm `fsradar.cpp`, read-only):
//!
//! - **Distances** are 3-D metres from the own avatar; `None` = unknown (a
//!   coarse-only avatar whose altitude byte is the 0 / 1020 sentinel). An
//!   unknown distance counts as *outside* every distance band, and alerts it
//!   participates in omit the "(x m)" suffix.
//! - **Bands**: chat range (say, default 20 m) and draw distance are alert
//!   bands; shout range only colours the range cell ([`range_band`]).
//! - **Region membership** is the avatar's region equal to the own region; a
//!   region change fires sim enter / leave alerts.
//! - The model updates on every sweep regardless of whether the floater is
//!   open, so alerts fire with the radar closed (reference behaviour).

use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use sl_client_bevy::{AgentKey, RegionHandle};

/// Payment-info status from the profile flags (`AVATAR_IDENTIFIED` /
/// `AVATAR_TRANSACTED`), shown as the reference's `$` / `$$` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaymentInfo {
    /// The profile reply has not arrived yet.
    #[default]
    Unknown,
    /// No payment info on file.
    None,
    /// Payment info on file (`AVATAR_IDENTIFIED`).
    Identified,
    /// Payment info used (`AVATAR_TRANSACTED`).
    Transacted,
}

impl PaymentInfo {
    /// The radar cell text: `$$` = payment info used, `$` = on file, blank
    /// otherwise (the reference's flag column).
    #[must_use]
    pub const fn cell_text(self) -> &'static str {
        match self {
            Self::Transacted => "$$",
            Self::Identified => "$",
            Self::Unknown | Self::None => "",
        }
    }

    /// The sort rank of the payment column (blank < `$` < `$$`).
    const fn rank(self) -> u8 {
        match self {
            Self::Unknown | Self::None => 0,
            Self::Identified => 1,
            Self::Transacted => 2,
        }
    }
}

/// One avatar's measurements for a single sweep, sampled from
/// `avatars::AvatarState::map_avatars`.
#[derive(Debug, Clone, Copy)]
pub struct RadarSample {
    /// The avatar's agent id.
    pub agent: AgentKey,
    /// 3-D metres to the own avatar; `None` = unknown (coarse altitude
    /// sentinel).
    pub distance: Option<f32>,
    /// The region the avatar's global position falls in, if on the grid.
    pub region: Option<RegionHandle>,
    /// Known only coarsely (no full object streamed).
    pub coarse_only: bool,
    /// Global position (east, north, up) in metres, for the track /
    /// teleport-to actions; `None` when the altitude is unknown.
    pub position: Option<(f64, f64, f32)>,
}

/// Thresholds and context for one sweep.
#[derive(Debug, Clone, Copy)]
pub struct SweepConfig {
    /// The chat (say) range in metres — the chat-band alert threshold.
    pub chat_range: f32,
    /// The draw distance in metres — the draw-band alert threshold.
    pub draw_distance: f32,
    /// The own avatar's region, if known; sim enter / leave alerts are
    /// suppressed while it is `None`.
    pub own_region: Option<RegionHandle>,
    /// The monotonic viewer time of this sweep, in seconds (drives the
    /// "seen" clock).
    pub now_seconds: f64,
    /// `Some(limit)` arms the young-account alert for accounts of at most
    /// `limit` days.
    pub age_alert_days: Option<u32>,
}

/// Persistent per-agent state across sweeps (the reference's
/// `mLastRadarSweep` snapshot plus the `FSRadarEntry` bookkeeping).
#[derive(Debug, Clone, Copy)]
pub struct RadarEntry {
    /// The sweep time the avatar was first seen (monotonic seconds).
    pub first_seen: f64,
    /// The distance recorded by the previous sweep.
    pub last_distance: Option<f32>,
    /// The region recorded by the previous sweep.
    pub last_region: Option<RegionHandle>,
    /// Whether the avatar was known only coarsely at the last sweep.
    pub coarse_only: bool,
    /// The last known global position (east, north, up), for row actions.
    pub position: Option<(f64, f64, f32)>,
    /// Whether a profile-properties request has been issued for this avatar
    /// (request-once; see [`RadarModel::take_property_requests`]).
    pub properties_requested: bool,
    /// The account age in days, once the profile reply arrived and parsed.
    pub age_days: Option<u32>,
    /// The payment-info status, once the profile reply arrived.
    pub payment: PaymentInfo,
    /// Whether the young-account alert has already fired for this entry
    /// (fired at most once, reference behaviour).
    age_alerted: bool,
}

/// The kind of one radar alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarAlertKind {
    /// Entered chat (say) range.
    ChatEnter,
    /// Left chat (say) range.
    ChatLeave,
    /// Entered draw distance.
    DrawEnter,
    /// Left draw distance.
    DrawLeave,
    /// Entered the own region.
    SimEnter,
    /// Left the own region.
    SimLeave,
    /// A young account (age at most the configured limit) was detected.
    Age,
}

/// One alert produced by a sweep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarAlert {
    /// The avatar the alert is about.
    pub agent: AgentKey,
    /// What happened.
    pub kind: RadarAlertKind,
    /// The distance at the moment of the alert; `None` omits the "(x m)"
    /// suffix (unknown-altitude parity with the reference).
    pub distance: Option<f32>,
}

/// Whether a distance is known and within `threshold` metres (an unknown
/// distance counts as outside every band).
fn within(distance: Option<f32>, threshold: f32) -> bool {
    distance.is_some_and(|distance| distance <= threshold)
}

/// The always-live radar bookkeeping: one [`RadarEntry`] per nearby avatar,
/// plus a revision stamp the view projection rebuilds against.
#[derive(Debug, Default)]
pub struct RadarModel {
    /// The tracked avatars, keyed by agent id.
    entries: HashMap<AgentKey, RadarEntry>,
    /// Bumped whenever a sweep or a profile reply changes anything the view
    /// could show.
    revision: u64,
}

impl RadarModel {
    /// The current revision stamp.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The entry for `agent`, if it is currently tracked.
    #[must_use]
    pub fn entry(&self, agent: AgentKey) -> Option<&RadarEntry> {
        self.entries.get(&agent)
    }

    /// Iterate over all tracked avatars.
    pub fn entries(&self) -> impl Iterator<Item = (&AgentKey, &RadarEntry)> {
        self.entries.iter()
    }

    /// Ingest one sweep of samples: reconcile the tracked set, detect band /
    /// region crossings and departures, and return the alerts they produced
    /// (unfiltered — the caller applies the per-kind notification settings).
    pub fn sweep(&mut self, samples: &[RadarSample], cfg: &SweepConfig) -> Vec<RadarAlert> {
        let mut alerts = Vec::new();
        let mut seen: HashSet<AgentKey> = HashSet::with_capacity(samples.len());
        for sample in samples {
            seen.insert(sample.agent);
            let in_own_region = cfg.own_region.is_some() && sample.region == cfg.own_region;
            match self.entries.entry(sample.agent) {
                Entry::Vacant(slot) => {
                    slot.insert(RadarEntry {
                        first_seen: cfg.now_seconds,
                        last_distance: sample.distance,
                        last_region: sample.region,
                        coarse_only: sample.coarse_only,
                        position: sample.position,
                        properties_requested: false,
                        age_days: None,
                        payment: PaymentInfo::Unknown,
                        age_alerted: false,
                    });
                    // First sighting: report the bands it is already inside
                    // (most specific distance band only, like the reference).
                    if in_own_region {
                        alerts.push(RadarAlert {
                            agent: sample.agent,
                            kind: RadarAlertKind::SimEnter,
                            distance: sample.distance,
                        });
                    }
                    if within(sample.distance, cfg.chat_range) {
                        alerts.push(RadarAlert {
                            agent: sample.agent,
                            kind: RadarAlertKind::ChatEnter,
                            distance: sample.distance,
                        });
                    } else if within(sample.distance, cfg.draw_distance) {
                        alerts.push(RadarAlert {
                            agent: sample.agent,
                            kind: RadarAlertKind::DrawEnter,
                            distance: sample.distance,
                        });
                    }
                }
                Entry::Occupied(mut slot) => {
                    let entry = slot.get_mut();
                    crossing(
                        entry.last_distance,
                        sample.distance,
                        cfg.chat_range,
                        RadarAlertKind::ChatEnter,
                        RadarAlertKind::ChatLeave,
                        sample.agent,
                        &mut alerts,
                    );
                    crossing(
                        entry.last_distance,
                        sample.distance,
                        cfg.draw_distance,
                        RadarAlertKind::DrawEnter,
                        RadarAlertKind::DrawLeave,
                        sample.agent,
                        &mut alerts,
                    );
                    let was_in = cfg.own_region.is_some() && entry.last_region == cfg.own_region;
                    if !was_in && in_own_region {
                        alerts.push(RadarAlert {
                            agent: sample.agent,
                            kind: RadarAlertKind::SimEnter,
                            distance: sample.distance,
                        });
                    } else if was_in && !in_own_region {
                        alerts.push(RadarAlert {
                            agent: sample.agent,
                            kind: RadarAlertKind::SimLeave,
                            distance: sample.distance,
                        });
                    }
                    entry.last_distance = sample.distance;
                    entry.last_region = sample.region;
                    entry.coarse_only = sample.coarse_only;
                    entry.position = sample.position;
                }
            }
        }
        // Young-account alerts: at most once per entry, whenever the age is
        // (or becomes) known and at most the configured limit.
        if let Some(limit) = cfg.age_alert_days {
            for (agent, entry) in &mut self.entries {
                if !entry.age_alerted && entry.age_days.is_some_and(|age| age <= limit) {
                    entry.age_alerted = true;
                    alerts.push(RadarAlert {
                        agent: *agent,
                        kind: RadarAlertKind::Age,
                        distance: None,
                    });
                }
            }
        }
        // Departures: tracked avatars absent from this sweep leave every band
        // they were last inside, then drop off the radar.
        let absent: Vec<AgentKey> = self
            .entries
            .keys()
            .filter(|agent| !seen.contains(agent))
            .copied()
            .collect();
        for agent in absent {
            if let Some(entry) = self.entries.remove(&agent) {
                if within(entry.last_distance, cfg.chat_range) {
                    alerts.push(RadarAlert {
                        agent,
                        kind: RadarAlertKind::ChatLeave,
                        distance: None,
                    });
                } else if within(entry.last_distance, cfg.draw_distance) {
                    alerts.push(RadarAlert {
                        agent,
                        kind: RadarAlertKind::DrawLeave,
                        distance: None,
                    });
                }
                if cfg.own_region.is_some() && entry.last_region == cfg.own_region {
                    alerts.push(RadarAlert {
                        agent,
                        kind: RadarAlertKind::SimLeave,
                        distance: None,
                    });
                }
            }
        }
        // The "seen" clocks tick and distances drift on every sweep with
        // content, so any non-trivial sweep is a view change.
        if !(self.entries.is_empty() && samples.is_empty() && alerts.is_empty()) {
            self.revision = self.revision.wrapping_add(1);
        }
        alerts
    }

    /// Record a profile reply for `agent`; returns whether anything changed
    /// (and bumps the revision if so). A reply for an avatar no longer
    /// tracked is ignored.
    pub fn set_properties(
        &mut self,
        agent: AgentKey,
        age_days: Option<u32>,
        payment: PaymentInfo,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&agent) else {
            return false;
        };
        if entry.age_days == age_days && entry.payment == payment {
            return false;
        }
        entry.age_days = age_days;
        entry.payment = payment;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// Up to `limit` tracked avatars whose profile properties have not been
    /// requested yet, marking each as requested (request-once, throttled by
    /// the caller's per-sweep limit).
    pub fn take_property_requests(&mut self, limit: usize) -> Vec<AgentKey> {
        let mut out = Vec::new();
        for (agent, entry) in &mut self.entries {
            if out.len() >= limit {
                break;
            }
            if !entry.properties_requested {
                entry.properties_requested = true;
                out.push(*agent);
            }
        }
        out
    }
}

/// Push enter / leave alerts for one distance band crossing between two
/// sweeps (an unknown distance counts as outside the band).
fn crossing(
    prev: Option<f32>,
    cur: Option<f32>,
    threshold: f32,
    enter: RadarAlertKind,
    leave: RadarAlertKind,
    agent: AgentKey,
    alerts: &mut Vec<RadarAlert>,
) {
    let was = within(prev, threshold);
    let is = within(cur, threshold);
    if !was && is {
        alerts.push(RadarAlert {
            agent,
            kind: enter,
            distance: cur,
        });
    }
    if was && !is {
        alerts.push(RadarAlert {
            agent,
            kind: leave,
            distance: cur,
        });
    }
}

// ---------------------------------------------------------------------------
// Row projection helpers.
// ---------------------------------------------------------------------------

/// One radar row, fully projected for display and sorting (built by the
/// floater's view rebuild from the model plus the live per-avatar statuses).
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent per-avatar display facts (typing / sitting / away / friend / muted / \
              coarse / in-region), each its own column or tint — not a disguised state machine"
)]
#[derive(Debug, Clone)]
pub struct RadarRow {
    /// The avatar's agent id.
    pub agent: AgentKey,
    /// The primary display label (display name, or the legacy name / a
    /// provisional id fragment while unresolved).
    pub name: String,
    /// The `username` line shown after the name; empty when unknown or
    /// redundant.
    pub username: String,
    /// The avatar's active group title (may be empty).
    pub title: String,
    /// Payment-info status (the `$` column).
    pub payment: PaymentInfo,
    /// Account age in days, when known.
    pub age_days: Option<u32>,
    /// Seconds since the avatar was first seen.
    pub seen_seconds: u64,
    /// 3-D metres to the own avatar; `None` = unknown.
    pub distance: Option<f32>,
    /// Known only coarsely (hollow region dot).
    pub coarse_only: bool,
    /// In the own avatar's region.
    pub in_own_region: bool,
    /// Currently typing in nearby chat.
    pub typing: bool,
    /// Currently seated.
    pub sitting: bool,
    /// Playing the away animation.
    pub away: bool,
    /// A friend of the own avatar.
    pub friend: bool,
    /// On the own avatar's mute list.
    pub muted: bool,
    /// The avatar's render cost (ARC), once it has been measured
    /// (`avatar_complexity`); `None` while it has not.
    pub complexity: Option<u32>,
    /// Whether the viewer is currently drawing this avatar as a jellydoll — the
    /// column's whole point is telling "expensive" from "expensive enough that I
    /// stopped drawing them".
    pub jellied: bool,
}

/// The distance band a row falls in, colouring its range cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeBand {
    /// Within chat (say) range.
    Chat,
    /// Within shout range.
    Shout,
    /// Beyond shout range.
    Beyond,
    /// Distance unknown (coarse altitude sentinel).
    Unknown,
}

/// Classify a distance into its colouring band.
#[must_use]
pub fn range_band(distance: Option<f32>, chat: f32, shout: f32) -> RangeBand {
    match distance {
        None => RangeBand::Unknown,
        Some(distance) if distance <= chat => RangeBand::Chat,
        Some(distance) if distance <= shout => RangeBand::Shout,
        Some(_) => RangeBand::Beyond,
    }
}

/// Format a range cell: metres with two decimals, or the reference's
/// `>draw-distance` form when the distance is unknown.
#[must_use]
pub fn format_range(distance: Option<f32>, draw_distance: f32) -> String {
    match distance {
        Some(distance) => format!("{distance:.2}"),
        None => format!(">{draw_distance:.2}"),
    }
}

/// Format the "seen" cell as `H:MM:SS` elapsed (hours unbounded, like the
/// reference's `%d:%02d:%02d`).
#[must_use]
pub fn format_seen(elapsed_seconds: u64) -> String {
    let hours = elapsed_seconds / 3600;
    let minutes = (elapsed_seconds % 3600) / 60;
    let seconds = elapsed_seconds % 60;
    format!("{hours}:{minutes:02}:{seconds:02}")
}

/// The whole (non-negative) seconds elapsed between two monotonic
/// timestamps.
#[must_use]
pub fn elapsed_seconds(now: f64, since: f64) -> u64 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped non-negative and floored; session times are far inside u64"
    )]
    let out = (now - since).max(0.0).floor() as u64;
    out
}

/// Parse a profile `born_on` date — the SL `MM/DD/YYYY` form or an ISO
/// `YYYY-MM-DD` (OpenSim) — into an account age in days as of `today`.
/// Unparsable or empty input yields `None`; a future date clamps to `0`.
#[must_use]
pub fn parse_born_on(born_on: &str, today: jiff::civil::Date) -> Option<u32> {
    let trimmed = born_on.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Some grids append a time; keep the leading date token only.
    let token = trimmed.split_whitespace().next()?;
    let date = jiff::civil::Date::strptime("%m/%d/%Y", token)
        .or_else(|_| jiff::civil::Date::strptime("%Y-%m-%d", token))
        .ok()?;
    let days = date.until(today).ok()?.get_days();
    u32::try_from(days.max(0)).ok()
}

/// Whether a row matches a name filter (case-insensitive substring over the
/// display name and username; an empty filter matches everything).
#[must_use]
pub fn matches_filter(row: &RadarRow, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    row.name.to_lowercase().contains(&needle) || row.username.to_lowercase().contains(&needle)
}

/// Whether a row passes the near-me range limit (`None` = unlimited; an
/// unknown distance passes, reference parity).
#[must_use]
pub fn within_limit(row: &RadarRow, limit: Option<f32>) -> bool {
    match (limit, row.distance) {
        (Some(limit), Some(distance)) => distance <= limit,
        _ => true,
    }
}

/// The `(total, in region, in chat range)` counts for the header line.
#[must_use]
pub fn counts(rows: &[RadarRow], chat_range: f32) -> (usize, usize, usize) {
    let total = rows.len();
    let in_region = rows.iter().filter(|row| row.in_own_region).count();
    let in_chat = rows
        .iter()
        .filter(|row| within(row.distance, chat_range))
        .count();
    (total, in_region, in_chat)
}

/// A sortable radar column (the table's sortable tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    /// The name column.
    Name,
    /// The group-title column.
    Title,
    /// The payment-info column.
    Payment,
    /// The account-age column.
    Age,
    /// The first-seen clock column.
    Seen,
    /// The distance column.
    Range,
    /// The render-cost (ARC) column.
    Complexity,
}

impl SortColumn {
    /// Resolve a table column token to its sort column.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "name" => Some(Self::Name),
            "title" => Some(Self::Title),
            "payment" => Some(Self::Payment),
            "age" => Some(Self::Age),
            "seen" => Some(Self::Seen),
            "range" => Some(Self::Range),
            "complexity" => Some(Self::Complexity),
            _ => None,
        }
    }
}

/// Compare two rows on one column, ascending (unknown ages and distances
/// sort last).
fn compare_rows(column: SortColumn, a: &RadarRow, b: &RadarRow) -> Ordering {
    match column {
        SortColumn::Name => a
            .name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.username.to_lowercase().cmp(&b.username.to_lowercase())),
        SortColumn::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortColumn::Payment => a.payment.rank().cmp(&b.payment.rank()),
        SortColumn::Age => a
            .age_days
            .unwrap_or(u32::MAX)
            .cmp(&b.age_days.unwrap_or(u32::MAX)),
        SortColumn::Seen => a.seen_seconds.cmp(&b.seen_seconds),
        SortColumn::Range => a
            .distance
            .unwrap_or(f32::INFINITY)
            .total_cmp(&b.distance.unwrap_or(f32::INFINITY)),
        // An unmeasured avatar sorts last, like an unknown age or distance —
        // "not scored yet" is not "cheap".
        SortColumn::Complexity => a
            .complexity
            .unwrap_or(u32::MAX)
            .cmp(&b.complexity.unwrap_or(u32::MAX)),
    }
}

/// Sort rows by a multi-key order (each key a column plus ascending flag),
/// tie-breaking by case-folded name and finally agent id for a total order.
pub fn sort_rows(rows: &mut [RadarRow], keys: &[(SortColumn, bool)]) {
    rows.sort_by(|a, b| {
        for (column, ascending) in keys {
            let ordering = compare_rows(*column, a, b);
            let ordering = if *ascending {
                ordering
            } else {
                ordering.reverse()
            };
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.agent.uuid().cmp(&b.agent.uuid()))
    });
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::Uuid;

    use super::*;

    /// A test agent key from a small integer.
    fn agent(id: u128) -> AgentKey {
        AgentKey::from(Uuid::from_u128(id))
    }

    /// The own region used by the test sweeps.
    fn own_region() -> RegionHandle {
        RegionHandle::from_grid(1000, 1000)
    }

    /// A default sweep config: chat 20 m, draw 128 m, own region known.
    fn config(now: f64) -> SweepConfig {
        SweepConfig {
            chat_range: 20.0,
            draw_distance: 128.0,
            own_region: Some(own_region()),
            now_seconds: now,
            age_alert_days: None,
        }
    }

    /// A sample in the own region at a known distance.
    fn sample(id: u128, distance: Option<f32>) -> RadarSample {
        RadarSample {
            agent: agent(id),
            distance,
            region: Some(own_region()),
            coarse_only: distance.is_none(),
            position: distance.map(|_| (256_000.0, 256_000.0, 25.0)),
        }
    }

    /// The alert kinds for one agent, in emission order.
    fn kinds_for(alerts: &[RadarAlert], id: u128) -> Vec<RadarAlertKind> {
        alerts
            .iter()
            .filter(|alert| alert.agent == agent(id))
            .map(|alert| alert.kind)
            .collect()
    }

    /// A display row with the given name / distance and defaults elsewhere.
    fn row(id: u128, name: &str, distance: Option<f32>) -> RadarRow {
        RadarRow {
            agent: agent(id),
            name: name.to_owned(),
            username: String::new(),
            title: String::new(),
            payment: PaymentInfo::Unknown,
            age_days: None,
            seen_seconds: 0,
            distance,
            coarse_only: false,
            in_own_region: true,
            typing: false,
            sitting: false,
            away: false,
            friend: false,
            muted: false,
            complexity: None,
            jellied: false,
        }
    }

    #[test]
    fn first_sighting_in_chat_range_fires_sim_and_chat_enter() {
        let mut model = RadarModel::default();
        let alerts = model.sweep(&[sample(1, Some(10.0))], &config(0.0));
        assert_eq!(
            kinds_for(&alerts, 1),
            vec![RadarAlertKind::SimEnter, RadarAlertKind::ChatEnter]
        );
        assert_eq!(alerts.first().and_then(|alert| alert.distance), Some(10.0));
        assert_eq!(model.entries().count(), 1);
    }

    #[test]
    fn first_sighting_in_draw_only_fires_draw_enter_not_chat() {
        let mut model = RadarModel::default();
        let alerts = model.sweep(&[sample(1, Some(50.0))], &config(0.0));
        assert_eq!(
            kinds_for(&alerts, 1),
            vec![RadarAlertKind::SimEnter, RadarAlertKind::DrawEnter]
        );
    }

    #[test]
    fn first_sighting_beyond_draw_fires_sim_enter_only() {
        let mut model = RadarModel::default();
        let alerts = model.sweep(&[sample(1, Some(500.0))], &config(0.0));
        assert_eq!(kinds_for(&alerts, 1), vec![RadarAlertKind::SimEnter]);
    }

    #[test]
    fn chat_threshold_crossings_fire_both_directions() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(30.0))], &config(0.0));
        let entered = model.sweep(&[sample(1, Some(15.0))], &config(1.0));
        assert_eq!(kinds_for(&entered, 1), vec![RadarAlertKind::ChatEnter]);
        assert_eq!(entered.first().and_then(|alert| alert.distance), Some(15.0));
        let left = model.sweep(&[sample(1, Some(25.0))], &config(2.0));
        assert_eq!(kinds_for(&left, 1), vec![RadarAlertKind::ChatLeave]);
        let unchanged = model.sweep(&[sample(1, Some(26.0))], &config(3.0));
        assert!(unchanged.is_empty());
    }

    #[test]
    fn draw_threshold_crossing_fires_draw_alerts() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(200.0))], &config(0.0));
        let entered = model.sweep(&[sample(1, Some(100.0))], &config(1.0));
        assert_eq!(kinds_for(&entered, 1), vec![RadarAlertKind::DrawEnter]);
        let left = model.sweep(&[sample(1, Some(300.0))], &config(2.0));
        assert_eq!(kinds_for(&left, 1), vec![RadarAlertKind::DrawLeave]);
    }

    #[test]
    fn departure_fires_most_specific_leave_and_sim_leave_and_removes() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(10.0))], &config(0.0));
        let alerts = model.sweep(&[], &config(1.0));
        assert_eq!(
            kinds_for(&alerts, 1),
            vec![RadarAlertKind::ChatLeave, RadarAlertKind::SimLeave]
        );
        assert!(alerts.iter().all(|alert| alert.distance.is_none()));
        assert_eq!(model.entries().count(), 0);
    }

    #[test]
    fn departure_from_draw_band_fires_draw_leave() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(100.0))], &config(0.0));
        let alerts = model.sweep(&[], &config(1.0));
        assert_eq!(
            kinds_for(&alerts, 1),
            vec![RadarAlertKind::DrawLeave, RadarAlertKind::SimLeave]
        );
    }

    #[test]
    fn region_change_fires_sim_leave_and_enter() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(500.0))], &config(0.0));
        let neighbour = RadarSample {
            region: Some(RegionHandle::from_grid(1001, 1000)),
            ..sample(1, Some(500.0))
        };
        let left = model.sweep(&[neighbour], &config(1.0));
        assert_eq!(kinds_for(&left, 1), vec![RadarAlertKind::SimLeave]);
        let back = model.sweep(&[sample(1, Some(500.0))], &config(2.0));
        assert_eq!(kinds_for(&back, 1), vec![RadarAlertKind::SimEnter]);
    }

    #[test]
    fn unknown_own_region_suppresses_sim_alerts() {
        let mut model = RadarModel::default();
        let mut cfg = config(0.0);
        cfg.own_region = None;
        let unknown_region = RadarSample {
            region: None,
            ..sample(1, Some(10.0))
        };
        let alerts = model.sweep(&[unknown_region], &cfg);
        assert_eq!(kinds_for(&alerts, 1), vec![RadarAlertKind::ChatEnter]);
    }

    #[test]
    fn unknown_distance_counts_as_outside_all_bands() {
        let mut model = RadarModel::default();
        let alerts = model.sweep(&[sample(1, None)], &config(0.0));
        assert_eq!(kinds_for(&alerts, 1), vec![RadarAlertKind::SimEnter]);
        // Known → unknown fires the leaves once, with no distance suffix.
        model.sweep(&[sample(1, Some(10.0))], &config(1.0));
        let lost = model.sweep(&[sample(1, None)], &config(2.0));
        assert_eq!(
            kinds_for(&lost, 1),
            vec![RadarAlertKind::ChatLeave, RadarAlertKind::DrawLeave]
        );
        assert!(lost.iter().all(|alert| alert.distance.is_none()));
        let still_lost = model.sweep(&[sample(1, None)], &config(3.0));
        assert!(still_lost.is_empty());
    }

    #[test]
    fn age_alert_fires_once_when_age_known_and_young() {
        let mut model = RadarModel::default();
        let mut cfg = config(0.0);
        cfg.age_alert_days = Some(7);
        model.sweep(&[sample(1, Some(10.0))], &cfg);
        // Age not yet known: no alert.
        assert!(model.set_properties(agent(1), Some(3), PaymentInfo::None));
        let alerts = model.sweep(&[sample(1, Some(10.0))], &cfg);
        assert_eq!(kinds_for(&alerts, 1), vec![RadarAlertKind::Age]);
        let again = model.sweep(&[sample(1, Some(10.0))], &cfg);
        assert!(again.is_empty());
    }

    #[test]
    fn age_alert_disabled_or_old_account_never_fires() {
        let mut model = RadarModel::default();
        model.sweep(&[sample(1, Some(10.0))], &config(0.0));
        model.set_properties(agent(1), Some(3), PaymentInfo::None);
        // Disabled config: no alert even though the account is young.
        let alerts = model.sweep(&[sample(1, Some(10.0))], &config(1.0));
        assert!(alerts.is_empty());
        let mut cfg = config(2.0);
        cfg.age_alert_days = Some(7);
        model.set_properties(agent(1), Some(400), PaymentInfo::None);
        let alerts = model.sweep(&[sample(1, Some(10.0))], &cfg);
        assert!(alerts.is_empty());
    }

    #[test]
    fn property_requests_are_taken_once_and_throttled() {
        let mut model = RadarModel::default();
        let samples: Vec<RadarSample> = (1..=4).map(|id| sample(id, Some(10.0))).collect();
        model.sweep(&samples, &config(0.0));
        let first = model.take_property_requests(3);
        assert_eq!(first.len(), 3);
        let second = model.take_property_requests(3);
        assert_eq!(second.len(), 1);
        assert!(model.take_property_requests(3).is_empty());
    }

    #[test]
    fn set_properties_ignores_untracked_and_detects_no_change() {
        let mut model = RadarModel::default();
        assert!(!model.set_properties(agent(9), Some(3), PaymentInfo::None));
        model.sweep(&[sample(1, Some(10.0))], &config(0.0));
        assert!(model.set_properties(agent(1), Some(3), PaymentInfo::Identified));
        assert!(!model.set_properties(agent(1), Some(3), PaymentInfo::Identified));
    }

    #[test]
    fn revision_bumps_on_sweeps_with_content_only() {
        let mut model = RadarModel::default();
        let before = model.revision();
        model.sweep(&[], &config(0.0));
        assert_eq!(model.revision(), before);
        model.sweep(&[sample(1, Some(10.0))], &config(1.0));
        assert!(model.revision() > before);
    }

    #[test]
    fn parse_born_on_handles_both_formats_and_junk() {
        let today = jiff::civil::date(2026, 8, 14);
        assert_eq!(parse_born_on("08/04/2026", today), Some(10));
        assert_eq!(parse_born_on("2026-08-04", today), Some(10));
        assert_eq!(parse_born_on("2026-08-04 12:30:00", today), Some(10));
        assert_eq!(parse_born_on("10/24/2006", today), Some(7234));
        // A future date clamps to zero rather than failing.
        assert_eq!(parse_born_on("2026-09-01", today), Some(0));
        assert_eq!(parse_born_on("", today), None);
        assert_eq!(parse_born_on("unknown", today), None);
    }

    #[test]
    fn format_range_shows_decimals_and_unknown_form() {
        assert_eq!(format_range(Some(12.345), 128.0), "12.35");
        assert_eq!(format_range(None, 128.0), ">128.00");
    }

    #[test]
    fn format_seen_renders_hours_minutes_seconds() {
        assert_eq!(format_seen(59), "0:00:59");
        assert_eq!(format_seen(3723), "1:02:03");
        assert_eq!(format_seen(36_000), "10:00:00");
    }

    #[test]
    fn elapsed_seconds_clamps_negative() {
        assert_eq!(elapsed_seconds(5.9, 1.0), 4);
        assert_eq!(elapsed_seconds(1.0, 5.0), 0);
    }

    #[test]
    fn range_band_edges() {
        assert_eq!(range_band(Some(20.0), 20.0, 100.0), RangeBand::Chat);
        assert_eq!(range_band(Some(20.1), 20.0, 100.0), RangeBand::Shout);
        assert_eq!(range_band(Some(100.0), 20.0, 100.0), RangeBand::Shout);
        assert_eq!(range_band(Some(100.1), 20.0, 100.0), RangeBand::Beyond);
        assert_eq!(range_band(None, 20.0, 100.0), RangeBand::Unknown);
    }

    #[test]
    fn filter_matches_name_and_username_case_insensitively() {
        let mut subject = row(1, "Avatar Tester", None);
        subject.username = "avatar.tester".to_owned();
        assert!(matches_filter(&subject, ""));
        assert!(matches_filter(&subject, "  "));
        assert!(matches_filter(&subject, "TESTER"));
        assert!(matches_filter(&subject, "avatar.t"));
        assert!(!matches_filter(&subject, "friend"));
    }

    #[test]
    fn range_limit_passes_unknown_distances() {
        let near = row(1, "Near", Some(50.0));
        let far = row(2, "Far", Some(300.0));
        let unknown = row(3, "Unknown", None);
        assert!(within_limit(&near, Some(162.0)));
        assert!(!within_limit(&far, Some(162.0)));
        assert!(within_limit(&unknown, Some(162.0)));
        assert!(within_limit(&far, None));
    }

    #[test]
    fn counts_total_region_chat() {
        let mut other_region = row(3, "Elsewhere", Some(15.0));
        other_region.in_own_region = false;
        let rows = vec![
            row(1, "Close", Some(10.0)),
            row(2, "Mid", Some(50.0)),
            other_region,
            row(4, "Unknown", None),
        ];
        assert_eq!(counts(&rows, 20.0), (4, 3, 2));
    }

    #[test]
    fn sort_rows_range_ascending_puts_unknown_last() {
        let mut rows = vec![
            row(1, "Bravo", None),
            row(2, "Alpha", Some(50.0)),
            row(3, "Charlie", Some(5.0)),
        ];
        sort_rows(&mut rows, &[(SortColumn::Range, true)]);
        let order: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(order, vec!["Charlie", "Alpha", "Bravo"]);
        sort_rows(&mut rows, &[(SortColumn::Range, false)]);
        let order: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(order, vec!["Bravo", "Alpha", "Charlie"]);
    }

    #[test]
    fn sort_rows_secondary_key_breaks_ties() {
        let mut a = row(1, "Zed", Some(10.0));
        a.age_days = Some(100);
        let mut b = row(2, "Ann", Some(10.0));
        b.age_days = Some(100);
        let mut c = row(3, "Mid", Some(10.0));
        c.age_days = Some(5);
        let mut rows = vec![a, b, c];
        sort_rows(
            &mut rows,
            &[(SortColumn::Age, true), (SortColumn::Name, true)],
        );
        let order: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(order, vec!["Mid", "Ann", "Zed"]);
    }

    #[test]
    fn sort_tokens_resolve() {
        assert_eq!(SortColumn::from_token("range"), Some(SortColumn::Range));
        assert_eq!(SortColumn::from_token("name"), Some(SortColumn::Name));
        assert_eq!(SortColumn::from_token("status"), None);
    }

    #[test]
    fn payment_cell_text() {
        assert_eq!(PaymentInfo::Transacted.cell_text(), "$$");
        assert_eq!(PaymentInfo::Identified.cell_text(), "$");
        assert_eq!(PaymentInfo::None.cell_text(), "");
        assert_eq!(PaymentInfo::Unknown.cell_text(), "");
    }
}
