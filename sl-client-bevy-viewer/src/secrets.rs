//! The viewer's **secret store** (`viewer-secret-store`): where a saved login
//! password is kept, and anything else that must never land in a plaintext
//! config as it comes.
//!
//! The store is `sl_secret_store`'s: the OS keychain for a resident, an
//! in-memory one for a test stage ([`ViewerAppOptions::secrets`]). Nothing
//! falls back to a file; where no keychain answers, the feature that asked
//! says why it cannot remember (`sl_secret_store` says why that is the whole
//! policy).
//!
//! # Off the frame thread
//!
//! A keychain call may block on a D-Bus round trip or an unlock prompt, so a
//! system never makes one. It writes a [`SecretRequest`]; one worker thread,
//! started with the first request, carries the requests out in the order they
//! were written, and each answer comes back as a [`SecretAnswer`] — as the
//! second-factor source answers through `crate::login_flow`.
//!
//! [`ViewerAppOptions::secrets`]: crate::assembly::ViewerAppOptions::secrets

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use bevy::prelude::*;
use sl_secret_store::{Secret, SecretError, SecretKey, SecretStore};

/// The store the viewer keeps its secrets in.
#[derive(Resource, Debug, Clone)]
pub(crate) struct ViewerSecrets(pub(crate) Arc<dyn SecretStore>);

/// What to do with the secret under a key.
#[derive(Debug, Clone)]
pub(crate) enum SecretOp {
    /// Read it.
    Load,
    /// Keep this secret there.
    Save(Secret),
    /// Forget it.
    Forget,
}

/// A request to the secret store, carried out on its worker.
#[derive(Message, Debug, Clone)]
pub(crate) struct SecretRequest {
    /// The secret it is about.
    pub(crate) key: SecretKey,
    /// What to do with it.
    pub(crate) op: SecretOp,
}

/// How a [`SecretRequest`] went.
#[derive(Debug, Clone)]
pub(crate) enum SecretOutcome {
    /// The secret read, or `None` when none is kept.
    Loaded(Option<Secret>),
    /// The secret is kept.
    Saved,
    /// The secret is forgotten.
    Forgotten,
    /// The store could not do it.
    Failed(SecretError),
}

/// The answer to a [`SecretRequest`].
#[derive(Message, Debug, Clone)]
pub(crate) struct SecretAnswer {
    /// The secret the request was about.
    pub(crate) key: SecretKey,
    /// How it went.
    pub(crate) outcome: SecretOutcome,
}

/// The secret store's worker thread: where requests go and answers come back.
#[derive(Debug)]
struct SecretWorker {
    /// The worker's queue.
    requests: Sender<SecretRequest>,
    /// Its answers.
    answers: Receiver<SecretAnswer>,
}

/// Carries [`SecretRequest`]s out and answers them, for a viewer holding
/// [`ViewerSecrets`].
pub(crate) struct SecretsPlugin;

impl Plugin for SecretsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SecretRequest>()
            .add_message::<SecretAnswer>()
            .add_systems(
                Update,
                run_secret_requests.run_if(resource_exists::<ViewerSecrets>),
            );
    }
}

/// Carry `request` out against `store`.
fn carry_out(store: &dyn SecretStore, request: &SecretRequest) -> SecretOutcome {
    let done = match &request.op {
        SecretOp::Load => store.load(&request.key).map(SecretOutcome::Loaded),
        SecretOp::Save(secret) => store
            .save(&request.key, secret)
            .map(|()| SecretOutcome::Saved),
        SecretOp::Forget => store
            .forget(&request.key)
            .map(|()| SecretOutcome::Forgotten),
    };
    done.unwrap_or_else(SecretOutcome::Failed)
}

/// Start the worker thread over `store`.
fn start_worker(store: Arc<dyn SecretStore>) -> std::io::Result<SecretWorker> {
    let (requests, queue) = std::sync::mpsc::channel::<SecretRequest>();
    let (reply, answers) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("sl-viewer-secrets".to_owned())
        .spawn(move || {
            for request in queue {
                let outcome = carry_out(store.as_ref(), &request);
                let answer = SecretAnswer {
                    key: request.key,
                    outcome,
                };
                // The viewer has gone, and nobody is left to tell.
                if reply.send(answer).is_err() {
                    break;
                }
            }
        })?;
    Ok(SecretWorker { requests, answers })
}

