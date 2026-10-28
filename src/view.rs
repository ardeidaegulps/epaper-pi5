//! View: renders the model into a frame without changing state.
use crate::area::{Area, Rect};
use crate::model::{Model, UpdateReport};
use crate::{FRAME_LEN, WIDTH_BYTES};
use embedded_graphics::image::Image;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_7X13_BOLD, FONT_10X20};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle, TextStyleBuilder};
use embedded_iconoir::icons::size24px::actions::CheckCircle;
use embedded_iconoir::icons::size24px::emojis::EmojiPuzzled;
use embedded_iconoir::prelude::IconoirNewIcon;
use std::convert::Infallible;

type Result<T> = std::result::Result<T, Infallible>;

trait Target: DrawTarget<Color = BinaryColor, Error = Infallible> {}

impl<T: DrawTarget<Color = BinaryColor, Error = Infallible>> Target for T {}

const DEBUG_GRID: bool = false;
const DEBUG_PLACEHOLDER: bool = false;

const WIDTH: i32 = crate::HEIGHT as i32;
const HEIGHT: i32 = (WIDTH_BYTES * 8) as i32;
const SCREEN: Rect = Rect::origin(WIDTH, HEIGHT);

const GRID_TEMPLATE_AREAS: [&str; 4] = ["aab", "aab", "aab", "cde"];
const AREA_A: Rect = SCREEN.cut(Area::from_template(&GRID_TEMPLATE_AREAS, 'a'));
const AREA_B: Rect = SCREEN.cut(Area::from_template(&GRID_TEMPLATE_AREAS, 'b'));
const AREA_C: Rect = SCREEN.cut(Area::from_template(&GRID_TEMPLATE_AREAS, 'c'));
const AREA_D: Rect = SCREEN.cut(Area::from_template(&GRID_TEMPLATE_AREAS, 'd'));
const AREA_E: Rect = SCREEN.cut(Area::from_template(&GRID_TEMPLATE_AREAS, 'e'));

const ITEM_TOP: i32 = 18;
const ICON_SIZE: i32 = 24;
const ID_RIGHT: i32 = 30;
const INCIDENT_LINES: i32 = (AREA_A.h - ITEM_TOP) / LINE_HEIGHT;
const INCIDENT_SLOTS: usize = 3;
const LINE_HEIGHT: i32 = FONT_6X10.character_size.height as i32 + LINE_SPACING;
const LINE_SPACING: i32 = 2;
const MESSAGE_CHARS: usize =
    ((AREA_A.w - MESSAGE_LEFT) / FONT_6X10.character_size.width as i32) as usize;
const MESSAGE_LEFT: i32 = ID_RIGHT + FONT_6X10.character_size.width as i32;
const UNAVAILABLE: &str = "---";

/// Renders the model into a frame. Pure: the frame depends only on the model.
pub fn view(model: &Model) -> Vec<u8> {
    let mut frame = vec![0xFFu8; FRAME_LEN];
    let mut canvas = Canvas { frame: &mut frame };
    draw_frame(model, &mut canvas).expect("drawing to an in-memory canvas cannot fail");
    frame
}

fn draw_frame<T: Target>(model: &Model, canvas: &mut T) -> Result<()> {
    if DEBUG_GRID {
        for rect in [AREA_A, AREA_B, AREA_C, AREA_D, AREA_E] {
            let outline = Rectangle::new(
                Point::new(rect.x, rect.y),
                Size::new(rect.w as u32, rect.h as u32),
            );
            outline
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                .draw(canvas)?;
        }
    }
    let average_time = match report(model) {
        None => {
            let icon = EmojiPuzzled::new(BinaryColor::On);
            let x = AREA_A.x + (AREA_A.w - ICON_SIZE) / 2;
            let y = AREA_A.y + (AREA_A.h - ICON_SIZE) / 2;
            Image::new(&icon, Point::new(x, y)).draw(canvas)?;
            draw_stat(canvas, UNAVAILABLE)?;
            None
        }
        Some(report) if report.runs.iter().all(|run| run.passed) => {
            let icon = CheckCircle::new(BinaryColor::On);
            let x = AREA_A.x + (AREA_A.w - ICON_SIZE) / 2;
            let y = AREA_A.y + (AREA_A.h - ICON_SIZE) / 2;
            Image::new(&icon, Point::new(x, y)).draw(canvas)?;
            let rate = format!("{}/{}", report.failed, report.failed + report.passed);
            draw_stat(canvas, &rate)?;
            compute_average_time(report)
        }
        Some(report) => {
            draw_list(canvas, report)?;
            let rate = format!("{}/{}", report.failed, report.failed + report.passed);
            draw_stat(canvas, &rate)?;
            compute_average_time(report)
        }
    };
    let average = average_time.as_deref().unwrap_or(UNAVAILABLE);
    draw_card(canvas, "AVERAGE", average, AREA_C)?;
    draw_card(canvas, "SERVICE", model.service_status.as_str(), AREA_D)?;
    draw_card(canvas, "ETHERNET", model.ethernet_status.as_str(), AREA_E)
}

