//! Watching the object-update stream: what each message carried, what
//! arrived, and what a kill took away.
//!
//! Shared by the cases that change one thing a viewer says to a simulator —
//! its handshake flags, its draw distance, its camera, its interest-list mode
//! — and then count what the stream does about it.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Event, InterestListReply, ObjectStreamBatch, ObjectUpdateForm, RegionLocalObjectId, pcode,
};

use crate::circuit::{Seen, listen, tally};
use crate::context::{Session, TestFailure};
use crate::metrics::Metrics;
use crate::support::{count_metric, secs_metric};

/// What the session knows of an object: enough to say what a kill took away.
#[derive(Debug, Clone, Copy)]
pub struct Known {
    /// Its parent's local id, zero for a root.
    pub parent: RegionLocalObjectId,
    /// Its `PCode`.
    pub pcode: u8,
}

/// The objects of every circuit, keyed by whether the circuit is a child's
/// and the object's local id — which is all a batch says of where it came
/// from. Two neighbours' ids can collide; the root's, which the checks are
/// about, cannot.
pub type World = BTreeMap<(bool, RegionLocalObjectId), Known>;

/// One watch of the stream.
#[derive(Debug, Default)]
pub struct Leg {
    /// Every batch, in order.
    pub batches: Vec<ObjectStreamBatch>,
    /// The root-circuit objects that arrived (`ObjectAdded`), by `PCode`.
    pub added: BTreeMap<u8, usize>,
    /// The root-circuit objects that arrived with a parent.
    pub added_children: usize,
    /// The root-circuit kills, by what was killed: `root`, `child`, `avatar`
    /// or `unknown` (an object the session did not hold).
    pub kills: BTreeMap<&'static str, usize>,
    /// How many objects the session held whose parent a root-circuit kill
    /// named and which the kill did not name itself: the rest of a linkset
    /// whose root alone was killed, which goes with it.
    pub orphaned: usize,
    /// The simulator's answer to an interest-list mode switch, if one came.
    pub interest_list: Option<InterestListReply>,
    /// Whether an `AvatarAppearance` about the agent itself came.
    pub own_appearance: bool,
    /// Seconds from the watch's start to the first root-circuit batch.
    pub first: Option<f64>,
    /// Seconds from the watch's start to the last root-circuit batch.
    pub last: Option<f64>,
}

impl Leg {
    /// The batches of `form` on the root circuit (`child == false`) or on the
    /// child circuits.
    pub fn of(
        &self,
        form: ObjectUpdateForm,
        child: bool,
    ) -> impl Iterator<Item = &ObjectStreamBatch> {
        self.batches
            .iter()
            .filter(move |batch| batch.form == form && batch.child == child)
    }

    /// How many objects the root circuit's batches of `form` named.
    #[must_use]
    pub fn named(&self, form: ObjectUpdateForm) -> usize {
        self.of(form, false).map(|batch| batch.entries.len()).sum()
    }

    /// How many root-circuit kills there were, of anything.
    #[must_use]
    pub fn kills(&self) -> usize {
        self.kills.values().sum()
    }

    /// How many root-circuit objects arrived, of any kind.
    #[must_use]
    pub fn arrivals(&self) -> usize {
        self.added.values().sum()
    }

    /// How many root-circuit kills there were of `what`.
    #[must_use]
    pub fn killed(&self, what: &str) -> usize {
        self.kills.get(what).copied().unwrap_or(0)
    }
}

