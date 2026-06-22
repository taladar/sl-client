//! The resolution context a registry build function consults to turn
//! `$placeholder` tokens into literal argument values, and its reverse — the
//! symbolizer that rewrites literals back into placeholders for clean cross-run
//! output diffs.
//!
//! [`ReplContext`] is the interface; [`NoContext`] resolves nothing (useful for
//! fully-literal lines and tests). [`SessionContext`] is the session-aware
//! implementation: it tracks the live identity/region/parcel/object bindings
//! (fed login-time facts by the runtime and updated from the [`Event`] stream
//! via [`SessionContext::apply_event`]) and the user variables set with the
//! `set`/`unset` meta commands, resolves `$self`, `$session`, `$circuit`,
//! `$region`, `$parcel`, `$lastobj`, `$cap:Name`, and `$var` at dispatch time,
//! and logs an `info!` binding line whenever a tracked value changes.

use std::collections::BTreeMap;

use sl_proto::{CircuitCode, CircuitId, Event, RegionHandle, Uuid};

/// Resolves the `$placeholder` argument tokens a REPL line may use, and the
/// reverse mapping from a literal back to the placeholder that stands for it.
///
/// A token of the form `$name` is handed to [`ReplContext::resolve_placeholder`]
/// with the text after the `$` (for example `self`, `session`, `cap:GetTexture`,
/// or a user variable). Returning `None` makes the argument fail to parse with
/// [`ReplError::Unresolved`](crate::ReplError::Unresolved).
///
/// [`ReplContext::symbolize`] is the inverse used by the formatters: given a
/// literal (a UUID, a region handle, a capability URL, …) it returns the
/// `$placeholder` that currently stands for it, so two runs against the same
/// grid produce diffable output even though the underlying ids differ.
#[expect(
    clippy::module_name_repetitions,
    reason = "`ReplContext` reads best as the crate's public trait name"
)]
pub trait ReplContext {
    /// Resolve a placeholder name (the text after the leading `$`) to the
    /// literal string it stands for, or `None` if it is unknown.
    fn resolve_placeholder(&self, name: &str) -> Option<String>;

    /// The inverse of [`resolve_placeholder`](ReplContext::resolve_placeholder):
    /// given a literal value, return the `$placeholder` token (with its leading
    /// `$`) that currently stands for it, or `None` if no binding matches.
    ///
    /// The default implementation symbolizes nothing.
    fn symbolize(&self, _literal: &str) -> Option<String> {
        None
    }

    /// The current root circuit's [`CircuitId`], if one is established, used to
    /// scope a freshly typed region-local object/parcel id into the
    /// [`ScopedObjectId`](sl_proto::ScopedObjectId) /
    /// [`ScopedParcelId`](sl_proto::ScopedParcelId) the object/parcel commands
    /// now take. `None` (the default) when no circuit is known yet.
    fn current_circuit_id(&self) -> Option<CircuitId> {
        None
    }
}

/// A [`ReplContext`] that resolves no placeholders.
///
/// Useful for parsing fully-literal lines (every argument spelled out) and for
/// unit tests that do not need session state.
#[expect(
    clippy::module_name_repetitions,
    reason = "`NoContext` names the empty `ReplContext` clearly"
)]
#[derive(Debug, Clone, Copy, Default)]
pub struct NoContext;

impl ReplContext for NoContext {
    fn resolve_placeholder(&self, _name: &str) -> Option<String> {
        None
    }
}

/// The session-aware [`ReplContext`]: the live bindings a REPL resolves its
/// `$placeholder` tokens against, and symbolizes its output with.
///
/// The runtime seeds the login-time facts ([`set_identity`](SessionContext::set_identity),
/// [`set_region`](SessionContext::set_region), [`set_caps`](SessionContext::set_caps)),
/// feeds every surfaced [`Event`] through [`apply_event`](SessionContext::apply_event)
/// to keep the region/parcel/object bindings current, and mirrors the
/// `set`/`unset` meta commands into the user variables
/// ([`set_var`](SessionContext::set_var) / [`unset_var`](SessionContext::unset_var)).
#[expect(
    clippy::module_name_repetitions,
    reason = "`SessionContext` names the session-aware `ReplContext` clearly"
)]
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    /// The agent's own id (`$self`), once login has completed.
    agent_id: Option<Uuid>,
    /// The session id (`$session`), once login has completed.
    session_id: Option<Uuid>,
    /// The circuit code (`$circuit`), once login has completed.
    circuit_code: Option<CircuitCode>,
    /// The current root circuit's instance id (`$circuitid`), tracked from the
    /// [`Event::CircuitEstablished`] / [`Event::RegionChanged`] stream. Used to
    /// scope freshly typed region-local object/parcel ids into the scoped form
    /// the commands take.
    circuit_id: Option<CircuitId>,
    /// The current region's handle (`$region`).
    region_handle: Option<RegionHandle>,
    /// The current region's name (tracked for symbolizing, not a placeholder).
    region_name: Option<String>,
    /// The region-local id of the most recently seen parcel (`$parcel`).
    parcel_local_id: Option<i32>,
    /// The persistent id of the most recently seen object (`$lastobj`).
    last_object: Option<Uuid>,
    /// The capability name → URL map (`$cap:Name`).
    caps: BTreeMap<String, String>,
    /// The user variables set with the `set` meta command (`$var`).
    vars: BTreeMap<String, String>,
}

