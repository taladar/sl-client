//! [`Relay`]: the bookkeeping every transport keeps between its clients and
//! one viewer's [`AutomationQueue`] — the same whether a client is a socket
//! connection ([`crate::RemoteEndpoint`]) or a test in the same process
//! ([`crate::InProcessTransport`]).
//!
//! - **Clients pick their own ids.** Each request goes into the queue under a
//!   fresh id of the relay's, from its base up, and its answer goes back under
//!   the client's — so two clients may both use id 1, and whatever else
//!   submits to the queue stays below the base.
//! - **A duplicate is refused at the door**: a request whose id is in flight
//!   for its client, a subscription already running under its id, the end of
//!   one that is not.
//! - **Subscriptions end with their client.** A client that finishes (sends
//!   nothing more) has its subscriptions ended and is let go once its last
//!   answer is delivered; one that closes has its pending answers dropped as
//!   well.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use sl_automation_proto::{
    AutomationError, Notification, Request, RequestBody, RequestId, Response, ViewerMessage,
};

use crate::executor::AutomationQueue;

/// One client, as the relay keeps it.
#[derive(Debug, Default)]
struct Client {
    /// The ids of its requests not answered yet.
    in_flight: HashSet<RequestId>,
    /// Its subscriptions: its id for each, and the queue's.
    subscriptions: HashMap<RequestId, RequestId>,
    /// Whether it has finished sending.
    finished: bool,
}

/// A request in the queue on a client's behalf.
#[derive(Debug)]
struct Pending<K> {
    /// Whose, and under which of its ids; `None` for the relay's own (ending
    /// a departed client's subscriptions), whose answer is dropped.
    client: Option<(K, RequestId)>,
    /// Whether it starts a subscription.
    subscribe: bool,
}

/// The clients of one viewer's queue, what each has in flight, and the
/// subscriptions running for them; clients are told apart by `K`.
#[derive(Debug)]
pub(crate) struct Relay<K> {
    /// The connected clients.
    clients: HashMap<K, Client>,
    /// The requests in the queue, by the queue's id.
    pending: HashMap<RequestId, Pending<K>>,
    /// The running subscriptions, by the queue's id: whose, under which id.
    subscriptions: HashMap<RequestId, (K, RequestId)>,
    /// The queue id the next request gets.
    next_id: u64,
}

impl<K: Copy + Eq + Hash> Relay<K> {
    /// A relay numbering its requests from `base`.
    pub(crate) fn new(base: u64) -> Self {
        Self {
            clients: HashMap::new(),
            pending: HashMap::new(),
            subscriptions: HashMap::new(),
            next_id: base,
        }
    }

