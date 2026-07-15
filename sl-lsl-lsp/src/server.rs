//! The **server run-loop** — the LSP state machine tying the document store and
//! the symbol extractor to an [`lsp_server::Connection`].
//!
//! The connection is deliberately abstract: [`run`] drives a `Connection` it is
//! *handed*, and `lsp-server` builds one two ways — [`Connection::stdio`] for the
//! standalone binary (the `sl-lsl-lsp` process an external editor spawns) and
//! [`Connection::memory`] for an in-process pair. The **same loop** therefore
//! runs embedded in the viewer (which holds the other end of an in-memory
//! channel) and as a subprocess, which is exactly why `lsp-server` was chosen
//! over an async framework — see the crate docs.
//!
//! The loop is synchronous and single-threaded: one message handled to
//! completion before the next. That is sound because every handler here is cheap
//! and non-blocking — a `didChange` re-parses in memory, a `documentSymbol`
//! walks the cached tree; nothing waits on the grid. When the
//! language-intelligence half adds a *save/compile* round-trip
//! (`viewer-lsl-lsp-diagnostics-nav`) that blocking work will need to move off
//! this thread, but document sync and symbols do not.
//!
//! The handshake **negotiates the position encoding** before answering
//! `initialize`: it reads the client's `general.positionEncodings`, picks one
//! (see [`PositionEncoding::negotiate`]) and both advertises it back and stores
//! it, so every span→position conversion downstream uses the unit the client
//! agreed to.

use std::collections::HashMap;

use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentHighlightRequest, DocumentSymbolRequest, GotoDefinition, HoverRequest,
    InlayHintRequest, References, Rename, Request as _, SignatureHelpRequest,
    WorkspaceSymbolRequest,
};
use lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentHighlight,
    DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InlayHint, InlayHintParams, Location, OneOf, ReferenceParams, RenameParams,
    ServerCapabilities, ServerInfo, SignatureHelp, SignatureHelpOptions, SignatureHelpParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};
use serde::de::DeserializeOwned;
use sl_lsl::LslSyntax;

use crate::document::Document;
use crate::position::PositionEncoding;
use crate::symbols::{document_symbols, workspace_symbols};
use crate::{completion, diagnostics, hover, inlay, navigate, signature};

/// The JSON-RPC error code for a request whose method the server does not
/// implement (named as a constant to avoid an `as`-cast of `lsp_server`'s
/// `ErrorCode`, which the workspace's clippy config forbids).
const METHOD_NOT_FOUND: i32 = -32601;

/// The JSON-RPC error code for a request whose parameters failed to
/// deserialise.
const INVALID_PARAMS: i32 = -32602;

/// The LSP `RequestFailed` error code — a request the server understood but
/// could not fulfil (a rename of a library symbol, an invalid new name), as
/// distinct from a malformed request.
const REQUEST_FAILED: i32 = -32803;

/// Something that went wrong driving the LSP connection: a transport/protocol
/// failure or a JSON (de)serialisation error.
#[derive(Debug, thiserror::Error)]
#[expect(
    clippy::module_name_repetitions,
    reason = "`ServerError` is the crate-root error type and reads best named for the server it \
              comes from; `server::Error` would collide with other crates' `Error` at import"
)]
pub enum ServerError {
    /// An `lsp-server` protocol or transport error (a malformed handshake, a
    /// disconnected channel).
    #[error("LSP protocol error: {0}")]
    Protocol(#[from] lsp_server::ProtocolError),
    /// A JSON (de)serialisation error building or reading a message body.
    #[error("LSP JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The outgoing channel was closed before a response could be sent.
    #[error("LSP connection closed: {0}")]
    Send(String),
}

/// The server's mutable state: the open documents, the grid's LSL library and
/// the negotiated position encoding.
///
/// The [`syntax`](Self::syntax) table is not read by the symbol handlers (an
/// outline describes the user's own code, not the library) but is held here for
/// the language-intelligence half — completion, hover and signature help
/// (`viewer-lsl-lsp-diagnostics-nav`) — and updated via [`Server::set_syntax`] when the
/// embedding client fetches a fresher `LSLSyntax` document from the grid.
#[derive(Debug)]
pub struct Server {
    /// The open documents, keyed by URI.
    documents: HashMap<Uri, Document>,
    /// The grid's LSL library, for the later language-intelligence handlers.
    syntax: LslSyntax,
    /// The position encoding negotiated with the client at initialisation.
    encoding: PositionEncoding,
}

impl Server {
    /// Build a server with the grid library `syntax` (an empty [`LslSyntax`] is
    /// fine — the symbol handlers do not read it) and the default (UTF-16)
    /// encoding, which [`run`] overrides with the negotiated one.
    #[must_use]
    pub fn new(syntax: LslSyntax) -> Self {
        Self {
            documents: HashMap::new(),
            syntax,
            encoding: PositionEncoding::default(),
        }
    }

