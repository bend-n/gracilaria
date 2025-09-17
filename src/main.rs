// this looks pretty good though
#![feature(tuple_trait, unboxed_closures, fn_traits)]
#![feature(
    import_trait_associated_functions,
    guard_patterns,
    if_let_guard,
    deref_patterns,
    generic_const_exprs,
    const_trait_impl,
    try_blocks,
    portable_simd
)]
#![allow(incomplete_features, redundant_semicolons)]
use std::convert::identity;
use std::fs::File;
use std::hint::assert_unchecked;
use std::iter::zip;
use std::num::NonZeroU32;
use std::simd::prelude::*;
use std::sync::LazyLock;
use std::time::Instant;

use Default::default;
use array_chunks::*;
use atools::prelude::*;
use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use rust_fsm::StateMachineImpl;
use swash::{FontRef, Instance};
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

use crate::bar::Bar;
use crate::text::TextArea;
mod bar;
mod text;
mod winit_app;
fn main() {
    entry(EventLoop::new().unwrap())
}

static mut MODIFIERS: ModifiersState = ModifiersState::empty();

const BG: [u8; 3] = [31, 36, 48];
const FG: [u8; 3] = [204, 202, 194];
#[implicit_fn::implicit_fn]
pub(crate) fn entry(event_loop: EventLoop<()>) {
    let ppem = 20.0;
    let ls = 20.0;

    let mut text = TextArea::default();
    let mut origin = std::env::args().nth(1);
    let mut fonts = dsb::Fonts::new(
        F::instance(*FONT, *BFONT),
        *FONT,
        *IFONT,
        F::instance(*IFONT, *BIFONT),
    );
    let mut state = State::Default;
    let mut bar =
        Bar { text: TextArea::default(), last_action: String::default() };
    let mut i = Image::build(1, 1).fill(BG);
    let mut cells = vec![];
    std::env::args().nth(1).map(|x| {
        text.insert(&std::fs::read_to_string(x).unwrap());
        text.cursor = 0;
    });
    macro_rules! save {
        () => {{
            std::fs::write(
                origin.as_ref().unwrap(),
                &text.rope.to_string(),
            )
            .unwrap();
            bar.last_action = "saved".into();
        }};
    }
    let app = winit_app::WinitAppBuilder::with_init(
        |elwt| {
            let window = winit_app::make_window(elwt, identity);
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
            elwt.set_control_flow(ControlFlow::Wait);
            let (c, r) = dsb::fit(
                &FONT,
                ppem,
                ls,
                (
                    window.inner_size().width as _,
                    window.inner_size().height as _,
                ),
            );
            match event {
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
                        let t_ox = text.line_numbers(
                            (c, r - 1),
                            [67, 76, 87],
                            BG,
                            (&mut cells, (c, r)),
                            (1, 0),
                        ) + 1;
                        text.write_to(
                            (c - t_ox, r - 1),
                            FG,
                            BG,
                            (&mut cells, (c, r)),
                            (t_ox, 0),
                            x,
                        );
                        text.c = c - t_ox;
                        text.r = r - 1;
                        bar.write_to(
                            BG,
                            FG,
                            (&mut cells, (c, r)),
                            r - 1,
                            &origin.as_deref().unwrap_or("new buffer"),
                            &state,
                            &text,
                        );

                        println!("cell=");
                        dbg!(now.elapsed());
                        let now = Instant::now();
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
                        eprint!("rend=");
                        dbg!(now.elapsed());
                        let met = FONT.metrics(&[]);
                        let fac = ppem / met.units_per_em as f32;
                        let now = Instant::now();

                        // if x.view_o == Some(x.cells.row) || x.view_o.is_none() {
                        use fimg::OverlayAt;
                        let (fw, fh) = dsb::dims(&FONT, ppem);
                        let cursor =
                            Image::<_, 4>::build(3, (fh).ceil() as u32)
                                .fill([0xFF, 0xCC, 0x66, 255]);
                        unsafe {
                            let (x, y) = text.cursor();
                            let x = x + t_ox;

                            if (text.vo..text.vo + r).contains(&y) {
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
                        eprint!("conv = ");

                        // }
                        let buffer = surface.buffer_mut().unwrap();

                        fimg::overlay::copy_rgb_bgr_(
                            i.flatten(),
                            unsafe {
                                std::slice::from_raw_parts_mut(
                                    buffer.as_ptr() as *mut u8,
                                    buffer.len() * 4,
                                )
                                .as_chunks_unchecked_mut::<4>()
                            },
                        );
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
                    window_id: _,
                    event:
                        WindowEvent::MouseWheel {
                            device_id: _,
                            delta: MouseScrollDelta::LineDelta(_, rows),
                            phase: _,
                        },
                } => {
                    let rows = rows * 3.;
                    if rows < 0.0 {
                        let rows = rows.ceil().abs() as usize;
                        text.vo = (text.vo + rows).min(text.l() - 1);
                    } else {
                        let rows = rows.floor() as usize;
                        text.vo = text.vo.saturating_sub(rows);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::ModifiersChanged(modifiers),
                    ..
                } => {
                    unsafe { MODIFIERS = modifiers.state() };
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event, .. },
                    ..
                } if event.state == ElementState::Pressed => {
                    if matches!(
                        event.logical_key,
                        Key::Named(
                            NamedKey::Shift
                                | NamedKey::Alt
                                | NamedKey::Control
                        )
                    ) {
                        return;
                    }
                    let o = state
                        .consume(Action::K(event.logical_key.clone()))
                        .unwrap();
                    match o {
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
                            origin = Some(x);
                            save!();
                        }
                        Some(Do::Edit) => {
                            handle2(event.logical_key, &mut text);
                        }
                        Some(Do::Quit) => elwt.exit(),
                        Some(Do::StartSelection) => {
                            let State::Selection(x) = &mut state else {
                                panic!()
                            };
                            let Key::Named(y) = event.logical_key else {
                                panic!()
                            };
                            *x = text.extend_selection(
                                y,
                                text.cursor..text.cursor,
                            );
                        }
                        Some(Do::UpdateSelection) => {
                            let State::Selection(x) = &mut state else {
                                panic!()
                            };
                            let Key::Named(y) = event.logical_key else {
                                panic!()
                            };
                            *x = text.extend_selection(y, x.clone());
                        }
                        Some(Do::Insert((x, c))) => {
                            text.rope.remove(x);
                            text.insert_(c);
                        }
                        Some(Do::Delete(x)) => {
                            text.rope.remove(x);
                        }
                        Some(Do::Copy(x)) => {
                            clipp::copy(text.rope.slice(x).to_string());
                        }
                        Some(Do::Cut(x)) => {
                            clipp::copy(
                                text.rope.slice(x.clone()).to_string(),
                            );
                            text.rope.remove(x);
                        }
                        Some(Do::Paste) => {
                            text.insert(&clipp::paste());
                        }
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

fn handle2(key: Key, text: &mut TextArea) {
    use Key::*;
    use NamedKey::*;

    match key {
        Named(Space) => text.insert(" "),
        Named(Backspace) => text.backspace(),
        Named(ArrowLeft) => text.left(),
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
        // Named(Tab)
        Named(ArrowRight) => text.right(),
        Named(ArrowUp) => text.up(),
        Named(ArrowDown) => text.down(),
        Named(Enter) => text.enter(),
        Character(x) => {
            text.insert(&*x);
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
fn ctrl() -> bool {
    unsafe { MODIFIERS }.control_key()
}

// use NamedKey::Arrow
use std::ops::Range;
rust_fsm::state_machine! {
#[derive(Clone, Debug)]
pub(crate) State => Action => Do

Dead => K(Key => _) => Dead,
Default => {
    K(Key::Character(x) if x == "s" && ctrl()) => Save [Save],
    K(Key::Character(x) if x == "q" && ctrl()) => Dead [Quit],
    K(Key::Character(x) if x == "v" && ctrl()) => Default [Paste],
    K(Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft | NamedKey::ArrowDown | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End) if shift()) => Selection(Range<usize> => 0..0) [StartSelection],
    K(_) => Default [Edit],
},
Selection(x) => {
    K(Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft | NamedKey::ArrowDown | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End) if shift()) => Selection(x) [UpdateSelection],
    K(Key::Named(NamedKey::Backspace)) => Default [Delete(Range<usize> => x)],
    K(Key::Character(y) if y == "x" && ctrl()) => Default [Cut(Range<usize> => x)],
    K(Key::Character(y) if y == "c" && ctrl()) => Default [Copy(Range<usize> => x)],
    K(Key::Character(y)) => Default [Insert((Range<usize>, SmolStr) => (x, y))],
    K(_) => Default [Edit],
},
Save => {
    RequireFilename => InputFname(TextArea => default()),
    Saved => Default,
},
InputFname(t) => K(Key::Named(NamedKey::Enter)) => Default [SaveTo(String => t.rope.to_string())],
InputFname(t) => K(k) => InputFname(TextArea => handle(k, t)),
}
