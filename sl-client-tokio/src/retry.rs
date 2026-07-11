//! Shared transient-HTTP-error retry policy for the async asset / texture / mesh
//! fetchers.
//!
//! The `ViewerAsset` / `GetTexture` / `GetMesh2` cap services answer a transient
//! `503` (and, behind a proxy, `502` / `504`) while they queue the requested asset
//! from the backing store, then serve the bytes once ready. A fetch that gives up
//! on the first `503` strands the asset (a texture that never resolves, a mesh
//! frozen at a coarse LOD), so the fetchers retry a bounded number of times with
//! **exponential backoff** (awaiting [`transient_backoff`] on the tokio timer),
//! matching the reference viewer's polling behaviour.

use std::time::Duration;

use reqwest::StatusCode as ReqwestStatusCode;

/// The maximum number of times a transient (`503`/`502`/`504`) response is retried
/// before the fetch fails.
pub(crate) const MAX_TRANSIENT_RETRIES: u32 = 8;

/// The first retry's backoff; each subsequent retry doubles it up to
/// [`MAX_TRANSIENT_BACKOFF`].
const INITIAL_TRANSIENT_BACKOFF: Duration = Duration::from_millis(200);

/// The cap on the exponential backoff, so a long-queuing service is polled at a
/// steady ceiling rather than an ever-growing delay.
const MAX_TRANSIENT_BACKOFF: Duration = Duration::from_secs(5);

/// Whether `status` is a transient poll-service response worth retrying: the cap
/// services answer `503` while queuing an asset, and a fronting proxy can surface
/// it as `502` / `504`.
pub(crate) fn is_transient_status(status: ReqwestStatusCode) -> bool {
    matches!(
        status,
        ReqwestStatusCode::SERVICE_UNAVAILABLE
            | ReqwestStatusCode::BAD_GATEWAY
            | ReqwestStatusCode::GATEWAY_TIMEOUT
    )
}

/// The backoff before retry number `attempt` (0-based): exponential
/// ([`INITIAL_TRANSIENT_BACKOFF`] doubled per attempt), capped at
/// [`MAX_TRANSIENT_BACKOFF`].
pub(crate) fn transient_backoff(attempt: u32) -> Duration {
    // Cap the shift so the multiplier never overflows before the cap clamps it.
    let factor = 1_u32.checked_shl(attempt.min(5)).unwrap_or(u32::MAX);
    INITIAL_TRANSIENT_BACKOFF
        .saturating_mul(factor)
        .min(MAX_TRANSIENT_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::{MAX_TRANSIENT_BACKOFF, is_transient_status, transient_backoff};
    use pretty_assertions::assert_eq;
    use reqwest::StatusCode as ReqwestStatusCode;
    use std::time::Duration;

    #[test]
    fn backoff_grows_exponentially_then_caps() {
        assert_eq!(transient_backoff(0), Duration::from_millis(200));
        assert_eq!(transient_backoff(1), Duration::from_millis(400));
        assert_eq!(transient_backoff(2), Duration::from_millis(800));
        assert_eq!(transient_backoff(20), MAX_TRANSIENT_BACKOFF);
    }

    #[test]
    fn only_service_unavailable_style_statuses_are_transient() {
        assert!(is_transient_status(ReqwestStatusCode::SERVICE_UNAVAILABLE));
        assert!(is_transient_status(ReqwestStatusCode::BAD_GATEWAY));
        assert!(is_transient_status(ReqwestStatusCode::GATEWAY_TIMEOUT));
        assert!(!is_transient_status(ReqwestStatusCode::NOT_FOUND));
        assert!(!is_transient_status(ReqwestStatusCode::OK));
    }
}
