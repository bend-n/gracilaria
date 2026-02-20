use dsb::cell::Style;
use log::error;
use lsp_types::{SemanticToken, SemanticTokensLegend};
use serde_derive::{Deserialize, Serialize};

pub mod theme;
pub(crate) use theme::theme;
use theme::{COLORS, MCOLORS, MODIFIED, MSTYLE, NAMES, STYLES};

use crate::text::TextArea;

#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct TokenD {
    pub range: (u32, u32),
    pub ty: u32,
    pub modifiers: u32,
}
impl TokenD {
    pub fn manip(&mut self, mut f: impl FnMut(usize) -> usize) {
        self.range.0 = f(self.range.0 as _) as _;
        self.range.1 = f(self.range.1 as _) as _;
    }
    pub fn style(self, leg: &SemanticTokensLegend) -> Style {
        let mut sty = Style::new(crate::FG, crate::BG);
        let Some(tty) = leg.token_types.get(self.ty as usize) else {
            error!(
                "issue while loading semantic token {self:?}; couldnt \
                 find in legend"
            );
            return sty;
        };
        if let Some(f) = NAMES.iter().position(|&x| x == tty.as_str()) {
            // cells
            //     .get_range_enumerated((x1, ln as _), (x2, ln as _))
            //     .filter(|(_, i)| {
            //         matches!(src_map.get(i.0), Some(Mapping::Char(..)))
            //     })
            //     .for_each(|(x, _)| {
            sty.fg = COLORS[f];
            sty.flags |= STYLES[f];
            // });
        }
        // println!(
        //     "{tty:?}: {}",
        //     slice.iter().flat_map(|x| x.letter).collect::<String>()
        // );
        let mut modi = self.modifiers;
        while modi != 0 {
            let bit = modi.trailing_zeros();

            leg.token_modifiers
                .get(bit as usize)
                .and_then(|modi| {
                    MODIFIED.iter().position(|&(x, y)| {
                        (x == tty.as_str()) & (y == modi.as_str())
                    })
                })
                .map(|i| {
                    sty.fg = MCOLORS[i];
                    sty.flags |= MSTYLE[i];
                });

            modi &= !(1 << bit);
        }
        sty
    }
}
impl TextArea {
    pub fn set_toks(&mut self, toks: &[SemanticToken]) {
        let mut ln = 0;
        let mut ch = 0;
        self.tokens.clear();
        for t in toks {
            ln += t.delta_line;
            ch = match t.delta_line {
                1.. => t.delta_start,
                0 => ch + t.delta_start,
            };
            if t.length == 0 {
                continue;
            }
            let Ok((x1, x2)): ropey::Result<_> = (try {
                let p1 = self.rope.try_byte_to_char(
                    self.rope.try_line_to_byte(ln as _)? + ch as usize,
                )?;
                let p2 = self.rope.try_byte_to_char(
                    self.rope.try_line_to_byte(ln as _)?
                        + ch as usize
                        + t.length as usize,
                )?;
                (p1 as u32, p2 as u32)
            }) else {
                continue;
            };
            self.tokens.push(TokenD {
                range: (x1, x2),
                ty: t.token_type,
                modifiers: t.token_modifiers_bitset,
            });
        }
        // tokens are sorted by definition
    }
}
