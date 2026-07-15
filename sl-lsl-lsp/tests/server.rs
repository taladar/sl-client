//! End-to-end tests driving the whole server through an **in-memory transport**.
//!
//! [`Connection::memory`] hands back the two ends of a channel pair; the server
//! runs [`sl_lsl_lsp::run`] on one end in a thread, and the test plays the client
//! on the other — the exact embedding shape the viewer will use, and proof that
//! the same [`sl_lsl_lsp::run`] that backs the stdio binary also works over an
//! in-process channel. The tests exercise the real handshake, document
//! synchronisation, and the symbol requests, so a regression in the message loop
//! (a wrong capability, a dropped notification, a garbled response body) fails
//! here rather than only in a live editor.
//!
//! Tests return `Result<(), String>` and surface failures with `Err` rather than
//! `panic!`/`unwrap`, which the workspace's clippy config denies even in tests.

#[cfg(test)]
mod tests {
    use std::thread::JoinHandle;

    use pretty_assertions::assert_eq;

    use lsp_server::{
        Connection, Message, Notification, Request, RequestId, Response, ResponseKind,
    };
    use lsp_types::{DocumentSymbol, SymbolInformation, SymbolKind};
    use serde_json::{Value as JsonValue, json};
    use sl_lsl::LslSyntax;

    /// A test harness: the client end of an in-memory connection plus the join
    /// handle of the server thread, with a monotonic request-id counter.
    struct Harness {
        /// The client end of the in-memory channel pair.
        client: Connection,
        /// The running server's join handle, joined at teardown.
        server: JoinHandle<Result<(), sl_lsl_lsp::ServerError>>,
        /// The next JSON-RPC request id to allocate.
        next_id: i32,
    }

    impl Harness {
        /// Spin up a server on an in-memory connection and complete the LSP
        /// `initialize`/`initialized` handshake, returning a ready harness.
        fn start() -> Result<Self, String> {
            let (server_conn, client_conn) = Connection::memory();
            let server =
                std::thread::spawn(move || sl_lsl_lsp::run(server_conn, LslSyntax::default()));
            let mut harness = Self {
                client: client_conn,
                server,
                next_id: 0,
            };
            // The handshake: initialize request, then the initialized notification.
            let _init = harness.request("initialize", json!({ "capabilities": {} }))?;
            harness.notify("initialized", json!({}))?;
            Ok(harness)
        }

        /// Allocate the next request id.
        const fn take_id(&mut self) -> i32 {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            id
        }

        /// Send a request and block for its response, returning the `result` value
        /// or the error message the server replied with.
        fn request(&mut self, method: &str, params: JsonValue) -> Result<JsonValue, String> {
            let id = self.take_id();
            let request = Request {
                id: RequestId::from(id),
                method: method.to_owned(),
                params,
            };
            self.client
                .sender
                .send(Message::Request(request))
                .map_err(|err| err.to_string())?;
            loop {
                let message = self.client.receiver.recv().map_err(|err| err.to_string())?;
                if let Message::Response(Response { response_kind, .. }) = message {
                    return match response_kind {
                        ResponseKind::Ok { result } => Ok(result),
                        ResponseKind::Err { error } => Err(error.message),
                    };
                }
                // The server issues no requests of its own, so anything that is not
                // the response is unexpected; keep waiting for the response.
            }
        }

        /// Send a notification (no reply expected).
        fn notify(&self, method: &str, params: JsonValue) -> Result<(), String> {
            let notification = Notification {
                method: method.to_owned(),
                params,
            };
            self.client
                .sender
                .send(Message::Notification(notification))
                .map_err(|err| err.to_string())
        }

