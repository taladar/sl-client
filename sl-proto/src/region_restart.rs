//! Region restart notices: the alerts a simulator sends ahead of a restart,
//! read into one countdown whichever grid sent them.
//!
//! The two grids say it differently (measured on 2026-10-17, the `estate`
//! census, `book/src/gridspec/estate.md`):
//!
//! - **Second Life** names the notification: an `AlertMessage` whose
//!   `AlertInfo` is keyed `RegionRestartMinutes` (parameters `MINUTES`,
//!   `NAME`) or `RegionRestartSeconds` (`SECONDS`, `NAME`). The reference
//!   viewer (`process_alert_core`) opens its countdown off the key.
//! - **OpenSim** writes it out: a non-modal `AgentAlertMessage` reading
//!   `/Region will restart in 30 seconds` (`RestartModule`, the duration in
//!   minutes and seconds — "2 minutes and 30 seconds"). The slash is
//!   OpenSim's mark on every non-modal agent alert.
//!
//! The reference also reads the `RESTART_X_MINUTES n` / `RESTART_X_SECONDS n`
//! form some OpenSim forks send after a `/`, and closes its countdown on a
//! system message beginning `Region restart cancelled.`; both are read here
//! too, as the reference reads them.

use crate::types::AlertInfo;

/// What a restart notice says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RegionRestartNotice {
    /// The region restarts in `seconds`.
    Scheduled {
        /// Seconds until the restart, as the notice states it.
        seconds: u32,
        /// The region's name, where the notice names it (Second Life's
        /// `NAME`); OpenSim's text names none.
        region_name: Option<String>,
    },
    /// A restart that was scheduled will not happen.
    Cancelled,
}

/// The key Second Life's minute-granularity restart notice carries.
pub const REGION_RESTART_MINUTES: &str = "RegionRestartMinutes";

/// The key Second Life's second-granularity restart notice carries.
pub const REGION_RESTART_SECONDS: &str = "RegionRestartSeconds";

/// The start of the system message the reference closes its countdown on.
const RESTART_CANCELLED: &str = "Region restart cancelled.";

/// The start of OpenSim's own restart notice.
const OPENSIM_RESTART_PREFIX: &str = "Region will restart in ";

impl RegionRestartNotice {
    /// Reads an `AlertMessage`: Second Life's keyed notice, else its text.
    #[must_use]
    pub fn from_alert(message: &str, alert_info: &[AlertInfo]) -> Option<Self> {
        alert_info
            .iter()
            .find_map(Self::from_alert_info)
            .or_else(|| Self::from_text(message))
    }

    /// Reads one `AlertInfo`: `RegionRestartMinutes` or
    /// `RegionRestartSeconds`, with the duration and the region's name from
    /// its parameters. A key without a readable duration is no notice.
    #[must_use]
    pub fn from_alert_info(info: &AlertInfo) -> Option<Self> {
        let unit = match info.message.as_str() {
            REGION_RESTART_MINUTES => ("MINUTES", 60),
            REGION_RESTART_SECONDS => ("SECONDS", 1),
            _ => return None,
        };
        let parameters = info.parameters();
        let value = |name: &str| {
            parameters
                .iter()
                .find(|(key, _value)| key == name)
                .map(|(_key, value)| value.clone())
        };
        let count: u32 = value(unit.0)?.trim().parse().ok()?;
        Some(Self::Scheduled {
            seconds: count.saturating_mul(unit.1),
            region_name: value("NAME").filter(|name| !name.is_empty()),
        })
    }

    /// Reads an alert's text: OpenSim's `Region will restart in …`, the
    /// `RESTART_X_MINUTES` / `RESTART_X_SECONDS` form and the cancellation,
    /// each with or without the leading `/`.
    #[must_use]
    pub fn from_text(message: &str) -> Option<Self> {
        let text = message.trim();
        let text = text.strip_prefix('/').unwrap_or(text);
        if text.starts_with(RESTART_CANCELLED) {
            return Some(Self::Cancelled);
        }
        for (prefix, unit) in [("RESTART_X_MINUTES", 60), ("RESTART_X_SECONDS", 1)] {
            if let Some(rest) = text.strip_prefix(prefix) {
                let count: u32 = rest.trim().parse().ok()?;
                return Some(Self::Scheduled {
                    seconds: count.saturating_mul(unit),
                    region_name: None,
                });
            }
        }
        let duration = text.strip_prefix(OPENSIM_RESTART_PREFIX)?;
        Some(Self::Scheduled {
            seconds: opensim_duration(duration)?,
            region_name: None,
        })
    }
}

