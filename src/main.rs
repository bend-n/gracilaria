// this looks pretty good though
#![feature(tuple_trait, unboxed_closures, fn_traits)]
#![feature(
    slice_as_array,
    str_as_str,
    lazy_type_alias,
    const_convert,
    const_result_trait_fn,
    thread_local,
    result_option_map_or_default,
    iter_intersperse,
    stmt_expr_attributes,
    new_range_api,
    iter_collect_into,
    mpmc_channel,
    const_cmp,
    super_let,
    gen_blocks,
    const_default,
    coroutines,
    iter_from_coroutine,
    coroutine_trait,
    cell_get_cloned,
    import_trait_associated_functions,
    if_let_guard,
    deref_patterns,
    generic_const_exprs,
    const_trait_impl,
    try_blocks,
    portable_simd
)]
#![allow(incomplete_features, redundant_semicolons)]
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Instant;

use Default::default;
use NamedKey::*;
use atools::prelude::AASAdd;
use diff_match_patch_rs::PatchInput;
use diff_match_patch_rs::traits::DType;
use dsb::cell::Style;
use dsb::{Cell, F, Fonts};
use fimg::pixels::Blend;
use fimg::{Image, OverlayAt};
use lsp::{PathURI, Rq};
use lsp_server::{Connection, Request as LRq};
use lsp_types::request::{HoverRequest, SignatureHelpRequest};
use lsp_types::*;
use regex::Regex;
use ropey::Rope;
use rust_fsm::StateMachine;
use swash::{FontRef, Instance};
use tokio::task::spawn_blocking;
use tokio_util::task::AbortOnDropHandle as DropH;
use url::Url;
use winit::event::{
    ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::Icon;

use crate::bar::Bar;
use crate::hov::Hovr;
use crate::lsp::{RedrawAfter, RqS};
use crate::text::{Diff, TextArea, col, color, is_word};
mod bar;
pub mod com;
pub mod hov;
mod lsp;
mod sig;
mod sni;
mod text;
mod winit_app;
fn main() {
    env_logger::init();
    // lsp::x();
    entry(EventLoop::new().unwrap())
}
#[derive(Debug)]
struct Hist {
    pub history: Vec<Diff>,
    pub redo_history: Vec<Diff>,
    pub last: TextArea,
    pub last_edit: std::time::Instant,
    pub changed: bool,
}
impl Hist {
    fn push(&mut self, x: &TextArea) {
        let d = diff_match_patch_rs::DiffMatchPatch::new();
        self.history.push(Diff {
            changes: (
                d.patch_make(PatchInput::new_text_text(
                    &x.rope.to_string(),
                    &self.last.rope.to_string(),
                ))
                .unwrap(),
                d.patch_make(PatchInput::new_text_text(
                    &self.last.rope.to_string(),
                    &x.rope.to_string(),
                ))
                .unwrap(),
            ),
            data: [
                (self.last.cursor, self.last.column, self.last.vo),
                (x.cursor, x.column, x.vo),
            ],
        });

        println!("push {}", self.history.last().unwrap());
        self.redo_history.clear();
        self.last = x.clone();
        self.last_edit = Instant::now();
        self.changed = false;
    }
    fn undo_(&mut self) -> Option<Diff> {
        self.history.pop().inspect(|x| self.redo_history.push(x.clone()))
    }
    fn redo_(&mut self) -> Option<Diff> {
        self.redo_history.pop().inspect(|x| self.history.push(x.clone()))
    }
    pub fn undo(&mut self, t: &mut TextArea) {
        self.push_if_changed(t);
        self.undo_().map(|x| {
            x.apply(t, false);
            self.last = t.clone();
        });
    }
    pub fn redo(&mut self, t: &mut TextArea) {
        self.redo_().map(|x| {
            x.apply(t, true);
            self.last = t.clone();
        });
    }
    pub fn push_if_changed(&mut self, x: &TextArea) {
        if self.changed || x.rope != self.last.rope {
            self.push(x);
        }
    }
    pub fn test_push(&mut self, x: &TextArea) {
        if self.last_edit.elapsed().as_millis() > 500 {
            self.push_if_changed(x);
        }
    }
    pub fn record(&mut self, new: &TextArea) -> bool {
        // self.test_push(x);
        if new.rope != self.last.rope {
            self.last_edit = Instant::now();
            self.changed = true;
        }
        new.rope != self.last.rope
    }
}

static mut MODIFIERS: ModifiersState = ModifiersState::empty();
static mut CLICKING: bool = false;

const BG: [u8; 3] = [31, 36, 48];
const FG: [u8; 3] = [204, 202, 194];
#[implicit_fn::implicit_fn]
pub(crate) fn entry(event_loop: EventLoop<()>) {
    let ppem = 20.0;
    let ls = 20.0;
    let mut text = TextArea::default();
    let mut origin = std::env::args()
        .nth(1)
        .and_then(|x| PathBuf::try_from(x).ok())
        .and_then(|x| x.canonicalize().ok());
    let mut fonts = dsb::Fonts::new(
        F::FontRef(*FONT, &[(2003265652, 550.0)]),
        F::instance(*FONT, *BFONT),
        F::FontRef(*IFONT, &[(2003265652, 550.0)]),
        F::instance(*IFONT, *BIFONT),
    );

    let mut cursor_position = (0, 0);

    let mut state = State::Default;
    let mut bar = Bar { last_action: String::default() };
    let mut i = Image::build(1, 1).fill(BG);
    let mut cells = vec![];
    std::env::args().nth(1).map(|x| {
        text.insert(&std::fs::read_to_string(x).unwrap());
        text.cursor = 0;
    });
    fn rooter(x: &Path) -> Option<PathBuf> {
        for f in std::fs::read_dir(&x).unwrap().filter_map(Result::ok) {
            if f.file_name() == "Cargo.toml" {
                return Some(f.path().with_file_name("").to_path_buf());
            }
        }
        x.parent().and_then(rooter)
    }
    let workspace = origin
        .as_ref()
        .and_then(|x| rooter(&x.parent().unwrap()))
        .and_then(|x| x.canonicalize().ok());
    let c = workspace.as_ref().zip(origin.clone()).map(
        |(workspace, origin)| {
            // let mut c = Command::new("rust-analyzer")
            //     .stdin(Stdio::piped())
            //     .stdout(Stdio::piped())
            //     .stderr(Stdio::inherit())
            //     .spawn()
            //     .unwrap();
            let (a, b) = Connection::memory();
            std::thread::Builder::new()
                .name("Rust Analyzer".into())
                .stack_size(1024 * 1024 * 8)
                .spawn(|| rust_analyzer::bin::run_server(b))
                .unwrap();
            let (c, t2, changed) = lsp::run(
                (a.sender, a.receiver),
                //    lsp_server::stdio::stdio_transport(
                //         BufReader::new(c.stdout.take().unwrap()),
                //         c.stdin.take().unwrap(),
                //     ),
                WorkspaceFolder {
                    uri: Url::from_file_path(&workspace).unwrap(),
                    name: workspace
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                },
            );
            c.open(&origin, std::fs::read_to_string(&origin).unwrap())
                .unwrap();
            (&*Box::leak(Box::new(c)), (t2), changed)
        },
    );
    let (lsp, _t, mut w) = match c {
        Some((a, b, c)) => (Some(a), Some(b), Some(c)),
        None => (None, None, None),
    };
    macro_rules! lsp {
        () => {
            lsp.zip(origin.as_deref())
        };
    }
    let mut hovering =
        Rq::<Hovr, Option<Hovr>, usize, anyhow::Error>::default();
    let mut complete = CompletionState::None;
    let mut sig_help = // vo, lines
        RqS::<(SignatureHelp, usize, Option<usize>), SignatureHelpRequest, ()>::default();
    let mut semantic_tokens = default();
    let mut diag =
        Rq::<String, Option<String>, (), anyhow::Error>::default();
    // let mut complete = None::<(CompletionResponse, (usize, usize))>;
    // let mut complete_ = None::<(
    //     JoinHandle<
    //         Result<
    //             Option<CompletionResponse>,
    //             tokio::sync::oneshot::error::RecvError,
    //         >,
    //     >,
    //     (usize, usize),
    // )>;
    // let mut hl_result = None;

    let mut hist = Hist {
        history: vec![],
        redo_history: vec![],
        last: text.clone(),
        last_edit: Instant::now(),
        changed: false,
    };
    macro_rules! modify {
        () => {
            origin
                .as_ref()
                .map(|x| x.metadata().unwrap().modified().unwrap())
        };
    }

    lsp!().map(|(x, origin)| {
        x.rq_semantic_tokens(&mut semantic_tokens, origin, None).unwrap()
    });
    let mut mtime: Option<std::time::SystemTime> = modify!();
    macro_rules! save {
        () => {{
            let t = text.rope.to_string();
            std::fs::write(origin.as_ref().unwrap(), &t).unwrap();
            bar.last_action = "saved".into();
            lsp!().map(|(l, o)| {
                l.notify::<lsp_notification!("textDocument/didSave")>(
                    &DidSaveTextDocumentParams {
                        text_document: o.tid(),
                        text: Some(t),
                    },
                )
                .unwrap();
            });
            mtime = modify!();
        }};
    }
    let app = winit_app::WinitAppBuilder::with_init(
        move |elwt| {
            let window = winit_app::make_window(elwt, |x| {
                x.with_title("gracilaria")
                    .with_decorations(false)
                    .with_name("com.bendn.gracilaria", "")
                    .with_window_icon(Some(Icon::from_rgba(include_bytes!("../dist/icon-32").to_vec(), 32, 32).unwrap()))
                    
            }); 
            if let Some(x) = w.take() {
                x.send(window.clone()).unwrap();
            }
            
            window.set_ime_allowed(true);
            window.set_ime_purpose(winit::window::ImePurpose::Terminal);
            let context =
                softbuffer::Context::new(window.clone()).unwrap();

            (window, context)
        },
        |_elwt, (window, context)| {
            softbuffer::Surface::new(context, window.clone()).unwrap()
        },
    )
    .with_event_handler(
        move |(window, _context), surface, event, elwt| {
                macro_rules! change {
        () => {
            lsp!().map(|(x, origin)| {
                x.edit(&origin, text.rope.to_string()).unwrap();
                x.rq_semantic_tokens(&mut semantic_tokens, origin, Some(window.clone())).unwrap();
            });
        };
    }
            elwt.set_control_flow(ControlFlow::Wait);
            let (fw, fh) = dsb::dims(&FONT, ppem);
            let (c, r) = dsb::fit(
                &FONT,
                ppem,
                ls,
                (
                    window.inner_size().width as _,
                    window.inner_size().height as _,
                ),
            );
            if modify!() != mtime {
                mtime = modify!();
                state.consume(Action::Changed).unwrap();
                window.request_redraw();
            }
            if let Some((l, o)) = lsp!() {
                for rq in l.req_rx.try_iter() {
                    match rq {                    
                        LRq { method: "workspace/diagnostic/refresh", .. } => {
                            let x = l.pull_diag(o.into(), diag.result.clone());
                            diag.request(l.runtime.spawn(x));
                        },
                        rq => 
                            log::debug!("discarding request {rq:?}"),
                    }
                }
                diag.poll(|x, _|x.ok().flatten(), &l.runtime);
                if let CompletionState::Complete(rq)= &mut complete {
                    rq.poll(|f, (c,_)| {
                        f.ok().flatten().map(|x| {Complete {r:x,start:c,selection:0,vo:0,}})
                    }, &l.runtime);
                };
                semantic_tokens.poll(|x, _| x.ok(), &l.runtime);
                sig_help.poll(|x, ((), y)| x.ok().flatten().map(|x| {                
                if let Some((old_sig, vo, max)) = y && &sig::active(&old_sig) == &sig::active(&x){
                    (x, vo, max)
                } else {
                    (x, 0, None)
                }
            }), &l.runtime);
            hovering.poll(|x, (_, p)|x.ok().flatten().or(p), &l.runtime);
            }            match event {
                Event::AboutToWait => {}
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::Resized(size),
                } if window_id == window.id() => {
                    let Some(surface) = surface else {
                        eprintln!(
                            "Resized fired before Resumed or after \
                             Suspended"
                        );
                        return;
                    };

                    if let (Some(width), Some(height)) = (
                        NonZeroU32::new(size.width),
                        NonZeroU32::new(size.height),
                    ) {
                        i = Image::build(size.width, size.height).fill(BG);
                        surface.resize(width, height).unwrap();
                        cells = vec![
                            Cell {
                                style: Style {
                                    color: BG,
                                    bg: BG,
                                    flags: 0
                                },
                                letter: None,
                            };
                            r * c
                        ]
                    }
                }
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::RedrawRequested,
                } if window_id == window.id() => {
                    let Some(surface) = surface else {
                        eprintln!(
                            "RedrawRequested fired before Resumed or \
                             after Suspended"
                        );
                        return;
                    };
                    let size = window.inner_size();

                    if size.height != 0 && size.width != 0 {
                        let now = Instant::now();
                        if c*r!=cells.len(){
                            return;
                        }
                        cells.fill(Cell {
                            style: Style { color: BG, bg: BG, flags: 0 },
                            letter: None,
                        });
                        let x = match &state {
                            State::Selection(x) => Some(x.clone()),
                            _ => None,
                        };
                        text.line_numbers(
                            (c, r - 1),
                            [67, 76, 87],
                            BG,
                            (&mut cells, (c, r)),
                            (1, 0),
                        );
                        let t_ox = text.line_number_offset() + 1;
                        text.c = c - t_ox;
                        text.r = r - 1;
                        text.write_to(
                            (&mut cells, (c, r)),
                            (t_ox, 0),
                            x,
                            |(_c, _r), text,  mut x| {
                                if let Some((lsp, p)) = lsp!() && let Some(diag) = lsp.diagnostics.get(&Url::from_file_path(p).unwrap(), &lsp.diagnostics.guard()) {
                                    for diag in diag {                                        
                                        let sev =  diag.severity.unwrap_or(DiagnosticSeverity::ERROR);
                                        let f = |cell:&mut Cell| {
                                                cell.style.bg.blend(match sev {
                                                    DiagnosticSeverity::ERROR => col!("#ff66662c"),
                                                    DiagnosticSeverity::WARNING => col!("#9469242c"),
                                                    _ => return
                                                });
                                            };
                                        if diag.range.start == diag.range.end {
                                            x.get((diag.range.start.character as _, diag.range.start.line as _)).map(f);
                                        } else {
                                            x.get_range((diag.range.start.character as _, diag.range.start.line as _), (diag.range.end.character as usize, diag.range.end.line as _))
                                                .for_each(f)
                                        }
                                        {
                                        let l = diag.range.start.line as usize;
                                        let x_ = text.rope.line(l).len_chars() + 2;
                                        x.get_range(
                                            (x_, l),
                                            (x_ + diag.message.chars().count(), l),
                                        ).zip(diag.message.chars()).for_each(|(x, c)| {
                                            *x=match sev {
                                                DiagnosticSeverity::WARNING => { Style{ bg :col!("#362f2f"),
                                                                                 color: col!("#fa973a"), flags: 0 }},
                                                DiagnosticSeverity::ERROR => {
                                                    Style { bg : col!("#342833"),
                                                    color : col!("#f26462"), flags: 0 }
                                                },
                                                _ => return
                                            }.basic(c)

                                        });
                                        }
                                    }
                                }
                                if let State::Search(re, j, _) = &state {
                                    re.find_iter(&text.rope.to_string())
                                        .enumerate()
                                        .for_each(|(i, m)| {
                                            for x in x.get_range(
                                            text.xy(text.rope.byte_to_char(m.start())), text.xy(text
                                                        .rope
                                                        .byte_to_char(
                                                            m.end(),
                                                        )))
                                             {
                                                x.style.bg = if i == *j {
                                                    [105, 83, 128]
                                                } else {
                                                    [65, 62, 83]
                                                }
                                            }
                                        });
                                }
                            },
                            origin.as_deref(),
                            semantic_tokens.result.as_deref().zip(
                                match lsp {
                                    Some(lsp::Client { initialized: Some(lsp_types::InitializeResult {
                                        capabilities: ServerCapabilities {
                                            semantic_tokens_provider:
                                            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions{
                                                legend,..
                                            })),..
                                        }, ..
                                    }), ..
                                }) => Some(legend),
                                _ => None,
                            }),
                        );

                        bar.write_to(
                            BG,
                            FG,
                            (&mut cells, (c, r)),
                            r - 1,
                            origin
                                .as_ref()
                                .map(|x| workspace.as_ref().and_then(|w| x.strip_prefix(w).ok()).unwrap_or(&x).to_str().unwrap())
                                .unwrap_or("new buffer"),
                            &state,
                            &text,
                            lsp
                        );
                        unsafe {
                            dsb::render(
                                &cells,
                                (c, r),
                                ppem,
                                &mut fonts,
                                ls,
                                true,
                                i.as_mut(),(0,0)
                        )
                        };

                        let place_around_cursor = |(_x, _y): (usize, usize), fonts: &mut Fonts,mut i:Image<&mut [u8], 3> ,c: &[Cell],columns:usize, ppem_:f32,ls_:f32, ox:f32, oy: f32, toy: f32| {
                            let met = FONT.metrics(&[]);
                            let fac = ppem / met.units_per_em as f32;
                            let position = (
                                (((_x) as f32 * fw).round() + ox) as usize,
                                (((_y) as f32 * (fh + ls * fac)).round() + oy) as usize,
                            );
                            assert!(position.0 < 8000 && position.1 < 8000);
                            let ppem = ppem_;
                            let ls = ls_;
                            let mut r = c.len()/columns;
                            let (w, h) = dsb::size(&fonts.regular, ppem, ls, (columns, r));                           
                            let is_above = position.1.checked_sub(h).is_some();
                            let top = position.1.checked_sub(h).unwrap_or(((((_y + 1) as f32) * (fh + ls * fac)).round() + toy) as usize);
                            let (_, y) = dsb::fit(&fonts.regular, ppem, ls, (window.inner_size().width as _ /* - left */,(window.inner_size().height as usize - top) ));
                            r = r.min(y);

                            let left =
                            if position.0 + w as usize > window.inner_size().width as usize {
                                window.inner_size().width as usize- w as usize
                            } else { position.0 };

                            let (w, h) = dsb::size(&fonts.regular, ppem, ls, (columns, r));
                            unsafe{ dsb::render(
                                &c,
                                (columns, 0),
                                ppem,
                                fonts,
                                ls,
                                true,
                                i.copy(),
                                (left as _, top as _)
                            )};
                            (is_above, left, top, w, h)
                        };
                        hovering.result.as_ref().map(|x| x.span.clone().map(|sp| {
                            let [(_x, _y), (_x2, _)] = text.position(sp);
                            let [_x, _x2] = [_x, _x2].add(text.line_number_offset()+1);
                            let _y = _y.wrapping_sub(text.vo);
                            if !(cursor_position.1 == _y && (_x..=_x2).contains(&cursor_position.0)) {
                                return;
                            }

                            let r = x.item.l().min(15);
                            let c = x.item.displayable(r);
                            let (_,left, top, w, h) = place_around_cursor(
                                (_x, _y),
                                &mut fonts,
                                i.as_mut(),
                                c, x.item.c,
                                18.0, 10.0, 0., 0., 0.
                            );
                            i.r#box((left .saturating_sub(1) as _, top.saturating_sub(1) as _), w as _,h as _, [0;3]);
                        }));
                        let com = match complete {
                            CompletionState::Complete(Rq{ result: Some(ref x,),..}) => {
                                let c = com::s(x, 40,&filter(&text));
                            if c.len() == 0 {
                                complete.consume(CompletionAction::NoResult).unwrap(); None
                            } else { Some(c) }},
                            _ => None,
                        };
                        'out: {if let Rq{result: Some((ref x, vo, ref mut max)), .. } = sig_help {
                            let (sig, p) = sig::active(x);
                            let c = sig::sig((sig, p), 40);
                            let (_x, _y) = text.cursor();
                            let _x = _x + text.line_number_offset()+1;
                            let Some(_y) = _y.checked_sub(text.vo) else { break 'out };
                            let (is_above,left, top, w, mut h) = place_around_cursor((_x, _y), &mut fonts, i.as_mut(), &c, 40, ppem, ls, 0., 0., 0.);
                                i.r#box((left .saturating_sub(1) as _, top.saturating_sub(1) as _), w as _,h as _, [0;3]);
                            
                            let com = com.map(|c| {
                                let (is_above_,left, top, w_, h_) = place_around_cursor(
                                    (_x, _y),
                                    &mut fonts,
                                    i.as_mut(),
                                    &c, 40, ppem, ls, 0., -(h as f32), if is_above { 0.0 } else { h as f32 }
                                );
                                i.r#box((left .saturating_sub(1) as _, top.saturating_sub(1) as _), w_ as _,h_ as _, [0;3]);
                                if is_above { // completion below, we need to push the docs, if any, below only below us, if the sig help is still above.
                                    h = h_;
                                } else {
                                    h+=h_;
                                }
                                (is_above_, left, top, w_, h_)
                            });
                            {
                            let ppem = 15.0;
                            let ls = 10.0;
                            let (fw, _) = dsb::dims(&FONT, ppem);
                            let cols = (w as f32 / fw).floor() as usize;
                            sig::doc(sig, cols) .map(|mut cells| {
                                *max = Some(cells.l());
                                cells.vo = vo;
                                let cells = cells.displayable(cells.l().min(15));
                                let (_,left_, top_, _w_, h_) = place_around_cursor((_x, _y),
                                    &mut fonts, i.as_mut(), cells, cols, ppem, ls,
                                    0., -(h as f32), if is_above { com.filter(|x| !x.0).map(|(_is, _l, _t, _w, h)| h).unwrap_or_default() as f32 } else { h as f32 });
                                i.r#box((left_.saturating_sub(1) as _, top_.saturating_sub(1) as _), w as _,h_ as _, [0;3]);
                            });
                            }
                        } else if let Some(c) = com {
                            let ppem = 20.0;
                            let (_x, _y) = text.cursor();
                            let _x = _x + text.line_number_offset()+1;
                            let _y = _y.wrapping_sub(text.vo);
                            let (_,left, top, w, h) = place_around_cursor(
                                (_x, _y),
                                &mut fonts,
                                i.as_mut(),
                                &c, 40, ppem, ls, 0., 0., 0.
                            );
                            i.r#box((left .saturating_sub(1) as _, top.saturating_sub(1) as _), w as _,h as _, [0;3]);
                        }
                        }
                        let met = FONT.metrics(&[]);
                        let fac = ppem / met.units_per_em as f32;
                        // if x.view_o == Some(x.cells.row) || x.view_o.is_none() {
                        let (fw, fh) = dsb::dims(&FONT, ppem);
                        let cursor =
                            Image::<_, 4>::build(3, (fh).ceil() as u32)
                                .fill([0xFF, 0xCC, 0x66, 255]);
                        let mut draw_at = |x: usize, y:usize, w| unsafe {
                            let x = (x + t_ox).saturating_sub(text.ho)%c;

                            if (text.vo..text.vo + r).contains(&y) {
                                i.as_mut().overlay_at(
                                    w,
                                    (x as f32 * fw).floor() as u32,
                                    ((y - text.vo) as f32
                                        * (fh + ls * fac))
                                        .floor()
                                        as u32,
                                    // 4 + ((x - 1) as f32 * sz) as u32,
                                    // (x as f32 * (ppem * 1.25)) as u32 - 20,
                                );
                            }
                        };
                        let (x, y) = text.cursor();
                        let image =
                            Image::<_, 4>::build(2, (fh).ceil() as u32)
                                .fill([82,82,82, 255]);
                        for stop in text.tabstops.as_ref().into_iter().flat_map(|x|x.list()) {
                            let (x, y) = text.xy(stop.clone().r().end); 
                            draw_at(x, y, &image);
                        }
                        if matches!(
                            state,
                            State::Default | State::Selection(_)
                        )
                        {
                            draw_at(x, y, &cursor);
                        }                        
                        let buffer = surface.buffer_mut().unwrap();
                        let x = unsafe {
                            std::slice::from_raw_parts_mut(
                                buffer.as_ptr() as *mut u8,
                                buffer.len() * 4,
                            )
                            .as_chunks_unchecked_mut::<4>()
                        };
                        fimg::overlay::copy_rgb_bgr_(i.flatten(), x);
                        dbg!(now.elapsed());
                        buffer.present().unwrap();
                    }
                }

                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    window_id,
                } if window_id == window.id() => {
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. },
                    ..
                } => {
                    let met = FONT.metrics(&[]);
                    let fac = ppem / met.units_per_em as f32;
                    cursor_position = (
                        (position.x / (fw) as f64).round() as usize,
                        (position.y / (fh + ls * fac) as f64).floor()
                            as usize,
                    );
                    match state
                        .consume(Action::C(cursor_position))
                        .unwrap()
                    {
                        Some(Do::ExtendSelectionToMouse) => {
                            *state.sel() = text.extend_selection_to(
                                text.index_at(cursor_position),
                                state.sel().clone(),
                            );
                            window.request_redraw();
                        }
                        Some(Do::StartSelection) => {
                            let x = text.index_at(cursor_position);
                            hist.last.cursor = x;
                            text.cursor = x;
                            *state.sel() = x..x;
                        }
                        Some(Do::Hover) if let Some(hover) = text.raw_index_at(cursor_position) => {
                            // assert_eq!(hover, text.index_at(cursor_position));
                            let (x, y) =text.xy(hover);
                            let text = text.clone();
                        'out: {
                            let l = &mut hovering.result;
                            if let  Some(Hovr{ span: Some(span),..}) = &*l {
                            let [(_x, _y), (_x2, _)] = text.position(span.clone());
                            let [_x, _x2] = [_x, _x2].add(text.line_number_offset()+1);
                            let Some(_y) = _y.checked_sub(text.vo) else { break 'out };
                            if cursor_position.1 == _y && (_x.._x2).contains(&cursor_position.0) {
                                break 'out;
                            } else {
                                *l = None;
                                window.request_redraw();
                            }
                            if let Some((_, c)) = hovering.request && c == hover {
                                break 'out;
                            }
                        }
                        if let Some((cl, o)) = lsp!() {
                        // if !running.insert(hover) {return}
                        let (rx, _) = cl.request::<HoverRequest>(&HoverParams {
text_document_position_params: TextDocumentPositionParams { text_document: TextDocumentIdentifier::new(Url::from_file_path(o).unwrap()), position: Position {
    line: y as _, character: x as _,
}},
work_done_progress_params:default() }).unwrap();
let handle = cl.runtime.spawn(window.redraw_after(async move {
    let Some(x) = rx.await? else {return Ok(None::<Hovr>)};
    let (w, cells) = spawn_blocking(move || {
        let x = match &x.contents {
            lsp_types::HoverContents::Scalar(marked_string) => {
                match marked_string{
                    MarkedString::LanguageString(x) =>Cow::Borrowed(&*x.value),
                    MarkedString::String(x) => Cow::Borrowed(&**x),
                }
            },
            lsp_types::HoverContents::Array(marked_strings) => {
                Cow::Owned(marked_strings.iter().map(|x| match x{
                    MarkedString::LanguageString(x) => &*x.value,
                    MarkedString::String(x) => &*x,
                }).collect::<String>())
            },
            lsp_types::HoverContents::Markup(markup_content) => {
                Cow::Borrowed(&*markup_content.value)
            },
        };
        let x = hov::p(&x).unwrap();
        let m = hov::l(&x).into_iter().max().map(_+2).unwrap_or(usize::MAX).min(c-10);
        (m, hov::markdown2(m, &x))
    }).await.unwrap();
    let span = x.range.and_then(|x| {
        Some(text.l_position(x.start).ok()?..text.l_position(x.end).ok()?)
    });
    anyhow::Ok(Some( hov::Hovr { span, item: text::CellBuffer { c: w, vo: 0, cells: cells.into() }}.into()))
}));
hovering.request = (DropH::new(handle), hover).into();
}
                    }
                            
//                             lsp!().map(|(cl, o)| {
// let window = window.clone();
// });
//                                 });
                        }
                        Some(Do::Hover) => {
                            hovering.result = None;
                            window.request_redraw();
                        }
                        None => {}
                        x => unreachable!("{x:?}"),
                    }
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::MouseInput { state: bt, button, .. },
                    ..
                } if bt.is_pressed() => {
                    if button == MouseButton::Left {
                        unsafe { CLICKING = true };
                    }
                    _ = complete.consume(CompletionAction::Click).unwrap();
                    match state.consume(Action::M(button)).unwrap() {
                        Some(Do::MoveCursor) => {
                            text.cursor = text.index_at(cursor_position);
                            text.setc();
                        }
                        Some(Do::ExtendSelectionToMouse) => {
                            *state.sel() = text.extend_selection_to(
                                text.index_at(cursor_position),
                                state.sel().clone(),
                            );
                        }
                        Some(Do::StartSelection) => {
                            let x = text.index_at(cursor_position);
                            hist.last.cursor = x;
                            *state.sel() = text.extend_selection_to(
                                x,
                                text.cursor..text.cursor,
                            );
                        }
                        None => {}
                        _ => unreachable!(),
                    }
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::MouseInput {
                            button: MouseButton::Left,
                            ..
                        },
                    ..
                } => unsafe { CLICKING = false },
                Event::WindowEvent {
                    window_id: _,
                    event:
                        WindowEvent::MouseWheel {
                            device_id: _,
                            delta: MouseScrollDelta::LineDelta(_, rows),
                            phase: _,
                        },
                } => {
                    let rows = if alt() { rows * 8. } else { rows * 3. };
                    
                    let (vo, max) = lower::saturating::math! { if let Some(x)= &mut hovering.result && shift() {
                        let n = x.item.l();
                        (&mut x.item.vo, n - 15)
                    } else if let Some((_, ref mut vo, Some(max))) = sig_help.result && shift(){
                        (vo, max - 15)
                    } else { 
                        let n = text.l() - 1; (&mut text.vo, n)
                    }};
                    if rows < 0.0 { 
                        let rows = rows.ceil().abs() as usize;
                        *vo = (*vo + rows).min(max);
                    } else {
                        let rows = rows.floor() as usize;
                        *vo = vo.saturating_sub(rows);
                    }
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::ModifiersChanged(modifiers),
                    ..
                } => {
                    unsafe { MODIFIERS = modifiers.state() };
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            event,
                            is_synthetic: false,
                            ..
                        },
                    ..
                } if event.state == ElementState::Pressed => {
                    if matches!(
                        event.logical_key,
                        Key::Named(Shift | Alt | Control | Super)
                    ) {
                        return;
                    }
                    let mut o = state
                        .consume(Action::K(event.logical_key.clone()))
                        .unwrap();
                    match o {
                        Some(Do::Reinsert) =>
                            o = state
                                .consume(Action::K(
                                    event.logical_key.clone(),
                                ))
                                .unwrap(),
                        _ => {}
                    }
                    match o {
                        Some(Do::Reinsert) => panic!(),
                        Some(Do::Save) => match &origin {
                            Some(_) => {
                                state.consume(Action::Saved).unwrap();
                                save!();
                            }
                            None => {
                                state
                                    .consume(Action::RequireFilename)
                                    .unwrap();
                            }
                        },
                        Some(Do::SaveTo(x)) => {
                            origin = Some(PathBuf::try_from(x).unwrap());
                            save!();
                        }
                        Some(Do::Edit) => {
                            hist.test_push(&text);
                            let cb4 = text.cursor;
                            if let Key::Named(Enter | ArrowUp | ArrowDown | Tab) = event.logical_key && let CompletionState::Complete(..) = complete{
                            } else {
                                handle2(&event.logical_key, &mut text);
                            }
                        
                            text.scroll_to_cursor();
                            if cb4 != text.cursor && let CompletionState::Complete(Rq{ result: Some(c),.. })= &mut complete
                                && ((text.cursor < c.start) || (!is_word(text.at_())&& (text.at_() != '.' || text.at_() != ':')) ) {                                
                                complete = CompletionState::None;
                            }
                            if sig_help.running() && cb4 != text.cursor && let Some((lsp, path)) = lsp!() {
                                sig_help.request(lsp.runtime.spawn(window.redraw_after(lsp.request_sig_help(path, text.cursor()))));
                            }
                            if hist.record(&text) {
                                change!();
                            }
    lsp!().map(|(lsp, o)|{
        let window = window.clone();
        match event.logical_key.as_ref() {
            Key::Character(y)
                if let Some(x) = &lsp.initialized
                    && let Some(x) = &x.capabilities.signature_help_provider
                    && let Some(x) = &x.trigger_characters && x.contains(&y.to_string()) => {
                        sig_help.request(lsp.runtime.spawn(window.redraw_after(lsp.request_sig_help(o, text.cursor()))));
                    },
            _ => {}
        }
        match complete.consume(CompletionAction::K(event.logical_key.as_ref())).unwrap(){
            Some(CDo::Request(ctx)) => {
                let h = DropH::new(lsp.runtime.spawn(
                    window.redraw_after(lsp.request_complete(o, text.cursor(), ctx))
                ));
                let CompletionState::Complete(Rq{ request : x, result: c, }) = &mut complete else { panic!()};
                *x = Some((h,c.as_ref().map(|x|x.start).or(x.as_ref().map(|x|x.1)).unwrap_or(text.cursor)));
            }
            Some(CDo::SelectNext) => {
                let CompletionState::Complete(Rq{ result: Some(c), ..  }) = &mut complete else { panic!()};
                c.next(&filter(&text));
            }
            Some(CDo::SelectPrevious) => {
                let CompletionState::Complete(Rq{ result: Some(c), .. }) = &mut complete else { panic!()};
                c.back(&filter(&text));
            }
            Some(CDo::Finish(x)) => {
                let sel = x.sel(&filter(&text));
                let sel = lsp.runtime.block_on(lsp.resolve(sel.clone()).unwrap()).unwrap();
                let CompletionItem { text_edit: Some(CompletionTextEdit::Edit(ed)), additional_text_edits, insert_text_format, ..  } = sel else { panic!() };
                match insert_text_format {
                    Some(InsertTextFormat::SNIPPET) => 
                        text.apply_snippet(&ed).unwrap(),
                    _ => {
                        text.apply(&ed).unwrap();
                        text.cursor = text.l_position(ed.range.start).unwrap() + ed.new_text.chars().count();
                    }
                }
                for additional in additional_text_edits.into_iter().flatten() {
                    let p = text.l_position(additional.range.end).unwrap();
                    if p < text.cursor {
                        if !text.visible(p) {
                            // line added behind, not visible
                            text.vo += additional.new_text.chars().filter(|x|x.is_newline()).count();
                        }
                        text.apply(&additional).unwrap();
                        text.cursor += additional.new_text.chars().count(); // compensate
                    }
                }

                if hist.record(&text) { change!();}
                sig_help = Rq::new(lsp.runtime.spawn(window.redraw_after(lsp.request_sig_help(o, text.cursor()))));
            }
            None => {return},
        };
        
    }); 

                        }
                        Some(Do::Undo) => {
                            hist.test_push(&text);
                            hist.undo(&mut text);
                            bar.last_action = "undid".to_string();
                            change!();
                        }
                        Some(Do::Redo) => {
                            hist.test_push(&text);
                            hist.redo(&mut text);
                            bar.last_action = "redid".to_string();
                            change!();
                        }
                        Some(Do::Quit) => elwt.exit(),
                        Some(Do::StartSelection) => {
                            let Key::Named(y) = event.logical_key else {
                                panic!()
                            };
                            *state.sel() = text.extend_selection(
                                y,
                                text.cursor..text.cursor,
                            );
                        }
                        Some(Do::UpdateSelection) => {
                            let Key::Named(y) = event.logical_key else {
                                panic!()
                            };
                            *state.sel() = text
                                .extend_selection(y, state.sel().clone());
                            text.scroll_to_cursor();
                        }
                        Some(Do::Insert(x, c)) => {
                            hist.push_if_changed(&text);
                            _ = text.remove(x.clone());
                            text.cursor = x.start;
                            text.setc();
                            text.insert(&c);
                            hist.push_if_changed(&text);
                        }
                        Some(Do::Delete(x)) => {
                            hist.push_if_changed(&text);
                            text.cursor = x.start;
                            _ = text.remove(x);
                            hist.push_if_changed(&text);
                        }
                        Some(Do::Copy(x)) => {
                            clipp::copy(text.rope.slice(x).to_string());
                        }
                        Some(Do::Cut(x)) => {
                            hist.push_if_changed(&text);
                            clipp::copy(
                                text.rope.slice(x.clone()).to_string(),
                            );
                            text.rope.remove(x.clone());
                            text.cursor = x.start;
                            hist.push_if_changed(&text);
                        }
                        Some(Do::Paste) => {
                            hist.push_if_changed(&text);
                            text.insert(&clipp::paste());
                            hist.push_if_changed(&text);
                        }
                        Some(Do::OpenFile(x)) => { let _: anyhow::Result<()> = try {
                            origin = Some(PathBuf::from(&x).canonicalize()?);
                            text = TextArea::default();
                            let new = std::fs::read_to_string(x)?;
                            text.insert(&new);
                            text.cursor = 0;
                            hist = Hist {
                                history: vec![],
                                redo_history: vec![],
                                last: text.clone(),
                                last_edit: Instant::now(),
                                changed: false,
                            };
                            complete = CompletionState::None;
                            mtime = modify!();

                            lsp!().map(|(x, origin)| {
                                semantic_tokens = default();
                                x.open(&origin,new).unwrap();
                                x.rq_semantic_tokens(&mut semantic_tokens, origin, Some(window.clone())).unwrap();
                            });
                            bar.last_action = "open".to_string();
                        };
                        }
                        Some(
                            Do::MoveCursor | Do::ExtendSelectionToMouse | Do::Hover,
                        ) => {
                            unreachable!()
                        }
                        Some(Do::StartSearch(x)) => {
                            let s = Regex::new(&x).unwrap();
                            let n = s
                                .find_iter(&text.rope.to_string())
                                .enumerate()
                                .count();
                            s.clone()
                                .find_iter(&text.rope.to_string())
                                .enumerate()
                                .find(|(_, x)| x.start() > text.cursor)
                                .map(|(x, m)| {
                                    state = State::Search(s, x, n);
                                    text.cursor =
                                        text.rope.byte_to_char(m.end());
                                    text.scroll_to_cursor_centering();
                                })
                                .unwrap_or_else(|| {
                                    bar.last_action = "no matches".into()
                                });
                        }
                        Some(Do::SearchChanged) => {
                            let (re, index, _) = state.search();
                            let s = text.rope.to_string();
                            let m = re.find_iter(&s).nth(*index).unwrap();
                            text.cursor = text.rope.byte_to_char(m.end());
                            text.scroll_to_cursor_centering();
                        }
                        Some(Do::Boolean(
                            BoolRequest::ReloadFile,
                            true,
                        )) => {
                            text.rope = Rope::from_str(
                                &std::fs::read_to_string(
                                    origin.as_ref().unwrap(),
                                )
                                .unwrap(),
                            );
                            text.cursor =
                                text.cursor.min(text.rope.len_chars());
                            mtime = modify!();
                            bar.last_action = "reloaded".into();
                        }
                        Some(Do::Boolean(
                            BoolRequest::ReloadFile,
                            false,
                        )) => {}
                        None => {}
                    }
                    window.request_redraw();
                }
                _ => {}
            };
        },
    );
    winit_app::run_app(event_loop, app);
}

