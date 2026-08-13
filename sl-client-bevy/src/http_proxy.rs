//! Process-global HTTP proxy for every reqwest client this crate (and the
//! viewer on top of it) builds.
//!
//! reqwest can only apply a proxy at client-*build* time, and this crate
//! builds its clients in many places (per worker thread, lazily, and once per
//! login), so the proxy is a process-global set exactly once, before the app
//! starts — the same idiom as the viewer's replay cache-root override. The
//! viewer reads its `HttpProxy` preferences setting during startup and calls
//! [`set_proxy`]; every construction site then goes through
//! [`blocking_client_builder`] / [`async_client_builder`] instead of a bare
//! `reqwest::…Client::builder()`.
//!
//! When no proxy is set the builders behave exactly like the bare ones —
//! direct connections, with reqwest's default honouring of the
//! `http_proxy` / `https_proxy` environment variables. An explicitly set
//! proxy takes precedence over the environment.
//!
//! Deliberately out of scope: SOCKS (the UDP circuit does not go through
//! reqwest at all) and the embedded CEF browser (its Chromium network stack
//! is configured separately).

use std::sync::OnceLock;

/// The proxy applied to every client built through this module, set at most
/// once by [`set_proxy`] before any client is constructed.
static HTTP_PROXY: OnceLock<reqwest::Proxy> = OnceLock::new();

/// Installs `host_port` (a `host:port` pair, e.g. `127.0.0.1:8888`) as the
/// HTTP proxy for all traffic of every reqwest client subsequently built
/// through [`blocking_client_builder`] / [`async_client_builder`].
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

/// A blocking reqwest client builder with the process-global proxy (if any)
/// pre-applied. Use this instead of `reqwest::blocking::Client::builder()`
/// everywhere, so the viewer's proxy preference covers all HTTP traffic.
pub fn blocking_client_builder() -> reqwest::blocking::ClientBuilder {
    let builder = reqwest::blocking::Client::builder();
    match HTTP_PROXY.get() {
        Some(proxy) => builder.proxy(proxy.clone()),
        None => builder,
    }
}

/// An async reqwest client builder with the process-global proxy (if any)
/// pre-applied. Use this instead of `reqwest::Client::builder()` everywhere,
/// so the viewer's proxy preference covers all HTTP traffic.
pub fn async_client_builder() -> reqwest::ClientBuilder {
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

    use super::{blocking_client_builder, set_proxy};

    /// The `OnceLock` is process-global, so one test exercises the whole
    /// lifecycle in order: builders work unset, garbage is rejected, a valid
    /// proxy installs, and builders still build with it applied.
    #[test]
    fn proxy_lifecycle() {
        // Unset: the builder builds a plain direct client.
        blocking_client_builder()
            .build()
            .expect("a direct client builds");
        // A host:port with embedded whitespace cannot form a proxy URL.
        set_proxy("not a proxy").expect_err("garbage host:port must be rejected");
        // A valid host:port installs…
        set_proxy("127.0.0.1:8888").expect("a valid host:port installs");
        // …and clients still build with the proxy applied.
        blocking_client_builder()
            .build()
            .expect("a proxied blocking client builds");
        super::async_client_builder()
            .build()
            .expect("a proxied async client builds");
    }
}
