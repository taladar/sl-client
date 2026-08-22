//! **Media stream diagnostics** — recover the *real* reason a network media
//! URL failed to play.
//!
//! Both media engines run on GStreamer (`sl-gst`): the parcel radio stream
//! (`parcel_audio`) and media-on-a-prim video
//! (`media_prim`). When a stream fails at its HTTP **source**,
//! GStreamer's `souphttpsrc` swallows the underlying DNS / TCP / TLS /
//! HTTP-status cause — it logs it only at its own debug level and hands the
//! pipeline a bare *"Internal data stream error."* / flow-return `-5`. So the
//! true reason never reaches the application bus (see
//! [`sl_gst::AudioStreamStatus::network_diagnosable`] /
//! `sl_media::SurfaceStatus::network_diagnosable`).
//!
//! This module recovers it the only way left: when a stream reports such a
//! generic HTTP-source failure, [`MediaDiagnostics::request`] probes the URL
//! ourselves on a background [`IoTaskPool`] task — resolve, connect, TLS,
//! GET — and classifies the failure into a readable sentence
//! (`diagnose_stream_url`). Consumers read it back with
//! [`MediaDiagnostics::reason`] and show it in place of the generic error.
//! Results are cached by URL, so a failing stream is probed once, not per
//! frame.

use std::collections::HashMap;
use std::net::ToSocketAddrs as _;
use std::time::Duration;

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};

/// The diagnostic probe's request timeout, in seconds.
const PROBE_TIMEOUT_SECS: u64 = 8;

/// The `User-Agent` the diagnostic probe presents — a neutral media-client
/// string, so a server that gates on `User-Agent` behaves as it would for the
/// stream player rather than 403-ing an empty agent.
const PROBE_USER_AGENT: &str = "sl-client-viewer/0.1 (media diagnostic probe)";

/// The most characters of a server error body to fold into the reason.
const BODY_HINT_MAX_CHARS: usize = 100;

/// The cap on cached diagnoses — a backstop; real sessions see a handful of
/// failing streams, not thousands.
const MAX_DIAGNOSES: usize = 64;

/// One URL's diagnosis: an in-flight probe, or its finished result (`Some`
/// reason, or `None` when the URL turned out to be reachable and the failure
/// is downstream of the network).
#[derive(Debug)]
enum Diagnosis {
    /// The probe is still running on a background task.
    Pending(Task<Option<String>>),
    /// The probe finished; the precise reason, or `None` if the URL was
    /// reachable.
    Done(Option<String>),
}

/// The viewer-wide cache of media-stream failure diagnoses, keyed by URL.
#[derive(Debug, Resource, Default)]
pub struct MediaDiagnostics {
    /// The per-URL diagnosis (probe in flight, or its result).
    entries: HashMap<String, Diagnosis>,
}

impl MediaDiagnostics {
    /// Ensure a diagnostic probe exists for a failed stream `url` (idempotent —
    /// a URL already probed or in flight is left alone). Called by a consumer
    /// the moment a stream reports a generic HTTP-source failure.
    pub fn request(&mut self, url: &str) {
        if self.entries.contains_key(url) || self.entries.len() >= MAX_DIAGNOSES {
            return;
        }
        let probe_url = String::from(url);
        let task = IoTaskPool::get().spawn(async move { diagnose_stream_url(&probe_url) });
        let _previous = self
            .entries
            .insert(String::from(url), Diagnosis::Pending(task));
    }

    /// The precise failure reason for `url`, once its probe has finished and
    /// found a network cause. [`None`] while the probe is in flight, or when
    /// the URL was reachable (so the caller keeps GStreamer's generic message).
    #[must_use]
    pub fn reason(&self, url: &str) -> Option<&str> {
        match self.entries.get(url) {
            Some(Diagnosis::Done(Some(reason))) => Some(reason.as_str()),
            _pending_or_none => None,
        }
    }

    /// Advance every in-flight probe; move each finished one to its result.
    fn advance(&mut self) {
        for entry in self.entries.values_mut() {
            if let Diagnosis::Pending(task) = entry
                && let Some(result) = block_on(poll_once(task))
            {
                if let Some(reason) = result.as_deref() {
                    debug!("media stream diagnosis: {reason}");
                }
                *entry = Diagnosis::Done(result);
            }
        }
    }
}

/// Registers the [`MediaDiagnostics`] cache and its per-frame probe pump.
#[derive(Debug)]
pub struct MediaDiagnosticsPlugin;

impl Plugin for MediaDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MediaDiagnostics>()
            .add_systems(Update, pump_media_diagnostics);
    }
}

