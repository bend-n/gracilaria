use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::{JoinHandle, sleep, spawn};
use std::time::{Duration, Instant};

use Default::default;
use anyhow::Error;
use arc_swap::ArcSwap;
use crossbeam::channel::{
    Receiver, RecvError, SendError, Sender, unbounded,
};
use log::{debug, error};
use lsp_server::{
    Message, Notification as N, Request as Rq, Response as Re,
};
use lsp_types::notification::{
    Cancel, DidOpenTextDocument, Notification, Progress, SetTrace,
};
use lsp_types::request::{
    Initialize, Request, SemanticTokensFullRequest, WorkDoneProgressCreate,
};
use lsp_types::*;
use parking_lot::Mutex;
use serde_json::json;

pub struct Client {
    pub runtime: tokio::runtime::Runtime,

    pub tx: Sender<Message>,
    pub id: AtomicI32,
    pub initialized: Option<InitializeResult>,
    // pub pending: HashMap<i32, oneshot::Sender<Re>>,
    pub send_to: Sender<(i32, oneshot::Sender<Re>)>,
    pub ch_tx: Sender<()>,
    pub progress:
        &'static papaya::HashMap<ProgressToken, Option<WorkDoneProgress>>,
    pub not_rx: Receiver<N>,
    pub req_rx: Receiver<Rq>,
    pub semantic_tokens: (
        &'static ArcSwap<Box<[SemanticToken]>>,
        Mutex<Option<(tokio::task::JoinHandle<Result<(), Error>>, i32)>>,
    ),
}

impl Drop for Client {
    fn drop(&mut self) {
        panic!("please dont")
    }
}

impl Client {
    pub fn notify<X: Notification>(
        &self,
        y: &X::Params,
    ) -> Result<(), SendError<Message>> {
        self.tx.send(Message::Notification(N {
            method: X::METHOD.into(),
            params: serde_json::to_value(y).unwrap(),
        }))
    }

