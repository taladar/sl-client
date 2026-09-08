//! Grid accounts: the credentials the login endpoint checks and the stable
//! per-account identity minted when the grid starts.

use sl_proto::Maturity;
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
    /// Whether this account may issue estate commands. `false` by default,
    /// because most avatars on most grids may not, and a check nobody ever
    /// fails is not a check — see
    /// [`AgentPolicy`](crate::agent_requests::AgentPolicy).
    pub estate_manager: bool,
    /// The highest content rating this account is entitled to
    /// (`agent_access_max`), and so the ceiling its maturity preference may not
    /// exceed.
    ///
    /// [`Maturity::Adult`] by default, which is what a viewer has always been
    /// told here — and precisely the reason this is a knob. A client's
    /// `canSetMaturity` rule reads this field, and against a grid that hands
    /// every account the maximum, **that rule has never once been exercised**.
    /// Lower it to build the account that makes it fire.
    pub maturity_ceiling: Maturity,
    /// The content rating this account has *chosen* (`agent_region_access`), or
    /// `None` to send no preference at all.
    ///
    /// Distinct from [`maturity_ceiling`](Self::maturity_ceiling): the ceiling
    /// is the entitlement, this is the setting, and a conforming grid keeps the
    /// second at or below the first. `None` is not "no preference" but "this
    /// grid does not send the field", which is every OpenSim grid — and it is
    /// the OpenSim flavour's answer whatever this says.
    pub preferred_maturity: Option<Maturity>,
    /// The subscription package this account is on (`account_type`), naming
    /// which entry of the grid's package table its benefits come from.
    ///
    /// `"Base"` by default. Ignored entirely on a grid whose flavour sends no
    /// benefits package.
    pub package: String,
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
            estate_manager: false,
            maturity_ceiling: Maturity::Adult,
            preferred_maturity: None,
            package: "Base".to_owned(),
        }
    }

    /// The same account, entitled only up to `ceiling`.
    ///
    /// The account to build when the thing under test is a *refusal*: a client
    /// may not raise its preference above this, and a grid may not let it into
    /// a region above it.
    #[must_use]
    pub const fn maturity_ceiling(mut self, ceiling: Maturity) -> Self {
        self.maturity_ceiling = ceiling;
        self
    }

    /// The same account, having chosen `preference` as its content rating.
    ///
    /// Only a Second-Life-flavoured grid sends this; the OpenSim flavour omits
    /// the field whatever is set here, because no OpenSim grid has ever sent it.
    #[must_use]
    pub const fn preferred_maturity(mut self, preference: Maturity) -> Self {
        self.preferred_maturity = Some(preference);
        self
    }

    /// The same account, on the named subscription package.
    #[must_use]
    pub fn package(mut self, package: impl Into<String>) -> Self {
        self.package = package.into();
        self
    }

    /// The same account, holding estate powers over the grid's regions.
    #[must_use]
    pub const fn estate_manager(mut self) -> Self {
        self.estate_manager = true;
        self
    }

    /// The same account, with the agent id fixed rather than minted at start.
    ///
    /// A minted id is stable within a run and (on a seeded grid) across runs,
    /// but it is not something a *caller* can name before the grid exists —
    /// which is what a harness needs when it has to tell a case "this is the
    /// other avatar" without logging that avatar in.
    #[must_use]
    pub const fn with_agent_id(mut self, agent_id: AgentKey) -> Self {
        self.agent_id = Some(agent_id);
        self
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
