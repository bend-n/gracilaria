use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use lsp_types::request::*;
use lsp_types::*;
use tokio::sync::oneshot::Sender;

use crate::bar::Bar;
use crate::hov::Hovr;
use crate::lsp::{Client, RequestError, Rq, RqS};
use crate::text::TextArea;
use crate::{ClickHistory, CompletionState, Hist, State};
#[derive(Default)]
pub struct Editor {
    text: TextArea,
    origin: Option<PathBuf>,
    state: State,
    bar: Bar,
    workspace: Option<PathBuf>,
    lsp: Option<(
        &'static Client,
        JoinHandle<()>,
        Sender<Arc<winit::window::Window>>,
    )>,
    tree: Option<Vec<PathBuf>>,
    hovering: Rq<Hovr, Option<Hovr>, (usize, usize), anyhow::Error>,
    document_highlights: Rq<
        Vec<DocumentHighlight>,
        Vec<DocumentHighlight>,
        (),
        RequestError<DocumentHighlightRequest>,
    >,
    complete: CompletionState,
    sig_help: Rq<
        (SignatureHelp, usize, Option<usize>),
        Option<SignatureHelp>,
        (),
        RequestError<SignatureHelpRequest>,
    >, // vo, lines
    semantic_tokens: Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    diag: Rq<String, Option<String>, (), anyhow::Error>,
    inlay: Rq<
        Vec<InlayHint>,
        Vec<InlayHint>,
        (),
        RequestError<lsp_request!("textDocument/inlayHint")>,
    >,
    def: Rq<
        LocationLink,
        Option<GotoDefinitionResponse>,
        (usize, usize),
        RequestError<lsp_request!("textDocument/definition")>,
    >,
    chist: ClickHistory,
    hist: Hist,
    mtime: Option<std::time::SystemTime>,
}

impl Editor {}
