//! The merchant-status probe (`GET /merchant`).
//!
//! The probe's payload is the HTTP status code itself — the reference
//! viewer (`getMerchantStatusCoro` in `llmarketplacefunctions.cpp`)
//! never parses a successful body and maps failures purely by status:
//! 404 means "not a merchant", 503 means "merchant not migrated to
//! DirectDelivery", anything else failing is a connection failure.

use serde_json::Value as JsonValue;

/// The merchant status of the agent as answered by `GET /merchant`.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::MerchantStatus`, where it reads clearly"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerchantStatus {
    /// Any 2xx reply: the agent is a marketplace merchant.
    Merchant,
    /// HTTP 404: the agent is not a marketplace merchant.
    NotMerchant,
    /// HTTP 503: the agent is a merchant whose store has not been
    /// migrated to DirectDelivery.
    NotMigratedMerchant,
    /// Any other failure (unexpected status, transport error, missing
    /// `DirectDelivery` capability).
    ConnectionFailure {
        /// The HTTP status code, when the probe produced a reply at
        /// all.
        status: Option<u16>,
        /// A human-readable reason (the service's `error_code` when
        /// the body carried one, otherwise a status description).
        reason: String,
    },
}

impl std::fmt::Display for MerchantStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Merchant => write!(f, "merchant"),
            Self::NotMerchant => write!(f, "not a merchant"),
            Self::NotMigratedMerchant => write!(f, "merchant not migrated"),
            Self::ConnectionFailure { status, reason } => match status {
                Some(status) => {
                    write!(f, "connection failure (HTTP {status}): {reason}")
                }
                None => write!(f, "connection failure: {reason}"),
            },
        }
    }
}

/// Map a `GET /merchant` reply (status code plus raw body text) to a
/// [`MerchantStatus`].
///
/// The body is only consulted on unexpected failure statuses, as a
/// best-effort source for a reason string (`error_code` /
/// `error_description` of a JSON-object body).
#[must_use]
pub fn parse_merchant_status(status: u16, body: &str) -> MerchantStatus {
    match status {
        200..=299 => MerchantStatus::Merchant,
        404 => MerchantStatus::NotMerchant,
        503 => MerchantStatus::NotMigratedMerchant,
        other => {
            let reason = serde_json::from_str::<JsonValue>(body)
                .ok()
                .as_ref()
                .and_then(|value| value.as_object())
                .and_then(|object| {
                    let code = object.get("error_code").and_then(JsonValue::as_str);
                    let description = object.get("error_description").and_then(JsonValue::as_str);
                    match (code, description) {
                        (Some(code), Some(description)) => Some(format!("{code}: {description}")),
                        (Some(code), None) => Some(code.to_owned()),
                        (None, Some(description)) => Some(description.to_owned()),
                        (None, None) => None,
                    }
                })
                .unwrap_or_else(|| format!("HTTP status {other}"));
            MerchantStatus::ConnectionFailure {
                status: Some(other),
                reason,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn success_statuses_mean_merchant() {
        assert_eq!(parse_merchant_status(200, ""), MerchantStatus::Merchant);
        assert_eq!(
            parse_merchant_status(201, "ignored"),
            MerchantStatus::Merchant
        );
    }

    #[test]
    fn not_found_means_not_a_merchant() {
        assert_eq!(parse_merchant_status(404, ""), MerchantStatus::NotMerchant);
    }

    #[test]
    fn service_unavailable_means_not_migrated() {
        assert_eq!(
            parse_merchant_status(503, ""),
            MerchantStatus::NotMigratedMerchant
        );
    }

    #[test]
    fn other_failures_extract_a_reason_from_a_json_object_body() {
        assert_eq!(
            parse_merchant_status(
                500,
                r#"{"error_code":"internal","error_description":"boom"}"#
            ),
            MerchantStatus::ConnectionFailure {
                status: Some(500),
                reason: "internal: boom".to_owned(),
            }
        );
    }

    #[test]
    fn other_failures_fall_back_to_the_status_code() {
        assert_eq!(
            parse_merchant_status(500, "<html>oops</html>"),
            MerchantStatus::ConnectionFailure {
                status: Some(500),
                reason: "HTTP status 500".to_owned(),
            }
        );
    }
}