fn draw_list<T: Target>(canvas: &mut T, report: &UpdateReport) -> Result<()> {
    let incidents: Vec<(u64, &str)> = report
        .runs
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, run)| !run.passed)
        .take(INCIDENT_SLOTS)
        .map(|(index, run)| {
            let reason = run.reason.as_deref().unwrap_or("run failed");
            (index as u64 + 1, reason)
        })
        .collect();
    let max_lines_per_item = INCIDENT_LINES as usize / incidents.len().max(1);
    let mut line = 0;

    for (run, reason) in incidents {
        let id = format!("#{:02}", run);
        let baseline = item_baseline(line);
        Text::with_text_style(
            &id,
            Point::new(AREA_A.x + ID_RIGHT, baseline),
            MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
            TextStyle::with_alignment(Alignment::Right),
        )
        .draw(canvas)?;

        let mut texts = wrap(reason, MESSAGE_CHARS).peekable();
        for rest in (0..max_lines_per_item).rev() {
            let Some(text) = texts.next() else { break };
            let ellipsized;
            let truncated = rest == 0 && texts.peek().is_some();
            let row = if truncated {
                ellipsized = text
                    .chars()
                    .take(MESSAGE_CHARS.saturating_sub(2))
                    .chain("..".chars())
                    .collect::<String>();
                ellipsized.as_str()
            } else {
                text
            };
            Text::new(
                row,
                Point::new(AREA_A.x + MESSAGE_LEFT, item_baseline(line)),
                MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
            )
            .draw(canvas)?;
            line += 1;
        }
    }
    Ok(())
}

fn draw_stat<T: Target>(canvas: &mut T, value: &str) -> Result<()> {
    Text::with_text_style(
        value,
        Point::new(AREA_B.center_x(), AREA_B.y + AREA_B.h / 2),
        MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
        TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Middle)
            .build(),
    )
    .draw(canvas)?;
    Ok(())
}

fn draw_card<T: Target>(canvas: &mut T, label: &str, value: &str, rect: Rect) -> Result<()> {
    Text::with_text_style(
        label,
        Point::new(rect.center_x(), rect.stack_label_baseline()),
        MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
        TextStyle::with_alignment(Alignment::Center),
    )
    .draw(canvas)?;
    Text::with_text_style(
        value,
        Point::new(rect.center_x(), rect.stack_value_baseline()),
        MonoTextStyle::new(&FONT_7X13_BOLD, BinaryColor::On),
        TextStyle::with_alignment(Alignment::Center),
    )
    .draw(canvas)?;
    Ok(())
}

fn report(model: &Model) -> Option<&UpdateReport> {
    model.report.as_ref().filter(|_| !DEBUG_PLACEHOLDER)
}

fn compute_average_time(report: &UpdateReport) -> Option<String> {
    let passed_runs = report.runs.iter().filter(|run| run.passed);
    let passed_count = passed_runs.clone().count() as i32;
    let total_time: time::Duration = passed_runs
        .map(|run| run.update_end - run.update_start)
        .sum();
    total_time
        .checked_div(passed_count)
        .map(format_minutes_seconds)
}

fn format_minutes_seconds(duration: time::Duration) -> String {
    let secs = duration.whole_seconds().max(0);
    format!("{}m{}s", secs / 60, secs % 60)
}

impl Rect {
    const fn center_x(self) -> i32 {
        self.x + self.w / 2
    }

