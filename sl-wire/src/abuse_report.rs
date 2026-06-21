//! The **`SendUserReport`** capability — the modern (CAPS) abuse-report path.
//!
//! A viewer's "Report Abuse" floater gathers a complaint (or bug report) and
//! delivers it to the grid. Second Life prefers the `SendUserReport` capability
//! (an LLSD POST), falling back to the legacy `UserReport` UDP message when the
//! cap is absent; OpenSim only implements the UDP path. Both carry the same
//! fields, so this module's [`AbuseReport`] is the shared payload: the UDP
//! encoder in `sl-proto` fills a `UserReport` message from it, and this module
//! builds/parses the capability's LLSD body.
//!
//! The LLSD keys are cross-checked against the Firestorm viewer's
//! `indra/newview/llfloaterreporter.cpp` (`gatherReport`). The
//! `SendUserReportWithScreenshot` variant (which first uploads a snapshot asset)
//! is out of scope; this is the no-screenshot path (a nil `screenshot_id`).

use std::collections::HashMap;

use sl_types::lsl::Vector;
use uuid::Uuid;

use crate::llsd::Llsd;

/// The kind of report carried by an [`AbuseReport`] (the `ReportType` byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AbuseReportType {
    /// A bug report (`1`).
    Bug,
    /// An abuse complaint (`2`) — the usual "Report Abuse" case.
    #[default]
    Complaint,
    /// Any other report-type byte, preserved verbatim.
    Other(u8),
}

impl AbuseReportType {
    /// Classifies a `ReportType` wire byte.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Bug,
            2 => Self::Complaint,
            other => Self::Other(other),
        }
    }

    /// The wire byte for this report type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Bug => 1,
            Self::Complaint => 2,
            Self::Other(value) => value,
        }
    }
}

/// An abuse report (or bug report) to send to the grid, shared by the legacy
/// `UserReport` UDP message and the `SendUserReport` capability.
///
/// `category` is the grid-defined report category (the viewer fills it from the
/// `GetUserReportCategories` cap / a hard-coded list); `position` is the
/// region-local position the snapshot was taken at. The `abuse_region_*` fields
/// describe the region the abuse occurred in, which need not be the reporter's
/// current region. `summary` is a one-line headline; `details` is the free-text
/// body; `version_string` is the reporting viewer's version.
#[derive(Debug, Clone, PartialEq)]
pub struct AbuseReport {
    /// Whether this is a bug report or an abuse complaint.
    pub report_type: AbuseReportType,
    /// The grid-defined report category code.
    pub category: u8,
    /// The region-local position the report's snapshot was taken at.
    pub position: Vector,
    /// Viewer "checkbox" flags (unused by the reference viewer; usually 0).
    pub check_flags: u8,
    /// The uploaded snapshot asset id, or nil for the no-screenshot path.
    pub screenshot_id: Uuid,
    /// The reported object's id, or nil when reporting an avatar/region.
    pub object_id: Uuid,
    /// The reported (abusing) avatar's id, or nil if unknown.
    pub abuser_id: Uuid,
    /// The name of the region the abuse occurred in.
    pub abuse_region_name: String,
    /// The id of the region the abuse occurred in (often nil; the grid fills it).
    pub abuse_region_id: Uuid,
    /// A one-line summary headline.
    pub summary: String,
    /// The free-text report body.
    pub details: String,
    /// The reporting viewer's version string.
    pub version_string: String,
}

impl Default for AbuseReport {
    fn default() -> Self {
        Self {
            report_type: AbuseReportType::default(),
            category: 0,
            position: Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            check_flags: 0,
            screenshot_id: Uuid::nil(),
            object_id: Uuid::nil(),
            abuser_id: Uuid::nil(),
            abuse_region_name: String::new(),
            abuse_region_id: Uuid::nil(),
            summary: String::new(),
            details: String::new(),
            version_string: String::new(),
        }
    }
}

/// Builds the LLSD body for a `SendUserReport` capability POST — the
/// capability-side equivalent of the `UserReport` UDP message. The keys mirror
/// the Firestorm viewer's `gatherReport`; `position` is an array of three reals.
#[must_use]
pub fn build_send_user_report(report: &AbuseReport) -> String {
    abuse_report_to_llsd(report).to_llsd_xml()
}