/// Watch the stream for `duration`, keeping `world` current.
///
/// # Errors
///
/// Returns [`TestFailure::Disconnected`] if the session ends during the
/// watch.
pub async fn watch(
    session: &mut Session,
    world: &mut World,
    duration: Duration,
) -> Result<Leg, TestFailure> {
    let started = Instant::now();
    let own = session.agent_id();
    let root = session.circuit_id();
    let mut leg = Leg::default();
    // A kill names an id; what it was is only known from before the kill,
    // and the batch comes ahead of the removal.
    let ended = listen(session, duration, |event| match event {
        Event::ObjectStreamBatch(batch) => {
            if !batch.child {
                let at = started.elapsed().as_secs_f64();
                leg.first.get_or_insert(at);
                leg.last = Some(at);
                if batch.form == ObjectUpdateForm::Kill {
                    for entry in &batch.entries {
                        let what = match world.get(&(false, entry.local_id)) {
                            None => "unknown",
                            Some(known) if known.pcode == pcode::AVATAR => "avatar",
                            Some(known) if known.parent == RegionLocalObjectId(0) => "root",
                            Some(_) => "child",
                        };
                        let count = leg.kills.entry(what).or_default();
                        *count = count.saturating_add(1);
                    }
                    let named: BTreeSet<RegionLocalObjectId> =
                        batch.entries.iter().map(|entry| entry.local_id).collect();
                    leg.orphaned = leg.orphaned.saturating_add(
                        world
                            .iter()
                            .filter(|((child, id), known)| {
                                !child && !named.contains(id) && named.contains(&known.parent)
                            })
                            .count(),
                    );
                }
            }
            if batch.form == ObjectUpdateForm::Kill {
                // What hangs off a killed object goes with it, as the session
                // does it: every prim under it, no avatar.
                let mut going: Vec<RegionLocalObjectId> =
                    batch.entries.iter().map(|entry| entry.local_id).collect();
                while let Some(id) = going.pop() {
                    let _gone = world.remove(&(batch.child, id));
                    going.extend(
                        world
                            .iter()
                            .filter(|((child, _), known)| {
                                *child == batch.child
                                    && known.parent == id
                                    && known.pcode != pcode::AVATAR
                            })
                            .map(|((_, dependent), _)| *dependent),
                    );
                }
            }
            leg.batches.push((**batch).clone());
        }
        Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
            let child = Some(object.circuit) != root;
            let _old = world.insert(
                (child, object.local_id),
                Known {
                    parent: object.parent_id,
                    pcode: object.pcode,
                },
            );
            if matches!(event, Event::ObjectAdded(_)) && !child {
                let count = leg.added.entry(object.pcode).or_default();
                *count = count.saturating_add(1);
                if object.parent_id != RegionLocalObjectId(0) {
                    leg.added_children = leg.added_children.saturating_add(1);
                }
            }
        }
        Event::InterestListMode(reply) => leg.interest_list = Some(*reply),
        Event::AvatarAppearance(appearance) if Some(appearance.avatar_id) == own => {
            leg.own_appearance = true;
        }
        _ => {}
    })
    .await?;
    if let Some(reason) = ended {
        return Err(TestFailure::Disconnected(format!("{reason:?}")));
    }
    Ok(leg)
}

