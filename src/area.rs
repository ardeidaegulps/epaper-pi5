//! Named-area grid layout, measured in cells. Independent of any display size.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Area {
    pub cols: Range<usize>,
    pub rows: Range<usize>,
    pub total_cols: usize,
    pub total_rows: usize,
}

impl Area {
    pub const fn columns(template: &[&str]) -> usize {
        template[0].len()
    }

    pub const fn rows(template: &[&str]) -> usize {
        template.len()
    }

    pub const fn from_template(template: &[&str], name: char) -> Area {
        let total_rows = Area::rows(template);
        let total_cols = Area::columns(template);
        let mut top = total_rows;
        let mut bottom = 0;
        let mut left = total_cols;
        let mut right = 0;
        let mut row = 0;
        while row < total_rows {
            let cells = template[row].as_bytes();
            let mut col = 0;
            while col < total_cols {
                if cells[col] as char == name {
                    if row < top {
                        top = row;
                    }
                    if row + 1 > bottom {
                        bottom = row + 1;
                    }
                    if col < left {
                        left = col;
                    }
                    if col + 1 > right {
                        right = col + 1;
                    }
                }
                col += 1;
            }
            row += 1;
        }
        Area {
            cols: left..right,
            rows: top..bottom,
            total_cols,
            total_rows,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn origin(w: i32, h: i32) -> Rect {
        Rect { x: 0, y: 0, w, h }
    }

    pub const fn cut(self, area: Area) -> Rect {
        let x0 = self.x + self.w * area.cols.start as i32 / area.total_cols as i32;
        let x1 = self.x + self.w * area.cols.end as i32 / area.total_cols as i32;
        let y0 = self.y + self.h * area.rows.start as i32 / area.total_rows as i32;
        let y1 = self.y + self.h * area.rows.end as i32 / area.total_rows as i32;
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: [&str; 4] = ["aab", "aab", "aab", "cde"];
    const IRREGULAR: [&str; 2] = ["ab", "aa"];

    #[test]
    fn grid_cuts_a_sub_rectangle() {
        let outer = Rect {
            x: 10,
            y: 20,
            w: 300,
            h: 200,
        };
        let area = Area {
            cols: 1..3,
            rows: 0..2,
            total_cols: 3,
            total_rows: 2,
        };
        let cut = outer.cut(area);
        assert_eq!((cut.x, cut.y, cut.w, cut.h), (110, 20, 200, 200));
    }

    #[test]
    fn grid_spans_its_rectangle() {
        assert_eq!(
            Area::from_template(&TEMPLATE, 'a'),
            Area {
                cols: 0..2,
                rows: 0..3,
                total_cols: 3,
                total_rows: 4,
            }
        );
        assert_eq!(
            Area::from_template(&TEMPLATE, 'e'),
            Area {
                cols: 2..3,
                rows: 3..4,
                total_cols: 3,
                total_rows: 4,
            }
        );
    }

    #[test]
    fn grid_is_bounding_box_of_irregular_shape() {
        assert_eq!(
            Area::from_template(&IRREGULAR, 'a'),
            Area {
                cols: 0..2,
                rows: 0..2,
                total_cols: 2,
                total_rows: 2,
            }
        );
    }
}