fn handle2<'a>(key: &'a Key, text: &mut TextArea) -> Option<&'a str> {
    use Key::*;

    match key {
        Named(Space) => text.insert(" "),
        Named(Backspace) if ctrl() => text.backspace_word(),
        Named(Backspace) => text.backspace(),
        Named(Home) if ctrl() => {
            text.cursor = 0;
            text.vo = 0;
        }
        Named(End) if ctrl() => {
            text.cursor = text.rope.len_chars();
            text.vo = text.l() - text.r;
        }
        Named(Home) => text.home(),
        Named(End) => text.end(),
        Named(Tab) => text.tab(),
        Named(Delete) => {
            text.right();
            text.backspace()
        }
        Named(ArrowLeft) if ctrl() => text.word_left(),
        Named(ArrowRight) if ctrl() => text.word_right(),
        Named(ArrowLeft) => text.left(),
        Named(ArrowRight) => text.right(),
        Named(ArrowUp) => text.up(),
        Named(ArrowDown) => text.down(),
        Named(PageDown) => text.page_down(),
        Named(PageUp) => text.page_up(),
        Named(Enter) => text.enter(),
        Character(x) => {
            text.insert(&x);
            return Some(x);
        }
        _ => {}
    };
    None
}
fn handle(key: Key, mut text: TextArea) -> TextArea {
    handle2(&key, &mut text);
    text
}
pub static FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("/home/os/CascadiaCodeNF.ttf")[..],
        0,
    )
    .unwrap()
});
pub static IFONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("/home/os/CascadiaCodeNFItalic.ttf")[..],
        0,
    )
    .unwrap()
});

