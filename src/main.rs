#![feature(
    unsized_const_params,
    exact_div,
    yeet_expr,
    const_array,
    const_closures,
    adt_const_params,
    inherent_associated_types,
    never_type,
    random,
    btree_set_entry,
    associated_type_defaults,
    array_try_map,
    unboxed_closures,
    decl_macro,
    duration_millis_float,
    anonymous_lifetime_in_impl_trait,
    try_blocks_heterogeneous,
    current_thread_id,
    vec_try_remove,
    // lazy_type_alias,
    thread_local,
    iter_intersperse,
    stmt_expr_attributes,
    iter_collect_into,
    super_let,
    gen_blocks,
    coroutines,
    iter_from_coroutine,
    coroutine_trait,
    import_trait_associated_functions,
    deref_patterns,
    generic_const_exprs,
    const_trait_impl,
    try_blocks
)]
#![allow(
    incomplete_features,
    irrefutable_let_patterns,
    static_mut_refs,
    unexpected_cfgs
)]
mod act;
mod edi;
mod error;
mod git;
mod gotolist;
mod killring;
mod meta;
mod rnd;
#[cfg(target_family = "unix")]
mod runnables;
mod sym;
#[cfg(target_family = "unix")]
mod trm;

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::mem::MaybeUninit;
use std::num::NonZeroU32;
use std::sync::LazyLock;

use Default::default;
use NamedKey::*;
use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use libc::{atexit, signal};
use lsp::Rq;
use lsp_types::*;
use rust_fsm::StateMachine;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use swash::FontRef;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    ButtonSource, ElementState, Ime, MouseButton, MouseScrollDelta,
    WindowEvent,
};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::icon::RgbaIcon;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeHint, ImeRequestData,
};

use crate::edi::Editor;
use crate::edi::lsp_mn::LSPM;
use crate::edi::st::*;
use crate::lsp::RqS;
use crate::text::{TextArea, col, is_word};
mod bar;
mod commands;
mod complete;
pub mod hov;
mod lsp;
pub mod menu;
mod sig;
mod sni;
mod text;
mod winit_app;
fn main() {
    let _x = 4;
    // let x = HashMap::new();
    unsafe { std::env::set_var("CARGO_UNSTABLE_RUSTC_UNICODE", "true") };
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_env("GRA_LOG")
        .init();
    // lsp::x();
    entry(EventLoop::new().unwrap())
}
static mut MODIFIERS: ModifiersState = ModifiersState::empty();
static mut CLICKING: bool = false;

