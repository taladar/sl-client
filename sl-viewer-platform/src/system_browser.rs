//! Handing a URL to the operating system's own browser.
//!
//! The counterpart to the in-viewer browser: a media control's "open
//! externally", a `secondlife:///` link that turns out to be an ordinary web
//! URL, and a link clicked in chat or a notice all end up here. Each wants the
//! desktop's default browser rather than a floater, and none of them wants to
//! know how that is done per platform.
//!
//! Nothing reaches the desktop untyped: [`open_in_system_browser`] takes an
//! [`ExternalUrl`], which exists only for a URL whose scheme passed
//! [`SYSTEM_BROWSER_SCHEMES`].

use bevy::prelude::*;

/// The schemes the desktop may be handed.
///
/// Deliberately narrower than `sl_media::MEDIA_URL_SCHEMES`: this sink is not
/// a browser (see [`ExternalUrl`]), and a streaming scheme has no business
/// reaching the desktop's handler table either — the in-viewer engines are
/// what play a stream.
///
/// The reference viewer's equivalent (`gURLProtocolWhitelist`,
/// `indra/llwindow/llwindow.cpp`) additionally admits `secondlife:`, `ftp:`,
/// `data:`, `mailto:` and — in Firestorm's Linux build — `file:`. Of those,
/// `secondlife:` is dispatched inside the viewer long before this sink, and
/// each of the rest is a scheme whose desktop handler is something other than
/// a browser, which is the whole hazard here.
pub const SYSTEM_BROWSER_SCHEMES: &[&str] = &["http", "https"];

/// Why a URL was refused the system browser.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExternalUrlError {
    /// The text is not a URL at all.
    #[error("not a URL: {0}")]
    Malformed(#[from] url::ParseError),
    /// The URL parses, but its scheme is not one the desktop may be handed.
    #[error(
        "URL scheme `{scheme}` is not one the desktop may be handed (allowed: {allowed}) — {url}"
    )]
    SchemeNotAllowed {
        /// The rejected scheme, lower-cased.
        scheme: String,
        /// The rejected URL, for the log line that reports the refusal.
        url: String,
        /// The allowed schemes, comma-separated, so the message is
        /// self-contained.
        allowed: String,
    },
}

/// A URL the desktop may be handed: one whose **scheme** passed
/// [`SYSTEM_BROWSER_SCHEMES`].
///
/// `xdg-open` is not a browser. It dispatches by scheme and MIME type, so a
/// `file://` URL opens the user's file manager or editor, and any scheme with
/// a desktop-file handler (`mailto:`, `tel:`, a game launcher, a package
/// installer) launches *that*. Every string that reaches this sink is remote
/// data — a link in chat, a notice or an IM (what counts as a link is the
/// linkifier's regex, not a human's judgement), a `secondlife:///` link that
/// turned out to carry an ordinary URL, or the current URL of a page the page
/// itself chose by navigating.
///
/// So the check happens **once**, where that data enters, and the type carries
/// the evidence from there: [`open_in_system_browser`] takes this and nothing
/// else. It is the same shape as `sl_media::ValidatedMediaUrl` at the
/// in-viewer engines, with a narrower allowlist, and a sibling type rather
/// than a reuse because a platform leaf crate does not depend on the media
/// stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalUrl(url::Url);

impl ExternalUrl {
    /// Parse and validate a URL supplied as text — a linkified chat link, a
    /// SLURL payload, a page's own address. The only constructor: every sink
    /// caller holds text, so there is no second door to keep honest.
    ///
    /// # Errors
    /// [`ExternalUrlError::Malformed`] when the text is not a URL,
    /// [`ExternalUrlError::SchemeNotAllowed`] when its scheme is not in
    /// [`SYSTEM_BROWSER_SCHEMES`].
    pub fn parse(text: &str) -> Result<Self, ExternalUrlError> {
        let url = url::Url::parse(text)?;
        let scheme = url.scheme().to_ascii_lowercase();
        if SYSTEM_BROWSER_SCHEMES.contains(&scheme.as_str()) {
            Ok(Self(url))
        } else {
            Err(ExternalUrlError::SchemeNotAllowed {
                url: url.to_string(),
                scheme,
                allowed: SYSTEM_BROWSER_SCHEMES.join(", "),
            })
        }
    }

