use helix_core::snippets::parser::SnippetElement;

#[derive(Debug)]
pub struct Snippet {
    stops: Vec<(Stop, usize)>,
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
        stops.iter_mut().for_each(|x| x.1 += at);
        Some((Snippet { stops }, o))
    }
    pub fn manipulate(&mut self, mut f: impl FnMut(usize) -> usize) {
        self.stops.iter_mut().for_each(|x| x.1 = f(x.1));
    }
    pub fn next(&mut self) -> usize {
        self.stops.pop().unwrap().1
    }

    pub fn apply(
        x: SnippetElement,
        stops: &mut Vec<(Stop, usize)>,
        cursor: &mut usize,
        text: &mut String,
    ) {
        match x {
            SnippetElement::Tabstop { tabstop, transform: None } =>
                stops.push((tabstop, *cursor)),
            SnippetElement::Placeholder { tabstop, value } => {
                for value in value {
                    Self::apply(value, stops, cursor, text)
                }
                stops.push((tabstop, *cursor))
            }
            SnippetElement::Choice { tabstop, choices } => {
                text.push_str(&choices[0]);
                *cursor += choices[0].chars().count();
                stops.push((tabstop, *cursor))
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
