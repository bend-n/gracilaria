use Default::default;
use lsp_types::{InlayHint, InlayHintLabel, Location};
use serde_derive::{Deserialize, Serialize};

use crate::text::{RopeExt, TextArea};

pub type Inlay = Marking<Box<[(char, Option<Location>)]>>;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Marking<D> {
    /// in characters
    pub position: u32,
    pub data: D,
}
impl<D: Default> Marking<D> {
    pub fn idx(x: u32) -> Self {
        Self { position: x, ..default() }
    }
}
impl<D> Ord for Marking<D> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.position.cmp(&other.position)
    }
}
impl<D> PartialOrd for Marking<D> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.position.partial_cmp(&other.position)
    }
}
impl<D> Eq for Marking<D> {}
impl<D> PartialEq for Marking<D> {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
    }
}
impl TextArea {
    #[implicit_fn::implicit_fn]
    pub fn set_inlay(&mut self, inlay: &[InlayHint]) {
        self.inlays = inlay
            .iter()
            .filter_map(|i| {
                let mut label = match &i.label {
                    InlayHintLabel::String(x) =>
                        x.chars().map(|x| (x, None)).collect::<Vec<_>>(),
                    InlayHintLabel::LabelParts(v) => v
                        .iter()
                        .flat_map(|x| {
                            x.value
                                .chars()
                                .map(|y| (y, x.location.clone()))
                        })
                        .collect(),
                };
                if i.padding_left == Some(true) {
                    label.insert(0, (' ', None));
                }
                if i.padding_right == Some(true) {
                    label.push((' ', None));
                }
                let position = self.l_position(i.position)? as _;
                Some(Marking { position, data: label.into() })
            })
            .collect();
    }
}
