#![allow(dead_code, unused, unexpected_cfgs)]
use Default::default;
use NamedKey::*;
use lsp_types::request::HoverRequest;
use lsp_types::*;
use regex::Regex;
use rust_analyzer::lsp::ext::Runnable;
use winit::event::MouseButton;
use winit::keyboard::{Key, NamedKey, SmolStr};

use crate::commands::Commands;
use crate::edi::handle2;
use crate::gotolist::GoToList;
use crate::hov::{Hoverable, Hovr, Hovring};
use crate::lsp::{AQErr, RequestError, Rq, RqS};
use crate::menu::generic::{GenericMenu, MenuData};
use crate::sym::{Symbols, SymbolsList};
use crate::text::TextArea;
use crate::{
    BoolRequest, CLICKING, InputRequest, act, alt, ctrl, handle, shift,
};

impl Default for State {
    fn default() -> Self {
        Self::Default
    }
}
#[derive(Debug, Copy, Clone)]
pub enum Direction {
    Above,
    Below,
}
#[derive(Debug, Copy, Clone)]
pub enum LR {
    Left,
    Right,
}
rust_fsm::state_machine! {
#[derive(Debug)]
pub(crate) State => #[derive(Debug)] pub(crate) Action => #[derive(Debug)] pub(crate) Do

Dead => K(Key => _) => Dead,
Default => {
    K(Key::Character("s") if ctrl()) => Save [Save],
    K(Key::Character("q") if ctrl()) => Dead [Quit],
    K(Key::Character("v") if ctrl()) => _ [Paste],
    K(Key::Character("z") if ctrl()) => _ [Undo],
    K(Key::Character("d") if ctrl()) => _ [GoToMatch],
    K(Key::Character("y") if ctrl()) => _ [Redo],
    K(Key::Character("f") if ctrl()) => Procure((default(), InputRequest::Search)),
    K(Key::Character("o") if ctrl()) => Procure((default(), InputRequest::OpenFile)),
    K(Key::Character("c") if ctrl()) => _,
    K(Key::Character("l") if ctrl()) => _ [Symbols],
    K(Key::Character(".") if ctrl()) => _ [CodeAction],
    K(Key::Character("0") if ctrl()) => _ [MatchingBrace],
    K(Key::Character("`") if ctrl()) => _ [SpawnTerminal],
    K(Key::Character("/") if ctrl()) => Default [Comment(State => State::Default)],
    K(Key::Character("p") if ctrl()) => Command(Commands => default()),
    K(Key::Named(Backspace) if alt()) => _ [DeleteBracketPair],
    K(Key::Named(F1)) => Procure((default(), InputRequest::RenameSymbol)),
    K(Key::Named(F10)) => GoToL(GoToList => default()) [GoToImplementations],
    K(Key::Named(k @ (ArrowUp | ArrowDown)) if alt()) => _ [InsertCursor(Direction => {
        if k == ArrowUp {Direction::Above} else { Direction::Below }
    })],
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End) if shift()) => Selection [StartSelection],
    M(MouseButton::Left if shift()) => Selection [StartSelection],
    M(MouseButton::Left if alt()) => _ [InsertCursorAtMouse],
    M(MouseButton::Left if ctrl()) => _ [GoToDefinition(Option<TextDocumentPositionParams> => None)],
    M(MouseButton::Left) => _ [MoveCursor],
    K(Key::Character("=") if ctrl()) => _ [NavForward],
    K(Key::Character("-") if ctrl()) => _ [NavBack],
    M(MouseButton::Back) => _ [NavBack],
    M(MouseButton::Forward) => _ [NavForward],
    C(((usize, usize)) => .. if unsafe { CLICKING }) => Selection [StartSelection],
    Changed => RequestBoolean(BoolRequest => BoolRequest::ReloadFile),
    K(Key::Named(Escape)) => _ [Escape],
    C(_) => Hovered [Hover],
    K(_) => _ [Edit],
    M(_) => _,
},
Hovered => {
    HOnSomething(((usize, usize)) => pos) => Hovering(Rq<Hovring, Option<Hovr>, ((usize, usize), TextDocumentPositionParams), RequestError<HoverRequest>> => default()) [SetHovering],
    HOnNothing => Default,
},
Hovering(x) => {
    // now hovering over something else, cancel existing hover
    HOnSomething(pos if let Some((_, (c, _))) = x.request && (c != pos)) => _ [SetHovering],
    HOnSomething(_) => _ [SetHovering],
    HOnNothing => Default,
    SetHovering((Option<Hovring>, tokio::task::JoinHandle<
            Result<Option<Hovr>, RequestError<HoverRequest>>,
        >, ((usize, usize), TextDocumentPositionParams)) => (v, h, d)) => Hovering({ let mut x = x; x.result = v; x.request_d(h, d); x }),
    C(_) => _ [Hover],
    MovedOut => Default,

    // reconsidering
    K(Key::Character("s") if ctrl()) => Save [Save],
    K(Key::Character("q") if ctrl()) => Dead [Quit],
    K(Key::Character("v") if ctrl()) => Default  [Paste],
    K(Key::Character("z") if ctrl()) => Default  [Undo],
    K(Key::Character("d") if ctrl()) => Default  [GoToMatch],
    K(Key::Character("y") if ctrl()) => Default  [Redo],
    K(Key::Character("f") if ctrl()) => Procure((default(), InputRequest::Search)),
    K(Key::Character("o") if ctrl()) => Procure((default(), InputRequest::OpenFile)),
    K(Key::Character("c") if ctrl()) => Default ,
    K(Key::Character("l") if ctrl()) => Default  [Symbols],
    K(Key::Character(".") if ctrl()) => Default  [CodeAction],
    K(Key::Character("0") if ctrl()) => Default  [MatchingBrace],
    K(Key::Character("`") if ctrl()) => Default  [SpawnTerminal],
    K(Key::Character("/") if ctrl()) => Default [Comment(State => State::Default)],
    K(Key::Character("p") if ctrl()) => Command(Commands => default()),
    K(Key::Named(Backspace) if alt()) => Default  [DeleteBracketPair],
    K(Key::Named(F1)) => Procure((default(), InputRequest::RenameSymbol)),
    K(Key::Named(F10)) => GoToL(GoToList => default()) [GoToImplementations],
    K(Key::Named(k @ (ArrowUp | ArrowDown)) if alt()) => Default  [InsertCursor(Direction => {
        if k == ArrowUp {Direction::Above} else { Direction::Below }
    })],
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End) if shift()) => Selection [StartSelection],
    M(MouseButton::Left if shift()) => Selection [StartSelection],
    M(MouseButton::Left if alt()) => Default  [InsertCursorAtMouse],
    M(MouseButton::Left if ctrl()) => Default  [GoToDefinition(Option<TextDocumentPositionParams> => None)],
    M(MouseButton::Left) => Default [MoveCursor],
    K(Key::Character("=") if ctrl()) => Default  [NavForward],
    K(Key::Character("-") if ctrl()) => Default  [NavBack],
    M(MouseButton::Back) => Default  [NavBack],
    M(MouseButton::Forward) => _ [NavForward],
    C(((usize, usize)) => .. if unsafe { CLICKING }) => Selection [StartSelection],
    Changed => RequestBoolean(BoolRequest => BoolRequest::ReloadFile),
    K(Key::Named(Escape)) => Default  [Escape],
    K(_) => Default  [Edit],
    // duplicate zone


    M(MouseButton::Left if ctrl()) => Default [GoToDefinition(x.request.map(|x| x.1.1).or(x.result.and_then(|x| x.of.iter().find_map(|x| x.tdpp()))))],
    M(_) => _ [ClickedHover],
},
Command(_) => K(Key::Named(Escape)) => Default,
Command(t) => K(Key::Named(Enter) if let Some(Ok(x)) = t.sel(None)) => Default [ProcessCommand((Commands, crate::commands::Cmd) => (t, x))],
Command(t) => K(Key::Named(Enter)) => _,
Command(mut t) => K(Key::Named(Tab) if shift()) => Command({ t.back();t }),
Command(mut t) => K(Key::Named(Tab)) => Command({ t.next(); t }),
Command(mut t) => K(k) => Command({ if let Some(_) = handle2(&k, &mut t.tedit, None) {
    t.selection = 0; t.vo = 0;
}; t }) [CmdTyped],
Command(t) => C(_) => _,
Command(t) => K(_) => _,
Runnables(_x) => {
    K(Key::Named(Escape)) => Default,
},
Runnables(RqS::<crate::runnables::Runnables, rust_analyzer::lsp::ext::Runnables> => Rq { result: Some(mut x), request }) => {
    K(Key::Named(Tab) if shift()) => Runnables({ x.next(); Rq { result: Some(x), request }}),
    K(Key::Named(ArrowDown)) => Runnables({ x.next(); Rq { result: Some(x), request }}),
    K(Key::Named(ArrowUp | Tab)) => Runnables({ x.back(); Rq { result: Some(x), request }}),
    K(Key::Named(Enter) if let Some(Ok(x_)) = x.clone().sel(None)) => Default [Run(Runnable => x_.clone())],
    K(k) => Runnables({
 if let Some(_) = handle2(&k, &mut x.tedit, None) {
    x.selection = 0; x.vo = 0;
}; Rq { result: Some(x), request } }),
},
Runnables(_x) => {
    C(_) => _,
    M(_) => _,
},