const BG: [u8; 3] = col!("#1f2430");
const FG: [u8; 3] = [204, 202, 194];
const BORDER: [u8; 3] = col!("#ffffff");
type KillRing = Vec<Box<[String]>>;
static mut __KR: MaybeUninit<KillRing> = MaybeUninit::uninit();
static mut __ED: MaybeUninit<Editor> = MaybeUninit::uninit();
static mut __FREQ: MaybeUninit<Freq> = MaybeUninit::uninit();
static mut __CLEAN: bool = false;
extern "C" fn cleanup() {
    unsafe {
        if __CLEAN == false {
            __CLEAN = true;

            match __ED
                .assume_init_read()
                .store(__FREQ.assume_init_mut(), __KR.assume_init_mut())
            {
                Ok(_) => {}
                Err(e) => eprintln!("{e}"),
            };
        }
    }
}
extern "C" fn sigint(_: i32) {
    cleanup();
    std::process::exit(12);
}
type FID = u8;
type Freq = FxHashMap<FID, FxHashMap<u64, u16>>;
pub(crate) fn entry(event_loop: EventLoop) {
    let mut lsp_mn = LSPM { four: default() };

    unsafe {
        let (ed, freq, kr) = match Editor::new(&mut lsp_mn) {
            Err(e) => {
                eprintln!("failure to launch: {e}");
                return;
            }
            Ok(x) => x,
        };
        __ED.write(ed);
        __FREQ.write(freq);
        __KR.write(kr);
    };
    assert_eq!(unsafe { atexit(cleanup) }, 0);
    unsafe { signal(libc::SIGINT, sigint as *const () as usize) };
    let ed: &'static mut Editor = unsafe { __ED.assume_init_mut() };
    let freq = unsafe { __FREQ.assume_init_mut() };
    let kr = unsafe { __KR.assume_init_mut() };
    let ppem = 18.0;
    let ls = 20.0;
    // let ed = Box::leak(Box::new(ed));
    let mut fonts = dsb::Fonts::new(
        F::FontRef(*FONT, &[]),
        F::FontRef(*BFONT, &[]),
        F::FontRef(*IFONT, &[]),
        F::FontRef(*BIFONT, &[]),
    );

    let mut cursor_position = (0, 0);
    let mut i = Image::build(1, 1).fill(BG);
    let mut cells = vec![];
    let mut w = match &mut ed.lsp {
        Some((.., c)) => c.take(),
        None => None,
    };
    let (_fw, _fh) = dsb::dims(&fonts.bold, ls);
    let title = ed.title();
    let app = winit_app::WinitAppBuilder::with_init(
        move |elwt| {
            let window = winit_app::make_window(elwt, |x| {
                #[cfg(target_family = "unix")]
                let x = x.with_platform_attributes(
                Box::new(winit::platform::wayland::WindowAttributesWayland::default().with_name("com.bendn.gracilaria", "com.bendn.gracilaria"))
                    ).with_decorations(false);
                x.with_title("gracilaria")
                    // .with_name("com.bendn.gracilaria", "")
                    // .with_resize_increments(PhysicalSize::new(fw, fh))
                    .with_window_icon(Some(
                        RgbaIcon::new(
                            include_bytes!("../dist/icon-32").to_vec(),
                            32,
                            32,
                        )
                        .unwrap().into()
                    ))
            });

            if let Some(x) = w.take() {
                x.send(window.clone()).unwrap();
            }
            let w_ = window.clone();
            title.as_deref().map(move |x| w_.set_title(x));
            window.request_redraw();
            window.request_ime_update(winit::window::ImeRequest::Enable(
                ImeEnableRequest::new(
                    ImeCapabilities::default()
                        .with_hint_and_purpose()
                        .with_cursor_area(),
                        // .with_surrounding_text(),
                    ImeRequestData::default().with_hint_and_purpose(
                        ImeHint::NONE,
                        winit::window::ImePurpose::Terminal,
                    ).with_cursor_area(winit::dpi::Position::Physical(PhysicalPosition::new(0, 0)), winit::dpi::Size::Physical(PhysicalSize::new(0,0))),
                )
                .unwrap(),
            )).unwrap();
            // window.set_ime_purpose(winit::window::ImePurpose::Terminal);
            let context =
                softbuffer::Context::new(window.clone()).unwrap();

            (window, context)
        },
        |_elwt, (window, context)| {
            softbuffer::Surface::new(context, window.clone()).unwrap()
        },
    )
    .with_event_handler(
        move |(window, _context), surface, window_id, event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);
            if let Some((.., c)) = &mut ed.lsp && let Some(c) = c.take() {
                c.send(window.clone()).unwrap();
            }
            let lsp_mn = &mut lsp_mn;
            let (fw, fh) = dsb::dims(&FONT, ppem);
            let (c, r) = dsb::fit(
                &FONT,
                ppem,
                ls,
                (
                    window.surface_size().width as _,
                    window.surface_size().height as _,
                ),
            );
            if let t = Editor::modify(ed.origin.as_deref())
                && t != ed.mtime
            {
                ed.mtime = t;
                ed.state.consume(Action::Changed).unwrap();
                window.request_redraw();
            }
            // let before = ed.state.name();
            ed.poll();
            // println!("{before} -> poll -> {}", ed.state.name());
            // let before = ed.state.name();
            // let ev = format!("{event:?}");
            // use WindowEvent as Event;
            match event {
                // Event::AboutToWait => {}                
                WindowEvent::SurfaceResized(size) if window_id == window.id() => {
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
                                    fg: BG,
                                    secondary_color: BG,
                                    bg: BG,
                                    flags: 0
                                },
                                letter: None,
                            };
                            r * c
                        ]
                    }
                }
                WindowEvent::Ime(Ime::Preedit(..)) => {}
                WindowEvent::Ime(Ime::Commit(x)) => {
                    ed.text.insert(&x);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested if window_id == window.id() => {
                    println!("running redraw");
                    rnd::render(
                        ed,
                        &mut cells,
                        ppem,
                        window,
                        fw,
                        fh,
                        ls,
                        c,
                        r,
                        surface,
                        cursor_position,
                        &mut fonts,
                        i.as_mut(),
                        &freq
                    );
                }

                 WindowEvent::CloseRequested => {
                    elwt.exit();
                }
                WindowEvent::PointerMoved { position, .. } => {
                    let met = FONT.metrics(&[]);
                    let fac = ppem / met.units_per_em as f32;
                    cursor_position = (
                        (position.x / (fw) as f64).round() as usize,
                        (position.y / (fh + ls * fac) as f64).floor()
                            as usize,
                    );
                    ed.cursor_moved(cursor_position, window.clone(), c);
                }
                WindowEvent::PointerButton {
                    state: bt,
                    button: ButtonSource::Mouse(button),
                    ..
                } if bt.is_pressed() => {
                    if button == MouseButton::Left {
                        unsafe { CLICKING = true };
                    }
                    ed.click(button, cursor_position, window.clone(), lsp_mn);
                    window.request_redraw();
                }
                WindowEvent::PointerButton {
                    button: ButtonSource::Mouse(MouseButton::Left),
                    ..
                } => unsafe { CLICKING = false },
                WindowEvent::MouseWheel {
                    device_id: _,
                    delta: MouseScrollDelta::LineDelta(_, rows),
                    phase: _,
                } => {
                    ed.scroll(rows);
                    window.request_redraw();
                }
                WindowEvent::MouseWheel {
                    device_id: _,
                    delta: MouseScrollDelta::PixelDelta(PhysicalPosition {  x:_, y }),
                    phase: _,
                } => {
                    ed.scroll(y as f32 *5.0);
                    window.request_redraw();
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    unsafe { MODIFIERS = modifiers.state() };
                    window.request_redraw();
                }
                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic: false,
                    ..
                } if event.state == ElementState::Pressed => {
                    // if event.logical_key == Key::Named(NamedKey::F12) {
                    //     lsp.unwrap().runtime.spawn(async move {
                    //         lsp.unwrap().symbols().await;
                    //     });
                    // }
                    if matches!(
                        event.logical_key,
                        Key::Named(Shift | Alt | Control | Meta)
                    ) {
                        return;
                    }
                    if ed.keyboard(event, window, freq, kr, lsp_mn).is_break() {
                        elwt.exit();
                    }
                    window.request_redraw();
                }
                _ => {}
            };
            // println!("{before} -> {ev} -> {}", ed.state.name());
        },
    );
    winit_app::run_app(event_loop, app);
}

