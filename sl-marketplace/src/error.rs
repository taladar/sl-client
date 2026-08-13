//! Typed errors for the SLM DirectDelivery API.
//!
//! The reference viewer (`log_SLM_warning` in
//! `llmarketplacefunctions.cpp`) distinguishes three wire error
//! shapes: a JSON object carrying `error_code` / `error_description`,
//! a JSON array (or scalar) of message strings — with HTTP 422 plus an
//! array of more than four messages special-cased as the
//! "partially-filled listing cannot be listed" condition — and 5xx
//! replies whose bodies are deliberately not parsed.

use serde_json::Value as JsonValue;

/// Failure to build an SLM request body.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::BuildRequestError`, where it reads clearly"
)]
#[derive(Debug, thiserror::Error)]
pub enum BuildRequestError {
    /// The request payload could not be serialized to JSON.
    #[error("failed to serialize SLM request body: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// An error reply from (or on the way to) the SLM service.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::ApiError`, where it reads clearly"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// The HTTP status code, when the request produced a reply at all.
    pub status: Option<u16>,
    /// What kind of error the reply (or its absence) encodes.
    pub kind: ApiErrorKind,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(f, "SLM error (HTTP {status}): {}", self.kind),
            None => write!(f, "SLM error: {}", self.kind),
        }
    }
}

/// The wire shapes an SLM error can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiErrorKind {
    /// HTTP 422 with more than four message strings: the listing is
    /// incomplete (e.g. no version folder / empty stock) and cannot be
    /// listed.
    UnprocessableListing {
        /// The per-problem message strings from the reply body.
        messages: Vec<String>,
    },
    /// A structured 4xx error reply.
    Api {
        /// The service's `error_code` field, when the body was a JSON
        /// object carrying one.
        error_code: Option<String>,
        /// The service's `error_description` field, when the body was
        /// a JSON object carrying one.
        error_description: Option<String>,
        /// Message strings when the body was a JSON array or scalar
        /// (or unparsable, in which case the raw body text).
        messages: Vec<String>,
    },
    /// A 5xx reply; the body is deliberately not parsed (reference
    /// viewer behaviour).
    Server,
    /// The request never produced an HTTP reply (connection failure,
    /// missing `DirectDelivery` capability, ...).
    Transport(String),
    /// A 2xx reply whose body could not be decoded as the expected
    /// JSON shape.
    Decode(String),
}

impl std::fmt::Display for ApiErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnprocessableListing { messages } => {
                write!(f, "unprocessable listing: {}", messages.join("; "))
            }
            Self::Api {
                error_code,
                error_description,
                messages,
            } => {
                let code = error_code.as_deref().unwrap_or("-");
                let description = error_description.as_deref().unwrap_or("-");
                write!(
                    f,
                    "api error (code {code}, description {description}): {}",
                    messages.join("; ")
                )
            }
            Self::Server => write!(f, "server error"),
            Self::Transport(reason) => write!(f, "transport failure: {reason}"),
            Self::Decode(reason) => write!(f, "undecodable success reply: {reason}"),
        }
    }
}

/// Turn a JSON value from an error body into a list of message
/// strings (array elements or a single scalar; non-strings are
/// rendered as compact JSON).
fn messages_from_json(value: &JsonValue) -> Vec<String> {
    /// Render one JSON element as a message string.
    fn message(value: &JsonValue) -> String {
        match value {
            JsonValue::String(s) => s.clone(),
            other => other.to_string(),
        }
    }
    match value {
        JsonValue::Array(elements) => elements.iter().map(message).collect(),
        other => vec![message(other)],
    }
}

/// Map a non-2xx SLM reply (status code plus raw body text) to a
/// typed [`ApiError`], mirroring the reference viewer's
/// `log_SLM_warning` handling.
#[must_use]
pub fn parse_error_body(status: u16, body: &str) -> ApiError {
    if (500..=599).contains(&status) {
        return ApiError {
            status: Some(status),
            kind: ApiErrorKind::Server,
        };
    }
    let kind = match serde_json::from_str::<JsonValue>(body) {
        Ok(JsonValue::Object(object)) => ApiErrorKind::Api {
            error_code: object
                .get("error_code")
                .map(|v| messages_from_json(v).join("; ")),
            error_description: object
                .get("error_description")
                .map(|v| messages_from_json(v).join("; ")),
            messages: Vec::new(),
        },
        Ok(value) => {
            let messages = messages_from_json(&value);
            if status == 422 && messages.len() > 4 {
                ApiErrorKind::UnprocessableListing { messages }
            } else {
                ApiErrorKind::Api {
                    error_code: None,
                    error_description: None,
                    messages,
                }
            }
        }
        Err(_) => ApiErrorKind::Api {
            error_code: None,
            error_description: None,
            messages: if body.trim().is_empty() {
                Vec::new()
            } else {
                vec![body.trim().to_owned()]
            },
        },
    };
    ApiError {
        status: Some(status),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn object_body_yields_code_and_description() {
        let error = parse_error_body(
            400,
            r#"{"error_code":"bad_request","error_description":"nope"}"#,
        );
        assert_eq!(error.status, Some(400));
        assert_eq!(
            error.kind,
            ApiErrorKind::Api {
                error_code: Some("bad_request".to_owned()),
                error_description: Some("nope".to_owned()),
                messages: Vec::new(),
            }
        );
    }

    #[test]
    fn unprocessable_listing_needs_422_and_more_than_four_messages() {
        let five = r#"["a","b","c","d","e"]"#;
        assert_eq!(
            parse_error_body(422, five).kind,
            ApiErrorKind::UnprocessableListing {
                messages: vec![
                    "a".to_owned(),
                    "b".to_owned(),
                    "c".to_owned(),
                    "d".to_owned(),
                    "e".to_owned(),
                ]
            }
        );
        let three = r#"["a","b","c"]"#;
        assert_eq!(
            parse_error_body(422, three).kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            }
        );
        assert_eq!(
            parse_error_body(400, five).kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: vec![
                    "a".to_owned(),
                    "b".to_owned(),
                    "c".to_owned(),
                    "d".to_owned(),
                    "e".to_owned(),
                ]
            }
        );
    }

    #[test]
    fn scalar_body_becomes_single_message() {
        assert_eq!(
            parse_error_body(404, r#""listing not found""#).kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: vec!["listing not found".to_owned()],
            }
        );
    }

    #[test]
    fn server_errors_leave_the_body_unparsed() {
        let error = parse_error_body(503, r#"{"error_code":"ignored"}"#);
        assert_eq!(error.status, Some(503));
        assert_eq!(error.kind, ApiErrorKind::Server);
    }

    #[test]
    fn unparsable_body_is_kept_as_raw_text() {
        assert_eq!(
            parse_error_body(400, "<html>oops</html>").kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: vec!["<html>oops</html>".to_owned()],
            }
        );
        assert_eq!(
            parse_error_body(400, "  ").kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: Vec::new(),
            }
        );
    }
}
