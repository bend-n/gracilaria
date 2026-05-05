use std::borrow::Cow;
use std::ops::ControlFlow;
use std::sync::Arc;

use Default::default;
use implicit_fn::implicit_fn;
use lsp_types::request::*;
use lsp_types::*;
use rust_fsm::StateMachine;
use tokio::task::spawn_blocking;
use tokio_util::task::AbortOnDropHandle as DropH;
use winit::window::Window;
enum Set<T> {
    To(T),
    Reset,
    Ignore,
}
use crate::edi::*;
use crate::rnd::CellBuffer;
impl Editor {
    #[implicit_fn]
    pub fn cursor_moved(
        &mut self,
        cursor_position: (usize, usize),
        w: Arc<dyn Window>,
        c: usize,
    ) {
        match self.state.consume(Action::C(cursor_position)).unwrap() {
            Some(Do::ExtendSelectionToMouse) => {
                let p = self.text.mapped_index_at(cursor_position);
                self.text
                    .cursor
                    .first_mut()
                    .extend_selection_to(p, &self.text.rope);
                w.request_redraw();
            }
            Some(Do::StartSelection) => {
                let x = self.text.mapped_index_at(cursor_position);
                self.text.cursor.first_mut().position = x;
                self.text.cursor.first_mut().sel = Some((x..x).into());
                self.hist.lc = self.text.cursor.clone();
            }
            Some(Do::Hover)
                if let Some(hover) = self
                    .text
                    .visual_index_at(cursor_position)
                    .map(Mapping::own)
                    && let Some(x) = lsp!(self)
                    && x.initialized.is_some()
                    && let Mapping::Char(e, ..) | Mapping::Fake(.., e) =
                        hover
                    && !e.is_whitespace() =>
            'out: {
                match self
                    .state
                    .consume(Action::HOnSomething(cursor_position))
                    .unwrap()
                {
                    Some(Do::SetHovering) => {
                        let State::Hovering(l) = &mut self.state else {
                            panic!()
                        };

                        let l2 = &mut l.result;
                        if let Some(Hovr {
                            span: Some([(_x, _y), (_x2, _)]),
                            ..
                        }) = &*l2
                            && let Some(_y) = _y.checked_sub(self.text.vo)
                            && let Some(_x) = _x.checked_sub(self.text.ho)
                            && let Some(_x2) =
                                _x2.checked_sub(self.text.ho)
                            && cursor_position.1 == _y
                            && (_x..=_x2).contains(
                                &&(cursor_position.0
                                    - self.text.line_number_offset()
                                    - 1),
                            )
                        {
                            break 'out;
                        } else {
                            // println!("span no longer below cursor; cancel hover {_x}..{_x2} {}", cursor_position.0 - text.line_number_offset() - 1);
                            *l2 = None;
                            w.request_redraw();
                        }
                        self.rq_hover(hover, cursor_position, c);
                    }
                    _ => {}
                }
                // let l = &mut self.requests.hovering.result;
            }
            Some(Do::Hover) => {
                self.state.consume(Action::HOnNothing).unwrap();
                // w.request_redraw();
            }
            None => {}
            x => unreachable!("{x:?}"),
        }
    }
    #[implicit_fn]
    pub fn rq_hover(
        &mut self,
        hover: Mapping<'_>,
        cursor_position: (usize, usize),
        c: usize,
    ) {
        let (lsp, o) = lsp!(self + p).unwrap();
        let text = self.text.clone();
        let mut rang = None;
        let z = match hover {
            Mapping::Char(_, _, i) => TextDocumentPositionParams {
                position: text.to_l_position(i).unwrap(),
                text_document: o.tid(),
            },
            Mapping::Fake(mark, relpos, abspos, _) => {
                let Some(ref loc) = mark.data[relpos as usize].1 else {
                    return;
                };
                let (x, y) = text.xy(abspos as _).unwrap();
                let Some(mut begin) = text.reverse_source_map(y) else {
                    return;
                };
                let start = begin.nth(x.saturating_sub(1)).unwrap() + 1;
                let left = mark.data[..relpos as usize]
                    .iter()
                    .rev()
                    .take_while(_.1.as_ref() == Some(loc))
                    .count();
                let start = start + relpos as usize - left;
                let length = mark.data[relpos as usize..]
                    .iter()
                    .take_while(_.1.as_ref() == Some(loc))
                    .count()
                    + left;
                rang = Some([(start, y), (start + length, y)]);
                TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: loc.uri.clone(),
                    },
                    position: loc.range.start,
                }
            }
        };
        let x = if ctrl() {
            if self
                .requests
                .def
                .request
                .as_ref()
                .is_none_or(|&(_, x)| x != cursor_position)
            {
                let handle = lsp.runtime.spawn(
                    lsp.request::<lsp_request!("textDocument/definition")>(
                        &GotoDefinitionParams {
                            text_document_position_params: z.clone(),
                            work_done_progress_params: default(),
                            partial_result_params: default(),
                        },
                    )
                    .unwrap()
                    .0,
                );
                Set::To((handle, cursor_position))
                // self.requests.def.request =
                // Some((DropH::new(handle), cursor_position));
            } else if self.requests.def.result.as_ref().is_some_and(|em| {
                let z = em.origin_selection_range.unwrap();
                (z.start.character..z.end.character).contains(
                    &((cursor_position.0 - text.line_number_offset() - 1)
                        as _),
                )
            }) {
                Set::Reset
            } else {
                Set::Ignore
            }
        } else {
            Set::Reset
        };

        // match self.state.consume(cursor_position) {}
        // if let Some((_, c)) = hov.request
        //     && c == cursor_position
        // {
        //     return;
        // }
        // if !running.insert(hover) {return}
        let (rx, _) = lsp
            .request::<HoverRequest>(&HoverParams {
                text_document_position_params: z,
                work_done_progress_params: default(),
            })
            .unwrap();
        // println!("rq hov of {hover:?} (cur {})", requests.hovering.request.is_some());
        let handle: tokio::task::JoinHandle<Result<Option<Hovr>, _>> =
            lsp.runtime.spawn(async move {
                let Some(x) = rx.await? else {
                    return Ok(None::<Hovr>);
                };
                let (w, cells) = spawn_blocking(move || {
                    let x = match &x.contents {
                        lsp_types::HoverContents::Scalar(
                            marked_string,
                        ) => match marked_string {
                            MarkedString::LanguageString(x) =>
                                Cow::Borrowed(&*x.value),
                            MarkedString::String(x) => Cow::Borrowed(&**x),
                        },
                        lsp_types::HoverContents::Array(
                            marked_strings,
                        ) => Cow::Owned(
                            marked_strings
                                .iter()
                                .map(|x| match x {
                                    MarkedString::LanguageString(x) =>
                                        &*x.value,
                                    MarkedString::String(x) => &*x,
                                })
                                .collect::<String>(),
                        ),
                        lsp_types::HoverContents::Markup(
                            markup_content,
                        ) => Cow::Borrowed(&*markup_content.value),
                    };
                    let x = hov::p(&x).unwrap();
                    let m = hov::l(&x)
                        .into_iter()
                        .max()
                        .map(_ + 2)
                        .unwrap_or(usize::MAX)
                        .min(c - 10);
                    (m, hov::markdown2(m, &x))
                })
                .await
                .unwrap();
                let span = rang.or_else(|| {
                    x.range.and_then(|range| try {
                        let (startx, starty) =
                            text.l_pos_to_char(range.start)?;
                        let (endx, endy) =
                            text.l_pos_to_char(range.end)?;
                        let x1 = text
                            .reverse_source_map(starty)?
                            .nth(startx)?;
                        let x2 =
                            text.reverse_source_map(endy)?.nth(endx)?;
                        [
                            (x1, range.start.line as _),
                            (x2, range.start.line as _),
                        ]
                    })
                });
                Ok(Some(
                    hov::Hovr {
                        span,
                        item: CellBuffer {
                            c: w,
                            vo: 0,
                            cells: cells.into(),
                        },
                        range: x.range,
                        // range: x.range.and_then(|x| text.l_range(x)),
                    }
                    .into(),
                ))
            });
        self.state
            .consume(Action::SetHovering(handle, cursor_position))
            .unwrap();
        // self.requests.hovering.request =
        // (DropH::new(handle), cursor_position).into();
        // requests.hovering.result = None;
        //                             lsp!().map(|(cl, o)| {
        // let window = window.clone();
        // });
        //                                 });
    }
}
