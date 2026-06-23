//! Avatar and group name lookups (UUID → legacy name).
//!
//! The simulator answers a `UUIDNameRequest` / `UUIDGroupNameRequest` with the
//! immutable *legacy* identity of each id. This is the lightweight, always-present
//! lookup used to turn the UUIDs that pervade the protocol (object owners, estate
//! managers, inventory creators, …) into something human-readable. SL's mutable
//! *display names* are a separate CAPS lookup and are deliberately not conflated
//! with these.

use sl_types::key::{AgentKey, GroupKey};

/// A legacy avatar name resolved from a `UUIDNameReply` — the reply to
/// [`Session::request_avatar_names`](crate::Session::request_avatar_names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarName {
    /// The agent id that was looked up.
    pub id: AgentKey,
    /// The agent's legacy first name.
    pub first_name: String,
    /// The agent's legacy last name. Modern single-name accounts use the
    /// placeholder `"Resident"`.
    pub last_name: String,
}

impl AvatarName {
    /// The display form of the legacy name: `"First Last"`, collapsing to just
    /// the first name when the last name is empty or the `"Resident"` placeholder
    /// of a modern single-name account.
    #[must_use]
    pub fn legacy_name(&self) -> String {
        if self.last_name.is_empty() || self.last_name.eq_ignore_ascii_case("Resident") {
            self.first_name.clone()
        } else {
            format!("{} {}", self.first_name, self.last_name)
        }
    }
}

/// A group name resolved from a `UUIDGroupNameReply` — the reply to
/// [`Session::request_group_names`](crate::Session::request_group_names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupName {
    /// The group id that was looked up.
    pub id: GroupKey,
    /// The group's name.
    pub name: String,
}
