use std::ops::Range;

use Default::default;
use NamedKey::*;
use lsp_types::*;
use regex::Regex;
use winit::event::MouseButton;
use winit::keyboard::{Key, NamedKey, SmolStr};

use crate::lsp::{RequestError, Rq, RqS};
use crate::sym::Symbols;
use crate::text::TextArea;
use crate::{
    BoolRequest, CLICKING, InputRequest, act, ctrl, handle, shift,
};

impl Default for State {
    fn default() -> Self {
        Self::Default
    }
}
rust_fsm::state_machine! {
#[derive(Debug)]
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
    K(Key::Character(x) if x == "l" && ctrl()) => _ [Symbols],
    K(Key::Character(x) if x == "." && ctrl()) => _ [CodeAction],
    K(Key::Character(x) if x == "0" && ctrl()) => _ [MatchingBrace],
    K(Key::Character(x) if x == "`" && ctrl()) => _ [SpawnTerminal],
    K(Key::Character(y) if y == "/" && ctrl()) => Default [Comment(Range<usize> => 0..0)],
    K(Key::Named(ArrowUp | ArrowLeft | ArrowDown | ArrowRight | Home | End) if shift()) => Selection(Range<usize> => 0..0) [StartSelection],
    M(MouseButton::Left if shift()) => Selection(Range<usize> => 0..0) [StartSelection],
    M(MouseButton::Left if ctrl()) => _ [GoToDefinition],
    M(MouseButton::Left) => _ [MoveCursor],
    M(MouseButton::Back) => _ [NavBack],
    M(MouseButton::Forward) => _ [NavForward],
    C(((usize, usize)) => .. if unsafe { CLICKING }) => Selection(0..0) [StartSelection],
    Changed => RequestBoolean(BoolRequest => BoolRequest::ReloadFile),
    C(_) => _ [Hover],
    K(_) => _ [Edit],
    M(_) => _,
},
Symbols(Rq { result: Some(_x), request: None }) => {
    K(Key::Named(Tab) if shift()) => _ [SymbolsSelectNext],
    K(Key::Named(ArrowDown)) => _ [SymbolsSelectNext],
    K(Key::Named(ArrowUp | Tab)) => _ [SymbolsSelectPrev],
    K(Key::Named(Enter)) => _ [SymbolsSelect],
    K(Key::Named(Escape)) => Default,
    K(_) => _ [SymbolsHandleKey],
},
Symbols(Rq::<Symbols, Vec<SymbolInformation>, (), RequestError<lsp_request!("workspace/symbol")>> => _rq) => {
    K(Key::Named(Escape)) => Default,
    C(_) => _,
    M(_) => _,
    K(_) => _,
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
    K(Key::Character(y) if y == "/" && ctrl()) => Default [Comment(Range<usize> => x)],

    K(Key::Character(y) if !ctrl()) => Default [Insert((Range<usize>, SmolStr) => (x, y))],
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
