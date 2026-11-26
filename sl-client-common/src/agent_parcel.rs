//! The agent's current parcel, fly permission and seat, as one value a runtime
//! hands its consumer whenever any of them changes.
//!
//! The resolution itself lives in `sl-proto`
//! ([`Session::current_parcel`], [`Session::can_fly`], [`Session::seat`]); this
//! is the snapshot both runtimes mirror out of the session they drive — the
//! Bevy runtime as its `SlAgentParcel` resource, the tokio runtime over the
//! channel set with `Client::set_agent_parcel_reporter`.

use sl_proto::{ObjectKey, ParcelInfo, Session};

/// The agent's current parcel and derived fly permission and seat.
///
/// [`current`](Self::current) is `None` until the agent's parcel resolves (the
/// simulator pushes it on region entry and parcel crossing);
/// [`can_fly`](Self::can_fly) combines the region-wide fly block and the
/// parcel's `ALLOW_FLY`, and is permissive while the parcel is unknown (so a
/// take-off is not blocked before the push arrives). The default, before any
/// session has been read, says `false`.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct AgentParcel {
    /// The parcel the agent currently stands on, or `None` before it resolves.
    pub current: Option<ParcelInfo>,
    /// Whether the agent may start flying where it currently stands.
    pub can_fly: bool,
    /// The object the agent is currently seated on, or `None` if it is not
    /// seated on an object (standing, ground-sitting, or a sit request still
    /// pending).
    ///
    /// Mirrored from [`Session::seat`], which **keeps the seat across a plain
    /// region crossing** (a vehicle carries the agent over the border), so a
    /// consumer can trust this signal through a crossing without debouncing: it
    /// only clears on a real stand or teleport.
    pub seated_on: Option<ObjectKey>,
}

impl AgentParcel {
    /// Refreshes the snapshot from the driven session and says whether anything
    /// changed, so a runtime forwards it only then. The fly permission and seat
    /// are read every call (cheap), while the current parcel is re-cloned only
    /// when it actually differs (a structural compare avoids cloning its
    /// half-kilobyte bitmap and strings on every unchanged tick).
    pub fn refresh_from(&mut self, session: &Session) -> bool {
        let mut changed = false;
        let can_fly = session.can_fly();
        if self.can_fly != can_fly {
            self.can_fly = can_fly;
            changed = true;
        }
        let seated_on = session.seat();
        if self.seated_on != seated_on {
            self.seated_on = seated_on;
            changed = true;
        }
        let current = session.current_parcel();
        if self.current.as_ref() != current {
            self.current = current.cloned();
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::{LoginParams, LoginRequest, Session, StartLocation};

    use super::AgentParcel;

    /// A session that has not logged in.
    fn session() -> Result<Session, url::ParseError> {
        let request = LoginRequest::new(
            "Test",
            "User",
            "",
            StartLocation::Last,
            "sl-client-agent-parcel-test",
            "0",
        );
        let login_uri = url::Url::parse("http://127.0.0.1:9000/")?;
        Ok(Session::new(LoginParams { login_uri, request }))
    }

    #[test]
    fn a_session_before_login_permits_flying_and_reports_that_once() -> Result<(), url::ParseError>
    {
        let session = session()?;
        let mut parcel = AgentParcel::default();
        assert!(
            parcel.refresh_from(&session),
            "the default does not permit flying"
        );
        assert_eq!(
            parcel,
            AgentParcel {
                current: None,
                can_fly: true,
                seated_on: None,
            }
        );
        assert!(!parcel.refresh_from(&session), "nothing changed since");
        Ok(())
    }
}