/// Update an `Option<T>` binding, logging an `info!` binding line when the value
/// actually changes (so a replayed script's bindings are visible in the log).
fn bind<T>(slot: &mut Option<T>, value: T, name: &str)
where
    T: std::fmt::Display + PartialEq,
{
    if slot.as_ref() != Some(&value) {
        tracing::info!("binding ${name} = {value}");
        *slot = Some(value);
    }
}

impl SessionContext {
    /// A fresh context with no bindings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the login-time identity facts: the agent id (`$self`), session id
    /// (`$session`), and circuit code (`$circuit`).
    pub fn set_identity(&mut self, agent_id: Uuid, session_id: Uuid, circuit_code: CircuitCode) {
        bind(&mut self.agent_id, agent_id, "self");
        bind(&mut self.session_id, session_id, "session");
        bind(&mut self.circuit_code, circuit_code, "circuit");
    }

    /// Set the current region's handle (`$region`) and name. Called for the
    /// login region and refreshed from the event stream on a region change.
    pub fn set_region(&mut self, handle: RegionHandle, name: &str) {
        bind(&mut self.region_handle, handle, "region");
        if self.region_name.as_deref() != Some(name) {
            self.region_name = Some(name.to_owned());
        }
    }

    /// Bind a single capability name to its URL (`$cap:Name`).
    pub fn set_cap(&mut self, name: &str, url: &str) {
        if self.caps.get(name).map(String::as_str) != Some(url) {
            tracing::info!("binding $cap:{name} = {url}");
            let _previous = self.caps.insert(name.to_owned(), url.to_owned());
        }
    }

    /// Replace the whole capability map with `caps` (the seed-capability reply).
    pub fn set_caps(&mut self, caps: BTreeMap<String, String>) {
        for (name, url) in &caps {
            self.set_cap(name, url);
        }
    }

    /// Bind a user variable `name` to `value` (the `set` meta command).
    pub fn set_var(&mut self, name: &str, value: &str) {
        if self.vars.get(name).map(String::as_str) != Some(value) {
            tracing::info!("binding ${name} = {value}");
            let _previous = self.vars.insert(name.to_owned(), value.to_owned());
        }
    }

    /// Remove a user variable (the `unset` meta command). Returns whether a
    /// binding was present.
    pub fn unset_var(&mut self, name: &str) -> bool {
        self.vars.remove(name).is_some()
    }

    /// The currently bound user variables (for the `vars` meta command).
    #[must_use]
    pub const fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }

    /// Update the region/parcel/object bindings from a surfaced [`Event`].
    ///
    /// Only the events that carry a binding-relevant id are acted on; every
    /// other event is ignored. A changed binding logs an `info!` binding line.
    pub fn apply_event(&mut self, event: &Event) {
        match event {
            Event::CircuitEstablished { circuit, .. } => {
                bind(&mut self.circuit_id, *circuit, "circuitid");
            }
            Event::RegionChanged {
                region_handle,
                circuit,
                ..
            } => {
                bind(&mut self.region_handle, *region_handle, "region");
                bind(&mut self.circuit_id, *circuit, "circuitid");
            }
            Event::RegionInfoHandshake(identity) => {
                if self.region_name.as_deref() != Some(identity.sim_name.as_str()) {
                    self.region_name = Some(identity.sim_name.clone());
                }
            }
            Event::ParcelProperties(parcel) => {
                bind(&mut self.parcel_local_id, parcel.local_id.0, "parcel");
            }
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                bind(&mut self.last_object, object.full_id, "lastobj");
            }
            Event::ObjectProperties(properties) => {
                bind(&mut self.last_object, properties.object_id, "lastobj");
            }
            _ => {}
        }
    }
}

impl ReplContext for SessionContext {
    fn resolve_placeholder(&self, name: &str) -> Option<String> {
        if let Some(cap) = name.strip_prefix("cap:") {
            return self.caps.get(cap).cloned();
        }
        match name {
            "self" => self.agent_id.map(|id| id.to_string()),
            "session" => self.session_id.map(|id| id.to_string()),
            "circuit" => self.circuit_code.map(|code| code.to_string()),
            "circuitid" => self.circuit_id.map(|id| id.get().to_string()),
            "region" => self.region_handle.map(|handle| handle.to_string()),
            "parcel" => self.parcel_local_id.map(|local| local.to_string()),
            "lastobj" => self.last_object.map(|id| id.to_string()),
            other => self.vars.get(other).cloned(),
        }
    }

    fn current_circuit_id(&self) -> Option<CircuitId> {
        self.circuit_id
    }

