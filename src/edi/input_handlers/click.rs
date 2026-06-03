use std::sync::Arc;

use rust_fsm::StateMachine;
use winit::event::MouseButton;
use winit::window::Window;

use crate::edi::*;
impl Editor {
    pub fn click(
        &mut self,
        bt: MouseButton,
        cursor_position: (usize, usize),
        w: Arc<dyn Window>,
    ) {
        _ = self
            .requests
            .complete
            .consume(CompletionAction::Click)
            .inspect_err(|x| log::error!("transition comact {x:?}"));

        let Some(mut o) = self.transition(Action::M(bt)) else { return };
        if let Do::Reinsert = o {
            let Some(o2) = self.transition(Action::M(bt)) else { return };
            o = o2;
        };

        let text = &mut self.text;
        match o {
            Do::MoveCursor => {
                text.cursor.just(
                    text.mapped_index_at(cursor_position),
                    &text.rope,
                );
                if let Some((lsp, path)) = lsp!(self + p)
                    && self.requests.sig_help.result.is_some()
                    && let Ok(fut) =
                        lsp.request_sig_help(path, text.primary_cursor())
                {
                    self.requests.sig_help.request(lsp.runtime.spawn(fut));
                }
                self.hist.lc = text.cursor.clone();
                self.chist.push(text.primary_cursor());
                text.cursor.first().setc(&text.rope);
                self.refresh_document_highlights();
            }
            Do::NavForward => self.nav_forward(),
            Do::NavBack => self.nav_back(),
            Do::ExtendSelectionToMouse => {
                let p = text.mapped_index_at(cursor_position);
                text.cursor.first_mut().extend_selection_to(p, &text.rope);
            }
            Do::StartSelection => {
                let p = text.mapped_index_at(cursor_position);

                let x = *text.cursor.first();
                text.cursor.first_mut().sel = Some((x..x).into());
                text.cursor.first_mut().extend_selection_to(p, &text.rope);
                self.hist.lc = text.cursor.clone();
            }
            Do::GoToDefinition(tdpp) => {
                dbg!(&tdpp);
                if let Some(x) = self.requests.def.result.clone().or_else(|| {
                   tdpp.zip(lsp!(self)).and_then(|(tdpp, lsp)| {
                      lsp.request_immediate::<lsp_request!("textDocument/definition")>(
                        &GotoDefinitionParams {
                            text_document_position_params: tdpp,
                            work_done_progress_params: default(),
                            partial_result_params: default(),
                        },
                    ).ok()
                   }).flatten().and_then(|x| match &x {
                        GotoDefinitionResponse::Link([x, ..]) => Some(x.clone()),
                    _ => None,
                   })
                })
                    && let Err(e) = self.go(&x, w.clone())
                {
                    log::error!("gtd: {e}");
                }
            }
            Do::InsertCursorAtMouse => {
                text.cursor.add(
                    text.mapped_index_at(cursor_position),
                    &text.rope,
                );
                self.hist.lc = text.cursor.clone();
                self.chist.push(text.primary_cursor());
                text.cursor.first().setc(&text.rope);
            }

            _ => unreachable!(),
        }
    }
}
