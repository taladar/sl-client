//! Per-avatar directories for a session's disk features, resolved at login.
//!
//! A host knows the grid and the avatar's name before it logs in, but not the
//! agent's UUID, and the UUID is what tells a paid name change from a new
//! avatar ([`sl_account_dirs::reconcile_account_dir`]). So a host hands the
//! runtime an [`AccountDirsConfig`] up front, and the runtime resolves it with
//! [`resolve_account_directories`] the moment the login response names the
//! agent, before the chat log or the inventory cache opens a file.

use std::path::PathBuf;

use sl_proto::{AgentKey, ClientDirectories};

/// Per-avatar directory derivation for the disk features, keyed by grid +
/// avatar name with UUID-based rename discovery (see [`sl_account_dirs`]).
///
/// When a runtime is given one, it resolves the avatar's directory at login
/// (once the login response yields the agent UUID, before any disk feature is
/// touched) and points the chat-log and inventory-cache directories at
/// `<base>/<grid>/<name>/` under each feature's own accounts root, overriding
/// any explicit [`ClientDirectories::agent_chat_log_dir`] /
/// [`ClientDirectories::agent_cache_dir`]. The two roots are kept separate so
/// each feature honours its XDG category (chat logs under a state root, the
/// regenerable inventory cache under a cache root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDirsConfig {
    /// The grid segment (from [`sl_account_dirs::grid_dir_name`]).
    pub grid: String,
    /// The readable avatar segment (from [`sl_account_dirs::avatar_dir_name`]).
    pub avatar: String,
    /// The accounts root the per-avatar chat-log directory lives under (e.g. an
    /// XDG state dir's `accounts` subdirectory), or `None` to leave chat logging
    /// to the fixed [`ClientDirectories::agent_chat_log_dir`].
    pub chat_log_base: Option<PathBuf>,
    /// The accounts root the per-avatar inventory-cache directory lives under
    /// (e.g. an XDG cache dir's `accounts` subdirectory), or `None` to leave the
    /// cache to the fixed [`ClientDirectories::agent_cache_dir`].
    pub inventory_cache_base: Option<PathBuf>,
}

/// Resolves the effective per-account directories at login.
///
/// With no [`AccountDirsConfig`] (or no agent id yet), the fixed `base`
/// directories are used unchanged. Otherwise the avatar's
/// `<accounts_base>/<grid>/<name>/` directory is reconciled under each
/// configured root (creating it, or renaming it if the agent UUID shows a name
/// change) and the chat-log / inventory-cache directories point at it; a root
/// left `None` leaves that feature's fixed directory alone. A reconcile failure
/// logs and falls back to the root itself, so a disk-permission problem never
/// blocks login and never silently disables the feature. The shared cache
/// directory is not per-account and always comes from `base`.
#[must_use]
pub fn resolve_account_directories(
    base: &ClientDirectories,
    account_dirs: Option<&AccountDirsConfig>,
    agent_id: Option<AgentKey>,
) -> ClientDirectories {
    let (Some(account), Some(agent)) = (account_dirs, agent_id) else {
        return base.clone();
    };
    let uuid = agent.uuid();
    let resolve = |accounts_base: Option<&PathBuf>, fixed: &Option<PathBuf>| -> Option<PathBuf> {
        let Some(accounts_base) = accounts_base else {
            return fixed.clone();
        };
        match sl_account_dirs::reconcile_account_dir(
            accounts_base,
            &account.grid,
            &account.avatar,
            uuid,
        ) {
            Ok(dir) => {
                if let Some(collision) = dir.outcome.collision_warning(&account.avatar) {
                    tracing::warn!("under {}: {collision}", accounts_base.display());
                }
                Some(dir.path)
            }
            Err(error) => {
                tracing::warn!(
                    "could not resolve account directory under {}: {error}",
                    accounts_base.display()
                );
                Some(accounts_base.clone())
            }
        }
    };
    ClientDirectories {
        agent_chat_log_dir: resolve(account.chat_log_base.as_ref(), &base.agent_chat_log_dir),
        agent_cache_dir: resolve(account.inventory_cache_base.as_ref(), &base.agent_cache_dir),
        shared_cache_dir: base.shared_cache_dir.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;
    use sl_proto::{AgentKey, ClientDirectories, Uuid};

    use super::{AccountDirsConfig, resolve_account_directories};

    /// A fresh scratch directory for one test, emptied first.
    fn temp_dir(tag: &str) -> PathBuf {
        let crate_name = env!("CARGO_PKG_NAME");
        let dir = std::env::temp_dir().join(format!("{crate_name}-account-dirs-test-{tag}"));
        drop(fs_err::remove_dir_all(&dir));
        dir
    }

    /// The fixed directories a host would configure without per-avatar ones.
    fn fixed(root: &Path) -> ClientDirectories {
        ClientDirectories {
            agent_chat_log_dir: Some(root.join("fixed-chat")),
            agent_cache_dir: Some(root.join("fixed-cache")),
            shared_cache_dir: Some(root.join("shared")),
        }
    }

    /// An agent id for the tests.
    fn agent() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0x1234))
    }

    #[test]
    fn no_config_or_no_agent_keeps_the_fixed_directories() {
        let root = temp_dir("unconfigured");
        let base = fixed(&root);
        assert_eq!(
            resolve_account_directories(&base, None, Some(agent())),
            base
        );
        let config = AccountDirsConfig {
            grid: "grid".to_owned(),
            avatar: "First Last".to_owned(),
            chat_log_base: Some(root.join("state")),
            inventory_cache_base: Some(root.join("cache")),
        };
        assert_eq!(
            resolve_account_directories(&base, Some(&config), None),
            base
        );
    }

    #[test]
    fn each_configured_root_gets_the_avatar_directory() {
        let root = temp_dir("both-roots");
        let base = fixed(&root);
        let config = AccountDirsConfig {
            grid: "grid".to_owned(),
            avatar: "First Last".to_owned(),
            chat_log_base: Some(root.join("state")),
            inventory_cache_base: Some(root.join("cache")),
        };
        let resolved = resolve_account_directories(&base, Some(&config), Some(agent()));
        assert_eq!(
            resolved,
            ClientDirectories {
                agent_chat_log_dir: Some(root.join("state").join("grid").join("First Last")),
                agent_cache_dir: Some(root.join("cache").join("grid").join("First Last")),
                shared_cache_dir: base.shared_cache_dir.clone(),
            }
        );
        assert!(root.join("state/grid/First Last").is_dir());
        assert!(root.join("cache/grid/First Last").is_dir());
    }

    #[test]
    fn an_unconfigured_root_keeps_its_fixed_directory() {
        let root = temp_dir("one-root");
        let base = fixed(&root);
        let config = AccountDirsConfig {
            grid: "grid".to_owned(),
            avatar: "First Last".to_owned(),
            chat_log_base: Some(root.join("state")),
            inventory_cache_base: None,
        };
        let resolved = resolve_account_directories(&base, Some(&config), Some(agent()));
        assert_eq!(
            resolved.agent_chat_log_dir,
            Some(root.join("state").join("grid").join("First Last"))
        );
        assert_eq!(resolved.agent_cache_dir, base.agent_cache_dir);
    }
}
