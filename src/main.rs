// this looks pretty good though
#![feature(tuple_trait, unboxed_closures, fn_traits)]
#![feature(
    stmt_expr_attributes,
    mpmc_channel,
    const_cmp,
    // generator_trait,
    gen_blocks,
    const_default,
    coroutines,iter_from_coroutine,coroutine_trait,
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
use std::convert::identity;
use std::io::BufReader;
use std::mem::forget;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, OnceLock};
use std::thread;
use std::time::Instant;

use Default::default;
use NamedKey::*;
use diff_match_patch_rs::PatchInput;
use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use lsp_types::request::HoverRequest;
use lsp_types::{
    Hover, HoverParams, Position, SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentIdentifier, TextDocumentPositionParams, WorkspaceFolder
};
use minimad::{Composite, CompositeStyle};
use parking_lot::RwLock;
use regex::Regex;
use ropey::Rope;
use rust_fsm::StateMachineImpl;
use swash::{FontRef, Instance};
use url::Url;
use winit::event::{
    ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Icon, Window};

use crate::bar::Bar;
use crate::text::{Diff, TextArea};
mod bar;
mod lsp;
mod text;
mod winit_app;
mod hov;
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
    let c = workspace.zip(origin.clone()).map(|(workspace, origin)| {
        let mut c = Command::new("rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();

        let (c, t, t2, changed) = lsp::run(
            lsp_server::stdio::stdio_transport(
                BufReader::new(c.stdout.take().unwrap()),
                c.stdin.take().unwrap(),
            ),
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
        ((c, origin), (t, t2), changed)
    });
    let (lsp, t, ch) = match c {
        Some((a,b,c)) => {
            (Some(a), Some(b), Some(c))
        },
        None => { (None, None, None) }
    }; 

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
    macro_rules! change {
        () => {
            lsp.as_ref().map(|(x, origin)| {
                x.edit(&origin, text.rope.to_string()).unwrap();
                x.rq_semantic_tokens(origin).unwrap();
            });
        };
    }
    lsp.as_ref().map(|(x, origin)| x.rq_semantic_tokens(origin).unwrap());
    let mut mtime = modify!();
    macro_rules! save {
        () => {{
            std::fs::write(
                origin.as_ref().unwrap(),
                &text.rope.to_string(),
            )
            .unwrap();
            bar.last_action = "saved".into();
            mtime = modify!();
        }};
    }
    static PUT: OnceLock<Arc<Window>> = OnceLock::new();
    let app = winit_app::WinitAppBuilder::with_init(
        move |elwt| {
            let window = winit_app::make_window(elwt, |x| {
                x.with_title("gracilaria")
                    .with_name("com.bendn.gracilaria", "")
                    .with_window_icon(Some(Icon::from_rgba(include_bytes!("../dist/icon-32").to_vec(), 32, 32).unwrap()))
            }); 
            window.set_ime_allowed(true);
            window.set_ime_purpose(winit::window::ImePurpose::Terminal);
            PUT.set(window.clone()).unwrap();
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
            match event {
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
                            FG,
                            BG,
                            (&mut cells, (c, r)),
                            (t_ox, 0),
                            x,
                            |(c, r), text, x| {
                                if let State::Search(re, j, _) = &state {
                                    re.find_iter(&text.rope.to_string())
                                        .enumerate()
                                        .for_each(|(i, m)| {
                                            for x in text.slice(
                                                (c, r),
                                                x,
                                                text.rope.byte_to_char(
                                                    m.start(),
                                                )
                                                    ..text
                                                        .rope
                                                        .byte_to_char(
                                                            m.end(),
                                                        ),
                                            ) {
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
                                 lsp.as_ref().and_then(|(x, _)| { match &x.initialized {
                                    Some(lsp_types::InitializeResult {
                                        capabilities: ServerCapabilities {
                                        semantic_tokens_provider:
                                        Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions{
                                        legend,..
                                        })),..
                                    },..
                                    }) => Some(legend), _ => None, }}.map(|leg|(x.semantic_tokens.0.load(), leg))
                                ),
                        );

                        bar.write_to(
                            BG,
                            FG,
                            (&mut cells, (c, r)),
                            r - 1,
                            origin
                                .as_ref()
                                .map(|x| x.to_str().unwrap())
                                .unwrap_or("new buffer"),
                            &state,
                            &text,
                            lsp.as_ref().map(|x| &x.0)
                        );
                        unsafe {
                            dsb::render(
                                &cells,
                                (c, r),
                                ppem,
                                BG,
                                &mut fonts,
                                ls,
                                true,
                                i.as_mut(),
                            )
                        };
                        let met = FONT.metrics(&[]);
                        let fac = ppem / met.units_per_em as f32;

                        // if x.view_o == Some(x.cells.row) || x.view_o.is_none() {
                        use fimg::OverlayAt;
                        let (fw, fh) = dsb::dims(&FONT, ppem);
                        let cursor =
                            Image::<_, 4>::build(3, (fh).ceil() as u32)
                                .fill([0xFF, 0xCC, 0x66, 255]);
                        unsafe {
                            let (x, y) = text.cursor();
                            let x = x + t_ox;

                            if (text.vo..text.vo + r).contains(&y)
                                && matches!(
                                    state,
                                    State::Default | State::Selection(_)
                                )
                            {
                                i.as_mut().overlay_at(
                                    &cursor,
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
                        Some(Do::Hover) => {
                            let hover = text.index_at(cursor_position);
                            let (x, y) =text.xy(hover);
                            let s = lsp.as_ref().map(|(c, o)| {
                                let (rx, id) = c.request::<HoverRequest>(&HoverParams {
                                    text_document_position_params: TextDocumentPositionParams { text_document: TextDocumentIdentifier::new(Url::from_file_path(o).unwrap()), position: Position {
                                        line: y as _, character: x as _,
                                    }},
                                    work_done_progress_params:default() }).unwrap();
                                    c.runtime.spawn(async {
                                        let x = rx.await?.load::<Hover>()?;
                                        // dbg!(&x);
                                        match x.contents {
                                            lsp_types::HoverContents::Scalar(marked_string) => {
                                                println!("{marked_string:?}");
                                            },
                                            lsp_types::HoverContents::Array(marked_strings) => {
                                                println!("{marked_strings:?}");

                                            },
                                            lsp_types::HoverContents::Markup(markup_content) => {
                                                println!("{}", markup_content.value);
                                                // dbg!(minimad::Text::from(&*markup_content.value));
                                            },
                                        }
                                        anyhow::Ok(())
                                    })
                                });
                                
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
                    if rows < 0.0 {
                        let rows = rows.ceil().abs() as usize;
                        text.vo = (text.vo + rows).min(text.l() - 1);
                    } else {
                        let rows = rows.floor() as usize;
                        text.vo = text.vo.saturating_sub(rows);
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
                            handle2(event.logical_key, &mut text);
                            text.scroll_to_cursor();
                            if hist.record(&text) {
                                change!();
                            }
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
                            text.rope.remove(x.clone());
                            text.cursor = x.start;
                            text.setc();
                            text.insert_(c);
                            hist.push_if_changed(&text);
                        }
                        Some(Do::Delete(x)) => {
                            hist.push_if_changed(&text);
                            text.cursor = x.start;
                            text.rope.remove(x);
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
                        Some(Do::OpenFile(x)) => {
                            origin = Some(PathBuf::from(&x));
                            text = TextArea::default();
                            text.insert(
                                &std::fs::read_to_string(x).unwrap(),
                            );
                            text.cursor = 0;
                            hist = Hist {
                                history: vec![],
                                redo_history: vec![],
                                last: text.clone(),
                                last_edit: Instant::now(),
                                changed: false,
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
    ch.map(|ch| thread::Builder::new().name("redrawer".into()).spawn(move || {
        for () in ch {
            PUT.get().map(|x| x.request_redraw());
        }
    }));
    winit_app::run_app(event_loop, app);
}

fn handle2(key: Key, text: &mut TextArea) {
    use Key::*;

    match key {
        Named(Space) => text.insert(" "),
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
        Named(Tab) => text.insert("    "),
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
        }
        _ => {}
    };
}
fn handle(key: Key, mut text: TextArea) -> TextArea {
    handle2(key, &mut text);
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
#[derive(Clone, Debug)]
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
    C(_ if false) => _ [Hover],
    C(_) => _,
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
