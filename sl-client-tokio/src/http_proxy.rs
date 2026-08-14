//! Process-global HTTP proxy for every reqwest client this crate builds.
//!
//! reqwest can only apply a proxy at client-*build* time, and this crate
//! builds its clients in several places (login, per-session caps, per
//! fetcher), so the proxy is a process-global set exactly once, before any
//! client exists. Binary hosts (sl-repl-tokio, sl-survey) expose it as an
//! `--http-proxy` CLI option and call [`set_proxy`] at startup; every
//! construction site then goes through [`client_builder`] instead of a bare
//! `reqwest::Client::builder()`.
//!
//! When no proxy is set the builder behaves exactly like the bare one —
//! direct connections, with reqwest's default honouring of the
//! `http_proxy` / `https_proxy` environment variables. An explicitly set
//! proxy takes precedence over the environment. SOCKS for the UDP circuit is
//! deliberately out of scope (the circuit does not go through reqwest).

use std::sync::OnceLock;

/// The proxy applied to every client built through this module, set at most
/// once by [`set_proxy`] before any client is constructed.
static HTTP_PROXY: OnceLock<reqwest::Proxy> = OnceLock::new();

/// Installs `host_port` (a `host:port` pair, e.g. `127.0.0.1:8888`) as the
/// HTTP proxy for all traffic of every reqwest client subsequently built
/// through [`client_builder`].
///
/// The proxy URL is validated eagerly (as `http://host:port`), so a bad value
/// fails loudly here — once, at startup — instead of silently degrading every
/// later client build. The first successful call wins; later calls are
/// ignored (clients already built keep their configuration anyway).
///
/// # Errors
///
/// Returns the underlying [`reqwest::Error`] if `host_port` does not form a
/// valid proxy URL.
pub fn set_proxy(host_port: &str) -> Result<(), reqwest::Error> {
    let proxy = reqwest::Proxy::all(format!("http://{host_port}"))?;
    let _first_call_wins = HTTP_PROXY.set(proxy);
    Ok(())
}

/// A reqwest client builder with the process-global proxy (if any)
/// pre-applied. Use this instead of `reqwest::Client::builder()` everywhere,
/// so the host's `--http-proxy` option covers all HTTP traffic.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    match HTTP_PROXY.get() {
        Some(proxy) => builder.proxy(proxy.clone()),
        None => builder,
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{client_builder, set_proxy};

    /// The `OnceLock` is process-global, so one test exercises the whole
    /// lifecycle in order: the builder works unset, garbage is rejected, a
    /// valid proxy installs, and the builder still builds with it applied.
    #[test]
    fn proxy_lifecycle() {
        // Unset: the builder builds a plain direct client.
        client_builder().build().expect("a direct client builds");
        // A host:port with embedded whitespace cannot form a proxy URL.
        set_proxy("not a proxy").expect_err("garbage host:port must be rejected");
        // A valid host:port installs…
        set_proxy("127.0.0.1:8888").expect("a valid host:port installs");
        // …and clients still build with the proxy applied.
        client_builder().build().expect("a proxied client builds");
    }
}
