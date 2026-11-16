//! How the viewer logs in, and why it all happens inside one App.
//!
//! There are two ways in. A viewer started with credentials
//! ([`ViewerLogin::Now`]) logs in at startup, which every test, harness and
//! headless run relies on; a viewer started without them ([`ViewerLogin::Screen`])
//! shows its login screen and logs in when the user asks
//! (`crate::login_screen`). Both end in the same [`SlLogin`] and the same
//! `LoginScope`: what a login decides beside the session.
//!
//! Every attempt after the first — the answer to a second-factor challenge, a
//! retry from the screen — is another [`SlLogin`] to the same App rather than a
//! new App. A windowed viewer could not build a second one: winit allows one
//! event loop per process, and `bevy_winit` builds it when its plugin is added,
//! so a rebuilt windowed viewer died of `RecreationAttempt` on the challenge it
//! meant to answer.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use bevy::prelude::*;
use sl_client_bevy::{AccountDirsConfig, LoginParams, SlLogin, SlMfaChallenge};
use sl_viewer_platform::paths::ViewerPaths;
use sl_viewer_settings::AccountContext;
use sl_viewer_world_scene::terrain_disk_cache::TerrainDiskCache;

/// Who the viewer logs in as, and when.
#[derive(Debug)]
#[non_exhaustive]
pub enum ViewerLogin {
    /// At startup, with these parameters: the credentials file, or a harness.
    Now {
        /// Who logs in, where, and as which viewer (boxed: the larger half of
        /// a choice whose other half is empty).
        params: Box<LoginParams>,
        /// Where the answer to a second-factor challenge comes from, or
        /// `None` for an account that is never challenged: a challenge then
        /// ends the run.
        second_factor: Option<SecondFactor>,
    },
    /// When the login screen's user asks, with what they entered there.
    Screen,
}

/// Where a viewer started with credentials gets a second-factor token: a
/// call that runs off the frame thread (it may run a command and wait out a
/// TOTP window) and answers the token, `None` when the account has no way to
/// make one, or why it failed.
#[derive(Clone)]
pub struct SecondFactor(Arc<dyn Fn() -> Result<Option<String>, String> + Send + Sync>);

impl SecondFactor {
    /// A source that answers by calling `acquire`.
    pub fn new(
        acquire: impl Fn() -> Result<Option<String>, String> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(acquire))
    }
}

impl core::fmt::Debug for SecondFactor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecondFactor(..)")
    }
}

/// The second-factor source of a viewer started with credentials, as a
/// resource for [`answer_second_factor`].
#[derive(Resource, Debug, Clone)]
pub(crate) struct SecondFactorSource(pub(crate) SecondFactor);

/// The login the session is on or was last on: what an answer to its
/// challenge is folded into and sent again.
#[derive(Resource, Debug, Clone)]
pub struct PendingLogin {
    /// The parameters it was sent with.
    pub params: LoginParams,
    /// Its per-avatar directory derivation.
    pub account_dirs: Option<AccountDirsConfig>,
}

impl PendingLogin {
    /// The login to send for these parameters.
    #[must_use]
    pub fn message(&self) -> SlLogin {
        SlLogin {
            params: self.params.clone(),
            account_dirs: self.account_dirs.clone(),
        }
    }
}

/// Everything a login's parameters decide about the viewer beside the session
/// itself: which grid and avatar its per-avatar directories are named after,
/// where its land cache lives, and what the About floater and an automation
/// hello say about it. Built once per login and inserted before it starts.
#[derive(Debug)]
pub(crate) struct LoginScope {
    /// The session's per-avatar directory derivation, or `None` with nowhere
    /// to store anything.
    pub(crate) account_dirs: Option<AccountDirsConfig>,
    /// The account-scope settings' identity.
    pub(crate) account_context: AccountContext,
    /// The land-height cache, for this grid.
    pub(crate) terrain_cache: TerrainDiskCache,
    /// The About floater's login facts.
    pub(crate) about: crate::about_floater::AboutSessionInfo,
    /// Who an automation hello names.
    pub(crate) automation_identity: sl_viewer_automation::AutomationIdentity,
}

