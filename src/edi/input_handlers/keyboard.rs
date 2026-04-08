use std::mem::take;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use Default::default;
use lsp_server::ResponseError;
use lsp_types::request::*;
use lsp_types::*;
use regex::Regex;
use ropey::Rope;
use rust_analyzer::lsp::ext::OnTypeFormatting;
use rust_fsm::StateMachine;
use tokio_util::task::AbortOnDropHandle as DropH;
use ttools::{IteratorOfTuples, IteratorOfTuplesWithF, hrf};
use winit::event::KeyEvent;
use winit::keyboard::Key;
use winit::window::Window;

use crate::edi::*;

impl Editor {
    pub fn keyboard(
        &mut self,
        event: KeyEvent,
        window: &mut Arc<dyn Window>,
    ) -> ControlFlow<()> {
        let mut o: Option<Do> = self
            .state
            .consume(Action::K(event.logical_key.clone()))
            .unwrap();

        match o {
            Some(Do::Reinsert) =>
                o = self
                    .state
                    .consume(Action::K(event.logical_key.clone()))
                    .unwrap(),
            _ => {}
        }
        match o {
            Some(Do::Escape) => {
                take(&mut self.requests.complete);
                take(&mut self.requests.sig_help);
                self.text.cursor.alone();
            }
            Some(Do::Comment(p)) => {
                ceach!(self.text.cursor, |cursor| {
                    Some(
                        if let Some(x) = cursor.sel
                            && matches!(p, State::Selection)
                        {
                            self.text.comment(x.into());
                        } else {
                            self.text
                                .comment(cursor.position..cursor.position);
                        },
                    )
                });
                self.text.cursor.clear_selections();
                change!(self, window.clone());
            }
            Some(Do::SpawnTerminal) => {
                if let Err(e) = trm::toggle(
                    self.workspace
                        .as_deref()
                        .unwrap_or(Path::new("/home/os/")),
                ) {
                    log::error!("opening terminal failed {e}");
                }
            }
            Some(Do::MatchingBrace) =>
                if let Some((l, f)) = lsp!(self + p) {
                    l.matching_brace(f, &mut self.text)
                },
            Some(Do::DeleteBracketPair) => self.delete_bracket_pair(),
            Some(Do::Symbols) =>
                if let Some((lsp, o)) = lsp!(self + p) {
                    let mut q = Rq::new(
                        lsp.runtime.spawn(
                            lsp.workspace_symbols("".into())
                                .map(|x| x.anonymize())
                                .map(|x| {
                                    x.map(|x| {
                                        x.map(SymbolsList::Workspace)
                                    })
                                }),
                        ),
                    );
                    q.result = Some(Symbols::new(
                        self.tree.as_deref().unwrap(),
                        self.text.bookmarks.clone(),
                        o.into(),
                    ));
                    self.state = State::Symbols(q);
                },
            Some(Do::SwitchType) =>
                if let Some((lsp, p)) = lsp!(self + p) {
                    let State::Symbols(Rq { result: Some(x), request }) =
                        &mut self.state
                    else {
                        unreachable!()
                    };
                    x.data.3 = sym::SymbolsType::Document;
                    let p = p.to_owned();
                    take(&mut x.data.0);
                    *request = Some((
                        DropH::new(lsp.runtime.spawn(async move {
                            lsp.document_symbols(&p)
                                .await
                                .anonymize()
                                .map(|x| x.map(SymbolsList::Document))
                        })),
                        (),
                    ));
                },
            Some(Do::ProcessCommand(mut x, z)) =>
                match Cmds::complete_or_accept(&z) {
                    crate::menu::generic::CorA::Complete => {
                        x.tedit.rope =
                            Rope::from_str(&format!("{} ", z.name()));
                        x.tedit.cursor.end(&x.tedit.rope);
                        self.state = State::Command(x);
                    }
                    crate::menu::generic::CorA::Accept => {
                        if let Err(e) =
                            self.handle_command(z, window.clone())
                        {
                            self.bar.last_action = format!("{e}");
                        }
                    }
                },
            Some(Do::CmdTyped) => {
                let State::Command(x) = &self.state else {
                    unreachable!()
                };
                if let Some(Ok(crate::commands::Cmd::GoTo(Some(x)))) =
                    x.sel()
                {
                    self.text.scroll_to_ln_centering(x as _);
                }
            }
            Some(Do::SymbolsHandleKey) => {
                if let Some(lsp) = lsp!(self) {
                    let State::Symbols(Rq { result: Some(x), request }) =
                        &mut self.state
                    else {
                        unreachable!()
                    };
                    let ptedit = x.tedit.rope.clone();
                    if handle2(
                        &event.logical_key,
                        &mut x.tedit,
                        lsp!(self + p),
                    )
                    .is_some()
                        || ptedit != x.tedit.rope
                    {
                        if x.data.3 == SymbolsType::Workspace {
                            *request = Some((
                                DropH::new(
                                    lsp.runtime.spawn(
                                        lsp.workspace_symbols(
                                            x.tedit.rope.to_string(),
                                        )
                                        .map(|x| {
                                            x.anonymize().map(|x| {
                                                x.map(
                                                    SymbolsList::Workspace,
                                                )
                                            })
                                        }),
                                    ),
                                ),
                                (),
                            ));
                        } else {
                            x.selection = 0;
                            x.vo = 0;
                        }
                        // state = State::Symbols(Rq::new(lsp.runtime.spawn(lsp.symbols("".into()))));
                    }
                }
            }
            Some(Do::SymbolsSelectNext) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &mut self.state
                else {
                    unreachable!()
                };
                x.next();
                if let Some(Ok(x)) = x.sel()
                    && Some(&*x.at.path) == self.origin.as_deref()
                {
                    match x.at {
                        sym::GoTo { path: _, at: At::R(x) } => {
                            let x = self.text.l_range(x).unwrap();
                            self.text.vo = self.text.char_to_line(x.start);
                        }
                        sym::GoTo { path: _, at: At::P(x) } =>
                            self.text.vo = self.text.char_to_line(x),
                    }
                }
            }
            Some(Do::SymbolsSelectPrev) => {
                let State::Symbols(Rq { result: Some(x), .. }) =
                    &mut self.state
                else {
                    unreachable!()
                };
                x.back();
                if let Some(Ok(x)) = x.sel()
                    && Some(&*x.at.path) == self.origin.as_deref()
                {
                    match x.at.at {
                        At::R(x) => {
                            let x = self.text.l_range(x).unwrap();
                            self.text.vo = self.text.char_to_line(x.start);
                        }
                        At::P(x) =>
                            self.text.vo = self.text.char_to_line(x),
                    }
                }
            }
            Some(Do::SymbolsSelect(x)) =>
                if let Some(Ok(x)) = x.sel()
                    && let Err(e) = self.go(x.at, window.clone())
                {
                    log::error!("alas! {e}")
                },
            Some(Do::RenameSymbol(to)) => self.rename_symbol(to),
            Some(Do::CodeAction) => self.request_code_actions(),
            Some(Do::CASelectLeft) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.left();
            }
            Some(Do::CASelectRight) => 'out: {
                let Some(lsp) = lsp!(self) else { unreachable!() };
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                let Some(act) = c.right() else { break 'out };
                let act = act.clone();
                self.state = State::Default;
                self.hist.lc = self.text.cursor.clone();
                self.hist.test_push(&mut self.text);
                let act = lsp
                    .request_immediate::<CodeActionResolveRequest>(&act)
                    .unwrap();
                if let Some(x) = act.edit
                    && let Err(e) = self.apply_wsedit(x)
                {
                    log::error!("{e}");
                }
            }
            Some(Do::CASelectNext) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.down();
            }
            Some(Do::CASelectPrev) => {
                let State::CodeAction(Rq { result: Some(c), .. }) =
                    &mut self.state
                else {
                    panic!()
                };
                c.up();
            }
            Some(Do::GoToMatch)
                if let Some(x) =
                    &self.requests.document_highlights.result =>
            {
                let lc = &self
                    .text
                    .cursor
                    .iter()
                    .max_by_key(|x| x.position)
                    .unwrap();
                let n = x
                    .iter()
                    .zip(0..)
                    .filter_map_at::<0>(|x| self.text.l_range(x.range))
                    .filter_on::<0>(hrf(|x: &std::ops::Range<usize>| {
                        x.contains(lc)
                    }))
                    .max_by_key(|x| x.0.start)
                    .unwrap()
                    .1;

                let p = self
                    .text
                    .l_position(x[(n + 1) % x.len()].range.start)
                    .unwrap();
                self.text.scroll_to(p);
                if !self.text.cursor.iter().any(|x| *x == p) {
                    self.text.cursor.add(p, &self.text.rope);
                }
            }
            Some(Do::GoToMatch) =>
                if self.requests.document_highlights.request.is_none() {
                    self.refresh_document_highlights();
                },
            Some(Do::NavBack) => self.nav_back(),
            Some(Do::NavForward) => self.nav_forward(),
            Some(
                Do::Reinsert
                | Do::GoToDefinition
                | Do::MoveCursor
                | Do::ExtendSelectionToMouse
                | Do::Hover
                | Do::InsertCursorAtMouse,
            ) => panic!(),
            Some(Do::Save) => match &self.origin {
                Some(_) => {
                    self.state.consume(Action::Saved).unwrap();
                    self.save();
                }
                None => {
                    self.state.consume(Action::RequireFilename).unwrap();
                }
            },
            Some(Do::SaveTo(x)) => {
                self.origin = Some(PathBuf::try_from(x).unwrap());
                self.save();
            }
            Some(Do::Edit) => self.handle_edit(event),
            Some(Do::Undo) => {
                self.hist.test_push(&mut self.text);
                self.hist.undo(&mut self.text).unwrap();
                self.bar.last_action = "undid".to_string();
                change!(self, window.clone());
            }
            Some(Do::Redo) => {
                self.hist.test_push(&mut self.text);
                self.hist.redo(&mut self.text).unwrap();
                self.bar.last_action = "redid".to_string();
                change!(self, window.clone());
            }
            Some(Do::Quit) => return ControlFlow::Break(()),
            Some(Do::SetCursor(x)) => {
                self.text.cursor.each(|c| {
                    let Some(r) = c.sel else { return };
                    match x {
                        LR::Left => c.position = r.start,
                        LR::Right => c.position = r.end,
                    }
                });
                self.text.cursor.clear_selections();
            }
            Some(Do::StartSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                // let mut z = vec![];
                self.text.cursor.each(|x| {
                    x.sel = Some(Ronge::from(**x..**x));
                    x.extend_selection(
                        y,
                        // **x..**x,
                        &self.text.rope,
                        &mut self.text.vo,
                        self.text.r,
                    );
                });
                // *self.state.sel() = z;
            }
            Some(Do::UpdateSelection) => {
                let Key::Named(y) = event.logical_key else { panic!() };
                self.text.cursor.each(|x| {
                    x.extend_selection(
                        y,
                        // dbg!(r.clone()),
                        &self.text.rope,
                        &mut self.text.vo,
                        self.text.r,
                    );
                });

                self.text.scroll_to_cursor();
                inlay!(self);
            }
            Some(Do::Insert(c)) => {
                // self.text.cursor.inner.clear();
                self.hist.push_if_changed(&mut self.text);
                ceach!(self.text.cursor, |cursor| {
                    let Some(r) = cursor.sel else { return };
                    _ = self.text.remove(r.into());
                    // self.text.cursor.first().setc(&self.text.rope);
                });
                self.text.insert(&c);
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::Delete) => {
                self.hist.push_if_changed(&mut self.text);
                ceach!(self.text.cursor, |cursor| {
                    let Some(r) = cursor.sel else { return };
                    _ = self.text.remove(r.into());
                });
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }

            Some(Do::Copy) => {
                self.hist.push_if_changed(&mut self.text);
                unsafe { take(&mut META) };
                let mut clip = String::new();
                self.text.cursor.each_ref(|x| {
                    if let Some(x) = x.sel {
                        unsafe {
                            META.count += 1;
                            META.splits.push(clip.len());
                        }
                        clip.extend(self.text.rope.slice(x).chars());
                    }
                });
                unsafe {
                    META.splits.push(clip.len());
                    META.hash = hash(&clip)
                };
                clipp::copy(clip);
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::Cut) => {
                self.hist.push_if_changed(&mut self.text);
                unsafe { take(&mut META) };
                let mut clip = String::new();
                self.text.cursor.each_ref(|x| {
                    if let Some(x) = x.sel {
                        unsafe {
                            META.count += 1;
                            META.splits.push(clip.len());
                        }
                        clip.extend(self.text.rope.slice(x).chars());
                    }
                });
                unsafe {
                    META.splits.push(clip.len());
                    META.hash = hash(&clip)
                };
                clipp::copy(clip);
                ceach!(self.text.cursor, |x| {
                    if let Some(x) = x.sel {
                        self.text.remove(x.into()).unwrap();
                    }
                });
                self.text.cursor.clear_selections();
                self.hist.push_if_changed(&mut self.text);
                change!(self, window.clone());
            }
            Some(Do::PasteOver) => {
                self.hist.push_if_changed(&mut self.text);
                ceach!(self.text.cursor, |cursor| {
                    let Some(r) = cursor.sel else { return };
                    _ = self.text.remove(r.into());
                });
                self.text.cursor.clear_selections();
                self.paste();
                // self.hist.push_if_changed(&mut self.text);
            }
            Some(Do::Paste) => self.paste(),
            Some(Do::OpenFile(x)) => {
                _ = self.open(Path::new(&x), window.clone());
            }
            Some(Do::StartSearch(x)) => {
                let s = Regex::new(&x).unwrap();
                let n = s
                    .find_iter(&self.text.rope.to_string())
                    .enumerate()
                    .count();
                s.clone()
                    .find_iter(&self.text.rope.to_string())
                    .enumerate()
                    .find(|(_, x)| x.start() > *self.text.cursor.first())
                    .map(|(x, m)| {
                        self.state = State::Search(s, x, n);
                        self.text.cursor.just(
                            self.text.rope.byte_to_char(m.end()),
                            &self.text.rope,
                        );
                        self.text.scroll_to_cursor_centering();
                        inlay!(self);
                    })
                    .unwrap_or_else(|| {
                        self.bar.last_action = "no matches".into()
                    });
            }
            Some(Do::SearchChanged) => {
                let (re, index, _) = self.state.search();
                let s = self.text.rope.to_string();
                let m = re.find_iter(&s).nth(*index).unwrap();
                self.text.cursor.just(
                    self.text.rope.byte_to_char(m.end()),
                    &self.text.rope,
                );
                self.text.scroll_to_cursor_centering();
                inlay!(self);
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, true)) => {
                self.hist.push_if_changed(&mut self.text);
                self.text.rope = Rope::from_str(
                    &std::fs::read_to_string(
                        self.origin.as_ref().unwrap(),
                    )
                    .unwrap(),
                );

                self.text.cursor.first_mut().position = self
                    .text
                    .cursor
                    .first()
                    .position
                    .min(self.text.rope.len_chars());
                self.mtime = Self::modify(self.origin.as_deref());
                self.bar.last_action = "reloaded".into();
                self.hist.push(&mut self.text)
            }
            Some(Do::Boolean(BoolRequest::ReloadFile, false)) => {}
            Some(Do::InsertCursor(dir)) => {
                let (x, y) = match dir {
                    Direction::Above => self.text.cursor.min(),
                    Direction::Below => self.text.cursor.max(),
                }
                .cursor(&self.text.rope);
                let y = match dir {
                    Direction::Above => y - 1,
                    Direction::Below => y + 1,
                };
                let position = self.text.line_to_char(y);
                self.text.cursor.add(position + x, &self.text.rope);
            }
            Some(Do::Run(x)) =>
                if let Some((l, ws)) =
                    lsp!(self).zip(self.workspace.as_deref())
                {
                    l.runtime
                        .block_on(crate::runnables::run(x, ws))
                        .unwrap();
                },
            Some(Do::GoToImplementations) => {
                let State::GoToL(x) = &mut self.state else {
                    unreachable!()
                };
                if let Some(l) = lsp!(self) {
                    x.data.1 = Some(crate::gotolist::O::Impl(Rq::new(
                        l.runtime.spawn(
                            l.go_to_implementations(tdpp!(self)).unwrap(),
                        ),
                    )));
                }
            }
            Some(Do::GTLSelect(x)) =>
                if let Some(Ok((g, _))) = x.sel()
                    && let Err(e) = self.go(g, window.clone())
                {
                    eprintln!("go-to-list select fail: {e}");
                },
            Some(Do::GT) => {
                let State::GoToL(x) = &mut self.state else {
                    unreachable!()
                };
                if let Some(Ok((GoTo { path: p, at: At::R(r) }, _))) =
                    x.sel()
                    && Some(&*p) == self.origin.as_deref()
                {
                    // let x = self.text.l_range(r).unwrap();
                    self.text.scroll_to_ln_centering(r.start.line as _);
                    // self.text.vo = self.text.char_to_line(x.start);
                }
            }
            None => {}
        }
        ControlFlow::Continue(())
    }
    fn handle_edit(&mut self, event: KeyEvent) {
        self.text.cursor.clear_selections();
        self.hist.test_push(&mut self.text);
        let cb4 = self.text.cursor.first();
        if let Key::Named(Enter | ArrowUp | ArrowDown | Tab) =
            event.logical_key
            && let CompletionState::Complete(..) = self.requests.complete
        {
            // dont
        } else if let Some(x) =
            handle2(&event.logical_key, &mut self.text, lsp!(self + p))
            && let Some((l, p)) = lsp!(self + p)
            && let Some(
                InitializeResult {
                    capabilities:
                        ServerCapabilities {
                            document_on_type_formatting_provider:
                                Some(DocumentOnTypeFormattingOptions {
                                    first_trigger_character,
                                    more_trigger_character: Some(t),
                                }),
                            ..
                        },
                    ..
                },
                ..,
            ) = &l.initialized
            && (first_trigger_character == first_trigger_character
                || t.iter().any(|y| y == x))
            && self.text.cursor.inner.len() == 1
            && change!(just self).is_some()
            && let Ok(Some(mut x)) = l
                .request_immediate::<OnTypeFormatting>(
                    &DocumentOnTypeFormattingParams {
                        text_document_position:
                            TextDocumentPositionParams {
                                text_document: p.tid(),
                                position: self
                                    .text
                                    .to_l_position(
                                        *self.text.cursor.first(),
                                    )
                                    .unwrap(),
                            },
                        ch: x.into(),
                        options: FormattingOptions {
                            tab_size: 4,
                            ..default()
                        },
                    },
                )
        {
            x.sort_tedits();
            for x in x {
                self.text.apply_snippet_tedit(&x).unwrap();
            }
        };
        self.text.scroll_to_cursor();
        if cb4 != self.text.cursor.first()
            && let CompletionState::Complete(Rq {
                result: Some(c), ..
            }) = &self.requests.complete
            && let at = self.text.cursor.first().at_(&self.text.rope)
            && ((self.text.cursor.first() < c.start)
                || (!crate::is_word(at) && (at != '.' || at != ':')))
        {
            self.requests.complete = CompletionState::None;
        }
        if self.requests.sig_help.running()
            && cb4 != self.text.cursor.first()
            && let Some((lsp, path)) = lsp!(self + p)
        {
            self.requests.sig_help.request(lsp.runtime.spawn(
                lsp.request_sig_help(
                    path,
                    self.text.cursor.first().cursor(&self.text.rope),
                ),
            ));
        }
        if self.hist.record(&self.text) {
            change!(self, window.clone());
        }
        lsp!(let lsp, o = self);
        match event.logical_key.as_ref() {
            Key::Character(y)
                if let Some(x) = &lsp.initialized
                    && let Some(x) =
                        &x.capabilities.signature_help_provider
                    && let Some(x) = &x.trigger_characters
                    && x.contains(&y.to_string()) =>
            {
                self.requests.sig_help.request(lsp.runtime.spawn(
                    lsp.request_sig_help(
                        o,
                        self.text.cursor.first().cursor(&self.text.rope),
                    ),
                ));
            }
            _ => {}
        }
        match self
            .requests
            .complete
            .consume(CompletionAction::K(event.logical_key.as_ref()))
            .unwrap()
        {
            Some(CDo::Request(ctx)) => {
                let h =
                    DropH::new(lsp.runtime.spawn(lsp.request_complete(
                        o,
                        self.text.cursor.first().cursor(&self.text.rope),
                        ctx,
                    )));
                let CompletionState::Complete(Rq {
                    request: x,
                    result: c,
                }) = &mut self.requests.complete
                else {
                    panic!()
                };
                use ttools::TryRefTuple;
                *x = Some((
                    h,
                    c.as_ref()
                        .map(|x| x.start)
                        .or(x.as_ref().on::<1>().copied())
                        .unwrap_or(*self.text.cursor.first()),
                ));
            }
            Some(CDo::SelectNext) => {
                let CompletionState::Complete(Rq {
                    result: Some(c), ..
                }) = &mut self.requests.complete
                else {
                    panic!()
                };
                c.next(&filter(&self.text));
            }
            Some(CDo::SelectPrevious) => {
                let CompletionState::Complete(Rq {
                    result: Some(c), ..
                }) = &mut self.requests.complete
                else {
                    panic!()
                };
                c.back(&filter(&self.text));
            }
            Some(CDo::Finish(x)) => self.apply_completion(x),
            None => return,
        };
    }
    pub fn delete_bracket_pair(&mut self) {
        lsp!(let l, f = self);
        let Ok(x) = l.matching_brace_at(
            f,
            self.text.cursor.positions(&self.text.rope),
        ) else {
            return;
        };
        use itertools::Itertools;
        for p in
            // self.text.cursor.iter()
            x
                .iter()
                .flatten()
                .flat_map(|(a, b)| {
                    [a, b].map(|c| self.text.rope.l_position(*c).unwrap())
                })
                .sorted()
                .rev()
        {
            self.text.remove(p..p + 1).unwrap();
        }
    }
    pub fn rename_symbol(&mut self, new_name: String) {
        lsp!(let lsp, f = self);
        let x = lsp
            .request_immediate::<lsp_request!("textDocument/rename")>(
                &RenameParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: f.tid(),
                        position: self
                            .text
                            .to_l_position(
                                self.text.cursor.first().position,
                            )
                            .unwrap(),
                    },
                    new_name,
                    work_done_progress_params: default(),
                },
            );

        match x {
            Ok(Some(x)) =>
                if let Err(e) = self.apply_wsedit(x) {
                    log::error!("couldnt apply one or more wsedits: {e}");
                },
            Err(RequestError::Failure(
                lsp_server::Response {
                    result: None,
                    error:
                        Some(ResponseError {
                            code: -32602,
                            message,
                            data: None,
                        }),
                    ..
                },
                ..,
            )) => self.bar.last_action = message,
            _ => {}
        }
    }

    pub fn request_code_actions(&mut self) {
        lsp!(let lsp, f = self);
        let r = lsp
            .request::<lsp_request!("textDocument/codeAction")>(
                &CodeActionParams {
                    text_document: f.tid(),
                    range: self
                        .text
                        .to_l_range(
                            self.text.cursor.first().position
                                ..self.text.cursor.first().position,
                        )
                        .unwrap(),
                    context: CodeActionContext {
                        trigger_kind: Some(CodeActionTriggerKind::INVOKED),
                        // diagnostics: if let Some((lsp, p)) = lsp!() && let uri = Url::from_file_path(p).unwrap() && let Some(diag) = lsp.requests.diagnostics.get(&uri, &lsp.requests.diagnostics.guard()) { dbg!(diag.iter().filter(|x| {
                        //                                             self.text.l_range(x.range).unwrap().contains(&self.text.cursor)
                        //                                         }).cloned().collect()) } else { vec![] },
                        ..default()
                    },
                    work_done_progress_params: default(),
                    partial_result_params: default(),
                },
            )
            .unwrap()
            .0;

        self.state = State::CodeAction(Rq::new(lsp.runtime.spawn(r)));
    }
    pub fn refresh_document_highlights(&mut self) {
        lsp!(let lsp, path = self);
        self.requests.document_highlights.request(
            lsp.runtime.spawn(
                lsp.document_highlights(
                    path,
                    self.text
                        .to_l_position(self.text.cursor.first().position)
                        .unwrap(),
                ),
            ),
        );
    }
}