    const fn stack_label_baseline(self) -> i32 {
        let block = LINE_HEIGHT + FONT_7X13_BOLD.character_size.height as i32;
        self.y + (self.h - block) / 2 + FONT_6X10.baseline as i32
    }

    const fn stack_value_baseline(self) -> i32 {
        self.stack_label_baseline() + LINE_HEIGHT
    }
}

const fn item_baseline(line: i32) -> i32 {
    AREA_A.y + ITEM_TOP + line * LINE_HEIGHT
}

fn wrap(text: &str, max_chars: usize) -> impl Iterator<Item = &str> {
    let base = text.as_ptr() as usize;
    let mut words = text.split_whitespace().peekable();
    std::iter::from_fn(move || {
        let first = words.next()?;
        let start = first.as_ptr() as usize - base;
        let mut end = start + first.len();
        while let Some(word) = words.peek() {
            let word_end = word.as_ptr() as usize - base + word.len();
            if word_end - start > max_chars {
                break;
            }
            end = word_end;
            words.next();
        }
        Some(&text[start..end])
    })
}

struct Canvas<'a> {
    frame: &'a mut [u8],
}

impl OriginDimensions for Canvas<'_> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for Canvas<'_> {
    type Color = BinaryColor;
    type Error = Infallible;

    // Landscape (x, y) maps to portrait (127 - y, x), matching a 90 degree clockwise rotation.
    fn draw_iter<I>(&mut self, pixels: I) -> std::result::Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<BinaryColor>>,
    {
        for Pixel(Point { x, y }, color) in pixels {
            if x < 0 || y < 0 || x >= WIDTH || y >= HEIGHT {
                continue;
            }
            let px = (HEIGHT as usize - 1) - y as usize;
            let py = x as usize;
            let idx = py * WIDTH_BYTES + px / 8;
            let bit = 0x80 >> (px % 8);
            match color {
                BinaryColor::On => self.frame[idx] &= !bit,
                BinaryColor::Off => self.frame[idx] |= bit,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UpdateRun;
    use time::OffsetDateTime;

    const EXPECTED: &[u8] = include_bytes!("../tests/fixtures/view.pbm");

    fn sample_model() -> Model {
        let start = OffsetDateTime::UNIX_EPOCH;
        Model {
            report: Some(UpdateReport {
                fingerprint: "abc123".into(),
                passed: 1,
                failed: 2,
                runs: vec![
                    UpdateRun {
                        update_start: start,
                        update_end: start + time::Duration::seconds(95),
                        passed: true,
                        reason: None,
                    },
                    UpdateRun {
                        update_start: start,
                        update_end: start + time::Duration::seconds(40),
                        passed: false,
                        reason: Some(
                            "ECU rejected the flashed image at boot check, rolled back to the previous firmware".into(),
                        ),
                    },
                    UpdateRun {
                        update_start: start,
                        update_end: start + time::Duration::seconds(12),
                        passed: false,
                        reason: Some("ECU flash write failed at block 214".into()),
                    },
                ],
            }),
            service_status: "ACTIVE".into(),
            ethernet_status: "UP".into(),
            ..Model::default()
        }
    }

    /// Renders the landscape frame as a PBM (P4) bitmap, 1 bit per pixel, MSB first, 1 = black.
    /// Inverts the mapping in `Canvas`.
    fn frame_to_pbm(frame: &[u8]) -> Vec<u8> {
        let row_bytes = (WIDTH as usize).div_ceil(8);
        let mut pbm = format!("P4\n{WIDTH} {HEIGHT}\n").into_bytes();
        for y in 0..HEIGHT as usize {
            let mut row = vec![0u8; row_bytes];
            for x in 0..WIDTH as usize {
                let px = (HEIGHT as usize - 1) - y;
                let py = x;
                let on = frame[py * WIDTH_BYTES + px / 8] & (0x80 >> (px % 8)) == 0;
                if on {
                    row[x / 8] |= 0x80 >> (x % 8);
                }
            }
            pbm.extend_from_slice(&row);
        }
        pbm
    }

    #[test]
    fn view_matches_snapshot() {
        let pbm = frame_to_pbm(&view(&sample_model()));
        assert_eq!(EXPECTED, &pbm, "frame differs from view.pbm");
    }
}
