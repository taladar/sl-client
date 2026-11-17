//! Keyed **secrets** for a viewer: a saved login password today, and anything
//! else that must never land in a plaintext config (a photo host's token, a
//! translation API key) as it comes.
//!
//! A secret is kept under a [`SecretKey`]: the [`SecretKind`] names the
//! keychain service, and the account within it says whose — for a login
//! password the grid and the avatar, named as `sl-account-dirs` names their
//! directories, so each avatar on each grid remembers on its own.
//!
//! # Two stores, and no third
//!
//! - [`KeychainStore`] is the OS keychain, through the `keyring` crate: the
//!   Secret Service on Linux and the BSDs, Keychain on macOS, the Credential
//!   Manager on Windows.
//! - [`MemoryStore`] keeps secrets in this process alone. It is what a test
//!   hands a viewer, so the features that keep secrets are tested on machines
//!   with no keychain, and it never touches the developer's own.
//!
//! **There is no file fallback.** Where no keychain answers (a headless box, a
//! locked or absent Secret Service), [`KeychainStore`] says so with
//! [`SecretError::Unavailable`], and the caller tells the user it cannot
//! remember rather than pretending to. A file encrypted with a key kept on the
//! same machine is obfuscation, not protection, and a plaintext one is worse.
//!
//! # Every call may block
//!
//! A keychain call is a D-Bus round trip on Linux, and may wait on an unlock
//! prompt, so none of [`SecretStore`]'s methods belong on a frame thread: a
//! caller runs them on a worker of its own.
//!
//! Reference (Firestorm, read-only): `llsecapi.h`, `llsechandler_basic.cpp`
//! (the protected credential store, and how it keys a saved password).

use std::collections::BTreeMap;
use std::sync::Mutex;

/// What a secret is for, which names the keychain service it is kept under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecretKind {
    /// A login password, kept by the login screen's "Remember password".
    LoginPassword,
}

impl SecretKind {
    /// The keychain service secrets of this kind are kept under: what a
    /// keychain manager shows beside the account.
    #[must_use]
    pub const fn service(self) -> &'static str {
        match self {
            Self::LoginPassword => "sl-client login password",
        }
    }
}

/// Where one secret is kept: its kind, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretKey {
    /// What the secret is for.
    kind: SecretKind,
    /// Whose it is, within the kind's service.
    account: String,
}

impl SecretKey {
    /// The key of `avatar`'s login password on `grid`, both named as
    /// `sl-account-dirs` names their directories (`grid_dir_name`,
    /// `avatar_dir_name`): `127.0.0.1:9000` and `Avatar Tester`.
    #[must_use]
    pub fn login_password(grid: &str, avatar: &str) -> Self {
        Self {
            kind: SecretKind::LoginPassword,
            account: format!("{avatar} @ {grid}"),
        }
    }

    /// What the secret is for.
    #[must_use]
    pub const fn kind(&self) -> SecretKind {
        self.kind
    }

    /// Whose the secret is, as the keychain shows it.
    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }
}

/// A secret's text. Its `Debug` never shows it; [`expose`](Self::expose) is
/// the one way to read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// A secret holding `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The secret's text.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Why a store could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// No keychain can keep a secret here: none is running, it refused
    /// access, or it is locked. Nothing is remembered, and the user is told
    /// why.
    #[error("no keychain can keep it: {reason}")]
    Unavailable {
        /// The keychain's own words.
        reason: String,
    },
    /// The keychain answered, but the call failed.
    #[error("the keychain failed: {reason}")]
    Failed {
        /// The keychain's own words.
        reason: String,
    },
}

/// Somewhere secrets are kept, by key. Every method may block (see the
/// [module documentation](self)).
pub trait SecretStore: Send + Sync + core::fmt::Debug {
    /// The secret kept under `key`, or `None` when there is none.
    ///
    /// # Errors
    ///
    /// [`SecretError`] when the store cannot be read.
    fn load(&self, key: &SecretKey) -> Result<Option<Secret>, SecretError>;

    /// Keep `secret` under `key`, replacing any kept there.
    ///
    /// # Errors
    ///
    /// [`SecretError`] when the store cannot be written.
    fn save(&self, key: &SecretKey, secret: &Secret) -> Result<(), SecretError>;

    /// Forget the secret kept under `key`. Forgetting one that is not kept
    /// succeeds.
    ///
    /// # Errors
    ///
    /// [`SecretError`] when the store cannot be written.
    fn forget(&self, key: &SecretKey) -> Result<(), SecretError>;
}

