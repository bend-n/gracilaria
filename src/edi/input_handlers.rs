use std::path::Path;

use winit::keyboard::Key;
mod click;
mod cursor;
mod keyboard;
use crate::edi::*;
pub fn handle2<'a>(
    key: &'a Key,
    text: &mut TextArea,
    l: Option<(Arc<Client>, &Path)>,
) -> Option<&'a str> {
    use Key::*;

    match key {
        Character(" ") => text.insert(" "),
        Named(Backspace) if ctrl() => text.backspace_word(),
        Named(Backspace) => text.backspace(),
        Named(Home) if ctrl() => {
            text.cursor.just(0, &text.rope);
            text.vo = 0;
        }
        Named(End) if ctrl() => {
            text.cursor.just(text.rope.len_chars(), &text.rope);
            text.vo = text.l().saturating_sub(text.r);
        }
        Character("e") if alt() => text.end(),
        Character("q") if alt() => text.home(),
        Named(Home) => text.home(),
        Named(End) => text.end(),
        Named(Tab) if shift() => text.dedent().unwrap(),
        Named(Tab) => text.tab(),
        Named(Delete) => {
            text.right();
            text.backspace()
        }
        Character("a") if alt() && ctrl() => text.word_left(),
        Character("d") if alt() && ctrl() => text.word_right(),
        Character("a") if alt() => text.left(),
        Character("w") if alt() => text.up(),
        Character("s") if alt() => text.down(),
        Character("d") if alt() => text.right(),
        Named(ArrowLeft) if ctrl() => text.word_left(),
        Named(ArrowRight) if ctrl() => text.word_right(),
        Named(ArrowLeft) => text.left(),
        Named(ArrowRight) => text.right(),
        Named(ArrowUp) => text.up(),
        Named(ArrowDown) => text.down(),
        Named(PageDown) => text.page_down(),
        Named(PageUp) => text.page_up(),
        Named(Enter) if let Some((l, p)) = l => l.enter(p, text).unwrap(),
        Named(Enter) => text.enter(),
        Character(x) => {
            text.insert(&x);
            return Some(x);
        }
        _ => {}
    };
    None
}
impl Editor {
    #[track_caller]
    pub fn transition(&mut self, a: Action) -> Option<Do> {
        self.state
            .consume(a)
            .inspect_err(|e| log::error!("transition failed: {e}"))
            .ok()
            .flatten()
    }
}