pub static BIFONT: LazyLock<Instance<'static>> = LazyLock::new(|| {
    IFONT.instances().find_by_name("Bold Italic").unwrap()
});

pub static BFONT: LazyLock<Instance<'static>> =
    LazyLock::new(|| FONT.instances().find_by_name("Bold").unwrap());
fn shift() -> bool {
    unsafe { MODIFIERS }.shift_key()
}
fn alt() -> bool {
    unsafe { MODIFIERS }.alt_key()
}
fn ctrl() -> bool {
    unsafe { MODIFIERS }.control_key()
}
impl State {
    fn sel(&mut self) -> &mut Range<usize> {
        let State::Selection(x) = self else { panic!() };
        x
    }
    fn search(&mut self) -> (&mut Regex, &mut usize, &mut usize) {
        let State::Search(x, y, z) = self else { panic!() };
        (x, y, z)
    }
}

use std::ops::Range;

rust_fsm::state_machine! {
#[derive(Debug)]
pub(crate) State => Action => Do

Dead => K(Key => _) => Dead,
Default => {
    K(Key::Character(x) if x == "s" && ctrl()) => Save [Save],
    K(Key::Character(x) if x == "q" && ctrl()) => Dead [Quit],
    K(Key::Character(x) if x == "v" && ctrl()) => _ [Paste],
    K(Key::Character(x) if x == "z" && ctrl()) => _ [Undo],
    K(Key::Character(x) if x == "y" && ctrl()) => _ [Redo],
    K(Key::Character(x) if x == "f" && ctrl()) => Procure((default(), InputRequest::Search)),
    K(Key::Character(x) if x == "o" && ctrl()) => Procure((default(), InputRequest::OpenFile)),
    K(Key::Character(x) if x == "c" && ctrl()) => _,
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End) if shift()) => Selection(Range<usize> => 0..0) [StartSelection],
    M(MouseButton => MouseButton::Left if shift()) => Selection(Range<usize> => 0..0) [StartSelection],
    M(MouseButton => MouseButton::Left) => _ [MoveCursor],
    C(((usize, usize)) => .. if unsafe { CLICKING }) => Selection(0..0) [StartSelection],
    Changed => RequestBoolean(BoolRequest => BoolRequest::ReloadFile),
    C(_) => _ [Hover],
    K(_) => _ [Edit],
    M(_) => _,
},
Selection(x if shift()) => {
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End)) => Selection(x) [UpdateSelection],
    M(MouseButton => MouseButton::Left) => Selection(x) [ExtendSelectionToMouse],
}, // note: it does in fact fall through. this syntax is not an arm, merely shorthand.
Selection(x) => {
    C(_ if unsafe { CLICKING }) => _ [ExtendSelectionToMouse],
    C(_) => Selection(x),
    M(MouseButton => MouseButton::Left) => Default [MoveCursor],
    K(Key::Named(Backspace)) => Default [Delete(Range<usize> => x)],
    K(Key::Character(y) if y == "x" && ctrl()) => Default [Cut(Range<usize> => x)],
    K(Key::Character(y) if y == "c" && ctrl()) => Default [Copy(Range<usize> => x)],
    K(Key::Character(y)) => Default [Insert((Range<usize>, SmolStr) => (x, y))],
    K(_) => Default [Edit],
},
Save => {
    RequireFilename => Procure((TextArea, InputRequest) => (default(), InputRequest::SaveFile)),
    Saved => Default,
},
Procure((_, _)) => K(Key::Named(Escape)) => Default,
Procure((t, InputRequest::Search)) => K(Key::Named(Enter)) => Default [StartSearch(String => t.rope.to_string())],
Procure((t, InputRequest::SaveFile)) => K(Key::Named(Enter)) => Default [SaveTo(String => t.rope.to_string())],
Procure((t, InputRequest::OpenFile)) => K(Key::Named(Enter)) => Default [OpenFile(String => t.rope.to_string())],
Procure((t, a)) => K(k) => Procure((handle(k, t), a)),
RequestBoolean(t) => {
    K(Key::Character(x) if x == "y") => Default [Boolean((BoolRequest, bool) => (t, true))],
    K(Key::Character(x) if x == "n") => Default [Boolean((t, false))],
    K(Key::Named(Escape)) => Default [Boolean((t, false))],
    K(_) => RequestBoolean(t),
    C(_) => _,
    Changed => _,
    M(_) => _,
},
Search((x, y, m)) => {
    M(MouseButton => MouseButton::Left) => Default [MoveCursor],
    C(_) => Search((x, y, m)),
    K(Key::Named(Enter) if shift()) => Search((x, y.checked_sub(1).unwrap_or(m-1), m)) [SearchChanged],
    K(Key::Named(Enter)) => Search((Regex, usize, usize) => (x,   (y+ 1) % m, m)) [SearchChanged],
    K(_) => Default [Reinsert],
}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum InputRequest {
    SaveFile,
    OpenFile,
    Search,
}

