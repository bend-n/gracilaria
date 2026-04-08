use std::ops::Range;

use implicit_fn::implicit_fn;
use lsp_types::Position;
use ropey::Rope;

pub trait RopeExt {
    fn position(&self, r: Range<usize>) -> Option<[(usize, usize); 2]>;
    /// number of lines
    fn l(&self) -> usize;

    // input: char, output: utf8
    fn x_bytes(&self, c: usize) -> Option<usize>;
    fn y(&self, c: usize) -> Option<usize>;
    fn x(&self, c: usize) -> Option<usize>;
    fn xy(&self, c: usize) -> Option<(usize, usize)>;

    fn beginning_of_line(&self, c: usize) -> Option<usize>;

    fn indentation_of(&self, n: usize) -> usize;

    /// or eof
    fn eol(&self, li: usize) -> usize;

    fn l_pos_to_char(&self, p: Position) -> Option<(usize, usize)>;
    fn l_position(&self, p: Position) -> Option<usize>;
    fn to_l_position(&self, l: usize) -> Option<lsp_types::Position>;
    fn l_range(&self, r: lsp_types::Range) -> Option<Range<usize>>;
    fn to_l_range(&self, r: Range<usize>) -> Option<lsp_types::Range>;
}
impl RopeExt for Rope {
    fn position(
        &self,
        Range { start, end }: Range<usize>,
    ) -> Option<[(usize, usize); 2]> {
        let y1 = self.try_char_to_line(start).ok()?;
        let y2 = self.try_char_to_line(end).ok()?;
        let x1 = start - self.try_line_to_char(y1).ok()?;
        let x2 = end - self.try_line_to_char(y2).ok()?;
        [(x1, y1), (x2, y2)].into()
    }
    /// number of lines
    fn l(&self) -> usize {
        self.len_lines()
    }

    // input: char, output: utf8
    fn x_bytes(&self, c: usize) -> Option<usize> {
        let y = self.try_char_to_line(c).ok()?;
        let x = self
            .try_char_to_byte(c)
            .ok()?
            .checked_sub(self.try_line_to_byte(y).ok()?)?;
        Some(x)
    }
    fn y(&self, c: usize) -> Option<usize> {
        self.try_char_to_line(c).ok()
    }

    fn xy(&self, c: usize) -> Option<(usize, usize)> {
        let y = self.try_char_to_line(c).ok()?;
        let x = c.checked_sub(self.try_line_to_char(y).ok()?)?;
        Some((x, y))
    }

    fn x(&self, c: usize) -> Option<usize> {
        self.xy(c).map(|x| x.0)
    }
    fn beginning_of_line(&self, c: usize) -> Option<usize> {
        self.y(c).and_then(|x| self.try_line_to_char(x).ok())
    }
    #[implicit_fn]
    fn indentation_of(&self, n: usize) -> usize {
        let Some(l) = self.get_line(n) else { return 0 };
        l.chars().filter(*_ != '\n').take_while(_.is_whitespace()).count()
    }

    /// or eof
    #[lower::apply(saturating)]
    fn eol(&self, li: usize) -> usize {
        self.try_line_to_char(li)
            .map(|l| {
                l + self
                    .get_line(li)
                    .map(|x| x.len_chars() - 1)
                    .unwrap_or_default()
            })
            .unwrap_or(usize::MAX)
            .min(self.len_chars())
    }
    fn l_pos_to_char(&self, p: Position) -> Option<(usize, usize)> {
        self.l_position(p).and_then(|x| self.xy(x))
    }

    fn l_position(&self, p: Position) -> Option<usize> {
        self.try_byte_to_char(
            self.try_line_to_byte(p.line as _).ok()?
                + (p.character as usize)
                    .min(self.get_line(p.line as _)?.len_bytes()),
        )
        .ok()
    }
    fn to_l_position(&self, l: usize) -> Option<lsp_types::Position> {
        Some(Position {
            line: self.y(l)? as _,
            character: self.x_bytes(l)? as _,
        })
    }
    fn l_range(&self, r: lsp_types::Range) -> Option<Range<usize>> {
        Some(self.l_position(r.start)?..self.l_position(r.end)?)
    }
    fn to_l_range(&self, r: Range<usize>) -> Option<lsp_types::Range> {
        Some(lsp_types::Range {
            start: self.to_l_position(r.start)?,
            end: self.to_l_position(r.end)?,
        })
    }
}
