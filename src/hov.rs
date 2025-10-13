use std::iter::{once, repeat};
use std::pin::pin;
use std::time::Instant;
use std::vec::Vec;

use dsb::cell::Style;
use dsb::{Cell, F};
use fimg::Image;
use itertools::Itertools;
use minimad::*;
use ropey::Rope;

use crate::{BG, FG, M, text};
fn f(x: &Compound) -> u8 {
    let mut s = 0;
    if x.bold {
        s |= Style::BOLD;
    }
    if x.italic {
        s |= Style::ITALIC;
    }
    if x.strikeout {
        s |= Style::STRIKETHROUGH
    }
    s
}
fn markdown(c: usize, x: &str) -> Vec<Cell> {
    let mut cells = vec![];
    // println!(
    //     "{:#?}",
    //     markdown::to_mdast(
    //         include_str!("../vec.md"),
    //         &markdown::ParseOptions::default()
    //     )
    //     .unwrap()
    // );
    use Default::default;
    // std::fs::write(
    //     "vec.ast",
    //     format!(
    //         "{:#?}",
    //         minimad::parse_text(
    //             include_str!("../vec.md"),
    //             Options { keep_code_fences: true, ..default() },
    //         )
    //         .lines
    //     ),
    // )
    // .unwrap();
    const D: Cell = Cell {
        letter: None,
        style: Style { bg: BG, color: FG, flags: 0 },
    };
    let mut l = minimad::parse_text(
        x,
        Options { keep_code_fences: true, ..default() },
    )
    .lines
    .into_iter()
    .peekable();
    'l: while let Some(line) = l.next() {
        match line {
            minimad::Line::Normal(Composite { style, compounds }) => {
                let mut compounded = l
                    .by_ref()
                    .peeking_take_while(|x| matches!(x, Line::Normal(Composite{style, compounds}) if !matches!(style, CompositeStyle::Header(_))))
                    .flat_map(|x| match x {
                        Line::Normal(Composite { style, compounds }) =>
                            compounds.into_iter().flat_map(move |x| {
                                x.as_str().chars().zip(repeat(f(&x)).map(
                                    move |flags| match style {
                                        CompositeStyle::Paragraph =>
                                            Style {
                                                color: FG,
                                                bg: BG,
                                                flags,
                                            },
                                        CompositeStyle::Header(x) =>
                                            Style {
                                                color: [255; 3],
                                                bg: BG,
                                                flags: flags | Style::BOLD,
                                            },
                                        CompositeStyle::ListItem(x) =>
                                            Style {
                                                color: FG,
                                                bg: BG,
                                                flags,
                                            },
                                        CompositeStyle::Code => Style {
                                            color: [244,244,244],
                                            bg: [5,5,5],
                                            flags,
                                        },
                                        CompositeStyle::Quote => Style {
                                            color: [128; 3],
                                            bg: [0; 3],
                                            flags,
                                        },
                                    },
                                ))
                            }).chain(once((' ', Style { color: FG, bg: BG, flags : 0 }))),
                        _ => panic!(),
                    })
                    .chunk_by(|x| x.0.is_whitespace());
                let mut compounded = compounded.into_iter();
                let mut out = vec![];

                // let mut compounds = compounds.iter();
                while let Some((x, word)) = compounded.next() {
                    if x {
                        continue;
                    }
                    if out.len() > c {
                        panic!()
                    }
                    let word = word.collect::<Vec<_>>();
                    if word.len() > c {
                        let mut w = word.iter().map(|&(x, style)| Cell {
                            letter: Some(x),
                            style,
                        });
                        out.extend(w);
                        cells.extend(
                            out.drain(..out.len() - out.len() % c),
                        );
                        // 'out: loop {
                        //     while out.len() != c {
                        //         let Some(x) = w.next() else {
                        //             break 'out;
                        //         };
                        //         out.push(x);
                        //     }
                        //     cells.extend(&out);
                        //     out.clear();
                        // }
                        continue;
                        // cells.extend(&out);
                        // out.clear();
                        // cells.extend(
                        //     w.by_ref()
                        //         .take(c.saturating_sub(out.len() + 1)),
                        // );
                        // cells.push(Cell::basic('-'));
                        // cells.extend(w);
                    }

                    if out.len() + word.len() > c {
                        assert_eq!(cells.len() % c, 0);
                        cells.extend(&out);
                        cells.extend(std::iter::repeat_n(
                            D,
                            c.saturating_sub(out.len()),
                        ));
                        dbg!(out.len(), c);
                        out.clear();
                        assert_eq!(cells.len() % c, 0);
                    }

                    out.extend(word.iter().map(|&(x, style)| Cell {
                        letter: Some(x),
                        style,
                    }));
                    if out.len() != c {
                        out.push(D);
                    } else {
                        cells.extend(&out);
                        out.clear();
                    }
                    dbg!(out.len());

                    // }
                }
                //     if out.len() > c {
                //         panic!();
                //     }
                //     if out.len() + next.char_length() > c {
                //         cells.extend(&out);
                //         cells.extend(std::iter::repeat_n(
                //             Cell::default(),
                //             c.saturating_sub(out.len()),
                //         ));
                //         out.clear();
                //         assert_eq!(cells.len() % c, 0);
                //     }
                //     out.extend(
                //         next.as_str()
                //             .chars()
                //             .map(|x| Cell { letter: Some(x), style }),
                //     );
                // }
                if !out.is_empty() {
                    assert!(out.len() <= c, "{} < c", out.len());
                    cells.extend(&out);
                    cells.extend(std::iter::repeat_n(
                        D,
                        c.saturating_sub(out.len()),
                    ));
                    assert_eq!(cells.len() % c, 0);
                }
                continue 'l;
            }
            minimad::Line::TableRow(table_row) => todo!(),
            minimad::Line::TableRule(table_rule) => todo!(),
            minimad::Line::HorizontalRule => vec![Cell::basic('-')],
            minimad::Line::CodeFence(Composite {
                compounds: [Compound { src: lang, .. }],
                ..
            }) => {
                let mut r = Rope::new();
                while let Some(x) = l.next() {
                    match x {
                        Line::CodeFence(Composite {
                            compounds: [],
                            ..
                        }) => {
                            break;
                        }
                        Line::Normal(Composite {
                            compounds: [Compound { src, .. }],
                            ..
                        }) => {
                            r.insert(r.len_chars(), src);
                            r.insert_char(r.len_chars(), '\n');
                        }
                        _ => {}
                    }
                }
                let mut cell = vec![D; c * r.len_lines()];

                for (l, y) in r.lines().zip(0..) {
                    for (e, x) in l.chars().take(c).zip(0..) {
                        if e != '\n' {
                            cell[y * c + x].letter = Some(e);
                        }
                    }
                }
                for ((x1, y1), (x2, y2), s, txt) in
                    std::iter::from_coroutine(pin!(text::hl(
                        text::LOADER.language_for_name(lang).unwrap_or(
                            text::LOADER
                                .language_for_name("rust")
                                .unwrap()
                        ),
                        &r,
                        ..,
                        c,
                    )))
                {
                    cell.get_mut(y1 * c + x1..y2 * c + x2).map(|x| {
                        x.iter_mut().zip(txt.chars()).for_each(|(x, c)| {
                            x.style |= s;
                            x.letter = Some(c);
                        })
                    });
                }
                cells.extend(cell);
                assert_eq!(cells.len() % c, 0);
                continue 'l;
            }
            _ => panic!(),
        };
        // if out.len() > c {
        //     out.drain(c - 1..);
        //     out.push(Cell::basic('…'));
        // } else {
        //     out.extend(std::iter::repeat_n(
        //         Cell::default(),
        //         c.saturating_sub(out.len()),
        //     ));
        // }
        // assert_eq!(out.len(), c);
        // cells.extend(out);
    }
    assert_eq!(cells.len() % c, 0);
    cells
}