impl InputRequest {
    fn prompt(self) -> &'static str {
        match self {
            InputRequest::SaveFile => "write to file: ",
            InputRequest::OpenFile => "open file: ",
            InputRequest::Search => "search: ",
        }
    }
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BoolRequest {
    ReloadFile,
}
impl BoolRequest {
    fn prompt(self) -> &'static str {
        match self {
            BoolRequest::ReloadFile => "file changed. reload? y/n.",
        }
    }
}

#[test]
fn history_test() {
    let mut t = TextArea::default();
    let mut h = Hist {
        history: vec![],
        redo_history: vec![],
        last: t.clone(),
        last_edit: Instant::now(),
        changed: false,
    };
    t.insert("echo");
    h.push(&t);
    t.insert(" test");
    h.push(&t);
    h.undo(&mut t);
    h.redo(&mut t);
    h.undo(&mut t);
    t.insert(" good");
    h.push(&t);
    h.undo(&mut t);
    assert_eq!(t.rope.to_string(), "echo");
}
pub trait M<T> {
    fn m(&mut self, f: impl FnOnce(T) -> T);
}
impl<T> M<T> for Option<T> {
    fn m(&mut self, f: impl FnOnce(T) -> T) {
        *self = self.take().map(f);
    }
}

rust_fsm::state_machine! {
    #[derive(Debug)]
    pub(crate) CompletionState => CompletionAction<'i> => CDo
    None => Click => None,
    None => K(Key<&'i str> => Key::Character(k @ ("." | ":"))) => Complete(
        RqS<Complete, lsp_types::request::Completion, usize> => default()
    ) [Request(CompletionContext => CompletionContext {trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER, trigger_character:Some(k.to_string()) })],
    None => K(Key::Named(NamedKey::Space) if ctrl()) => Complete(default()) [Request(CompletionContext { trigger_kind: CompletionTriggerKind::INVOKED, trigger_character:None })],
    None => K(Key::Character(x) if x.chars().all(char::is_alphabetic)) => Complete(default()) [Request(CompletionContext { trigger_kind: CompletionTriggerKind::INVOKED, trigger_character:None })],
    None => K(_) => _,

    // when
    Complete(Rq{ result: Some(_x),request: _y }) => K(Key::Named(NamedKey::Tab) if shift()) => _ [SelectPrevious],
    Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::Tab)) => _ [SelectNext],
    Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::ArrowDown)) => _ [SelectNext],
    Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::ArrowUp)) => _ [SelectPrevious],

    // exit cases
    Complete(_) => Click => None,
    Complete(_) => NoResult => None,
    Complete(_)=> K(Key::Named(Escape)) => None,
    Complete(_) => K(Key::Character(x) if !x.chars().all(is_word)) => None,

    Complete(Rq { result: Some(x), .. }) => K(Key::Named(NamedKey::Enter)) => None [Finish(Complete => x)],

    Complete(_x) => K(_) => _ [Request(CompletionContext { trigger_kind: CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS, trigger_character:None })],
}

use com::Complete;
impl Default for CompletionState {
    fn default() -> Self {
        Self::None
    }
}
fn filter(text: &TextArea) -> String {
    if matches!(text.rope.get_char(text.cursor - 1), Some('.' | ':')) {
        "".to_string()
    } else {
        text.rope
            .slice(text.word_left_p()..text.cursor)
            .chars()
            .collect::<String>()
    }
}
fn frunctinator(
    parameter1: usize,
    _parameter2: u8,
    _paramter4: u16,
) -> usize {
    lower::saturating::math! { parameter1  };
    0
}