    /// Replace the grid library table — called by an embedding client when it
    /// fetches a fresher `LSLSyntax` document from the grid.
    pub fn set_syntax(&mut self, syntax: LslSyntax) {
        self.syntax = syntax;
    }

    /// The grid library table the server currently holds.
    #[must_use]
    pub const fn syntax(&self) -> &LslSyntax {
        &self.syntax
    }

    /// The negotiated position encoding.
    #[must_use]
    pub const fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    /// The open document for `uri`, if one is open.
    #[must_use]
    pub fn document(&self, uri: &Uri) -> Option<&Document> {
        self.documents.get(uri)
    }

    /// Apply a `textDocument/didOpen`: store the freshly parsed buffer.
    fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let uri = item.uri.clone();
        let document = Document::open(item.uri, item.version, item.text);
        let _previous = self.documents.insert(uri, document);
    }

    /// Apply a `textDocument/didChange`: swap in the full new text (the server
    /// advertises full-text sync, so the last content change carries the whole
    /// buffer). A change for an unopened document is ignored.
    fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let Some(text) = params.content_changes.into_iter().last().map(|c| c.text) else {
            return;
        };
        if let Some(document) = self.documents.get_mut(&uri) {
            document.update(version, text);
        } else {
            // A change before an open should not happen, but tolerate it by
            // treating the change as an open rather than dropping the text.
            let document = Document::open(uri.clone(), version, text);
            let _previous = self.documents.insert(uri, document);
        }
    }

    /// Apply a `textDocument/didClose`: forget the buffer.
    fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        let _removed = self.documents.remove(&params.text_document.uri);
    }

    /// Answer a `textDocument/documentSymbol`: the hierarchical outline of the
    /// named buffer, or an empty outline if it is not open.
    #[must_use]
    fn document_symbol(&self, params: &DocumentSymbolParams) -> DocumentSymbolResponse {
        let symbols = self
            .documents
            .get(&params.text_document.uri)
            .map(|document| document_symbols(document, self.encoding))
            .unwrap_or_default();
        DocumentSymbolResponse::Nested(symbols)
    }

    /// Answer a `workspace/symbol`: the query-filtered top-level symbols across
    /// every open document.
    #[must_use]
    fn workspace_symbol(&self, params: &WorkspaceSymbolParams) -> WorkspaceSymbolResponse {
        let mut symbols = Vec::new();
        for document in self.documents.values() {
            symbols.extend(workspace_symbols(document, &params.query, self.encoding));
        }
        WorkspaceSymbolResponse::Flat(symbols)
    }
}

/// Drive `connection` through the LSP lifecycle to completion: negotiate and
/// answer `initialize`, then run the message loop until the client shuts the
/// server down or the transport closes. `syntax` is the grid library the server
/// starts with (may be empty).
///
/// # Errors
///
/// Returns a [`ServerError`] if the handshake fails, a message cannot be
/// (de)serialised, or the outgoing channel closes unexpectedly.
pub fn run(connection: Connection, syntax: LslSyntax) -> Result<(), ServerError> {
    let (initialize_id, initialize_value) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(initialize_value)?;
    let encoding = PositionEncoding::negotiate(
        params
            .capabilities
            .general
            .as_ref()
            .and_then(|general| general.position_encodings.as_deref()),
    );
    let result = InitializeResult {
        capabilities: server_capabilities(encoding),
        server_info: Some(ServerInfo {
            name: "sl-lsl-lsp".to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(result)?)?;

    let mut server = Server::new(syntax);
    server.encoding = encoding;
    main_loop(&mut server, &connection)
}

/// The [`ServerCapabilities`] to advertise: full-text document sync, the symbol
/// providers, the whole language-intelligence surface (definition, references,
/// document highlight, rename, hover, completion, signature help, inlay hints),
/// and the negotiated position encoding. Diagnostics are *pushed*
/// (`publishDiagnostics`) rather than declared as a provider, so they need no
/// capability flag here.
#[must_use]
fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.to_kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions::default()),
        signature_help_provider: Some(SignatureHelpOptions {
            // The parentheses opening a call and the comma between arguments both
            // re-trigger the popup so it tracks the active parameter as typed.
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: None,
            work_done_progress_options: lsp_types::WorkDoneProgressOptions::default(),
        }),
        inlay_hint_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    }
}

