//! Grid accounts: the credentials the login endpoint checks and the stable
//! per-account identity minted when the grid starts.

use sl_types::key::AgentKey;
use sl_wire::{Credential, MfaPolicy, password_hash};

/// An account a [`crate::FakeGridBuilder`] registers: who may log in.
#[derive(Debug, Clone)]
pub struct AccountConfig {
    /// The avatar's first name (login field, case-sensitive match).
    pub first_name: String,
    /// The avatar's last name.
    pub last_name: String,
    /// The plaintext password the login endpoint accepts.
    pub password: String,
    /// A fixed agent id, or `None` to mint one when the grid starts.
    pub agent_id: Option<AgentKey>,
    /// The region the avatar logs into; `None` means the grid's first region.
    pub start_region: Option<String>,
    /// The account's multi-factor policy, if logins must pass an MFA
    /// challenge (see [`sl_wire::MfaPolicy`]).
    pub mfa: Option<MfaPolicy>,
}

impl AccountConfig {
    /// A plain account with a minted agent id starting in the first region.
    #[must_use]
    pub fn new(
        first_name: impl Into<String>,
        last_name: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            first_name: first_name.into(),
            last_name: last_name.into(),
            password: password.into(),
            agent_id: None,
            start_region: None,
            mfa: None,
        }
    }
}

/// A registered account after the grid started: the config plus the minted
/// identity and hashed credential the login endpoint verifies against.
#[derive(Debug, Clone)]
pub(crate) struct Account {
    /// The builder-supplied account data.
    pub(crate) config: AccountConfig,
    /// The stable agent id for this grid instance (fixed or minted once).
    pub(crate) agent_id: AgentKey,
    /// The `$1$<md5>` credential [`sl_wire::LoginServer::respond`] checks.
    pub(crate) credential: Credential,
}

impl Account {
    /// Registers `config`, minting the agent id unless one was fixed.
    pub(crate) fn register(config: AccountConfig, minter: &crate::runtime::IdMinter) -> Self {
        let agent_id = config
            .agent_id
            .unwrap_or_else(|| AgentKey::from(minter.uuid()));
        let credential = Credential {
            password_hash: password_hash(&config.password),
            mfa: config.mfa.clone(),
        };
        Self {
            config,
            agent_id,
            credential,
        }
    }
}