    /// The URL as text, the form the desktop takes.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl core::fmt::Display for ExternalUrl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Hand a URL to the operating system's browser.
pub fn open_in_system_browser(url: &ExternalUrl) {
    #[cfg(target_os = "linux")]
    {
        if let Err(error) = std::process::Command::new("xdg-open")
            .arg(url.as_str())
            .spawn()
        {
            warn!("xdg-open failed for {url}: {error}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        warn!("open-in-system-browser is not wired on this platform yet ({url})");
    }
}

/// Normalise what a user typed into the address bar into a navigable URL:
/// scheme kept when present, `https://` assumed otherwise. `None` when it
/// cannot be a URL at all.
///
/// Normalisation only — no allowlist. Its callers are the **in-viewer**
/// address bars (the web floater, the media controls, the profile web panel),
/// which then check the result against the wider `MEDIA_URL_SCHEMES`; a typed
/// `rtsp://` stream is a media URL those bars accept. The system browser's own
/// filter is [`ExternalUrl`], at that sink.
#[must_use]
pub fn normalize_web_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    url::Url::parse(&candidate).ok().map(|url| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ExternalUrl, ExternalUrlError, SYSTEM_BROWSER_SCHEMES, normalize_web_url};
    use pretty_assertions::assert_eq;

    /// Assert the allowlist refuses `text` for its **scheme**, naming that
    /// scheme, and reporting what happened instead when it does not.
    fn refused_scheme(text: &str, expected: &str) -> Result<(), String> {
        match ExternalUrl::parse(text) {
            Err(ExternalUrlError::SchemeNotAllowed {
                scheme, allowed, ..
            }) => {
                if scheme != expected {
                    return Err(format!("{text} was refused as {scheme}, not {expected}"));
                }
                if allowed != SYSTEM_BROWSER_SCHEMES.join(", ") {
                    return Err(format!("{text}'s refusal does not name the allowlist"));
                }
                Ok(())
            }
            Err(other) => Err(format!("{text} refused for the wrong reason: {other}")),
            Ok(accepted) => Err(format!("{accepted} must not reach the desktop")),
        }
    }

    #[test]
    fn web_schemes_are_accepted() -> Result<(), String> {
        for text in [
            "http://example.com/",
            "https://example.com/page?q=1#frag",
            "https://user:pw@example.com:8443/deep/path",
        ] {
            let url = ExternalUrl::parse(text)
                .map_err(|error| format!("{text} should be openable: {error}"))?;
            assert_eq!(url.as_str(), text);
        }
        Ok(())
    }

    /// The hazard this type exists for: `xdg-open` dispatches by scheme, so
    /// each of these launches something that is not a browser. `ftp:` and
    /// `mailto:` are in the reference's own whitelist and refused here anyway
    /// — see [`SYSTEM_BROWSER_SCHEMES`].
    #[test]
    fn schemes_the_desktop_dispatches_elsewhere_are_refused() -> Result<(), String> {
        for (text, scheme) in [
            ("file:///etc/passwd", "file"),
            ("file:///home/someone/.ssh/id_ed25519", "file"),
            ("mailto:someone@example.com", "mailto"),
            ("tel:+15551234", "tel"),
            ("data:text/html,<h1>hi", "data"),
            ("javascript:alert(1)", "javascript"),
            ("ftp://ftp.example.com/pub", "ftp"),
            ("rtsp://example.com/stream", "rtsp"),
            ("about:blank", "about"),
            ("chrome://settings", "chrome"),
            ("steam://run/440", "steam"),
            (
                "secondlife:///app/agent/00000000-0000-0000-0000-000000000000/about",
                "secondlife",
            ),
        ] {
            refused_scheme(text, scheme)?;
        }
        Ok(())
    }

    /// Case does not smuggle a scheme past the allowlist: the URL parser
    /// lower-cases the scheme, and the check lower-cases again rather than
    /// trusting that.
    #[test]
    fn scheme_matching_is_case_folded() -> Result<(), String> {
        let url = ExternalUrl::parse("HTTPS://example.com/")
            .map_err(|error| format!("an upper-cased allowed scheme is still allowed: {error}"))?;
        assert_eq!(url.as_str(), "https://example.com/");
        refused_scheme("FILE:///etc/passwd", "file")
    }

    #[test]
    fn non_urls_are_refused() -> Result<(), String> {
        for text in ["", "   ", "example.com", "not a url at all", "://"] {
            match ExternalUrl::parse(text) {
                Err(ExternalUrlError::Malformed(_error)) => {}
                Err(other) => {
                    return Err(format!("{text:?} refused for the wrong reason: {other}"));
                }
                Ok(accepted) => return Err(format!("{text:?} was accepted as {accepted}")),
            }
        }
        Ok(())
    }

    /// `normalize_web_url` is the in-viewer address bars' normaliser, not this
    /// sink's filter: it must keep passing the streaming schemes those bars
    /// accept, or the media path regresses.
    #[test]
    fn normalize_web_url_keeps_a_scheme_the_desktop_is_refused() -> Result<(), String> {
        assert_eq!(
            normalize_web_url("rtsp://example.com/stream"),
            Some("rtsp://example.com/stream".to_owned())
        );
        refused_scheme("rtsp://example.com/stream", "rtsp")
    }
}
