//! The transport-neutral answer every non-CAPS endpoint returns, converted
//! to a hyper response by the HTTP service.

use bytes::Bytes;

/// A finished HTTP answer: status, content type, extra headers, body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpAnswer {
    /// The HTTP status code.
    pub(crate) status: u16,
    /// The `Content-Type` (empty = none).
    pub(crate) content_type: &'static str,
    /// Additional response headers (lower-case names).
    pub(crate) headers: Vec<(&'static str, String)>,
    /// The body.
    pub(crate) body: Bytes,
}

impl HttpAnswer {
    /// An empty answer with the given status.
    pub(crate) const fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "",
            headers: Vec::new(),
            body: Bytes::new(),
        }
    }

    /// A 200 answer with a typed body.
    pub(crate) fn ok(content_type: &'static str, body: impl Into<Bytes>) -> Self {
        Self {
            status: 200,
            content_type,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// A typed answer with an explicit status.
    pub(crate) fn with_status(
        status: u16,
        content_type: &'static str,
        body: impl Into<Bytes>,
    ) -> Self {
        Self {
            status,
            content_type,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Adds a header.
    #[must_use]
    pub(crate) fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}