/// The message loop: handle each request and notification until a `shutdown`
/// request or an `exit` notification, or the receiver closes.
fn main_loop(server: &mut Server, connection: &Connection) -> Result<(), ServerError> {
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                let response = server.handle_request(request);
                connection
                    .sender
                    .send(Message::Response(response))
                    .map_err(|err| ServerError::Send(err.to_string()))?;
            }
            Message::Notification(notification) => {
                if notification.method == Exit::METHOD {
                    return Ok(());
                }
                for outgoing in server.handle_notification(notification) {
                    connection
                        .sender
                        .send(outgoing)
                        .map_err(|err| ServerError::Send(err.to_string()))?;
                }
            }
            // Responses are to requests the server itself made; this server
            // makes none, so there is nothing to correlate.
            Message::Response(_) => {}
        }
    }
    Ok(())
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the request/notification dispatch methods live next to `main_loop` that calls them, \
              deliberately separated from the state accessors above the free run-loop functions"
)]
impl Server {
    /// Dispatch one request to its handler and build the [`Response`], answering
    /// an unknown method or a malformed parameter body with a JSON-RPC error
    /// rather than leaving the client's request unanswered.
    #[must_use]
    fn handle_request(&self, request: Request) -> Response {
        let id = request.id.clone();
        match request.method.as_str() {
            DocumentSymbolRequest::METHOD => match parse_params::<DocumentSymbolParams>(request) {
                Ok(params) => Response::new_ok(id, self.document_symbol(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            WorkspaceSymbolRequest::METHOD => {
                match parse_params::<WorkspaceSymbolParams>(request) {
                    Ok(params) => Response::new_ok(id, self.workspace_symbol(&params)),
                    Err(message) => Response::new_err(id, INVALID_PARAMS, message),
                }
            }
            GotoDefinition::METHOD => match parse_params::<GotoDefinitionParams>(request) {
                Ok(params) => Response::new_ok(id, self.definition(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            References::METHOD => match parse_params::<ReferenceParams>(request) {
                Ok(params) => Response::new_ok(id, self.references(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            DocumentHighlightRequest::METHOD => {
                match parse_params::<DocumentHighlightParams>(request) {
                    Ok(params) => Response::new_ok(id, self.document_highlight(&params)),
                    Err(message) => Response::new_err(id, INVALID_PARAMS, message),
                }
            }
            HoverRequest::METHOD => match parse_params::<HoverParams>(request) {
                Ok(params) => Response::new_ok(id, self.hover(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            Completion::METHOD => match parse_params::<CompletionParams>(request) {
                Ok(params) => Response::new_ok(id, self.completion(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            SignatureHelpRequest::METHOD => match parse_params::<SignatureHelpParams>(request) {
                Ok(params) => Response::new_ok(id, self.signature_help(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            InlayHintRequest::METHOD => match parse_params::<InlayHintParams>(request) {
                Ok(params) => Response::new_ok(id, self.inlay_hints(&params)),
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            Rename::METHOD => match parse_params::<RenameParams>(request) {
                Ok(params) => match self.rename(&params) {
                    Ok(edit) => Response::new_ok(id, edit),
                    Err(message) => Response::new_err(id, REQUEST_FAILED, message),
                },
                Err(message) => Response::new_err(id, INVALID_PARAMS, message),
            },
            other => Response::new_err(
                id,
                METHOD_NOT_FOUND,
                format!("unsupported request method `{other}`"),
            ),
        }
    }

    /// Dispatch one notification to its handler and return the messages the
    /// server should send in response — a `publishDiagnostics` after a document
    /// changes, an empty diagnostics list after it closes. An unrecognised
    /// notification is ignored (the protocol requires notifications never be
    /// answered with an error), as is a malformed body — logged, not fatal.
    fn handle_notification(&mut self, notification: Notification) -> Vec<Message> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Some(params) = parse_notification::<DidOpenTextDocumentParams>(notification)
                {
                    let uri = params.text_document.uri.clone();
                    self.did_open(params);
                    return self.diagnostics_for(&uri).into_iter().collect();
                }
            }
            DidChangeTextDocument::METHOD => {
                if let Some(params) =
                    parse_notification::<DidChangeTextDocumentParams>(notification)
                {
                    let uri = params.text_document.uri.clone();
                    self.did_change(params);
                    return self.diagnostics_for(&uri).into_iter().collect();
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Some(params) = parse_notification::<DidCloseTextDocumentParams>(notification)
                {
                    let uri = params.text_document.uri.clone();
                    self.did_close(params);
                    return clear_message(&uri).into_iter().collect();
                }
            }
            _other => {}
        }
        Vec::new()
    }

    /// The go-to-definition location of the symbol at the request position, or
    /// [`None`] (serialised as JSON `null`) when the cursor is not on a user
    /// symbol or the document is not open.
    #[must_use]
    fn definition(&self, params: &GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let position = &params.text_document_position_params;
        let document = self.documents.get(&position.text_document.uri)?;
        let location =
            navigate::goto_definition(document, position.position, &self.syntax, self.encoding)?;
        Some(GotoDefinitionResponse::Scalar(location))
    }

    /// Every reference to the symbol at the request position, honouring the
    /// `includeDeclaration` flag. An empty list when the document is not open or
    /// the cursor is not on a symbol.
    #[must_use]
    fn references(&self, params: &ReferenceParams) -> Vec<Location> {
        let position = &params.text_document_position;
        let Some(document) = self.documents.get(&position.text_document.uri) else {
            return Vec::new();
        };
        navigate::references(
            document,
            position.position,
            params.context.include_declaration,
            &self.syntax,
            self.encoding,
        )
    }

    /// The document highlights for the symbol at the request position.
    #[must_use]
    fn document_highlight(&self, params: &DocumentHighlightParams) -> Vec<DocumentHighlight> {
        let position = &params.text_document_position_params;
        let Some(document) = self.documents.get(&position.text_document.uri) else {
            return Vec::new();
        };
        navigate::document_highlights(document, position.position, &self.syntax, self.encoding)
    }

    /// The hover for the symbol at the request position, or [`None`].
    #[must_use]
    fn hover(&self, params: &HoverParams) -> Option<Hover> {
        let position = &params.text_document_position_params;
        let document = self.documents.get(&position.text_document.uri)?;
        hover::hover(document, position.position, &self.syntax, self.encoding)
    }

    /// The completion items for the request position, as a flat array.
    #[must_use]
    fn completion(&self, params: &CompletionParams) -> CompletionResponse {
        let position = &params.text_document_position;
        let items = match self.documents.get(&position.text_document.uri) {
            Some(document) => {
                completion::completion(document, position.position, &self.syntax, self.encoding)
            }
            None => Vec::new(),
        };
        CompletionResponse::Array(items)
    }

    /// The signature help for the call at the request position, or [`None`].
    #[must_use]
    fn signature_help(&self, params: &SignatureHelpParams) -> Option<SignatureHelp> {
        let position = &params.text_document_position_params;
        let document = self.documents.get(&position.text_document.uri)?;
        signature::signature_help(document, position.position, &self.syntax, self.encoding)
    }

    /// The inlay hints for the requested range of the document.
    #[must_use]
    fn inlay_hints(&self, params: &InlayHintParams) -> Vec<InlayHint> {
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Vec::new();
        };
        inlay::inlay_hints(document, params.range, &self.syntax, self.encoding)
    }

    /// The workspace edit renaming the symbol at the request position, or a
    /// human-readable refusal message.
    fn rename(&self, params: &RenameParams) -> Result<lsp_types::WorkspaceEdit, String> {
        let position = &params.text_document_position;
        let document = self
            .documents
            .get(&position.text_document.uri)
            .ok_or_else(|| "document is not open".to_owned())?;
        navigate::rename(
            document,
            position.position,
            &params.new_name,
            &self.syntax,
            self.encoding,
        )
        .map_err(|err| err.to_string())
    }

    /// The `publishDiagnostics` message for the (open) document `uri`, or
    /// [`None`] if it is not open or its payload cannot be serialised.
    #[must_use]
    fn diagnostics_for(&self, uri: &Uri) -> Option<Message> {
        let document = self.documents.get(uri)?;
        let params = diagnostics::publish_params(document, &self.syntax, self.encoding);
        publish_message(params)
    }
}

/// The `publishDiagnostics` message that **clears** diagnostics for `uri` (an
/// empty list), or [`None`] if the payload cannot be serialised.
#[must_use]
fn clear_message(uri: &Uri) -> Option<Message> {
    publish_message(diagnostics::clear_params(uri))
}

/// Wrap a [`PublishDiagnosticsParams`](lsp_types::PublishDiagnosticsParams) as a
/// `textDocument/publishDiagnostics` notification message, logging and dropping
/// it if it cannot be serialised (a diagnostics push must never abort the loop).
#[must_use]
fn publish_message(params: lsp_types::PublishDiagnosticsParams) -> Option<Message> {
    match serde_json::to_value(params) {
        Ok(value) => Some(Message::Notification(Notification {
            method: PublishDiagnostics::METHOD.to_owned(),
            params: value,
        })),
        Err(err) => {
            tracing::warn!(error = %err, "dropping publishDiagnostics: serialisation failed");
            None
        }
    }
}

/// Deserialise a request's parameter body, returning a human-readable message on
/// failure (surfaced to the client as an `InvalidParams` error).
fn parse_params<P: DeserializeOwned>(request: Request) -> Result<P, String> {
    serde_json::from_value(request.params).map_err(|err| err.to_string())
}

/// Deserialise a notification's parameter body, logging and dropping it on
/// failure (a notification cannot be answered with an error).
#[must_use]
fn parse_notification<P: DeserializeOwned>(notification: Notification) -> Option<P> {
    let method = notification.method.clone();
    match serde_json::from_value(notification.params) {
        Ok(params) => Some(params),
        Err(err) => {
            tracing::warn!(%method, error = %err, "ignoring notification with malformed params");
            None
        }
    }
}
