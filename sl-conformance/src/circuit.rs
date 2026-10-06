//! Reading a probed circuit: what the simulator sent, datagram by datagram.
//!
//! A case that measures how a grid treats a circuit turns a
//! [`CircuitProbe`](sl_client_tokio::CircuitProbe) on and then has two things
//! to do — stay listening for a while without failing on the quiet, and turn
//! the [`Diagnostic::Datagram`]s the probe produced into counts and intervals.
//! Both live here, once, for `keepalive-ping`, `circuit-unacked-resend` and
//! `circuit-silence`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sl_client_tokio::{Diagnostic, DisconnectReason, Event, PacketFlags};

use crate::context::{Session, TestFailure};

/// One datagram the simulator sent, as a probed session reported it.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    /// Seconds from the instant the caller measures from to its arrival
    /// (negative for a datagram that arrived before it).
    pub offset: f64,
    /// The simulator that sent it.
    pub from: SocketAddr,
    /// Whether it arrived on a child circuit.
    pub child: bool,
    /// Whether it was sent reliably.
    pub reliable: bool,
    /// Whether the simulator flagged it as a retransmission.
    pub resent: bool,
    /// The simulator's sequence number for it.
    pub sequence: u32,
    /// The message it carried.
    pub name: Option<&'static str>,
    /// Its length on the wire.
    pub len: usize,
}

impl Seen {
    /// Whether the datagram carried the message called `name`.
    #[must_use]
    pub fn is(&self, name: &str) -> bool {
        self.name == Some(name)
    }
}

/// Every datagram `session`'s probe has reported since its first `skip`
/// diagnostics, timed from `origin`.
#[must_use]
pub fn seen_since(session: &Session, skip: usize, origin: Instant) -> Vec<Seen> {
    session
        .diagnostics()
        .iter()
        .skip(skip)
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::Datagram {
                at,
                from,
                child,
                flags,
                sequence,
                len,
                name,
                ..
            } => Some(Seen {
                offset: signed_seconds(*at, origin),
                from: *from,
                child: *child,
                reliable: flags.contains(PacketFlags::RELIABLE),
                resent: flags.contains(PacketFlags::RESENT),
                sequence: sequence.get(),
                name: *name,
                len: *len,
            }),
            _ => None,
        })
        .collect()
}

/// `at - origin` in seconds, negative when `at` is the earlier.
fn signed_seconds(at: Instant, origin: Instant) -> f64 {
    if at >= origin {
        at.duration_since(origin).as_secs_f64()
    } else {
        -origin.duration_since(at).as_secs_f64()
    }
}

/// Stay in world for `duration`, handing every event to `on_event`, and say
/// how the session ended if it did.
///
/// # Errors
///
/// Returns [`TestFailure::Disconnected`] if the event channel closed without
/// the session reporting why.
pub async fn listen<F>(
    session: &mut Session,
    duration: Duration,
    mut on_event: F,
) -> Result<Option<DisconnectReason>, TestFailure>
where
    F: FnMut(&Event),
{
    let watched = session
        .wait_for(duration, |event| {
            on_event(event);
            match event {
                Event::Disconnected(reason) => Some(reason.clone()),
                _ => None,
            }
        })
        .await;
    match watched {
        Ok(reason) => Ok(Some(reason)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// `count` as the `u32` a metric holds, saturating.
#[must_use]
pub fn tally(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The arrival offsets of the datagrams in `seen` that carried `name`.
#[must_use]
pub fn offsets_of<'a>(seen: impl IntoIterator<Item = &'a Seen>, name: &str) -> Vec<f64> {
    seen.into_iter()
        .filter(|datagram| datagram.is(name))
        .map(|datagram| datagram.offset)
        .collect()
}

/// The gaps between consecutive offsets, in seconds.
#[must_use]
pub fn gaps(offsets: &[f64]) -> Vec<f64> {
    offsets
        .iter()
        .zip(offsets.iter().skip(1))
        .map(|(earlier, later)| later - earlier)
        .collect()
}

/// The median of `values`, or `None` of an empty list.
#[must_use]
pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len().midpoint(0)).copied()
}

/// `values` as one metric string, two decimals each — a timeline a record can
/// carry whole.
#[must_use]
pub fn timeline(values: &[f64]) -> String {
    values
        .iter()
        .map(|value| format!("{value:.2}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The arrival offsets of every transmission of each reliable packet in
/// `seen`, keyed by the circuit it came down and its sequence number.
#[must_use]
pub fn transmissions(seen: &[Seen]) -> BTreeMap<(SocketAddr, u32), Vec<f64>> {
    let mut by_packet: BTreeMap<(SocketAddr, u32), Vec<f64>> = BTreeMap::new();
    for datagram in seen.iter().filter(|datagram| datagram.reliable) {
        by_packet
            .entry((datagram.from, datagram.sequence))
            .or_default()
            .push(datagram.offset);
    }
    by_packet
}

/// How many of `seen` carried each message, most first, as one metric
/// string (`ObjectUpdate:12 LayerData:3`).
#[must_use]
pub fn histogram<'a>(seen: impl IntoIterator<Item = &'a Seen>) -> String {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for datagram in seen {
        let count = counts.entry(datagram.name.unwrap_or("?")).or_default();
        *count = count.saturating_add(1);
    }
    let mut ordered: Vec<(&'static str, usize)> = counts.into_iter().collect();
    ordered.sort_by(|(_, left), (_, right)| right.cmp(left));
    ordered
        .iter()
        .map(|(name, count)| format!("{name}:{count}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{gaps, median, timeline};
    use pretty_assertions::assert_eq;

    #[test]
    fn gaps_are_between_neighbours() {
        assert_eq!(timeline(&gaps(&[1.0, 3.0, 6.5])), "2.00 3.50");
        assert_eq!(timeline(&gaps(&[1.0])), "");
    }

    #[test]
    fn the_median_of_nothing_is_nothing() {
        assert_eq!(median(&[]).map(|middle| timeline(&[middle])), None);
        assert_eq!(
            median(&[5.0, 1.0, 3.0]).map(|middle| timeline(&[middle])),
            Some("3.00".to_owned())
        );
    }

    #[test]
    fn a_timeline_is_two_decimals_each() {
        assert_eq!(timeline(&[1.0, 2.345]), "1.00 2.35");
    }
}