    #[must_use]
    pub fn request<X: Request>(
        &self,
        y: &X::Params,
    ) -> Result<(oneshot::Receiver<Re>, i32), SendError<Message>> {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.tx.send(Message::Request(Rq {
            id: id.into(),
            method: X::METHOD.into(),
            params: serde_json::to_value(y).unwrap(),
        }))?;
        let (tx, rx) = oneshot::channel();
        if self.initialized.is_some() {
            debug!("sent request {id}'s handler");
            self.send_to.send((id, tx)).expect("oughtnt really fail");
        }
        Ok((rx, id))
    }
    pub fn open(
        &self,
        f: &Path,
        text: String,
    ) -> Result<(), SendError<Message>> {
        self.notify::<DidOpenTextDocument>(&DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url::Url::from_file_path(f).unwrap(),
                language_id: "rust".into(),
                version: 0,
                text,
            },
        })
    }
    pub fn edit(
        &self,
        f: &Path,
        text: String,
    ) -> Result<(), SendError<Message>> {
        static V: AtomicI32 = AtomicI32::new(0);
        self.notify::<lsp_types::notification::DidChangeTextDocument>(
            &DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: url::Url::from_file_path(f).unwrap(),
                    version: V.fetch_add(1, Relaxed),
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text,
                }],
            },
        )
    }
    pub fn rq_semantic_tokens(&self, f: &Path) -> anyhow::Result<()> {
        debug!("requested semantic tokens");
        let mut p = self.semantic_tokens.1.lock();
        if let Some((h, task)) = &*p {
            if !h.is_finished() {
                debug!("cancelled previous semantic tokens request");
                self.notify::<Cancel>(&CancelParams { id: task.into() })?;
                h.abort();
            }
        }
        let (rx, id) = self.request::<SemanticTokensFullRequest>(
            &SemanticTokensParams {
                work_done_progress_params: default(),
                partial_result_params: default(),
                text_document: TextDocumentIdentifier::new(
                    url::Url::from_file_path(f).unwrap(),
                ),
            },
        )?;
        let d = self.semantic_tokens.0;
        let ch = self.ch_tx.clone();
        let x = self.runtime.spawn(async move {
            let x = rx.await?;
            debug!("received semantic tokens");

            let y = x.load::<SemanticTokensResult>()?;
            match y {
                SemanticTokensResult::Partial(_) =>
                    panic!("i told the lsp i dont support this"),
                SemanticTokensResult::Tokens(x) =>
                    d.store(x.data.into_boxed_slice().into()),
            };
            ch.send(())?;
            anyhow::Ok(())
        });
        *p = Some((x, id));

        Ok(())
    }
}
pub fn run(
    (tx, rx, iot): (
        Sender<Message>,
        Receiver<Message>,
        lsp_server::IoThreads,
    ),
    workspace: WorkspaceFolder,
) -> (Client, lsp_server::IoThreads, JoinHandle<()>, Receiver<()>) {
    let now = Instant::now();
    let (req_tx, req_rx) = unbounded();
    let (not_tx, not_rx) = unbounded();
    let (_req_tx, _req_rx) = unbounded();
    let (ch_tx, ch_rx) = unbounded();
    let mut c = Client {
        tx,
        req_rx: _req_rx,
        ch_tx: ch_tx.clone(),
        progress: Box::leak(Box::new(papaya::HashMap::new())),
        runtime: tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("lsp runtime")
            .build()
            .unwrap(),
        id: AtomicI32::new(0),
        initialized: None,
        semantic_tokens: (
            Box::leak(Box::new(ArcSwap::new(
                vec![].into_boxed_slice().into(),
            ))),
            Mutex::new(None),
        ),
        send_to: req_tx,
        not_rx,
    };
    c.request::<Initialize>(&InitializeParams {
        process_id: Some(std::process::id()),

        capabilities: ClientCapabilities {
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(true),
                ..default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                hover: Some(HoverClientCapabilities {
                    dynamic_registration: None,
                    content_format: Some(vec![
                        MarkupKind::PlainText,
                        MarkupKind::Markdown,
                    ]),
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        range: Some(true),
                        full: Some(
                            lsp_types::SemanticTokensFullOptions::Bool(
                                true,
                            ),
                        ),
                    },
                    token_modifiers: [
                        "associated",
                        "attribute",
                        "callable",
                        "constant",
                        "consuming",
                        "controlFlow",
                        "crateRoot",
                        "injected",
                        "intraDocLink",
                        "library",
                        "macro",
                        "mutable",
                        "procMacro",
                        "public",
                        "reference",
                        "trait",
                        "unsafe",
                    ]
                    .map(|x| x.into())
                    .to_vec(),
                    overlapping_token_support: Some(true),
                    multiline_token_support: Some(true),
                    server_cancel_support: Some(false),
                    augments_syntax_tokens: Some(false),
                    ..default()
                }),
                ..default()
            }),
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![PositionEncodingKind::UTF8]),
                ..default()
            }),
            ..default()
        },
        client_info: Some(ClientInfo {
            name: "gracilaria".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
        initialization_options: Some(json! {{
            "cargo": {
                "buildScripts": {
                    "enable": true,
                }
            },
            "procMacro": {
                "enable": true,
            },
            "procMacro.attributes.enable": true,
            "inlayHints.closureReturnTypeHints.enable": "with_block",
            "inlayHints.closingBraceHints.minLines": 5,
            "inlayHints.closureStyle": "rust_analyzer",
            "showUnlinkedFileNotification": false,
            "inlayHints.genericParameterHints.type.enable": true,
            "inlayHints.rangeExclusiveHints.enable": true,
            "inlayHints.closureCaptureHints.enable": true,
            "inlayHints.expressionAdjustmentHints.hideOutsideUnsafe": true,
            "inlayHints.expressionAdjustmentHints.enable": "never",
            "inlayHints.expressionAdjustmentHints.mode": "prefer_prefix",
            "semanticHighlighting.punctuation.separate.macro.bang": true,
            "semanticHighlighting.punctuation.enable": true,
            "semanticHighlighting.punctuation.specialization.enable": true,
        }}),
        trace: None,
        workspace_folders: Some(vec![workspace]),

        ..default()
    })
    .unwrap();
    let x = serde_json::from_value::<InitializeResult>(
        rx.recv().unwrap().response().unwrap().result.unwrap(),
    )
    .unwrap();
    assert_eq!(
        x.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
    c.initialized = Some(x);
    c.notify::<lsp_types::notification::Initialized>(
        &InitializedParams {},
    )
    .unwrap();
    c.notify::<SetTrace>(&SetTraceParams {
        value: lsp_types::TraceValue::Verbose,
    })
    .unwrap();
    let progress = c.progress;
    log::info!("lsp took {:?} to initialize", now.elapsed());
    let h = spawn(move || {
        let mut map = HashMap::new();
        loop {
            crossbeam::select! {
                recv(req_rx) -> x => match x {
                    Ok((x, y)) => {
                        debug!("received request {x}");
                        assert!(map.insert(x, (y, Instant::now())).is_none());
                    }
                    Err(RecvError) => return,
                },
                recv(rx) -> x => match x {
                    Ok(Message::Request(rq @ Rq { method: "window/workDoneProgress/create", .. })) => {
                         match rq.load::<WorkDoneProgressCreate>() {
                            Ok((_, x)) => {
                                let g = progress.guard();
                                progress.insert(x.token, None, &g);
                            },
                            Err(lsp_server::ExtractError::MethodMismatch(..)) => {},
                            e => {
                                error!("{e:?}");
                            }
                         };
                    }
                    Ok(Message::Request(x)) => {
                        if let Err(e) = _req_tx.send(x) {
                            error!("couldnt receive request {e:?}");
                        }
                    }
                    Ok(Message::Response(x)) => {
                        if let Some((s, took)) = map.remove(&x.id.i32()) {
                            debug!("request {} took {:?}", x.id, took.elapsed());
                            match s.send(x) {
                                Ok(()) => {}
                                Err(e) => {
                                    error!(
                                        "unable to respond to {:?}",
                                        e.into_inner()
                                    );
                                }
                            }
                        } else {
                            error!("request {x:?} was dropped.")
                        }
                    }
                    Ok(Message::Notification(x @ N { method: "$/progress", .. })) => {
                        let ProgressParams {token,value:ProgressParamsValue::WorkDone(x) } = x.load::<Progress>().unwrap();
                        progress.update(token, move |_| Some(x.clone()), &progress.guard());
                        _ = ch_tx.send(());
                    }
                    Ok(Message::Notification(notification)) => {
                        debug!("rx {notification:?}");
                        not_tx
                            .send(notification)
                            .expect("why library drop this??? no drop!!");
                    }
                    Err(RecvError) => return,
                }
            }
        }
    });
    (c, iot, h, ch_rx)
}