        /// Open a document with the given URI, version and text.
        fn did_open(&self, uri: &str, version: i32, text: &str) -> Result<(), String> {
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "lsl",
                        "version": version,
                        "text": text,
                    }
                }),
            )
        }

        /// Replace a document's full text (full-text sync `didChange`).
        fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<(), String> {
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [ { "text": text } ],
                }),
            )
        }

        /// Request the document symbol outline for a URI.
        fn document_symbols(&mut self, uri: &str) -> Result<Vec<DocumentSymbol>, String> {
            let result = self.request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
            )?;
            serde_json::from_value(result).map_err(|err| err.to_string())
        }

        /// Request the workspace symbols matching `query`.
        fn workspace_symbols(&mut self, query: &str) -> Result<Vec<SymbolInformation>, String> {
            let result = self.request("workspace/symbol", json!({ "query": query }))?;
            serde_json::from_value(result).map_err(|err| err.to_string())
        }

        /// Complete the LSP shutdown/exit handshake and join the server thread,
        /// asserting it returned cleanly.
        fn shutdown(mut self) -> Result<(), String> {
            let _null = self.request("shutdown", JsonValue::Null)?;
            self.notify("exit", JsonValue::Null)?;
            self.server
                .join()
                .map_err(|_err| "server thread panicked".to_owned())?
                .map_err(|err| err.to_string())
        }
    }

    /// A representative LSL script used across the tests.
    const SCRIPT: &str = "integer counter;\n\
    default\n\
    {\n\
    state_entry()\n\
    {\n\
    llSay(0, \"hi\");\n\
    }\n\
    }\n";

    /// A freshly opened document answers `documentSymbol` with the outline of its
    /// globals and states.
    #[test]
    fn open_then_document_symbols() -> Result<(), String> {
        let mut harness = Harness::start()?;
        let uri = "file:///test.lsl";
        harness.did_open(uri, 1, SCRIPT)?;

        let symbols = harness.document_symbols(uri)?;
        // The global `counter` and the `default` state.
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["counter", "default"]);
        let default = symbols.get(1).ok_or("missing state")?;
        assert_eq!(default.kind, SymbolKind::CLASS);
        let events = default.children.as_ref().ok_or("state has no children")?;
        assert_eq!(
            events.first().ok_or("missing event")?.name.as_str(),
            "state_entry"
        );

        harness.shutdown()
    }

    /// A `didChange` is reflected in a subsequent `documentSymbol` — the sync loop
    /// re-parses the swapped-in text.
    #[test]
    fn did_change_updates_symbols() -> Result<(), String> {
        let mut harness = Harness::start()?;
        let uri = "file:///test.lsl";
        harness.did_open(uri, 1, SCRIPT)?;
        harness.did_change(
            uri,
            2,
            "float gain;\nfloat level;\ndefault { timer() {} }\n",
        )?;

        let symbols = harness.document_symbols(uri)?;
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["gain", "level", "default"]);

        harness.shutdown()
    }

    /// `workspace/symbol` searches across open documents and filters by query.
    #[test]
    fn workspace_symbol_query() -> Result<(), String> {
        let mut harness = Harness::start()?;
        harness.did_open(
            "file:///a.lsl",
            1,
            "integer alpha;\ndefault { timer() {} }\n",
        )?;
        harness.did_open(
            "file:///b.lsl",
            1,
            "integer beta;\ndefault { timer() {} }\n",
        )?;

        // An empty query returns everything across both documents: two globals, two
        // `default` states, two `timer` events.
        let all = harness.workspace_symbols("")?;
        assert_eq!(all.len(), 6);

        // A query narrows to the matching name.
        let alpha = harness.workspace_symbols("alpha")?;
        assert_eq!(alpha.len(), 1);
        assert_eq!(alpha.first().ok_or("missing symbol")?.name, "alpha");

        harness.shutdown()
    }

    /// A `documentSymbol` for an unopened document is an empty outline, not an
    /// error.
    #[test]
    fn unopened_document_is_empty() -> Result<(), String> {
        let mut harness = Harness::start()?;
        let symbols = harness.document_symbols("file:///never-opened.lsl")?;
        assert_eq!(symbols, vec![]);
        harness.shutdown()
    }

    /// An unknown request method is answered with a JSON-RPC error rather than left
    /// hanging.
    #[test]
    fn unknown_method_errors() -> Result<(), String> {
        let mut harness = Harness::start()?;
        let outcome = harness.request("textDocument/foldingRange", json!({}));
        match outcome {
            Err(message) => assert!(
                message.contains("foldingRange"),
                "unexpected error message: {message}"
            ),
            Ok(value) => return Err(format!("expected an error, got a result: {value}")),
        }
        harness.shutdown()
    }
}