/// Per frame: advance the in-flight diagnostic probes.
fn pump_media_diagnostics(mut diagnostics: ResMut<MediaDiagnostics>) {
    diagnostics.advance();
}

/// Probe a failed stream URL to recover the precise reason GStreamer hid:
/// name-resolution, TCP connect, TLS, or an HTTP error status (with the first
/// line of any error body). Returns [`None`] when the URL is actually
/// reachable — then the failure is downstream (codec / format), not the
/// network, and the generic message is left in place. Runs on a background
/// task; the blocking calls never touch the render thread.
fn diagnose_stream_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;

    // 1. Name resolution.
    match (host, port).to_socket_addrs() {
        Ok(mut addresses) => {
            if addresses.next().is_none() {
                return Some(format!(
                    "cannot reach {host} — the host name resolved to no addresses"
                ));
            }
        }
        Err(error) => {
            return Some(format!("cannot reach {host} — DNS lookup failed ({error})"));
        }
    }

    // 2-4. TCP connect / TLS / HTTP status, via a short blocking request.
    let client = sl_client_bevy::http_proxy::blocking_client_builder()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECS))
        .user_agent(PROBE_USER_AGENT)
        .build()
        .ok()?;
    match client.get(url).header("Icy-MetaData", "1").send() {
        Ok(response) => {
            let status = response.status();
            (status.is_client_error() || status.is_server_error())
                .then(|| http_status_message(status, response))
        }
        Err(error) => Some(classify_transport_error(&error, host, port)),
    }
}

/// A readable message for an HTTP error status, folding in the first non-empty
/// line of any (short) text body the server returned.
fn http_status_message(
    status: reqwest::StatusCode,
    response: reqwest::blocking::Response,
) -> String {
    let code = status.as_u16();
    let reason = status.canonical_reason().unwrap_or("error");
    let body_hint = response.text().ok().and_then(|body| {
        body.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(BODY_HINT_MAX_CHARS).collect::<String>())
    });
    match body_hint {
        Some(hint) if !hint.is_empty() => {
            format!("server returned HTTP {code} {reason} — {hint}")
        }
        _absent => format!("server returned HTTP {code} {reason}"),
    }
}

/// Classify a transport-level request failure (no HTTP response arrived) into a
/// human reason: timeout, TLS/certificate, refused connection, or a generic
/// connect failure — using the innermost error in the source chain, which
/// carries the specific OS / TLS message.
fn classify_transport_error(error: &reqwest::Error, host: &str, port: u16) -> String {
    if error.is_timeout() {
        return format!("connection to {host}:{port} timed out");
    }
    let chain = transport_error_chain(error);
    let haystack = chain.join(" | ").to_lowercase();
    let innermost = chain.last().map_or("connection failed", String::as_str);
    if [
        "certificate",
        "tls",
        "ssl",
        "handshake",
        "self-signed",
        "expired",
    ]
    .iter()
    .any(|needle| haystack.contains(needle))
    {
        return format!("TLS/certificate error connecting to {host} — {innermost}");
    }
    if haystack.contains("refused") {
        return format!("connection refused by {host}:{port}");
    }
    if error.is_connect() {
        return format!("cannot connect to {host}:{port} — {innermost}");
    }
    format!("cannot fetch the stream from {host} — {innermost}")
}

/// The de-duplicated messages of an error and its `source()` chain, outermost
/// first — the last entry is the most specific (e.g. the raw OS / TLS error).
fn transport_error_chain(error: &reqwest::Error) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    let mut current: Option<&dyn std::error::Error> = Some(error);
    while let Some(source) = current {
        let text = source.to_string();
        if !messages.contains(&text) {
            messages.push(text);
        }
        current = source.source();
    }
    messages
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::diagnose_stream_url;

    /// A URL whose host cannot resolve is diagnosed as a DNS failure — the
    /// reserved `.invalid` TLD never resolves (RFC 6761), so this is hermetic.
    #[test]
    fn diagnose_reports_dns_failure_for_unresolvable_host() {
        let reason =
            diagnose_stream_url("http://nonexistent-host-xyz.invalid/stream").unwrap_or_default();
        assert!(
            reason.contains("DNS lookup failed") || reason.contains("resolved to no addresses"),
            "expected a DNS diagnosis, got: {reason:?}"
        );
    }

    /// A malformed URL yields no diagnosis rather than panicking.
    #[test]
    fn diagnose_ignores_malformed_url() {
        assert_eq!(diagnose_stream_url("not a url"), None);
    }
}
