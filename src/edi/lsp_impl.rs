use std::iter::repeat;

use Default::default;
use lsp_server::Request as LRq;
use lsp_types::request::*;
use lsp_types::*;
use serde::{Deserialize, Serialize};
use ttools::{Tupl, With};

use crate::complete::Complete;
use crate::edi::st::*;
use crate::hov::{Hoverable, Hovring};
use crate::lsp::{RequestError, Rq};
use crate::runnables::Runnables;
use crate::sym::GoTo;
use crate::{CompletionState, act, sig, sym};

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Requests {
    // pub hovering:
    // Rq<Hovr, Option<Hovr>, (usize, usize), RequestError<HoverRequest>>,
    pub document_highlights: Rq<
        Vec<DocumentHighlight>,
        Vec<DocumentHighlight>,
        (),
        RequestError<DocumentHighlightRequest>,
    >,
    pub complete: CompletionState,
    pub sig_help: Rq<
        (SignatureHelp, usize, Option<usize>),
        Option<SignatureHelp>,
        (),
        RequestError<SignatureHelpRequest>,
    >, // vo, lines
    #[serde(serialize_with = "serialize_tokens")]
    #[serde(deserialize_with = "deserialize_tokens")]
    #[serde(default)]
    // #[serde(skip)]
    pub semantic_tokens: Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    pub diag: Rq<
        String,
        Option<String>,
        (),
        RequestError<DocumentDiagnosticRequest>,
    >,
    #[serde(default)]
    pub inlay: Rq<
        Vec<InlayHint>,
        Vec<InlayHint>,
        (),
        RequestError<lsp_request!("textDocument/inlayHint")>,
    >,
    pub def: Rq<
        LocationLink,
        Option<GotoDefinitionResponse>,
        (usize, usize),
        RequestError<lsp_request!("textDocument/definition")>,
    >,
    #[serde(skip)]
    pub document_symbols: Rq<
        Option<Vec<DocumentSymbol>>,
        Option<DocumentSymbolResponse>,
        (),
        RequestError<lsp_request!("textDocument/documentSymbol")>,
    >,
    #[serde(skip)]
    pub git_diff: Rq<imara_diff::Diff, imara_diff::Diff, (), ()>,
}
pub fn deserialize_tokens<'de, D: serde::Deserializer<'de>>(
    ser: D,
) -> Result<
    Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    D::Error,