    /// Whether nothing is in the queue on anyone's behalf and no subscription
    /// is running: there is nothing to deliver.
    pub(crate) fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.subscriptions.is_empty()
    }

    /// How many subscriptions are running.
    #[cfg(test)]
    pub(crate) fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// A client connected.
    pub(crate) fn connect(&mut self, client: K) {
        let _previous = self.clients.insert(client, Client::default());
    }

    /// A fresh queue id.
    const fn allocate(&mut self) -> RequestId {
        let id = RequestId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Put `client`'s `request` into `queue` under a fresh id. A request that
    /// cannot be is answered at once: the returned message goes to the
    /// client. A client that is not connected is ignored.
    pub(crate) fn submit(
        &mut self,
        client: K,
        request: Request,
        queue: &mut AutomationQueue,
    ) -> Option<ViewerMessage> {
        let Request { id, body } = request;
        let book = self.clients.get(&client)?;
        if book.in_flight.contains(&id) {
            return Some(refusal(
                id,
                format!("request {} is already in flight", id.0),
            ));
        }
        let subscribe = matches!(body, RequestBody::Subscribe { .. });
        if subscribe && book.subscriptions.contains_key(&id) {
            return Some(refusal(
                id,
                format!("subscription {} is already running", id.0),
            ));
        }
        let body = match body {
            RequestBody::Unsubscribe { subscription } => {
                let Some(&queued) = book.subscriptions.get(&subscription) else {
                    return Some(refusal(
                        id,
                        format!("there is no subscription {}", subscription.0),
                    ));
                };
                RequestBody::Unsubscribe {
                    subscription: queued,
                }
            }
            body => body,
        };
        let queued = self.allocate();
        let book = self.clients.get_mut(&client)?;
        let _new = book.in_flight.insert(id);
        if subscribe {
            let _previous = book.subscriptions.insert(id, queued);
            let _previous = self.subscriptions.insert(queued, (client, id));
        }
        let _previous = self.pending.insert(
            queued,
            Pending {
                client: Some((client, id)),
                subscribe,
            },
        );
        queue.submit(Request { id: queued, body });
        None
    }

    /// `client` sends nothing more: end its subscriptions. It stays until
    /// its last answer is delivered, and goes now when nothing is left to
    /// answer.
    pub(crate) fn finish(&mut self, client: K, queue: &mut AutomationQueue) {
        let Some(book) = self.clients.get_mut(&client) else {
            return;
        };
        book.finished = true;
        let answered = book.in_flight.is_empty();
        let subscriptions: Vec<RequestId> = book
            .subscriptions
            .drain()
            .map(|(_id, queued)| queued)
            .collect();
        if answered {
            let _gone = self.clients.remove(&client);
        }
        for subscription in subscriptions {
            self.end_subscription(subscription, queue);
        }
    }

    /// `client` went away: end its subscriptions and forget its requests.
    pub(crate) fn close(&mut self, client: K, queue: &mut AutomationQueue) {
        self.finish(client, queue);
        let _gone = self.clients.remove(&client);
        for pending in self.pending.values_mut() {
            if pending.client.is_some_and(|(owner, _id)| owner == client) {
                pending.client = None;
            }
        }
    }

    /// Whether `client` is still connected: not closed, and not finished with
    /// every answer delivered.
    pub(crate) fn is_connected(&self, client: K) -> bool {
        self.clients.contains_key(&client)
    }

    /// End the subscription the queue knows as `subscription`, on the relay's
    /// own behalf.
    fn end_subscription(&mut self, subscription: RequestId, queue: &mut AutomationQueue) {
        let _gone = self.subscriptions.remove(&subscription);
        let queued = self.allocate();
        let _previous = self.pending.insert(
            queued,
            Pending {
                client: None,
                subscribe: false,
            },
        );
        queue.submit(Request {
            id: queued,
            body: RequestBody::Unsubscribe { subscription },
        });
    }

    /// Every answer and notification `queue` holds for a client, under the
    /// client's ids, in the order to send them; a finished client with
    /// nothing left to wait for is let go.
    pub(crate) fn deliver(&mut self, queue: &mut AutomationQueue) -> Vec<(K, ViewerMessage)> {
        let mut out = Vec::new();
        // In the order the executor answered: two answers of one frame reach
        // the client in that order.
        let answered = queue.take_responses(|queued| self.pending.contains_key(&queued));
        for mut response in answered {
            let queued = response.id;
            let Some(pending) = self.pending.remove(&queued) else {
                continue;
            };
            let Some((client, id)) = pending.client else {
                continue;
            };
            let Some(book) = self.clients.get_mut(&client) else {
                continue;
            };
            let _answered = book.in_flight.remove(&id);
            if pending.subscribe && response.result.is_err() {
                let _gone = book.subscriptions.remove(&id);
                let _gone = self.subscriptions.remove(&queued);
            }
            response.id = id;
            out.push((client, ViewerMessage::Response(Box::new(response))));
        }
        for (&queued, &(client, id)) in &self.subscriptions {
            let notifications = queue.take_notifications(queued);
            if !self.clients.contains_key(&client) {
                continue;
            }
            for notification in notifications {
                let notification = match notification {
                    Notification::Log { page, .. } => Notification::Log {
                        subscription: id,
                        page,
                    },
                    other @ Notification::Rejected { .. } => other,
                };
                out.push((client, ViewerMessage::Notification(notification)));
            }
        }
        self.clients
            .retain(|_client, book| !(book.finished && book.in_flight.is_empty()));
        out
    }
}

/// The answer to the request `id` refused without the queue.
fn refusal(id: RequestId, reason: String) -> ViewerMessage {
    ViewerMessage::Response(Box::new(Response {
        id,
        result: Err(AutomationError::InvalidRequest { reason }),
        report: None,
    }))
}