/// The OS keychain, through the `keyring` crate's platform store.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeychainStore;

impl KeychainStore {
    /// The keychain entry `key` names.
    fn entry(key: &SecretKey) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(key.kind.service(), &key.account).map_err(classify)
    }
}

/// Sort a keychain error into one the user is told about as "cannot keep it"
/// and one where the keychain answered and failed.
fn classify(error: keyring::Error) -> SecretError {
    match error {
        // The store's start-up failed; its own error says why (no session
        // bus, no Secret Service on it).
        keyring::Error::NoDefaultStore => SecretError::Unavailable {
            reason: match keyring::Entry::store_status() {
                Err(cause) => cause.to_string(),
                Ok(()) => error.to_string(),
            },
        },
        keyring::Error::NoStorageAccess(cause) => SecretError::Unavailable {
            reason: cause.to_string(),
        },
        other => SecretError::Failed {
            reason: other.to_string(),
        },
    }
}

impl SecretStore for KeychainStore {
    fn load(&self, key: &SecretKey) -> Result<Option<Secret>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(text) => Ok(Some(Secret(text))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(classify(error)),
        }
    }

    fn save(&self, key: &SecretKey, secret: &Secret) -> Result<(), SecretError> {
        Self::entry(key)?
            .set_password(secret.expose())
            .map_err(classify)
    }

    fn forget(&self, key: &SecretKey) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(classify(error)),
        }
    }
}

/// Secrets kept in this process alone, gone when it ends: a test's store.
#[derive(Debug, Default)]
pub struct MemoryStore {
    /// The secrets, by key.
    secrets: Mutex<BTreeMap<SecretKey, Secret>>,
}

impl MemoryStore {
    /// Run `act` on the secrets, or fail if a panic poisoned their lock.
    fn with<T>(
        &self,
        act: impl FnOnce(&mut BTreeMap<SecretKey, Secret>) -> T,
    ) -> Result<T, SecretError> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|_poisoned| SecretError::Failed {
                reason: "a panic poisoned the in-memory store's lock".to_owned(),
            })?;
        Ok(act(&mut secrets))
    }
}

impl SecretStore for MemoryStore {
    fn load(&self, key: &SecretKey) -> Result<Option<Secret>, SecretError> {
        self.with(|secrets| secrets.get(key).cloned())
    }

    fn save(&self, key: &SecretKey, secret: &Secret) -> Result<(), SecretError> {
        self.with(|secrets| {
            secrets.insert(key.clone(), secret.clone());
        })
    }

    fn forget(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.with(|secrets| {
            secrets.remove(key);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryStore, Secret, SecretError, SecretKey, SecretKind, SecretStore as _};
    use pretty_assertions::assert_eq;

    /// A secret saved is loaded back, replaced by the next save, and gone
    /// once forgotten; forgetting it again still succeeds.
    #[test]
    fn a_memory_store_keeps_and_forgets_a_secret() -> Result<(), SecretError> {
        let store = MemoryStore::default();
        let key = SecretKey::login_password("127.0.0.1:9000", "Avatar Tester");
        assert_eq!(store.load(&key)?, None);
        store.save(&key, &Secret::new("first"))?;
        assert_eq!(store.load(&key)?, Some(Secret::new("first")));
        store.save(&key, &Secret::new("second"))?;
        assert_eq!(store.load(&key)?, Some(Secret::new("second")));
        store.forget(&key)?;
        assert_eq!(store.load(&key)?, None);
        store.forget(&key)?;
        Ok(())
    }

    /// The same avatar name on two grids is two accounts, each with its own
    /// secret.
    #[test]
    fn each_grid_keeps_its_own_password_for_a_name() -> Result<(), SecretError> {
        let store = MemoryStore::default();
        let local = SecretKey::login_password("127.0.0.1:9000", "Avatar Tester");
        let beta = SecretKey::login_password("login.aditi.lindenlab.com", "Avatar Tester");
        assert_eq!(local.kind(), SecretKind::LoginPassword);
        assert_eq!(local.account(), "Avatar Tester @ 127.0.0.1:9000");
        store.save(&local, &Secret::new("local"))?;
        assert_eq!(store.load(&beta)?, None);
        store.save(&beta, &Secret::new("beta"))?;
        assert_eq!(store.load(&local)?, Some(Secret::new("local")));
        Ok(())
    }

    /// A secret's `Debug` hides its text.
    #[test]
    fn a_secret_is_redacted_in_debug() {
        let shown = format!("{:?}", Secret::new("hunter2"));
        assert_eq!(shown, "Secret(<redacted>)");
    }
}
