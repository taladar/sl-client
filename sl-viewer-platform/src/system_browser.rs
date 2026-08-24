//! Handing a URL to the operating system's own browser.
//!
//! The counterpart to the in-viewer browser: a media control's "open
//! externally", a `secondlife:///` link that turns out to be an ordinary web
//! URL, and a link clicked in chat or a notice all end up here. Each wants the
//! desktop's default browser rather than a floater, and none of them wants to
//! know how that is done per platform.

use bevy::prelude::*;

/// Hand a URL to the operating system's browser.
pub fn open_in_system_browser(url: &str) {
    if url.is_empty() {
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if let Err(error) = std::process::Command::new("xdg-open").arg(url).spawn() {
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
