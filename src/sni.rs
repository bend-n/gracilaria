use std::ops::Range;

use helix_core::snippets::parser::SnippetElement;

#[derive(Debug)]
pub struct Snippet {
    stops: Vec<(Stop, StopP)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StopP {
    Just(usize),
    Range(Range<usize>),
}
impl StopP {
    pub fn r(self) -> Range<usize> {
        match self {
            Self::Just(x) => x..x,
            Self::Range(range) => range,
        }
    }
    pub fn manipulate(&mut self, mut f: impl FnMut(usize) -> usize) {
        match self {
            Self::Just(x) => *x = f(*x),
            Self::Range(range) => {
                range.start = f(range.start);
                range.end = f(range.end);
            }
        }
    }
}

impl Snippet {
    pub fn parse(x: &str, at: usize) -> Option<(Self, String)> {
        let value = helix_core::snippets::parser::parse(x).ok()?;

        let mut stops = vec![];
        let mut cursor = 0;
        let mut o = String::default();
        // snippets may request multiple cursors; i dont support multiple cursors yet
        for value in value.into_iter() {
            Self::apply(value, &mut stops, &mut cursor, &mut o);
        }
        stops.sort_by_key(|x| x.0);
        stops.iter_mut().for_each(|x| x.1.manipulate(|x| x + at));
        Some((Snippet { stops }, o))
    }
    pub fn manipulate(&mut self, f: impl FnMut(usize) -> usize + Clone) {
        self.stops.iter_mut().for_each(|x| x.1.manipulate(f.clone()));
    }
    pub fn next(&mut self) -> StopP {
        self.stops.pop().unwrap().1
    }

    pub fn apply(
        x: SnippetElement,
        stops: &mut Vec<(Stop, StopP)>,
        cursor: &mut usize,
        text: &mut String,
    ) {
        match x {
            SnippetElement::Tabstop { tabstop, transform: None } =>
                stops.push((tabstop, StopP::Just(*cursor))),
            SnippetElement::Placeholder { tabstop, value } => {
                let start = *cursor;
                for value in value {
                    Self::apply(value, stops, cursor, text)
                }
                stops.push((tabstop, StopP::Range(start..*cursor)))
            }
            SnippetElement::Choice { tabstop, choices } => {
                let start = *cursor;
                text.push_str(&choices[0]);
                *cursor += choices[0].chars().count();
                stops.push((tabstop, StopP::Range(start..*cursor)))
            }
            SnippetElement::Text(x) => {
                text.push_str(&x);
                *cursor += x.chars().count();
            }
            _ => unimplemented!(),
        }
    }
}
type Stop = usize;