pub fn x() {
    let mut c = Command::new("/home/os/.cargo/bin/rust-analyzer")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    log::info!("helol");

    let (c, rx, iot, ch) = run(
        lsp_server::stdio::stdio_transport(
            BufReader::new(c.stdout.take().unwrap()),
            c.stdin.take().unwrap(),
        ),
        WorkspaceFolder {
            uri: "file:///home/os/gracilaria".parse().unwrap(),
            name: "gracilaria".into(),
        },
    );
    let n = c.not_rx.clone();
    let r = c.req_rx.clone();
    let p = c.progress;

    // c.request::<SemanticTokensFullRequest>(&SemanticTokensParams {
    //     work_done_progress_params: default(),
    //     partial_result_params: default(),
    //     text_document: TextDocumentIdentifier::new(
    //         url::Url::from_file_path(Path::new(
    //             "/home/os/gracilaria/src/text.rs",
    //         ))
    //         .unwrap(),
    //     ),
    // })
    // .unwrap();
    sleep(Duration::from_secs(40));
    c.open(
        Path::new("/home/os/gracilaria/src/user.rs"),
        "fn main() {}".into(),
    )
    .unwrap();
    c.rq_semantic_tokens(Path::new("/home/os/gracilaria/src/user.rs"))
        .unwrap();

    dbg!(rx.writer.join().unwrap()).unwrap();

    spawn(|| {
        for elem in r {
            match &*elem.method {
                x if x == WorkDoneProgressCreate::METHOD => {
                    elem.load::<WorkDoneProgressCreate>().unwrap();
                }
                _ => {}
            }
        }
    });
    loop {}
    drop(c);

    //     let wait = c
    //     .request::<SemanticTokensFullRequest>(&SemanticTokensParams {
    //         work_done_progress_params: default(),
    //         partial_result_params: default(),
    //         text_document: TextDocumentIdentifier {
    //             uri: "file:///home/os/gracilaria/src/main.rs"
    //                 .parse()
    //                 .unwrap(),
    //         },
    //     })
    //     .unwrap();
    // spawn(|| {
    //     let x = wait.recv_eepy().unwrap();
    //     println!(
    //         "found! {:#?}",
    //         x.extract::<SemanticTokensResult>().unwrap()
    //     );
    // });
}

trait RecvEepy<T>: Sized {
    fn recv_eepy(self) -> Result<T, RecvError> {
        self.recv_sleepy(100)
    }
    fn recv_sleepy(self, x: u64) -> Result<T, RecvError>;
}

impl<T> RecvEepy<T> for oneshot::Receiver<T> {
    fn recv_sleepy(self, x: u64) -> Result<T, RecvError> {
        loop {
            return match self.recv_timeout(Duration::from_millis(x)) {
                Err(oneshot::RecvTimeoutError::Timeout) => continue,
                Ok(x) => Ok(x),
                Err(oneshot::RecvTimeoutError::Disconnected) =>
                    Err(crossbeam::channel::RecvError),
            };
        }
    }
}

trait Void<T> {
    fn void(self) -> Result<T, ()>;
}
impl<T, E> Void<T> for Result<T, E> {
    fn void(self) -> Result<T, ()> {
        self.map_err(|_| ())
    }
}

#[test]
fn x22() {
    let (tx, rx) = std::sync::mpmc::channel::<u8>();

    let rx2 = rx.clone();
    spawn(move || {
        loop {
            println!("t1 {}", rx.recv().unwrap());
        }
    });
    spawn(move || {
        loop {
            println!("t2 {}", rx2.recv().unwrap());
        }
    });
    spawn(move || {
        for n in 0..20 {
            tx.send(n).unwrap();
        }
    });
    loop {}
}

#[test]
fn x33() {
    let y = serde_json::to_string(&SemanticTokensParams {
        work_done_progress_params: default(),
        partial_result_params: default(),
        text_document: TextDocumentIdentifier::new(
            url::Url::from_file_path(Path::new(
                "/home/os/gracilaria/src/text.rs",
            ))
            .unwrap(),
        ),
    })
    .unwrap();
    println!("{y}");
    let y = serde_json::from_str::<SemanticTokensParams>(&y).unwrap();
}
