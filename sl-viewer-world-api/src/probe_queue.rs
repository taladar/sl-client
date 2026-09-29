//! [`ProbeQueue`]: questions one layer asks another that owns the answer —
//! the automation layer asking the build tool, which it must not depend on.
//! The asker files a query and polls for its answer by ticket; the owner takes
//! every waiting query the next time it runs and files the answers back.

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;

/// Names one query in a [`ProbeQueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProbeTicket(u64);

/// Queries of type `Q` waiting for an owner's answer of type `A`.
#[derive(Resource, Debug)]
pub struct ProbeQueue<Q: Send + Sync + 'static, A: Send + Sync + 'static> {
    /// The last ticket handed out.
    last: u64,
    /// Asked, not yet answered.
    queued: VecDeque<(ProbeTicket, Q)>,
    /// Answered, not yet taken.
    answers: HashMap<ProbeTicket, A>,
}

impl<Q: Send + Sync + 'static, A: Send + Sync + 'static> Default for ProbeQueue<Q, A> {
    fn default() -> Self {
        Self {
            last: 0,
            queued: VecDeque::new(),
            answers: HashMap::new(),
        }
    }
}

impl<Q: Send + Sync + 'static, A: Send + Sync + 'static> ProbeQueue<Q, A> {
    /// Ask `query`.
    pub fn request(&mut self, query: Q) -> ProbeTicket {
        self.last = self.last.wrapping_add(1);
        let ticket = ProbeTicket(self.last);
        self.queued.push_back((ticket, query));
        ticket
    }

    /// The answer to `ticket`, once given. Taking it forgets it.
    pub fn take_answer(&mut self, ticket: ProbeTicket) -> Option<A> {
        self.answers.remove(&ticket)
    }

    /// Give up on `ticket`: dropped if unanswered, forgotten if answered.
    pub fn abandon(&mut self, ticket: ProbeTicket) {
        self.queued.retain(|(queued, _query)| *queued != ticket);
        let _answer = self.answers.remove(&ticket);
    }

    /// Whether anything waits for an answer — the owner's cue to work.
    #[must_use]
    pub fn has_requests(&self) -> bool {
        !self.queued.is_empty()
    }

    /// The owner's side: every waiting query.
    pub fn take_requests(&mut self) -> Vec<(ProbeTicket, Q)> {
        self.queued.drain(..).collect()
    }

    /// The owner's side: file the answer to `ticket`.
    pub fn answer(&mut self, ticket: ProbeTicket, answer: A) {
        let _previous = self.answers.insert(ticket, answer);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::ProbeQueue;

    #[test]
    fn a_query_is_answered_once_and_an_abandoned_one_never() {
        let mut queue = ProbeQueue::<u32, String>::default();
        let kept = queue.request(1);
        let dropped = queue.request(2);
        queue.abandon(dropped);
        assert!(queue.has_requests());
        let taken = queue.take_requests();
        assert_eq!(taken.len(), 1, "the abandoned query is never handed out");
        assert!(!queue.has_requests());
        queue.answer(kept, "one".to_owned());
        assert_eq!(queue.take_answer(kept), Some("one".to_owned()));
        assert_eq!(queue.take_answer(kept), None);
    }
}