fn handle(key: Key, mut text: TextArea) -> TextArea {
    edi::handle2(&key, &mut text, None);
    text
}
pub static FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("../dist/fonts/CascadiaCodeNF.ttf")[..],
        0,
    )
    .unwrap()
});
pub static IFONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("../dist/fonts/CascadiaCodeNFItalic.ttf")[..],
        0,
    )
    .unwrap()
});

pub static BIFONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("../dist/fonts/CascadiaCodeNFBoldItalic.ttf")[..],
        0,
    )
    .unwrap()
});

pub static BFONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::from_index(
        &include_bytes!("../dist/fonts/CascadiaCodeNFBold.ttf")[..],
        0,
    )
    .unwrap()
});
fn shift() -> bool {
    unsafe { MODIFIERS }.shift_key()
}
fn alt() -> bool {
    unsafe { MODIFIERS }.alt_key()
}
fn ctrl() -> bool {
    unsafe { MODIFIERS }.control_key()
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum InputRequest {
    SaveFile,
    OpenFile,
    Search,
    RenameSymbol,
}

impl InputRequest {
    fn prompt(self) -> &'static str {
        match self {
            InputRequest::SaveFile => "write to file: ",
            InputRequest::OpenFile => "open file: ",
            InputRequest::Search => "search: ",
            InputRequest::RenameSymbol => "rename symbol: ",
        }
    }
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
// #[rustc_x]
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
    let mut h = text::hist::Hist {
        history: vec![],
        redo_history: vec![],
        last: default(),
        lc: default(),
        last_edit: std::time::Instant::now(),
        changed: false,
    };
    t.insert("echo");
    h.push(&mut t);
    t.insert(" test");
    h.push(&mut t);
    h.undo(&mut t).unwrap();
    h.redo(&mut t).unwrap();
    h.undo(&mut t).unwrap();
    t.insert(" good");
    h.push(&mut t);
    h.undo(&mut t).unwrap();
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
    #[derive(Debug,Serialize,Deserialize)]
    pub(crate) CompletionState => #[derive(Debug)] pub(crate) CompletionAction<'i> => pub(crate) CDo
    None => Click => None,
    None => K(Key<&'i str> => Key::Character(k @ ("." | ":"))) => Complete(
        RqS<Complete, lsp_types::request::Completion, usize> => default()
    ) [Request(CompletionContext => CompletionContext {trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER, trigger_character:Some(k.to_string()) })],
    None => K(Key::Character(" ") if ctrl()) => Complete(default()) [Request(CompletionContext { trigger_kind: CompletionTriggerKind::INVOKED, trigger_character:None })],
    None => K(Key::Character(x) if x.chars().all(char::is_alphabetic)
        && !matches!(x, "w"|"a"|"s"|"d" if alt())) => Complete(default()) [Request(CompletionContext { trigger_kind: CompletionTriggerKind::INVOKED, trigger_character:None })],
    None => K(_) => _,

    // when
    Complete(Rq{ result: Some(_x),request: _y }) => K(Key::Named(NamedKey::Tab) if shift()) => _ [SelectPrevious],
    Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::Tab)) => _ [SelectNext],
    // Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::ArrowDown)) => _ [SelectNext],
    // Complete(Rq { result: Some(_x),request: _y }) => K(Key::Named(NamedKey::ArrowUp)) => _ [SelectPrevious],

    // exit cases
    Complete(_) => Click => None,
    Complete(_) => NoResult => None,
    Complete(_) => K(Key::Named(Escape|ArrowDown|ArrowUp)) => None,
    Complete(_) => K(Key::Character(x) if !x.chars().all(is_word)) => None,
    Complete(Rq { result: None, request: _y }) => K(Key::Named(NamedKey::ArrowUp | NamedKey::ArrowUp)) => None,

    Complete(Rq { result: Some(x), .. }) => K(Key::Named(NamedKey::Enter)) => None [Finish(Complete => x)],

    Complete(_x) => K(_) => _ [Request(CompletionContext { trigger_kind: CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS, trigger_character:None })],
}

use complete::Complete;
impl Default for CompletionState {
    fn default() -> Self {
        Self::None
    }
}
fn filter(text: &TextArea) -> String {
    if text
        .cursor
        .first()
        .checked_sub(1)
        .is_none_or(|x| matches!(text.rope.get_char(x), Some('.' | ':')))
    {
        "".to_string()
    } else {
        text.rope
            .slice(
                text.cursor.first().word_left_p(&text.rope)
                    ..*text.cursor.first(),
            )
            .chars()
            .collect::<String>()
    }
}

pub fn hash(x: &impl Hash) -> u64 {
    use std::hash::BuildHasher;
    rustc_hash::FxBuildHasher::default().hash_one(x)
}

pub fn serialize_debug<S: serde::Serializer, T: Debug>(
    s: &T,
    ser: S,
) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&format!("{s:?}"))
}
pub fn serialize_display<S: serde::Serializer, T: Display>(
    s: &T,
    ser: S,
) -> Result<S::Ok, S::Error> {
    ser.serialize_str(&format!("{s}"))
}