/// Hand the frame's requests to the worker, starting it with the first, and
/// write back the answers it has.
fn run_secret_requests(
    secrets: Res<ViewerSecrets>,
    mut requests: MessageReader<SecretRequest>,
    mut answers: MessageWriter<SecretAnswer>,
    mut worker: Local<Option<SecretWorker>>,
) {
    for request in requests.read() {
        if worker.is_none() {
            match start_worker(Arc::clone(&secrets.0)) {
                Ok(started) => *worker = Some(started),
                Err(error) => {
                    answers.write(SecretAnswer {
                        key: request.key.clone(),
                        outcome: SecretOutcome::Failed(SecretError::Failed {
                            reason: format!("the secret store's thread did not start: {error}"),
                        }),
                    });
                    continue;
                }
            }
        }
        let sent = worker
            .as_ref()
            .map(|running| running.requests.send(request.clone()));
        if let Some(Err(lost)) = sent {
            *worker = None;
            answers.write(SecretAnswer {
                key: lost.0.key,
                outcome: SecretOutcome::Failed(SecretError::Failed {
                    reason: "the secret store's thread has ended".to_owned(),
                }),
            });
        }
    }
    let Some(running) = worker.as_ref() else {
        return;
    };
    loop {
        match running.answers.try_recv() {
            Ok(answer) => {
                answers.write(answer);
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                // Started again by the next request.
                *worker = None;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_secret_store::{MemoryStore, Secret, SecretKey, SecretStore as _};

    use super::{
        SecretAnswer, SecretOp, SecretOutcome, SecretRequest, SecretsPlugin, ViewerSecrets,
    };

    /// What the app has answered so far.
    #[derive(Resource, Default)]
    struct Heard(Vec<SecretAnswer>);

    /// Keep every answer.
    fn hear(mut answers: MessageReader<SecretAnswer>, mut heard: ResMut<Heard>) {
        heard.0.extend(answers.read().cloned());
    }

    /// Step `app` until it has heard `count` answers, or give up.
    fn step_until_heard(app: &mut App, count: usize) {
        for _ in 0..2000 {
            app.update();
            if app.world().resource::<Heard>().0.len() >= count {
                return;
            }
            std::thread::sleep(core::time::Duration::from_millis(1));
        }
    }

    /// Requests are carried out on the worker in the order they were written:
    /// a save, a load that reads it back, a forget, and a load that finds
    /// nothing.
    #[test]
    fn requests_are_answered_in_order_off_the_frame_thread() {
        let store = Arc::new(MemoryStore::default());
        let key = SecretKey::login_password("127.0.0.1:9000", "Avatar Tester");
        let mut app = App::new();
        app.add_plugins(SecretsPlugin)
            .insert_resource(ViewerSecrets(Arc::<MemoryStore>::clone(&store)))
            .init_resource::<Heard>()
            .add_systems(PostUpdate, hear);
        for op in [
            SecretOp::Save(Secret::new("sesame")),
            SecretOp::Load,
            SecretOp::Forget,
            SecretOp::Load,
        ] {
            app.world_mut().write_message(SecretRequest {
                key: key.clone(),
                op,
            });
        }
        step_until_heard(&mut app, 4);
        let heard: Vec<String> = app
            .world()
            .resource::<Heard>()
            .0
            .iter()
            .map(|answer| match &answer.outcome {
                SecretOutcome::Loaded(Some(secret)) => format!("loaded {}", secret.expose()),
                SecretOutcome::Loaded(None) => "loaded nothing".to_owned(),
                SecretOutcome::Saved => "saved".to_owned(),
                SecretOutcome::Forgotten => "forgotten".to_owned(),
                SecretOutcome::Failed(error) => format!("failed: {error}"),
            })
            .collect();
        assert_eq!(
            heard,
            ["saved", "loaded sesame", "forgotten", "loaded nothing"]
        );
        assert_eq!(store.load(&key), Ok(None));
    }
}