/// `values` as one metric string of their distinct values and how often each
/// came (`4:63 2:1`), most frequent first.
pub fn distribution<T: Ord + core::fmt::Display>(values: impl IntoIterator<Item = T>) -> String {
    let mut counts: BTreeMap<T, usize> = BTreeMap::new();
    for value in values {
        let count = counts.entry(value).or_default();
        *count = count.saturating_add(1);
    }
    let mut ordered: Vec<(T, usize)> = counts.into_iter().collect();
    ordered.sort_by(|(_, left), (_, right)| right.cmp(left));
    ordered
        .iter()
        .map(|(value, count)| format!("{value}:{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A form's name in a metric key, and the message it is.
pub const FORMS: [(ObjectUpdateForm, &str, &str); 5] = [
    (ObjectUpdateForm::Full, "full", "ObjectUpdate"),
    (
        ObjectUpdateForm::Compressed,
        "compressed",
        "ObjectUpdateCompressed",
    ),
    (ObjectUpdateForm::Cached, "cached", "ObjectUpdateCached"),
    (
        ObjectUpdateForm::Terse,
        "terse",
        "ImprovedTerseObjectUpdate",
    ),
    (ObjectUpdateForm::Kill, "kill", "KillObject"),
];

/// Record what one leg held, under `name`.
pub fn record(metrics: &mut Metrics, name: &str, leg: &Leg) {
    for child in [false, true] {
        let side = if child { "child" } else { "root" };
        for (form, key, _message) in FORMS {
            let batches: Vec<&ObjectStreamBatch> = leg.of(form, child).collect();
            if batches.is_empty() {
                continue;
            }
            let prefix = format!("{name}_{side}_{key}");
            metrics.set(
                &count_metric(&format!("{prefix}_messages")),
                tally(batches.len()),
            );
            let entries = || batches.iter().flat_map(|batch| batch.entries.iter());
            metrics.set(
                &count_metric(&format!("{prefix}_objects")),
                tally(entries().count()),
            );
            metrics.set(
                &count_metric(&format!("{prefix}_objects_distinct")),
                tally(
                    entries()
                        .map(|entry| entry.local_id)
                        .collect::<BTreeSet<_>>()
                        .len(),
                ),
            );
            metrics.set(
                &count_metric(&format!("{prefix}_objects_held_already")),
                tally(entries().filter(|entry| entry.known).count()),
            );
            metrics.set(
                &format!("{prefix}_per_message"),
                distribution(batches.iter().map(|batch| batch.entries.len())),
            );
            if form == ObjectUpdateForm::Cached {
                metrics.set(
                    &count_metric(&format!("{prefix}_hits")),
                    tally(
                        entries()
                            .filter(|entry| entry.cache_hit == Some(true))
                            .count(),
                    ),
                );
            }
        }
    }
    for (code, count) in &leg.added {
        let kind = match *code {
            pcode::PRIMITIVE => "primitives".to_owned(),
            pcode::AVATAR => "avatars".to_owned(),
            other => format!("pcode_{other}"),
        };
        metrics.set(
            &count_metric(&format!("{name}_added_{kind}")),
            tally(*count),
        );
    }
    metrics.set(
        &count_metric(&format!("{name}_added_children")),
        tally(leg.added_children),
    );
    for (what, count) in &leg.kills {
        metrics.set(
            &count_metric(&format!("{name}_killed_{what}")),
            tally(*count),
        );
    }
    metrics.set(
        &count_metric(&format!("{name}_killed_unnamed_children")),
        tally(leg.orphaned),
    );
    metrics.set(&format!("{name}_own_appearance"), leg.own_appearance);
    if let Some(reply) = leg.interest_list {
        metrics.set(
            &format!("{name}_interest_list_reply"),
            format!(
                "mode {} previous {}",
                reply.mode.as_str(),
                reply.previous_mode.as_str()
            ),
        );
    }
    if let (Some(first), Some(last)) = (leg.first, leg.last) {
        metrics.set(&secs_metric(&format!("{name}_first")), first);
        metrics.set(&secs_metric(&format!("{name}_last")), last);
    }
}

/// Record how the arrival's messages travelled: each form's datagram lengths
/// and reliability, off the probe.
pub fn record_datagrams(metrics: &mut Metrics, seen: &[Seen]) {
    for child in [false, true] {
        let side = if child { "child" } else { "root" };
        for (_form, key, message) in FORMS {
            let datagrams: Vec<&Seen> = seen
                .iter()
                .filter(|datagram| datagram.child == child && datagram.is(message))
                .collect();
            if datagrams.is_empty() {
                continue;
            }
            let prefix = format!("arrival_{side}_{key}");
            let low = datagrams.iter().map(|datagram| datagram.len).min();
            let high = datagrams.iter().map(|datagram| datagram.len).max();
            if let (Some(low), Some(high)) = (low, high) {
                metrics.set(
                    &format!("{prefix}_datagram_bytes"),
                    format!("{low}..{high}"),
                );
            }
            metrics.set(
                &format!("{prefix}_reliable"),
                distribution(datagrams.iter().map(|datagram| datagram.reliable)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::distribution;
    use pretty_assertions::assert_eq;

    /// A distribution lists the most frequent value first.
    #[test]
    fn a_distribution_puts_the_commonest_value_first() {
        assert_eq!(distribution([4, 4, 2, 4]), "4:3 2:1");
    }
}