> {
    SemanticToken::deserialize_tokens_opt(ser)
        .map(|x| Rq { result: x.map(Into::into), request: None })
}
pub fn serialize_tokens<S: serde::Serializer>(
    s: &Rq<
        Box<[SemanticToken]>,
        Box<[SemanticToken]>,
        (),
        RequestError<SemanticTokensFullRequest>,
    >,
    ser: S,
) -> Result<S::Ok, S::Error> {
    SemanticToken::serialize_tokens_opt(
        &s.result.clone().map(|x| x.to_vec()),
        ser,
    )
}
impl crate::edi::Editor {
    pub fn poll(&mut self) {
        let Some((l, ..)) = self.lsp else { return };
        for rq in l.req_rx.try_iter() {
            match rq {
                LRq { method: "workspace/diagnostic/refresh", .. } => {
                    // let x = l.pull_diag(o.into(), diag.result.clone());
                    // diag.request(l.runtime.spawn(x));
                }
                rq => log::debug!("discarding request {rq:?}"),
            }
        }
        self.requests.inlay.poll(|x, p| {
            x.ok().or(p.1).inspect(|x| {
                self.text.set_inlay(x);
            })
        });
        self.requests.document_highlights.poll(|x, _| {
            x.ok().map(|mut x| {
                x.sort_unstable_by_key(|x| x.range.start);
                x
            })
        });
        self.requests.diag.poll(|x, _| x.ok().flatten());
        if let CompletionState::Complete(rq) = &mut self.requests.complete
        {
            rq.poll(|f, (c, _)| {
                f.ok().flatten().map(|x| Complete {
                    r: x,
                    start: c,
                    selection: 0,
                    vo: 0,
                })
            });
        };
        match &mut self.state {
            State::Symbols(x) => {
                x.poll(|x, (_, p)| {
                    let Some(p) = p else { unreachable!() };

                    x.ok().flatten().map(|r| sym::Symbols {
                        data: r.with(p.data.drop::<1>()),
                        selection: 0,
                        vo: 0,
                        ..p
                    })
                });
            }
            State::CodeAction(x) => {
                if x.poll(|x, _| {
                    let lems: Vec<CodeAction> = x
                        .ok()??
                        .into_iter()
                        .map(|x| match x {
                            CodeActionOrCommand::CodeAction(x) => x,
                            _ => panic!("alas we dont like these"),
                        })
                        .collect();
                    if lems.is_empty() {
                        self.bar.last_action =
                            "no code actions available".into();
                        None
                    } else {
                        self.bar.last_action =
                            format!("{} code actions", lems.len());
                        Some(act::CodeActions::new(lems))
                    }
                }) && x.result.is_none()
                {
                    self.state = State::Default;
                }
            }
            State::Runnables(x) => {
                x.poll(|x, ((), old)| {
                    Some(Runnables {
                        data: x.ok()?,
                        ..old.unwrap_or_default()
                    })
                });
            }
            State::Hovering(x) => {
                if x.poll(|x, (_, p)| {
                    Some(match p {
                        Some(mut p) if !p.of.is_empty() => {
                            p.of.extend(
                                x.ok().flatten().map(Hoverable::Lsp),
                            );
                            p
                        }
                        _ => Hovring {
                            of: vec![Hoverable::Lsp(x.ok()??)],
                            ..default()
                        },
                    })
                }) && super::lsp!(self).unwrap().redraw_now().unwrap() // im not a fan of this, but its kinda. necessary. annoyingly.
                    == ()
                    && x.result.is_none()
                {
                    self.state = State::Default;
                }
            }
            State::GoToL(z) => match &mut z.data.1 {
                Some(crate::gotolist::O::References(y)) => {
                    y.poll(|x, _| {
                        x.ok().flatten().map(|x| {
                            z.data.0 = x
                                .iter()
                                .map(GoTo::from)
                                .zip(repeat(None))
                                .collect()
                        })
                    });
                }
                Some(crate::gotolist::O::Impl(y)) => {
                    y.poll(|x, _| {
                        x.ok().map(|x| {
                            x.and_then(|x| try {
                                z.data.0 = match x {
                                    GotoDefinitionResponse::Scalar(
                                        location,
                                    ) => vec![(GoTo::from(
                                        location,
                                    ),None)],
                                    GotoDefinitionResponse::Array(
                                        locations,
                                    ) => locations
                                        .into_iter()
                                        .map(GoTo::from).zip(repeat(None))
                                        .collect(),
                                    GotoDefinitionResponse::Link(
                                        location_links,
                                    ) => location_links
                                        .into_iter()
                                        .map(|LocationLink {target_uri, target_range, .. }| {
                                            GoTo::from(
                                                Location {
                                                    uri: target_uri,
                                                    range: target_range,
                                                }
                                            )
                                        }).zip(repeat(None))
                                        .collect(),
                                };
                            });
                        })
                    });
                }
                Some(crate::gotolist::O::Bmk) => {}
                Some(crate::gotolist::O::Incoming(x)) => {
                    x.poll(|x, _| {
                        let x = x.ok()?;
                        z.data.0 = x
                            .into_iter()
                            .map(|x| {
                                let y = Some(x.from.name.clone());
                                (GoTo::from(x), y)
                            })
                            .collect();
                        Some(())
                    });
                }
                Some(crate::gotolist::O::Outgoing(x)) => {
                    x.poll(|x, _| {
                        let x = x.ok()?;
                        z.data.0 = x
                            .into_iter()
                            .map(|x| {
                                let y = Some(x.to.name.clone());
                                (GoTo::from(x), y)
                            })
                            .collect();
                        Some(())
                    });
                }

                None => {}
            },
            _ => {}
        }
        self.requests.def.poll(|x, _| {
            x.ok().flatten().and_then(|x| match &x {
                GotoDefinitionResponse::Link([x, ..]) => Some(x.clone()),
                _ => None,
            })
        });
        self.requests
            .semantic_tokens
            .poll(|x, _| x.ok().inspect(|x| self.text.set_toks(&x)));
        self.requests.sig_help.poll(|x, ((), y)| {
            x.ok().flatten().map(|x| {
                if let Some((old_sig, vo, max)) = y
                    && &sig::active(&old_sig) == &sig::active(&x)
                {
                    (x, vo, max)
                } else {
                    (x, 0, None)
                }
            })
        });
        // self.requests.hovering.poll(|x, _| x.ok().flatten());
        self.requests.git_diff.poll(|x, _| x.ok());
        self.requests.document_symbols.poll(|x, _| {
            x.ok().flatten().map(|x| match x {
                DocumentSymbolResponse::Flat(_) => None,
                DocumentSymbolResponse::Nested(x) => Some(x),
            })
        });
    }
}
