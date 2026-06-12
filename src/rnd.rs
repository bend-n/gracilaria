use std::iter::{chain, once, repeat_n};
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use atools::prelude::*;
pub use cell_buffer::CellBuffer;
use dsb::cell::Style;
use dsb::{Cell, Fonts};
use fimg::pixels::Blend;
use fimg::{Image, OverlayAt};
use lsp_types::*;
use regex::Regex;
use rust_fsm::StateMachine;
use softbuffer::Surface;
use url::Url;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::{ImeRequestData, Window};

use crate::edi::st::State;
use crate::edi::{Editor, lsp};
use crate::gotolist::{At, GoTo};
use crate::hov::{
    DiagnosticHovr, HOV_HEIGHT, Hoverable, Hovr, Hovring, Rendered,
};
use crate::lsp::Rq;
use crate::sym::UsedSI;
use crate::text::{CoerceOption, RopeExt, TextArea, col, color_};
use crate::{
    BG, BORDER, CompletionAction, CompletionState, FG, FONT, Freq,
    complete, filter, hash, sig,
};
mod cell_buffer;

#[implicit_fn::implicit_fn]
pub fn render(
    ed: &mut Editor,
    cells: &mut [Cell],
    ppem: f32,
    window: &mut Arc<dyn Window>,
    fw: f32,
    fh: f32,
    ls: f32,
    c: usize,
    r: usize,
    surface: Option<&mut Surface<Arc<dyn Window>, Arc<dyn Window>>>,
    cursor_position: (usize, usize),
    fonts: &mut dsb::Fonts,
    mut i: Image<&mut [u8], 3>,
    freq: &Freq,
) {
    let text = &mut ed.text;
    let (cx, cy) = text.primary_cursor_visual();
    let met = super::FONT.metrics(&[]);
    let fac = ppem / met.units_per_em as f32;
    window
        .request_ime_update(winit::window::ImeRequest::Update(
            ImeRequestData::default().with_cursor_area(
                PhysicalPosition::new(
                    ((cx + text.line_number_offset()) as f64
                        * (fw) as f64)
                        .round(),
                    ((cy.saturating_sub(text.vo)) as f64
                        * (fh + ls * fac) as f64)
                        .floor(),
                )
                .into(),
                PhysicalSize::new(fw, fh).into(),
            ),
        ))
        .unwrap();

    let Some(surface) = surface else {
        eprintln!(
            "RedrawRequested fired before Resumed or after Suspended"
        );
        return;
    };
    let size = window.surface_size();

    if size.height != 0 && size.width != 0 {
        let now = Instant::now();
        if c * r != cells.len() {
            return;
        }
        cells.fill(Cell {
            style: Style { fg: BG, secondary_color: BG, bg: BG, flags: 0 },
            letter: None,
        });
        let bmks = text
            .bookmarks
            .iter()
            .filter_map(|x| text.try_char_to_line(x.position).ok())
            .collect::<Vec<_>>();

        let z = match &ed.state {
            State::Selection => Some(
                text.cursor
                    .iter()
                    .filter_map(|x| x.sel)
                    .map(std::ops::Range::from)
                    .collect(),
            ),
            _ => None,
        };

        text.line_numbers(
            (c, r - 1),
            [67, 76, 87],
            BG,
            (cells, (c, r)),
            (1, 0),
            |_text, mut f, y| {
                if let State::GoToL(menu) = &ed.state
                    && let Some(Ok((GoTo { at: At::R(r), path }, _))) =
                        menu.sel(None)
                    && Some(&*path) == ed.origin.as_deref()
                {
                    if (r.start.line..=r.end.line).contains(&(y as _)) {
                        f.style.fg = col!("#FFCC66");
                    }
                }
                if let State::Symbols(Rq { result: Some(menu), .. }) =
                    &ed.state
                    && let Some(Ok(UsedSI {
                        at: GoTo { at: At::R(x), .. },
                        ..
                    })) = menu.sel(None)
                {
                    if (x.start.line..=x.end.line).contains(&(y as _)) {
                        f.style.fg = col!("#FFCC66");
                    }
                }
                if bmks.contains(&y) {
                    f.style.bg = col!("#4060aa");
                    f.style.fg = col!("#a7bcd6");
                }
                f
            },
        );
        let t_ox = text.line_number_offset() + 1;
        text.c = c - t_ox;
        text.r = r - 1;
        // let mut text = text.clone();
        // for (_, inlay) in inlay.result.as_ref().into_iter().flatten().chunk_by(|x| x.position.line).into_iter() {
        //     let mut off = 0;
        //     for inlay in inlay {
        //     let label = match &inlay.label {
        //         InlayHintLabel::String(x) => x.clone(),
        //         InlayHintLabel::LabelParts(v) => {
        //             v.iter().map(_.value.clone()).collect::<String>()
        //         },
        //     };
        //     text.rope.insert(text.l_position(inlay.position) + off, &label);
        //     off += label.chars().count();
        //     }
        // }
        let wt = |(_c, _r),
                  text: &TextArea,
                  mut x: crate::text::Output<'_>| {
            if let Some(hl) = &ed.requests.document_highlights.result {
                for DocumentHighlight { range: r, .. } in hl {
                    // let s = match kind {
                    //     Some(DocumentHighlightKind::READ) => Style::UNDERLINE,
                    //     Some(DocumentHighlightKind::WRITE) => Style::UNDERLINE,
                    //     _ => Style::UNDERCURL,
                    // };
                    let (x1, y1) = text.map_to_visual((
                        r.start.character as _,
                        r.start.line as _,
                    ));
                    let (x2, y2) = text.map_to_visual((
                        r.end.character as _,
                        r.end.line as _,
                    ));

                    x.get_simple((x1, y1), (x2, y2)).coerce().for_each(
                        |x| {
                            x.style.bg = col!("#3a4358");
                        },
                    );
                }
            }
            if let Some(LocationLink {
                origin_selection_range: Some(r),
                ..
            }) = ed.requests.def.result
            {
                _ = try {
                    let (x1, y1) = text.map_to_visual((
                        r.start.character as _,
                        r.start.line as _,
                    ));
                    let (x2, y2) = text.map_to_visual((
                        r.end.character as _,
                        r.end.line as _,
                    ));
                    x.get_simple((x1, y1), (x2, y2))?.iter_mut().for_each(
                        |x| {
                            x.style.flags |= Style::UNDERLINE;
                            x.style.fg = col!("#FFD173");
                        },
                    );
                }
            }

            if let State::Hovering(Rq {
                result: Some(crate::hov::Hovring { of, .. }),
                ..
            }) = &ed.state
            {
                for thing in of {
                    // if let Some(Hoverable::Diagnostic(DiagnosticHovr{ span, .. })) = thing {

                    // }
                    if let Hoverable::Lsp(Hovr {
                        range: Some(r), ..
                    }) = thing
                    {
                        x.get_range(
                            text.map_to_visual((
                                r.start.character as _,
                                r.start.line as _,
                            )),
                            text.map_to_visual((
                                r.end.character as usize,
                                r.end.line as _,
                            )),
                        )
                        .for_each(|x| {
                            x.style.secondary_color = col!("#73d0ff");
                            x.style.flags |= Style::UNDERCURL;
                        });
                    }
                }
                // x.range;
            }
            if let Some((lsp, p)) = lsp!(ed + p)
                && let uri = Url::from_file_path(p).unwrap()
                && let Some(diag) =
                    lsp.diagnostics.get(&uri, &lsp.diagnostics.guard())
            {
                #[derive(Copy, Clone, Debug)]
                enum EType {
                    Hint,
                    Info,
                    Error,
                    Warning,
                    Related(DiagnosticSeverity),
                }
                let mut occupied = vec![];
                diag.iter()
                    .flat_map(|diag| {
                        let sev = diag
                            .severity
                            .unwrap_or(DiagnosticSeverity::ERROR);
                        let sev_ = match sev {
                            DiagnosticSeverity::ERROR => EType::Error,
                            DiagnosticSeverity::WARNING => EType::Warning,
                            DiagnosticSeverity::HINT => EType::Hint,
                            _ => EType::Info,
                        };
                        once((diag.range, &*diag.message, sev_)).chain(
                            diag.related_information
                                .iter()
                                .flatten()
                                .filter(|sp| sp.location.uri == uri)
                                .map(move |x| {
                                    (
                                        x.location.range,
                                        &*x.message,
                                        EType::Related(sev),
                                    )
                                }),
                        )
                    })
                    .for_each(|(mut r, m, sev)| {
                        if let EType::Related(x) = sev
                            && x != DiagnosticSeverity::ERROR
                        {
                            return;
                        }
                        let p = r.start.line;
                        while occupied.contains(&r.start.line) {
                            r.start.line += 1;
                        }
                        occupied.push(r.start.line);
                        let f = |cell: &mut Cell| {
                            cell.style.bg.blend(match sev {
                                EType::Error => col!("#ff66662c"),
                                EType::Warning
                                | EType::Hint
                                | EType::Info => col!("#9469242c"),
                                EType::Related(
                                    DiagnosticSeverity::ERROR,
                                ) => col!("#dfbfff26"),
                                EType::Related(_) => col!("#ffad6625"),
                            });
                        };
                        if r.start == r.end {
                            x.get(text.map_to_visual((
                                r.start.character as _,
                                p as _,
                            )))
                            .map(f);
                        } else {
                            x.get_range(
                                text.map_to_visual((
                                    r.start.character as _,
                                    p as _,
                                )),
                                text.map_to_visual((
                                    r.end.character as usize,
                                    r.end.line as _,
                                )),
                            )
                            .for_each(f)
                        }
                        let l = r.start.line as usize;
                        let Some(x_) = text.visual_eol(l).map(_ + 2)
                        else {
                            return;
                        };
                        let m = m.lines().next().unwrap_or(m);
                        x.get_range((x_, l), (x_ + m.chars().count(), l))
                            .zip(m.chars())
                            .for_each(|(x, ch)| {
                                let (bg, fg) = match sev {
                                    EType::Warning => {
                                        col!("#ff942f1b", "#fa973a")
                                    }
                                    EType::Error => {
                                        col!("#ff942f1b", "#f26462")
                                    }
                                    EType::Related(
                                        DiagnosticSeverity::WARNING,
                                    ) => {
                                        col!("#dfbfff26", "#DFBFFF")
                                    }
                                    _ => return,
                                };
                                x.style.bg.blend(bg);
                                x.style.fg = fg;
                                x.letter = Some(ch);
                            })
                    });
            }
            if let State::Search(re, j, _) = &ed.state {
                re.find_iter(&text.rope.to_string()).enumerate().for_each(
                    |(i, m)| {
                        for x in x.get_range(
                            text.map_to_visual(
                                text.xy(text.rope.byte_to_char(m.start()))
                                    .unwrap(),
                            ),
                            text.map_to_visual(
                                text.xy(text.rope.byte_to_char(m.end()))
                                    .unwrap(),
                            ),
                        ) {
                            x.style.bg = if i == *j {
                                [105, 83, 128]
                            } else {
                                [65, 62, 83]
                            }
                        }
                    },
                );
            }
        };
        text.write_to(
            (cells, (c, r)),
            (t_ox, 0),
            z,
            wt,
            ed.origin.as_deref(),
            lsp!(ed).and_then(|x| x.legend()),
            ed.language,
        );

        ed.bar.write_to(
            BG,
            FG,
            (cells, (c, r)),
            r - 1,
            ed.origin
                .as_ref()
                .map(|x| {
                    ed.workspace
                        .as_ref()
                        .and_then(|w| x.strip_prefix(w).ok())
                        .unwrap_or(&x)
                        .to_str()
                        .unwrap()
                })
                .unwrap_or("new buffer"),
            &ed.state,
            &text,
            lsp!(ed),
        );
        unsafe {
            dsb::render(
                &cells,
                (c, r),
                ppem,
                fonts,
                ls,
                true,
                i.copy(),
                (0, 0),
            )
        };

        let text = text.clone();
        if let Some(d) = &ed.requests.git_diff.result {
            for h in d.hunks() {
                // let b = text.rope.byte_to_line(h.after.start as _);
                if h.after.start == h.after.end
                    && (text.vo..text.vo + r)
                        .contains(&(h.after.start as usize))
                {
                    let l = h.after.start - text.vo as u32;
                    let f = fh + ls * fac;
                    i.tri::<f32>(
                        (0.0f32, (l as f32 - 0.15) * f - (ls * fac)),
                        (6.0f32, (l as f32) * f - (ls * fac)),
                        (0.0f32, (l as f32 + 0.15) * f - (ls * fac)),
                        col!("#F27983"),
                    );
                }
                for l in h.after.clone() {
                    if (text.vo..text.vo + r).contains(&(l as usize)) {
                        let l = l - text.vo as u32;
                        let col = if h.is_pure_insertion() {
                            col!("#87D96C")
                        } else {
                            col!("#80BFFF")
                        };
                        for x in 0..(fw / 2.0) as u32 {
                            for y in (l as f32 * (fh + ls * fac)) as u32
                                ..(((l + 1) as f32) * (fh + ls * fac))
                                    as u32
                            {
                                if let Some(x) = i.get_pixel_mut(x, y) {
                                    *x = col;
                                }
                                // unsafe { i.pixel(x, y, &col)};
                            }
                        }
                    }
                }
            }
        }

        let position = |(_x, _y): (usize, usize),
                        (w, h): (usize, usize),
                        // i: Image<&mut [u8], 3>,
                        // c: &[Cell],
                        // columns: usize,
                        ox: f32,
                        oy: f32,
                        toy: f32,
                        add1_below: bool| {
            let met = super::FONT.metrics(&[]);
            let fac = ppem / met.units_per_em as f32;
            let (px, py) = (
                (((_x) as f32 * fw).round() + ox) as usize,
                (((_y) as f32 * (fh + ls * fac)).round() + oy) as usize,
            );

            // let mut r = c.len() / columns;
            // assert_eq!(c.len() % columns, 0);
            // std::fs::write("cells", Cell::store(c));

            if w >= size.width as usize
                || py >= size.height as usize
                || px >= size.width as usize
                || w >= window.surface_size().width as _
                || h >= window.surface_size().height as _
            {
                return Err(((px, py), size, (w, h)));
            }
            let is_above = py.checked_sub(h).is_some();
            let top = py.checked_sub(h).unwrap_or(
                ((((_y + add1_below as usize) as f32) * (fh + ls * fac))
                    .round()
                    + toy) as usize,
            );

            let left = if px + w as usize > size.width as usize {
                size.width as usize - w as usize
            } else {
                px
            };
            if top + h > size.height as usize {
                return Err(((px, py), size, (w, h)));
            }

            assert!(left + w <= size.width as usize);
            assert!(top + h <= size.height as usize);

            // let (w, h) =
            // dsb::size(&fonts.regular, ppem, ls, (columns, r));
            Ok((is_above, left, top))
        };
        let place_around =
            |(_x, _y): (usize, usize),
             i: Image<&mut [u8], 3>,
             c: &[Cell],
             columns: usize,
             ppem_: f32,
             ls_: f32,
             ox: f32,
             oy: f32,
             toy: f32,
             add1_below: bool,
             fonts: &mut Fonts<'_, '_, '_, '_>| {
                let ppem = ppem_;
                let ls = ls_;
                let r = c.len().div_exact(columns).unwrap();
                let (w, h) =
                    dsb::size(&fonts.regular, ppem, ls, (columns, r));

                let (is_above, left, top) =
                    position((_x, _y), (w, h), ox, oy, toy, add1_below)
                        .inspect_err(|(..)| {
                            // std::fs::write("cells", Cell::store(c));
                            unsafe {
                                dsb::render_owned(
                                    c,
                                    (columns, c.len() / columns),
                                    ppem,
                                    fonts,
                                    ls,
                                    true,
                                )
                                .save("fail.png")
                            };
                        })?;

                let (w, h) =
                    dsb::size(&fonts.regular, ppem, ls, (columns, r));
                unsafe {
                    dsb::render(
                        &c,
                        (columns, 0),
                        ppem,
                        fonts,
                        ls,
                        true,
                        i,
                        (left as _, top as _),
                    )
                };
                Ok::<_, ((usize, usize), PhysicalSize<u32>, (usize, usize))>(
                    (is_above, left, top, w, h),
                )
            };
        if let Some(Some(x)) = &ed.requests.document_symbols.result
            && let Ok(y) = text.try_line_to_char(text.vo.saturating_sub(1))
            && let Some((_, y, z)) = text.sticky_context(&x, y)
            && let Ok(l) = text.try_line_to_char(text.vo + 4)
            && y.contains(&l)
        // && text.cursor.iter().any(|x| y.contains(&*x))
        {
            let mut cells = vec![];
            for e in z {
                let p = text.l_position(e.selection_range.start).unwrap();
                if text.char_to_line(text.l_position(e.range.end).unwrap())
                    == text.vo
                {
                    continue;
                }

                let r = if let Some(r) = e.sticky_range {
                    let r = text.l_range(r).unwrap();
                    text.char_to_line(r.start)..=text.char_to_line(r.end)
                } else {
                    let x = text.char_to_line(p);
                    x..=x
                };

                let d = Cell {
                    style: Style {
                        fg: crate::FG,
                        bg: col!("#191d27"),
                        ..Default::default()
                    },
                    letter: None,
                };

                // let mut cells =
                // chain([Cell::default()], x.to_string().chars().map(Cell::basic)).chain([Cell::default()]).collect();

                for (x, rel, mut cell) in text
                    .colored_lines(r, lsp!(ed).and_then(|x| x.legend()))
                {
                    if rel == 0 {
                        let rem = cells.len() % c;
                        if rem != 0 {
                            cells.extend(repeat_n(d, c - rem));
                        }
                        cells.extend(
                            chain(
                                [d],
                                x.to_string().chars().map(|x| {
                                    Style::new(
                                        [67, 76, 87],
                                        col!("#191d27"),
                                    )
                                    .basic(x)
                                }),
                            )
                            .chain([d]),
                        );
                    }
                    cell.style.bg = col!("#191d27");
                    cells.push(cell);
                }

                let rem = cells.len() % c;
                if rem != 0 {
                    cells.extend(repeat_n(d, c - rem));
                }
            }

            if let Ok((.., w, h)) = place_around(
                (0, 0),
                i.copy(),
                &cells,
                c,
                14.0,
                -200.,
                0.,
                0.,
                0.,
                false,
                fonts,
            ) {
                i.filled_box(
                    (w as u32, 0),
                    i.width() - w as u32,
                    h as _,
                    color_("#191d27"),
                );
            }
        }
        if let State::Hovering(
            Rq { result: Some(Hovring { of, rndr }), .. },
            ..,
        ) = &mut ed.state
            && !of.is_empty()
            // && !dbg!(&of).is_empty()
            && let sized = of.iter().map(|hovr| {
                (
                    match hovr {
                        Hoverable::Lsp(x) => dsb::size(
                            &fonts.regular,
                            ppem,
                            ls,
                            (x.item.c, x.item.l()),
                        ),
                        Hoverable::Diagnostic(x) => dsb::size(
                            &fonts.regular,
                            15.0,
                            -200.,
                            (x.t.c, x.t.l().into()),
                        ),
                    },
                    hovr,
                )
            })
            && let (tw, th) = sized
                .clone()
                .fold((0, 0), |(w_, h_), ((w, h), _)| (w_.max(w), h_ + h))
            && th != 0
            && tw != 0
        // && pass
        {
            let hof = hash(&of);
            let rh = th.min(HOV_HEIGHT);
            let (buffer, scr) = match rndr {
                Some(Rendered { hash, image, scroll }) if *hash == hof =>
                    (image, *scroll),
                _ => {
                    let mut buffer =
                        Image::<_, 3>::build(tw as _, th as _).alloc();
                    let mut oy = 0;
                    for ((_, h), item) in sized.collect::<Vec<_>>() {
                        // println!("{item:?}");
                        let c = match item {
                            Hoverable::Diagnostic(x) => &x.t,
                            Hoverable::Lsp(x) => &x.item,
                        };
                        if c.cells.is_empty() {
                            continue;
                        }
                        // TODO: reflow lsp documentation to fit wide diagnostics?
                        unsafe {
                            dsb::render(
                                &c.cells,
                                (c.c, c.l()),
                                match item {
                                    Hoverable::Diagnostic(_) => 15.0,
                                    _ => ppem,
                                },
                                fonts,
                                match item {
                                    Hoverable::Diagnostic(_) => -200.,
                                    _ => ls,
                                },
                                true,
                                buffer.as_mut(),
                                (0, oy),
                            );
                            //                dsb::render_owned(
                            //     &c.cells,
                            //     (c.c, c.l()),
                            //     match item {
                            //         Hoverable::Diagnostic(_) => 15.0,
                            //         _ => ppem,
                            //     },
                            //     fonts,
                            //     match item {
                            //         Hoverable::Diagnostic(_) => -200.,
                            //         _ => ls,
                            //     },
                            //     true,
                            // ).save(format!("{oy}.png"));

                            oy += h as u32;
                        }
                    }
                    *rndr = Some(Rendered {
                        image: buffer.boxed(),
                        hash: hof,
                        scroll: 0,
                    });
                    let r = rndr.as_mut().unwrap();
                    (&mut r.image, r.scroll)
                }
            };

            if let Some(
                Hoverable::Diagnostic(DiagnosticHovr { span, .. })
                | Hoverable::Lsp(Hovr { span, .. }),
            ) = of.iter().next()
                && let (_x, _y) = if let Some([(_x, _y), (_x2, _)]) = *span
                    && (_x..=_x2).contains(
                        &(cursor_position
                            .0
                            .wrapping_sub(text.line_number_offset() + 1)),
                    )
                    && let [_x, _x2] =
                        [_x, _x2].add(text.line_number_offset() + 1)
                    && let Some(_y) = _y.checked_sub(text.vo)
                    && let Some(_x) = _x.checked_sub(text.ho)
                    && (cursor_position.1 == _y
                        && (_x..=_x2).contains(&cursor_position.0))
                {
                    (_x, _y)
                } else {
                    cursor_position
                }
                && let Ok((_, x, y)) =
                    position((_x, _y), (tw, rh), 0.0, 0.0, 0.0, true)
            {
                let top = scr * tw as u32 * 3;
                let o = &buffer.bytes()[top as usize
                    ..(top + (rh as u32 * tw as u32 * 3)) as usize];
                unsafe {
                    i.overlay_at(
                        &Image::<_, 3>::build(tw as _, rh as _).buf(o),
                        x as _,
                        y as _,
                    )
                };
                i.r#box(
                    (x.saturating_sub(1) as _, y.saturating_sub(1) as _),
                    tw as _,
                    rh as _,
                    BORDER,
                );
            }
        }
        let mut drawb = |cells, c| {
            // let ws = ed.workspace.as_deref().unwrap();
            // let (_x, _y) = text.cursor_visual();
            let _x = 0;
            let _y = r - 1;
            let Ok((_, left, top, w, h)) = place_around(
                (_x, _y),
                i.copy(),
                cells,
                c,
                ppem,
                ls,
                0.,
                0.,
                0.,
                true,
                fonts,
            ) else {
                println!("ra?");
                return;
            };
            i.r#box(
                (left.saturating_sub(1) as _, top.saturating_sub(1) as _),
                w as _,
                h as _,
                BORDER,
            );
        };
        match &ed.state {
            State::CodeAction(Rq { result: Some(x), .. }) => 'out: {
                let m = x.maxc();
                let c = x.write(m);
                let (_x, _y) = (cx, cy);
                let _x = _x + text.line_number_offset() + 1;
                let Some(_y) = _y.checked_sub(text.vo) else {
                    println!("rah");
                    break 'out;
                };
                let Ok((_is_above, left, top, w, h)) = place_around(
                    (_x, _y),
                    i.copy(),
                    &c,
                    m,
                    ppem,
                    ls,
                    0.,
                    0.,
                    0.,
                    true,
                    fonts,
                ) else {
                    println!("ra?");
                    break 'out;
                };
                i.r#box(
                    (
                        left.saturating_sub(1) as _,
                        top.saturating_sub(1) as _,
                    ),
                    w as _,
                    h as _,
                    BORDER,
                );
            }
            State::Command(x) if x.should_render() => {
                let ws = ed.workspace.as_deref().unwrap();
                let c = x.cells(50, ws, None);
                drawb(&c, 50);
            }
            State::Symbols(Rq { result: Some(x), .. }) => {
                let ws = ed.workspace.as_deref().unwrap();
                let c = x.cells(50, ws, Some(freq));
                drawb(&c, 50);
            }
            State::Runnables(Rq { result: Some(x), .. }) => {
                let ws = ed.workspace.as_deref().unwrap();
                let c = x.cells(50, ws, None);
                drawb(&c, 50);
            }
            State::GoToL(y) => {
                let ws = ed.workspace.as_deref().unwrap();
                let c = y.cells(50, ws, None);
                drawb(&c, 50);
            }
            State::KillRing(y) => {
                dbg!(y);
                let ws = ed.workspace.as_deref().unwrap();
                let c = y.cells(50, ws, None);
                println!("???? {c:?}");
                drawb(&c, 50);
            }
            _ => {}
        }
        let com = match ed.requests.complete {
            CompletionState::Complete(Rq {
                result: Some(ref x), ..
            }) => {
                let c = complete::s(x, 40, &filter(&text));
                if c.len() == 0 {
                    ed.requests
                        .complete
                        .consume(CompletionAction::NoResult)
                        .unwrap();
                    None
                } else {
                    Some(c)
                }
            }
            _ => None,
        };

        'out: {
            if let Rq { result: Some((ref x, vo, ref mut max)), .. } =
                ed.requests.sig_help
                && let Some((sig, p)) = sig::active(x)
            {
                let c = sig::sig((sig, p), 40);
                let (_x, _y) = (cx, cy);
                let _x = _x + text.line_number_offset() + 1;
                let Some(_y) = _y.checked_sub(text.vo) else { break 'out };
                let Ok((is_above, left, top, w, mut h)) = place_around(
                    (_x, _y),
                    i.copy(),
                    &c,
                    40,
                    ppem,
                    ls,
                    0.,
                    0.,
                    0.,
                    true,
                    fonts,
                ) else {
                    break 'out;
                };
                i.r#box(
                    (
                        left.saturating_sub(1) as _,
                        top.saturating_sub(1) as _,
                    ),
                    w as _,
                    h as _,
                    BORDER,
                );
                let com = com.and_then(|c| {
                    let Ok((is_above_, left, top, w_, h_)) = place_around(
                        (_x, _y),
                        i.copy(),
                        &c,
                        40,
                        ppem,
                        ls,
                        0.,
                        -(h as f32),
                        if is_above { 0.0 } else { h as f32 },
                        true,
                        fonts,
                    ) else {
                        return None;
                    };
                    i.r#box(
                        (
                            left.saturating_sub(1) as _,
                            top.saturating_sub(1) as _,
                        ),
                        w_ as _,
                        h_ as _,
                        BORDER,
                    );
                    if is_above {
                        // completion below, we need to push the docs, if any, below only below us, if the sig help is still above.
                        h = h_;
                    } else {
                        h += h_;
                    }
                    Some((is_above_, left, top, w_, h_))
                });
                {
                    let ppem = 15.0;
                    let ls = 10.0;
                    let (fw, _) = dsb::dims(&FONT, ppem);
                    let cols = (w as f32 / fw).floor() as usize;
                    sig::doc(sig, cols, ed.language).map(|mut cells| {
                        *max = Some(cells.l());
                        cells.vo = vo;
                        let cells = cells.displayable(cells.l().min(15));
                        let Ok((_, left_, top_, _w_, h_)) = place_around(
                            (_x, _y),
                            i.copy(),
                            cells,
                            cols,
                            ppem,
                            ls,
                            0.,
                            -(h as f32),
                            if is_above {
                                com.filter(|x| !x.0)
                                    .map(|(_is, _l, _t, _w, h)| h)
                                    .unwrap_or_default()
                                    as f32
                            } else {
                                h as f32
                            },
                            true,
                            fonts,
                        ) else {
                            return;
                        };
                        i.r#box(
                            (
                                left_.saturating_sub(1) as _,
                                top_.saturating_sub(1) as _,
                            ),
                            w as _,
                            h_ as _,
                            BORDER,
                        );
                    });
                }
            } else if let Some(c) = com {
                let ppem = 20.0;
                let (_x, _y) = text.primary_cursor_visual();
                let _x = _x + text.line_number_offset() + 1;
                let _y = _y.wrapping_sub(text.vo);
                let Ok((_, left, top, w, h)) = place_around(
                    (_x, _y),
                    i.copy(),
                    &c,
                    40,
                    ppem,
                    ls,
                    0.,
                    0.,
                    0.,
                    true,
                    fonts,
                ) else {
                    println!("RAHHH");
                    break 'out;
                };
                i.r#box(
                    (
                        left.saturating_sub(1) as _,
                        top.saturating_sub(1) as _,
                    ),
                    w as _,
                    h as _,
                    BORDER,
                );
            }
        }
        let met = FONT.metrics(&[]);
        let fac = ppem / met.units_per_em as f32;
        // if x.view_o == Some(x.cells.row) || x.view_o.is_none() {
        let (fw, fh) = dsb::dims(&FONT, ppem);
        let cursor = Image::<_, 4>::build(3, (fh).ceil() as u32)
            .fill([0xFF, 0xCC, 0x66, 255]);
        let mut draw_at = |x: usize, y: usize, w| unsafe {
            let x = (x + t_ox).saturating_sub(text.ho) % c;

            if (text.vo..text.vo + r).contains(&y) {
                i.overlay_at(
                    w,
                    (x as f32 * fw).floor() as u32,
                    ((y - text.vo) as f32 * (fh + ls * fac)).floor()
                        as u32,
                    // 4 + ((x - 1) as f32 * sz) as u32,
                    // (x as f32 * (ppem * 1.25)) as u32 - 20,
                );
            }
        };
        if matches!(ed.state, State::Default | State::Selection) {}
        text.cursor.each_ref(|c| {
            let (x, y) = text.visual_xy(*c).unwrap();
            draw_at(x, y, &cursor);
        });
        // let (x, y) = text.cursor_visual();
        let image = Image::<_, 4>::build(2, (fh).ceil() as u32)
            .fill([82, 82, 82, 255]);
        for stop in
            text.tabstops.as_ref().into_iter().flat_map(|x| x.list())
        {
            let Some((x, y)) = text.visual_xy(stop.clone().r().end) else {
                continue;
            };
            draw_at(x, y, &image);
        }

        window.pre_present_notify();
        let buffer = surface.buffer_mut().unwrap();
        let x = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_ptr() as *mut u8,
                buffer.len() * 4,
            )
            .as_chunks_unchecked_mut::<4>()
        };
        fimg::overlay::copy_rgb_bgr_(i.flatten(), x);
        println!("rnd took: {:.3}", now.elapsed().as_millis_f32());
        buffer.present().unwrap();
    }
}

pub fn simplify_path(x: &str) -> String {
    static DEP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\.cargo\/git\/checkouts\/(?<name>[^/]+)\-[a-f0-9]+\/[a-f0-9]+").unwrap()
    });
    static DEP2: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\.cargo\/registry\/src/index.crates.io-[0-9a-f]+/(?<name>[^/]+)\-(?<version>[0-9]+\.[0-9]+\.[0-9]+)").unwrap()
    });
    static RUST_SRC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r".rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library").unwrap()
    });
    let x = x.replace(env!("HOME"), " ");
    [(&*RUST_SRC, " "), (&*DEP, " /$name"), (&*DEP2, " /$name")]
        .into_iter()
        .fold(x, |acc, (r, repl)| r.replace(&acc, repl).into_owned())
}
