#![feature(tuple_trait, unboxed_closures, fn_traits)]
#![feature(
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

use array_chunks::*;
use atools::prelude::*;
use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use rust_fsm::StateMachineImpl;
use swash::{FontRef, Instance};
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};

use crate::bar::Bar;
use crate::bar::state::{Input, State};
use crate::text::TextArea;
mod bar;
mod text;
mod winit_app;
fn main() {
    entry(EventLoop::new().unwrap())
}

const BG: [u8; 3] = [31, 36, 48];
const FG: [u8; 3] = [204, 202, 194];
#[implicit_fn::implicit_fn]
pub(crate) fn entry(event_loop: EventLoop<()>) {
    let ppem = 20.0;
    let ls = 20.0;

    let mut text = TextArea::default();
    let mut origin = std::env::args().nth(1);
    let mut fonts = dsb::Fonts::new(
        *FONT,
        F::instance(*FONT, *BFONT),
        *IFONT,
        F::instance(*IFONT, *BIFONT),
    );
    let mut bar = Bar {
        text: TextArea::default(),
        state: bar::state::StateMachine::new(),
        last_action: String::default(),
    };
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
            dbg!(&bar.state);
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
                        );
                        bar.write_to(
                            BG,
                            FG,
                            (&mut cells, (c, r)),
                            r - 1,
                            &origin.as_deref().unwrap_or("new buffer"),
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
                    event: WindowEvent::KeyboardInput { event, .. },
                    ..
                } if event.state == ElementState::Released
                    && *bar.state.state() == State::Control
                    && event.logical_key == NamedKey::Control =>
                {
                    bar.state.consume(&Input::Released).unwrap();
                    window.request_redraw();
                }
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event, .. },
                    ..
                } if *bar.state.state() == State::Control
                    && event.state == ElementState::Pressed =>
                {
                    use Key::*;
                    match event.logical_key {
                        Character(x) if x == "s" => match &origin {
                            Some(_) => {
                                bar.state.consume(&Input::Saved).unwrap();
                                save!();
                            }
                            None => {
                                bar.state
                                    .consume(&Input::WaitingForFname)
                                    .unwrap();
                            }
                        },
                        // Character(x)
                        //     if x == "s"
                        //         && =>
                        // {
                        //     bar.state.consume(State::Save);
                        //     text.rope
                        //         .write_to(File::open(origin).unwrap())
                        //         .unwrap();
                        // }
                        // Character(x) if x == "s" => {}
                        _ => panic!(),
                    }
                    window.request_redraw();
                }

                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event, .. },
                    ..
                } if event.state == ElementState::Pressed => {
                    use Key::*;
                    use NamedKey::*;
                    let text = if bar.state.state() == &State::InputFname {
                        &mut bar.text
                    } else {
                        &mut text
                    };
                    match event.logical_key {
                        Named(Space) => text.insert(" "),
                        Named(Backspace) => text.backspace(),
                        Named(ArrowLeft) => text.left(),
                        Named(Home) => text.home(),
                        Named(End) => text.end(),
                        Named(ArrowRight) => text.right(),
                        Named(ArrowUp) => text.up(),
                        Named(ArrowDown) => text.down(r),
                        Named(Enter)
                            if bar.state.state() == &State::InputFname =>
                        {
                            bar.state.consume(&Input::Enter).unwrap();
                            origin = Some(
                                std::mem::take(&mut bar.text)
                                    .rope
                                    .to_string(),
                            );
                            save!();
                        }
                        Named(Enter) => text.enter(),
                        Named(Control) => {
                            bar.state.consume(&Input::Control).unwrap();
                        }
                        Character(x) => {
                            text.insert(&*x);
                        }
                        _ => {}
                    };
                    window.request_redraw();
                }
                _ => {}
            };
        },
    );

    winit_app::run_app(event_loop, app);
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
