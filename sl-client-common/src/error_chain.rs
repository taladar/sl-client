//! An error with every cause it carries, as one line.
//!
//! An HTTP client's error says what step failed ("error sending request for
//! url …") and keeps *why* in its [`source`](std::error::Error::source) — the
//! connection closed before the answer, a reset, a refused connect. Its
//! `Display` is only the first, so a reason built from it alone hides the one
//! part that tells two failures apart.

use std::error::Error;

/// `error` and each of its causes, outermost first, joined by `": "`.
#[must_use]
pub fn error_chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use pretty_assertions::assert_eq;

    use super::error_chain;

    /// A test error with an optional cause.
    #[derive(Debug)]
    struct Layer {
        /// What it says.
        text: &'static str,
        /// What caused it.
        cause: Option<Box<Self>>,
    }

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.text)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            let cause: &(dyn std::error::Error + 'static) = self.cause.as_deref()?;
            Some(cause)
        }
    }

    /// Every cause is kept, outermost first; an error without one is itself.
    #[test]
    fn every_cause_is_kept_in_order() {
        let error = Layer {
            text: "error sending request",
            cause: Some(Box::new(Layer {
                text: "client error (SendRequest)",
                cause: Some(Box::new(Layer {
                    text: "connection closed before message completed",
                    cause: None,
                })),
            })),
        };
        assert_eq!(
            error_chain(&error),
            "error sending request: client error (SendRequest): connection closed before message \
             completed"
        );
        let alone = Layer {
            text: "alone",
            cause: None,
        };
        assert_eq!(error_chain(&alone), "alone");
    }
}