GoToL(x) => {
    K(Key::Named(Tab) if shift()) => GoToL({ let mut x = x; x.next(); x }) [GT],
    K(Key::Named(ArrowDown)) => GoToL({ let mut x = x; x.next(); x }) [GT],
    K(Key::Named(ArrowUp | Tab)) => GoToL({ let mut x = x; x.back(); x }) [GT],
    K(Key::Named(Enter)) => Default [GTLSelect(GoToList => x)],
    K(Key::Named(Escape)) => Default,
    // K(_) => _ [GTLHandleKey],
    C(_) => _,
    M(_) => _,
},
Symbols(Rq { result: Some(_x), request: _rq }) => {
    K(Key::Named(Tab) if shift()) => _ [SymbolsSelectNext],
    K(Key::Named(ArrowDown)) => _ [SymbolsSelectNext],
    K(Key::Named(ArrowUp | Tab)) => _ [SymbolsSelectPrev],
    K(Key::Named(Enter)) => Default [SymbolsSelect(Symbols => _x)],
    K(Key::Named(Escape)) => Default,
},
Symbols(Rq::<Symbols, Option<SymbolsList>, (), AQErr> => _rq) => {
    K(Key::Character("d") if ctrl()) => _ [SwitchType], // crahs cond methinks
    K(Key::Named(Escape)) => Default,
    K(_) => _ [SymbolsHandleKey],
    C(_) => _,
    M(_) => _,
},
CodeAction(Rq { result : Some(_x), request }) => {
    K(Key::Named(Tab) if shift()) => _ [CASelectPrev],
    K(Key::Named(ArrowDown | Tab)) => _ [CASelectNext],
    K(Key::Named(ArrowUp)) => _ [CASelectPrev],
    K(Key::Named(Enter | ArrowRight)) => _ [CASelectRight],
    K(Key::Named(ArrowLeft)) => _ [CASelectLeft],
},
CodeAction(RqS<act::CodeActions, lsp_request!("textDocument/codeAction")> => rq) => {
    K(Key::Named(Escape)) => Default,
    C(_) => _,
    M(_) => _,
    K(_) => _,
},
Selection => {
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End) if shift()) => Selection [UpdateSelection],
    M(MouseButton::Left if shift()) => Selection [ExtendSelectionToMouse],
}, // note: it does in fact fall through. this syntax is not an arm, merely shorthand.
Selection => {
    C(_ if unsafe { CLICKING }) => _ [ExtendSelectionToMouse],
    C(_) => _,
    M(MouseButton => MouseButton::Left) => Default [MoveCursor],
    K(Key::Named(Backspace)) => Default [Delete],
    K(Key::Character("x") if ctrl()) => Default [Cut],
    K(Key::Character("c") if ctrl()) => Default [Copy],
    K(Key::Character("v") if ctrl()) => Default [PasteOver],
    K(Key::Character("/") if ctrl()) => Default [Comment(State::Selection)],
    M(_) => _,

    K(Key::Character(y) if !ctrl()) => Default [Insert(SmolStr => y)],
    K(Key::Named(ArrowLeft)) => Default [SetCursor(LR => LR::Left)],
    K(Key::Named(ArrowRight)) => Default [SetCursor(LR::Right)],
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
Procure((t, InputRequest::RenameSymbol)) => K(Key::Named(Enter)) => Default [RenameSymbol(String => t.rope.to_string())],
Procure((t, a)) => K(k) => Procure((handle(k, t), a)),
Procure((t, a)) => C(_) => Procure((t, a)),
RequestBoolean(t) => {
    K(Key::Character("y")) => Default [Boolean((BoolRequest, bool) => (t, true))],
    K(Key::Character("n")) => Default [Boolean((t, false))],
    K(Key::Named(Escape)) => Default [Boolean((t, false))],
    K(_) => RequestBoolean(t),
    C(_) => _,
    Changed => _,
    M(_) => _,
},
Search((x, y, m)) => {
    M(MouseButton::Left) => Default [MoveCursor],
    C(_) => Search((x, y, m)),
    K(Key::Named(Enter) if shift()) => Search((x, y.checked_sub(1).unwrap_or(m-1), m)) [SearchChanged],
    K(Key::Named(Enter)) => Search((Regex, usize, usize) => (x,   (y+ 1) % m, m)) [SearchChanged],
    K(_) => Default [Reinsert],
}
}