/// Decodes a `SendUserReport` capability body back into an [`AbuseReport`]
/// (server side) — the inverse of [`build_send_user_report`]. Absent keys
/// default, so a partial body still decodes.
#[must_use]
pub fn parse_send_user_report(body: &Llsd) -> AbuseReport {
    let read_uuid = |key: &str| {
        body.get(key)
            .and_then(Llsd::as_uuid)
            .unwrap_or_else(Uuid::nil)
    };
    let read_str = |key: &str| {
        body.get(key)
            .and_then(Llsd::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let read_u8 = |key: &str| {
        body.get(key)
            .and_then(Llsd::as_i32)
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(0)
    };
    AbuseReport {
        report_type: AbuseReportType::from_u8(read_u8("report-type")),
        category: read_u8("category"),
        position: vector_from_llsd(body.get("position")),
        check_flags: read_u8("check-flags"),
        screenshot_id: read_uuid("screenshot-id"),
        object_id: read_uuid("object-id"),
        abuser_id: read_uuid("abuser-id"),
        abuse_region_name: read_str("abuse-region-name"),
        abuse_region_id: read_uuid("abuse-region-id"),
        summary: read_str("summary"),
        details: read_str("details"),
        version_string: read_str("version-string"),
    }
}

/// Serialises an [`AbuseReport`] to the `SendUserReport` LLSD map.
fn abuse_report_to_llsd(report: &AbuseReport) -> Llsd {
    Llsd::Map(HashMap::from([
        (
            "report-type".to_owned(),
            Llsd::Integer(i32::from(report.report_type.to_u8())),
        ),
        (
            "category".to_owned(),
            Llsd::Integer(i32::from(report.category)),
        ),
        ("position".to_owned(), vector_to_llsd(&report.position)),
        (
            "check-flags".to_owned(),
            Llsd::Integer(i32::from(report.check_flags)),
        ),
        ("screenshot-id".to_owned(), Llsd::Uuid(report.screenshot_id)),
        ("object-id".to_owned(), Llsd::Uuid(report.object_id)),
        ("abuser-id".to_owned(), Llsd::Uuid(report.abuser_id)),
        (
            "abuse-region-name".to_owned(),
            Llsd::String(report.abuse_region_name.clone()),
        ),
        (
            "abuse-region-id".to_owned(),
            Llsd::Uuid(report.abuse_region_id),
        ),
        ("summary".to_owned(), Llsd::String(report.summary.clone())),
        (
            "version-string".to_owned(),
            Llsd::String(report.version_string.clone()),
        ),
        ("details".to_owned(), Llsd::String(report.details.clone())),
    ]))
}

/// Encodes a [`Vector`] as an LLSD array of three reals (the report `position`).
fn vector_to_llsd(value: &Vector) -> Llsd {
    Llsd::Array(vec![
        Llsd::Real(f64::from(value.x)),
        Llsd::Real(f64::from(value.y)),
        Llsd::Real(f64::from(value.z)),
    ])
}

/// Decodes an LLSD array of three reals back into a [`Vector`] (missing or
/// short arrays yield zeros).
fn vector_from_llsd(value: Option<&Llsd>) -> Vector {
    let component = |index: usize| {
        value
            .and_then(|llsd| llsd.index(index))
            .and_then(Llsd::as_f32)
            .unwrap_or(0.0)
    };
    Vector {
        x: component(0),
        y: component(1),
        z: component(2),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::Vector;
    use uuid::Uuid;

    use super::{AbuseReport, AbuseReportType, build_send_user_report, parse_send_user_report};
    use crate::llsd::parse_llsd_xml;

    /// An abuse report round-trips through the capability body builder and the
    /// server-side parser.
    #[test]
    fn abuse_report_round_trips() -> Result<(), String> {
        let report = AbuseReport {
            report_type: AbuseReportType::Complaint,
            category: 66,
            position: Vector {
                x: 128.0,
                y: 64.5,
                z: 22.0,
            },
            check_flags: 0,
            screenshot_id: Uuid::from_u128(0x11),
            object_id: Uuid::from_u128(0x22),
            abuser_id: Uuid::from_u128(0x33),
            abuse_region_name: "Test Region".to_owned(),
            abuse_region_id: Uuid::from_u128(0x44),
            summary: "Griefing".to_owned(),
            details: "Detailed account of the abuse.".to_owned(),
            version_string: "7.1.0 Lnx".to_owned(),
        };
        let xml = build_send_user_report(&report);
        let parsed =
            parse_send_user_report(&parse_llsd_xml(&xml).map_err(|error| format!("{error:?}"))?);
        assert_eq!(parsed, report);
        Ok(())
    }

    /// The report-type byte classifies and round-trips.
    #[test]
    fn report_type_round_trips() {
        assert_eq!(AbuseReportType::from_u8(1), AbuseReportType::Bug);
        assert_eq!(AbuseReportType::from_u8(2), AbuseReportType::Complaint);
        assert_eq!(AbuseReportType::from_u8(9), AbuseReportType::Other(9));
        assert_eq!(AbuseReportType::Complaint.to_u8(), 2);
        assert_eq!(AbuseReportType::Other(9).to_u8(), 9);
    }
}
