//! Running a real `sl-client-tokio` client against the grid without ever
//! stalling it.
//!
//! [`Client::run`] awaits the send of every event and diagnostic onto bounded
//! channels, so a session whose events go unread stops once a channel is full
//! — and with it stops decoding datagrams and sending what it was commanded
//! to. A test that reads events only until the one it waits for, then sends a
//! command and waits on the *grid* to hear it, is exactly that session: a
//! region's ground alone is more events than a channel of a few hundred, and
//! whether the command got out before the channel filled is a race.
//!
//! [`run_client`] starts the run loop with a forwarder per channel that drains
//! it onto an unbounded one, as `sl-conformance`'s sessions do, so the run loop
//! never waits on whoever reads its events. A forwarder whose reader has gone
//! keeps draining and drops what it reads, so a test that never looks at its
//! diagnostics, or stops reading events, still leaves the client running.

use sl_client_tokio::{Client, Command, Diagnostic, Event};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// How many events the run loop may have in flight to its forwarder. The
/// forwarder takes each at once, so this only smooths bursts.
const EVENT_CHANNEL: usize = 256;

/// How many commands a test may queue ahead of the run loop.
const COMMAND_CHANNEL: usize = 64;

/// How many diagnostics the run loop may have in flight to its forwarder.
const DIAGNOSTIC_CHANNEL: usize = 64;

/// A client whose run loop is live, with its events and diagnostics drained
/// onto unbounded channels (see the [module docs](self)).
#[derive(Debug)]
pub struct RunningClient {
    /// Every event the run loop produced, in order.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// Every diagnostic the run loop produced (only when the client had
    /// [`Client::set_diagnostics`] on), in order.
    pub diagnostics: mpsc::UnboundedReceiver<Diagnostic>,
    /// Commands for the run loop. The loop ends once every sender is gone.
    pub commands: mpsc::Sender<Command>,
    /// The run loop itself.
    pub run: JoinHandle<Result<(), sl_client_tokio::Error>>,
}

/// Starts `client`'s run loop on a task, behind the forwarders.
#[must_use]
pub fn run_client(client: Client) -> RunningClient {
    let (event_tx, event_rx) = mpsc::channel::<Event>(EVENT_CHANNEL);
    let (diag_tx, diag_rx) = mpsc::channel::<Diagnostic>(DIAGNOSTIC_CHANNEL);
    let (commands, command_rx) = mpsc::channel::<Command>(COMMAND_CHANNEL);
    let events = forward(event_rx);
    let diagnostics = forward(diag_rx);
    let run = tokio::spawn(client.run(event_tx, diag_tx, command_rx));
    RunningClient {
        events,
        diagnostics,
        commands,
        run,
    }
}

/// Drains `bounded` onto a new unbounded channel until the run loop drops its
/// sender, and hands back the unbounded receiver.
fn forward<T: Send + 'static>(mut bounded: mpsc::Receiver<T>) -> mpsc::UnboundedReceiver<T> {
    let (unbounded_tx, unbounded_rx) = mpsc::unbounded_channel();
    drop(tokio::spawn(async move {
        while let Some(item) = bounded.recv().await {
            // A reader that has gone is no reason to stop draining: the run
            // loop would stall on the full channel otherwise.
            let _unread = unbounded_tx.send(item);
        }
    }));
    unbounded_rx
}