#[test]
fn t() {
    let ppem = 18.0;
    let lh = 10.0;
    let (w, h) = (800, 8000);
    let (c, r) = dsb::fit(&crate::FONT, ppem, lh, (w, h));
    std::fs::write(
        "mdast2",
        format!(
            "{:#?}",
            markdown::to_mdast(
                include_str!("../vec.md"),
                &markdown::ParseOptions::gfm()
            )
            .unwrap()
        ),
    );
    let mut cells = markdown(c, include_str!("../vec.md"));
    // use Default::default;
    // for line in dbg!(minimad::parse_text(
    //     include_str!("../vec.md"),
    //     Options { keep_code_fences: true, ..default() },
    // ))
    // .lines
    // {
    //     fn f(x: &Compound) -> u8 {
    //         let mut s = 0;
    //         if x.bold {
    //             s |= Style::BOLD;
    //         }
    //         if x.italic {
    //             s |= Style::ITALIC;
    //         }
    //         if x.strikeout {
    //             s |= Style::STRIKETHROUGH
    //         }
    //         s
    //     }
    //     let mut out = match line {
    //         minimad::Line::Normal(Composite { style, compounds }) =>
    //             compounds
    //                 .iter()
    //                 .flat_map(|c| {
    //                     c.as_str().chars().map(move |x| {
    //                         let flags = f(&c);
    //                         let style = match style {
    //                             CompositeStyle::Paragraph => Style {
    //                                 color: [128; 3],
    //                                 bg: [0; 3],
    //                                 flags,
    //                             },
    //                             CompositeStyle::Header(x) => Style {
    //                                 color: [255; 3],
    //                                 bg: [0; 3],
    //                                 flags: flags | Style::BOLD,
    //                             },
    //                             CompositeStyle::ListItem(x) => Style {
    //                                 color: [128; 3],
    //                                 bg: [0; 3],
    //                                 flags,
    //                             },
    //                             CompositeStyle::Code => Style {
    //                                 color: [100; 3],
    //                                 bg: [0; 3],
    //                                 flags,
    //                             },
    //                             CompositeStyle::Quote => Style {
    //                                 color: [128; 3],
    //                                 bg: [0; 3],
    //                                 flags,
    //                             },
    //                         };
    //                         Cell { letter: Some(x), style }
    //                     })
    //                 })
    //                 .collect::<Vec<_>>(),
    //         minimad::Line::TableRow(table_row) => todo!(),
    //         minimad::Line::TableRule(table_rule) => todo!(),
    //         minimad::Line::HorizontalRule => vec![Cell::basic('-')],
    //         minimad::Line::CodeFence(composite) => {
    //             vec![]
    //         }
    //     };
    //     if out.len() > c {
    //         out.drain(c - 1..);
    //         out.push(Cell::basic('…'));
    //     } else {
    //         out.extend(std::iter::repeat_n(
    //             Cell::default(),
    //             c - out.len(),
    //         ));
    //     }
    //     assert_eq!(out.len(), c);
    //     cells.extend(out);
    // }
    dbg!(cells.len() / c);
    // let (fw, fh) = dsb::dims(&FONT, ppem);
    dbg!(w, h);
    dbg!(c, r);
    // panic!();

    let mut fonts = dsb::Fonts::new(
        F::FontRef(*crate::FONT, &[(2003265652, 550.0)]),
        F::instance(*crate::FONT, *crate::BFONT),
        F::FontRef(*crate::IFONT, &[(2003265652, 550.0)]),
        F::instance(*crate::IFONT, *crate::BIFONT),
    );

    let now = Instant::now();
    let mut x = Image::build(w as _, h as _).fill(BG);
    unsafe {
        dsb::render(
            &cells,
            (c, r),
            ppem,
            BG,
            &mut fonts,
            lh,
            true,
            x.as_mut(),
        )
    };
    println!("{:?}", now.elapsed());
    x.as_ref().save("x");
}
