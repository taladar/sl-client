//! Scripts' HTTP on the fake grid (`server-lsl-lib-http-url`): the URLs the
//! grid has granted, the requests travelling to them and back, and what
//! answers a request sent anywhere else.
//!
//! Nothing leaves the process. A script's request to one of the grid's own
//! URLs reaches that script in-process, whichever region it is in; a request
//! from outside reaches it through the grid's HTTP service; and a request to
//! any other address is answered by the grid's [`HttpResponder`] — by default
//! [`NoResponder`], a stated failure, so a `cargo test` run never touches the
//! network and never hangs. A scenario that wants a canned service mounts a
//! [`TableResponder`]. Every hand-over happens at a region's tick, so how
//! long a request takes is counted in ticks, never measured.
//!
//! What a grid's URL surface does with a request — what it answers itself,
//! what headers it adds, how long it waits — is the runtime's
//! (`sl_lsl_runtime::vm::http`), shared with its tests.

use std::collections::BTreeMap;
use std::sync::Arc;

use sl_lsl_runtime::vm::http::{Answered, Inbound, Outbound};
use sl_lsl_runtime::vm::{CallerId, EngineFlavour};

/// What answers a script's request to an address that is not one of the
/// grid's own URLs.
pub trait HttpResponder: Send + Sync + core::fmt::Debug {
    /// The answer to `request`.
    fn respond(&self, request: &Outbound) -> Answered;
}

/// The default responder: every request off the grid fails, saying why.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoResponder;

/// The status [`NoResponder`] answers with: the one both grids' scripts read
/// as a request their simulator could not send.
pub const NO_RESPONDER_STATUS: i32 = 499;

impl HttpResponder for NoResponder {
    fn respond(&self, request: &Outbound) -> Answered {
        Answered {
            status: NO_RESPONDER_STATUS,
            content_type: "text/plain".to_owned(),
            body: format!(
                "The fake grid sends no request off the grid: nothing answers {}",
                request.url
            ),
        }
    }
}

/// A responder that answers from a table of URLs (without their query), and
/// as [`NoResponder`] for any other.
#[derive(Debug, Clone, Default)]
pub struct TableResponder {
    /// The answers, by URL.
    pub routes: BTreeMap<String, Answered>,
}

impl HttpResponder for TableResponder {
    fn respond(&self, request: &Outbound) -> Answered {
        let url = request.url.split('?').next().unwrap_or_default();
        self.routes
            .get(url)
            .cloned()
            .unwrap_or_else(|| NoResponder.respond(request))
    }
}

/// Where the answer to a request that reached a script goes.
#[derive(Debug)]
pub(crate) enum Reply {
    /// Back to the script that sent it, as `http_response`.
    Script {
        /// The sender's region.
        region: usize,
        /// The sender.
        caller: CallerId,
        /// The request's key.
        request: String,
    },
    /// Back to the HTTP service, which answers whoever sent it from outside.
    Outside(tokio::sync::oneshot::Sender<Answered>),
}

/// A URL the grid has granted.
#[derive(Debug, Clone)]
pub(crate) struct Granted {
    /// The holder's region.
    pub(crate) region: usize,
    /// The holder.
    pub(crate) caller: CallerId,
    /// The URL as granted.
    pub(crate) url: String,
    /// The flavour of the holder's region, whose URL surface it is.
    pub(crate) flavour: EngineFlavour,
}

/// A request on its way into a script.
#[derive(Debug)]
pub(crate) struct Arriving {
    /// The holder's region.
    pub(crate) region: usize,
    /// The holder.
    pub(crate) caller: CallerId,
    /// The request.
    pub(crate) inbound: Inbound,
    /// Where its answer goes.
    pub(crate) reply: Reply,
}

/// An answer on its way back to a script.
#[derive(Debug, Clone)]
pub(crate) struct Returning {
    /// The sender's region.
    pub(crate) region: usize,
    /// The sender.
    pub(crate) caller: CallerId,
    /// The request's key.
    pub(crate) request: String,
    /// What came back.
    pub(crate) answered: Answered,
}

/// The grid's scripts' HTTP, under one lock: taken by a region's tick and by
/// the HTTP service, briefly, by nothing else.
#[derive(Debug)]
pub(crate) struct ScriptHttp {
    /// The URLs granted, by key.
    pub(crate) urls: BTreeMap<String, Granted>,
    /// Requests waiting for their region's tick to reach their script.
    pub(crate) arriving: Vec<Arriving>,
    /// Answers waiting for their region's tick to reach their script.
    pub(crate) returning: Vec<Returning>,
    /// What answers a request off the grid.
    pub(crate) responder: Arc<dyn HttpResponder>,
}

impl ScriptHttp {
    /// No URLs, nothing travelling, and `responder` for requests off the
    /// grid.
    pub(crate) fn new(responder: Arc<dyn HttpResponder>) -> Self {
        Self {
            urls: BTreeMap::new(),
            arriving: Vec::new(),
            returning: Vec::new(),
            responder,
        }
    }

    /// How many URLs the scripts of `region` hold.
    pub(crate) fn held_in(&self, region: usize) -> usize {
        self.urls
            .values()
            .filter(|granted| granted.region == region)
            .count()
    }

    /// The requests for scripts of `region`, taken.
    pub(crate) fn take_arriving(&mut self, region: usize) -> Vec<Arriving> {
        let (here, rest) = core::mem::take(&mut self.arriving)
            .into_iter()
            .partition(|arriving| arriving.region == region);
        self.arriving = rest;
        here
    }

    /// The answers for scripts of `region`, taken.
    pub(crate) fn take_returning(&mut self, region: usize) -> Vec<Returning> {
        let (here, rest) = core::mem::take(&mut self.returning)
            .into_iter()
            .partition(|returning| returning.region == region);
        self.returning = rest;
        here
    }
}

/// How many URLs a region of `flavour` allows: OpenSim's 100 (the local
/// grid's, `http/url`); Second Life ties them to the land's prim allowance,
/// which the fake grid states as `prims`.
pub(crate) const fn allowance(flavour: EngineFlavour, prims: i32) -> i32 {
    if flavour.is_opensim() { 100 } else { prims }
}
