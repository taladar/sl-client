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
        /// Spin up a server on an in-memory connection with an empty library and
        /// complete the LSP handshake.
        fn start() -> Result<Self, String> {
            Self::start_with(LslSyntax::default())
        }

        /// Spin up a server on an in-memory connection carrying the grid library
        /// `syntax` and complete the LSP `initialize`/`initialized` handshake,
        /// returning a ready harness. The grid library is what the
        /// language-intelligence handlers (hover, completion, the lints) read.
        fn start_with(syntax: LslSyntax) -> Result<Self, String> {
            let (server_conn, client_conn) = Connection::memory();
            let server = std::thread::spawn(move || sl_lsl_lsp::run(server_conn, syntax));
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

        /// Block until the server sends a notification with `method`, returning
        /// its params (used to receive a pushed `publishDiagnostics`).
        fn recv_notification(&self, method: &str) -> Result<JsonValue, String> {
            loop {
                let message = self.client.receiver.recv().map_err(|err| err.to_string())?;
                if let Message::Notification(Notification {
                    method: got,
                    params,
                }) = message
                    && got == method
                {
                    return Ok(params);
                }
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

    /// A grid library with `llSay(integer, string)`, a deprecated `llSleep` that
    /// costs 0.2s, and the `state_entry` event — enough to drive diagnostics,
    /// hover, completion and inlay hints end to end.
    fn test_library() -> LslSyntax {
        use sl_lsl::ast::TypeName;
        use sl_lsl::{LslArgument, LslEvent, LslFunction};
        let mut syntax = LslSyntax::default();
        let _prev = syntax.functions.insert(
            "llSay".to_owned(),
            LslFunction {
                arguments: vec![
                    LslArgument {
                        name: "channel".to_owned(),
                        arg_type: Some(TypeName::Integer),
                        tooltip: None,
                    },
                    LslArgument {
                        name: "msg".to_owned(),
                        arg_type: Some(TypeName::String),
                        tooltip: None,
                    },
                ],
                tooltip: Some("Says text.".to_owned()),
                ..LslFunction::default()
            },
        );
        let _prev = syntax.functions.insert(
            "llSleep".to_owned(),
            LslFunction {
                arguments: vec![LslArgument {
                    name: "sec".to_owned(),
                    arg_type: Some(TypeName::Float),
                    tooltip: None,
                }],
                sleep: Some(0.2),
                deprecated: true,
                ..LslFunction::default()
            },
        );
        let _prev = syntax
            .events
            .insert("state_entry".to_owned(), LslEvent::default());
        syntax
    }

    /// The `initialize` result advertises the language-intelligence capabilities.
    #[test]
    fn initialize_advertises_capabilities() -> Result<(), String> {
        let (server_conn, client_conn) = Connection::memory();
        let server = std::thread::spawn(move || sl_lsl_lsp::run(server_conn, LslSyntax::default()));
        client_conn
            .sender
            .send(Message::Request(Request {
                id: RequestId::from(0),
                method: "initialize".to_owned(),
                params: json!({ "capabilities": {} }),
            }))
            .map_err(|err| err.to_string())?;
        let result = loop {
            let message = client_conn.receiver.recv().map_err(|err| err.to_string())?;
            if let Message::Response(Response { response_kind, .. }) = message {
                match response_kind {
                    ResponseKind::Ok { result } => break result,
                    ResponseKind::Err { error } => return Err(error.message),
                }
            }
        };
        let caps = result
            .get("capabilities")
            .ok_or("no capabilities in initialize result")?;
        for provider in [
            "definitionProvider",
            "referencesProvider",
            "renameProvider",
            "hoverProvider",
            "completionProvider",
            "signatureHelpProvider",
            "inlayHintProvider",
            "documentHighlightProvider",
        ] {
            assert!(
                caps.get(provider).is_some(),
                "missing capability {provider}: {caps}"
            );
        }
        // Finish the handshake and shut the server down cleanly.
        client_conn
            .sender
            .send(Message::Notification(Notification {
                method: "initialized".to_owned(),
                params: json!({}),
            }))
            .map_err(|err| err.to_string())?;
        client_conn
            .sender
            .send(Message::Request(Request {
                id: RequestId::from(1),
                method: "shutdown".to_owned(),
                params: JsonValue::Null,
            }))
            .map_err(|err| err.to_string())?;
        loop {
            let message = client_conn.receiver.recv().map_err(|err| err.to_string())?;
            if matches!(message, Message::Response(_)) {
                break;
            }
        }
        client_conn
            .sender
            .send(Message::Notification(Notification {
                method: "exit".to_owned(),
                params: JsonValue::Null,
            }))
            .map_err(|err| err.to_string())?;
        server
            .join()
            .map_err(|_err| "server thread panicked".to_owned())?
            .map_err(|err| err.to_string())
    }

    /// Opening a document with a mistake pushes a `publishDiagnostics` naming it.
    #[test]
    fn diagnostics_pushed_on_open() -> Result<(), String> {
        let harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        // `llSay` called with one argument instead of two.
        harness.did_open(uri, 1, "default { state_entry() { llSay(0); } }\n")?;
        let params = harness.recv_notification("textDocument/publishDiagnostics")?;
        let diagnostics = params
            .get("diagnostics")
            .and_then(JsonValue::as_array)
            .ok_or("no diagnostics array")?;
        assert!(
            diagnostics.iter().any(|d| d
                .get("message")
                .and_then(JsonValue::as_str)
                .is_some_and(|m| m.contains("llSay") && m.contains("argument"))),
            "expected an arity diagnostic, got {diagnostics:?}"
        );
        harness.shutdown()
    }

    /// Go-to-definition on a use of a global returns the declaration location.
    #[test]
    fn definition_returns_declaration() -> Result<(), String> {
        let mut harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        harness.did_open(
            uri,
            1,
            "integer counter;\ndefault { state_entry() { counter = 1; } }\n",
        )?;
        let result = harness.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                // The `counter` use on line 1.
                "position": { "line": 1, "character": 26 },
            }),
        )?;
        // A single Location (Scalar): its range starts on line 0.
        let line = result
            .get("range")
            .and_then(|r| r.get("start"))
            .and_then(|s| s.get("line"))
            .and_then(JsonValue::as_i64)
            .ok_or_else(|| format!("no location line in {result}"))?;
        assert_eq!(line, 0);
        harness.shutdown()
    }

    /// Hover on a library call returns Markdown with its signature.
    #[test]
    fn hover_returns_signature() -> Result<(), String> {
        let mut harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        let source = "default { state_entry() { llSay(0, \"hi\"); } }\n";
        harness.did_open(uri, 1, source)?;
        let column = u32::try_from(source.find("llSay").ok_or("no llSay")?)
            .map_err(|err| err.to_string())?;
        let result = harness.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": column },
            }),
        )?;
        let value = result
            .get("contents")
            .and_then(|c| c.get("value"))
            .and_then(JsonValue::as_str)
            .ok_or_else(|| format!("no hover markup in {result}"))?;
        assert!(
            value.contains("llSay(integer channel, string msg)"),
            "{value}"
        );
        harness.shutdown()
    }

    /// Completion inside a state block offers the grid's event names.
    #[test]
    fn completion_offers_events_in_state() -> Result<(), String> {
        let mut harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        harness.did_open(uri, 1, "default {\n\n}\n")?;
        let result = harness.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": 0 },
            }),
        )?;
        let items = result.as_array().ok_or("completion is not an array")?;
        assert!(
            items
                .iter()
                .any(|i| i.get("label").and_then(JsonValue::as_str) == Some("state_entry")),
            "expected state_entry completion, got {items:?}"
        );
        harness.shutdown()
    }

    /// Rename rewrites every occurrence of a user symbol; a library symbol is
    /// refused.
    #[test]
    fn rename_edits_and_refuses_library() -> Result<(), String> {
        let mut harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        let source = "integer counter;\ndefault { state_entry() { counter = counter + 1; llSay(0, \"\"); } }\n";
        harness.did_open(uri, 1, source)?;
        // Rename the global `counter` at its declaration (line 0, column 8).
        let edit = harness.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 8 },
                "newName": "total",
            }),
        )?;
        let edits = edit
            .get("changes")
            .and_then(|c| c.get(uri))
            .and_then(JsonValue::as_array)
            .ok_or_else(|| format!("no edits for {uri} in {edit}"))?;
        // Declaration plus two uses.
        assert_eq!(edits.len(), 3);

        // Renaming the library call `llSay` is refused with an error. Find its
        // column on line 1.
        let say_col = u32::try_from(
            source
                .lines()
                .nth(1)
                .and_then(|line| line.find("llSay"))
                .ok_or("no llSay on line 1")?,
        )
        .map_err(|err| err.to_string())?;
        let library_refusal = harness.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 1, "character": say_col },
                "newName": "myFn",
            }),
        );
        match library_refusal {
            Err(message) => assert!(message.contains("library"), "unexpected: {message}"),
            Ok(value) => return Err(format!("expected refusal, got {value}")),
        }
        harness.shutdown()
    }

    /// Inlay hints surface the sleep cost of a costly library call.
    #[test]
    fn inlay_hints_show_cost() -> Result<(), String> {
        let mut harness = Harness::start_with(test_library())?;
        let uri = "file:///test.lsl";
        harness.did_open(uri, 1, "default { state_entry() { llSleep(1.0); } }\n")?;
        let result = harness.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 1, "character": 0 },
                },
            }),
        )?;
        let hints = result.as_array().ok_or("inlay hints not an array")?;
        assert!(
            hints.iter().any(|h| h
                .get("label")
                .and_then(JsonValue::as_str)
                .is_some_and(|l| l.contains("sleep"))),
            "expected a sleep-cost hint, got {hints:?}"
        );
        harness.shutdown()
    }
}