/// OpenSim's spelled-out duration — `1 minute`, `2 minutes and 30 seconds`,
/// `15 seconds` — in seconds.
fn opensim_duration(text: &str) -> Option<u32> {
    let mut total: u32 = 0;
    let mut any = false;
    for part in text.trim_end_matches('.').split(" and ") {
        let (count, unit) = part.trim().split_once(' ')?;
        let count: u32 = count.parse().ok()?;
        let scale = match unit.trim() {
            "minute" | "minutes" => 60,
            "second" | "seconds" => 1,
            _ => return None,
        };
        total = total.saturating_add(count.saturating_mul(scale));
        any = true;
    }
    any.then_some(total)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::RegionRestartNotice;
    use crate::types::AlertInfo;

    /// A Second Life notice, keyed, its parameters a serialized map.
    fn keyed(key: &str, unit: &str, count: u32) -> AlertInfo {
        AlertInfo {
            message: key.to_owned(),
            extra_params: format!(
                "<? LLSD/XML ?>\n<llsd><map><key>{unit}</key><integer>{count}</integer>\
                 <key>NAME</key><string>Mauve</string></map></llsd>\n"
            ),
        }
    }

    #[test]
    fn second_life_keyed_notices_read_as_seconds() {
        assert_eq!(
            RegionRestartNotice::from_alert(
                "The region you are in now is about to restart.",
                &[keyed("RegionRestartMinutes", "MINUTES", 5)]
            ),
            Some(RegionRestartNotice::Scheduled {
                seconds: 300,
                region_name: Some("Mauve".to_owned()),
            })
        );
        assert_eq!(
            RegionRestartNotice::from_alert("", &[keyed("RegionRestartSeconds", "SECONDS", 30)]),
            Some(RegionRestartNotice::Scheduled {
                seconds: 30,
                region_name: Some("Mauve".to_owned()),
            })
        );
    }

    /// OpenSim's notice as `RestartModule` writes it, slash and all.
    #[test]
    fn opensim_text_reads_as_seconds() {
        for (text, seconds) in [
            ("/Region will restart in 30 seconds", 30),
            ("/Region will restart in 15 seconds", 15),
            ("/Region will restart in 1 minute", 60),
            ("Region will restart in 2 minutes and 30 seconds", 150),
            ("/Region will restart in 1 minute and 1 second", 61),
        ] {
            assert_eq!(
                RegionRestartNotice::from_text(text),
                Some(RegionRestartNotice::Scheduled {
                    seconds,
                    region_name: None,
                }),
                "{text:?}"
            );
        }
    }

    /// The forms the reference reads besides: `RESTART_X_*` and the
    /// cancellation.
    #[test]
    fn reference_forms_are_read() {
        assert_eq!(
            RegionRestartNotice::from_text("/RESTART_X_MINUTES 3"),
            Some(RegionRestartNotice::Scheduled {
                seconds: 180,
                region_name: None,
            })
        );
        assert_eq!(
            RegionRestartNotice::from_text("/RESTART_X_SECONDS 45"),
            Some(RegionRestartNotice::Scheduled {
                seconds: 45,
                region_name: None,
            })
        );
        assert_eq!(
            RegionRestartNotice::from_text("/Region restart cancelled."),
            Some(RegionRestartNotice::Cancelled)
        );
    }

    /// Anything else is no notice, including a restart key with no duration.
    #[test]
    fn other_alerts_are_no_notice() {
        for text in [
            "",
            "/estate census message",
            "Region will restart in a while",
            "Region will restart in 3 fortnights",
        ] {
            assert_eq!(RegionRestartNotice::from_text(text), None, "{text:?}");
        }
        let empty = AlertInfo {
            message: "RegionRestartMinutes".to_owned(),
            extra_params: String::new(),
        };
        assert_eq!(RegionRestartNotice::from_alert("", &[empty]), None);
    }
}