    fn symbolize(&self, literal: &str) -> Option<String> {
        // The UUID/handle identity bindings first (the least ambiguous), then
        // the small-integer ones, then capability URLs, then user variables.
        if self.agent_id.map(|id| id.to_string()).as_deref() == Some(literal) {
            return Some("$self".to_owned());
        }
        if self.session_id.map(|id| id.to_string()).as_deref() == Some(literal) {
            return Some("$session".to_owned());
        }
        if self.last_object.map(|id| id.to_string()).as_deref() == Some(literal) {
            return Some("$lastobj".to_owned());
        }
        if self
            .region_handle
            .map(|handle| handle.to_string())
            .as_deref()
            == Some(literal)
        {
            return Some("$region".to_owned());
        }
        if self.circuit_code.map(|code| code.to_string()).as_deref() == Some(literal) {
            return Some("$circuit".to_owned());
        }
        if self.circuit_id.map(|id| id.get().to_string()).as_deref() == Some(literal) {
            return Some("$circuitid".to_owned());
        }
        if self
            .parcel_local_id
            .map(|local| local.to_string())
            .as_deref()
            == Some(literal)
        {
            return Some("$parcel".to_owned());
        }
        for (name, url) in &self.caps {
            if url == literal {
                return Some(format!("$cap:{name}"));
            }
        }
        for (name, value) in &self.vars {
            if value == literal {
                return Some(format!("${name}"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::{CircuitCode, RegionHandle, Uuid};

    use super::{ReplContext as _, SessionContext};

    /// A UUID built from a single repeated hex nibble, for stable test ids.
    fn uuid(n: char) -> Uuid {
        let group = |len: usize| n.to_string().repeat(len);
        let text = format!(
            "{}-{}-{}-{}-{}",
            group(8),
            group(4),
            group(4),
            group(4),
            group(12)
        );
        Uuid::parse_str(&text).unwrap_or_else(|_| Uuid::nil())
    }

    #[test]
    fn resolves_identity_placeholders() {
        let mut ctx = SessionContext::new();
        ctx.set_identity(uuid('1'), uuid('2'), CircuitCode(12345));
        assert_eq!(ctx.resolve_placeholder("self"), Some(uuid('1').to_string()));
        assert_eq!(
            ctx.resolve_placeholder("session"),
            Some(uuid('2').to_string())
        );
        assert_eq!(ctx.resolve_placeholder("circuit"), Some("12345".to_owned()));
    }

    #[test]
    fn unknown_placeholder_is_none() {
        let ctx = SessionContext::new();
        assert_eq!(ctx.resolve_placeholder("self"), None);
        assert_eq!(ctx.resolve_placeholder("nope"), None);
    }

    #[test]
    fn caps_and_vars_resolve() {
        let mut ctx = SessionContext::new();
        ctx.set_cap("GetTexture", "https://sim/cap/abc");
        ctx.set_var("dest", "Da Boom");
        assert_eq!(
            ctx.resolve_placeholder("cap:GetTexture"),
            Some("https://sim/cap/abc".to_owned())
        );
        assert_eq!(ctx.resolve_placeholder("cap:Missing"), None);
        assert_eq!(ctx.resolve_placeholder("dest"), Some("Da Boom".to_owned()));
    }

    #[test]
    fn unset_var_removes_binding() {
        let mut ctx = SessionContext::new();
        ctx.set_var("dest", "Da Boom");
        assert!(ctx.unset_var("dest"));
        assert!(!ctx.unset_var("dest"));
        assert_eq!(ctx.resolve_placeholder("dest"), None);
    }

    #[test]
    fn region_binding_set_and_resolved() {
        let mut ctx = SessionContext::new();
        ctx.set_region(RegionHandle(1_099_511_628_032), "Da Boom");
        assert_eq!(
            ctx.resolve_placeholder("region"),
            Some("1099511628032".to_owned())
        );
    }

    #[test]
    fn symbolize_inverts_resolution() {
        let mut ctx = SessionContext::new();
        ctx.set_identity(uuid('1'), uuid('2'), CircuitCode(12345));
        ctx.set_region(RegionHandle(1_099_511_628_032), "Da Boom");
        ctx.set_cap("GetTexture", "https://sim/cap/abc");
        ctx.set_var("dest", "Da Boom Plaza");
        assert_eq!(
            ctx.symbolize(&uuid('1').to_string()),
            Some("$self".to_owned())
        );
        assert_eq!(
            ctx.symbolize(&uuid('2').to_string()),
            Some("$session".to_owned())
        );
        assert_eq!(ctx.symbolize("1099511628032"), Some("$region".to_owned()));
        assert_eq!(ctx.symbolize("12345"), Some("$circuit".to_owned()));
        assert_eq!(
            ctx.symbolize("https://sim/cap/abc"),
            Some("$cap:GetTexture".to_owned())
        );
        assert_eq!(ctx.symbolize("Da Boom Plaza"), Some("$dest".to_owned()));
        assert_eq!(ctx.symbolize("nothing"), None);
    }
}