impl LoginScope {
    /// The scope of a login with `params` by a viewer storing under `paths`.
    pub(crate) fn of(params: &LoginParams, paths: &ViewerPaths) -> Self {
        let grid = sl_account_dirs::grid_dir_name(&params.login_uri);
        let avatar =
            sl_account_dirs::avatar_dir_name(&params.request.first_name, &params.request.last_name);
        Self {
            account_dirs: paths.stores_anything().then(|| AccountDirsConfig {
                grid: grid.clone(),
                avatar: avatar.clone(),
                chat_log_base: paths.state_accounts_base(),
                inventory_cache_base: paths.cache_accounts_base(),
            }),
            // Region handles are grid coordinates, so another grid's file
            // would be another region.
            terrain_cache: TerrainDiskCache::new(paths.terrain_cache_dir(&grid)),
            about: crate::about_floater::AboutSessionInfo {
                grid: grid.clone(),
                login_uri: params.login_uri.to_string(),
                channel: params.request.channel.clone(),
                version: params.request.version.clone(),
            },
            automation_identity: sl_viewer_automation::AutomationIdentity {
                viewer: crate::build_info::VIEWER_NAME.to_owned(),
                version: crate::build_info::full_version(),
                grid: Some(grid.clone()),
                agent_name: Some(format!(
                    "{} {}",
                    params.request.first_name, params.request.last_name
                )),
            },
            account_context: AccountContext {
                accounts_base: paths.config_accounts_base(),
                grid,
                avatar,
            },
        }
    }

    /// What a viewer on its login screen holds before anybody has logged in:
    /// no per-avatar settings, no land cache, no login facts, and an
    /// automation hello naming no grid or avatar.
    pub(crate) fn before_login() -> (AccountContext, sl_viewer_automation::AutomationIdentity) {
        (
            AccountContext {
                accounts_base: None,
                grid: String::new(),
                avatar: String::new(),
            },
            sl_viewer_automation::AutomationIdentity {
                viewer: crate::build_info::VIEWER_NAME.to_owned(),
                version: crate::build_info::full_version(),
                grid: None,
                agent_name: None,
            },
        )
    }

    /// Insert the scope's resources into `world`, the automation identity
    /// only where an automation executor reads it (`automated`).
    pub(crate) fn insert(self, world: &mut World, automated: bool) {
        world.insert_resource(self.account_context);
        world.insert_resource(self.terrain_cache);
        world.insert_resource(self.about);
        if automated {
            world.insert_resource(self.automation_identity);
        }
    }
}

/// A token being acquired for a challenge, on its own thread.
#[derive(Debug)]
pub(crate) struct Acquiring {
    /// The challenge's hash, which the answer carries back, if it sent one.
    mfa_hash: Option<String>,
    /// The thread's answer.
    answer: Receiver<Result<Option<String>, String>>,
}

/// Answer a second-factor challenge to a viewer started with credentials:
/// acquire the token from its [`SecondFactorSource`] on a thread of its own
/// and send the login again with it folded in. A source with no token, or one
/// that fails, ends the run with the failure — nobody is there to type one in.
/// A viewer on its login screen has no source: its screen asks the user.
pub(crate) fn answer_second_factor(
    mut challenges: MessageReader<SlMfaChallenge>,
    source: Option<Res<SecondFactorSource>>,
    pending: Option<ResMut<PendingLogin>>,
    mut acquiring: Local<Option<Acquiring>>,
    mut logins: MessageWriter<SlLogin>,
    mut exit: MessageWriter<AppExit>,
) {
    let (Some(source), Some(mut pending)) = (source, pending) else {
        return;
    };
    for challenge in challenges.read() {
        info!(
            "multi-factor authentication required: {}",
            challenge.0.message
        );
        let (sender, answer) = std::sync::mpsc::channel();
        let acquire = Arc::clone(&source.0.0);
        let spawned = std::thread::Builder::new()
            .name("sl-viewer-mfa".to_owned())
            .spawn(move || {
                // Nobody is left to tell if the viewer has gone.
                sender.send(acquire()).ok();
            });
        if let Err(error) = spawned {
            error!("could not start acquiring the second factor: {error}");
            exit.write(AppExit::error());
            return;
        }
        *acquiring = Some(Acquiring {
            mfa_hash: challenge.0.mfa_hash.clone(),
            answer,
        });
    }
    let Some(waiting) = acquiring.as_ref() else {
        return;
    };
    let answer = match waiting.answer.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {
            Err("the second-factor thread ended without an answer".to_owned())
        }
    };
    let mfa_hash = waiting.mfa_hash.clone();
    *acquiring = None;
    match answer {
        Ok(Some(token)) => {
            pending.params.request = pending.params.request.clone().with_mfa(token, mfa_hash);
            logins.write(pending.message());
        }
        Ok(None) => {
            error!("the grid asks for a second factor and this avatar has no way to make one");
            exit.write(AppExit::error());
        }
        Err(error) => {
            error!("could not acquire the second factor: {error}");
            exit.write(AppExit::error());
        }
    }
}
