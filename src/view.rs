//! View: renders the model into a frame without changing state.
use crate::model::{Model, UpdateReport};
use crate::{FRAME_LEN, WIDTH_BYTES};
use embedded_graphics::image::Image;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_7X13_BOLD, FONT_10X20};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use embedded_iconoir::icons::size24px::actions::CheckCircle;
use embedded_iconoir::icons::size24px::emojis::EmojiPuzzled;
use embedded_iconoir::prelude::IconoirNewIcon;
use std::borrow::Cow;

use std::convert::Infallible;

const WIDTH: i32 = crate::HEIGHT as i32;
const HEIGHT: i32 = (WIDTH_BYTES * 8) as i32;

const GRID_TEMPLATE_AREAS: [&str; 4] = ["aab", "aab", "aab", "cde"];
const DEBUG_GRID: bool = false;
const DEBUG_PLACEHOLDER: bool = false;
const COLUMNS: i32 = GRID_TEMPLATE_AREAS[0].len() as i32;
const ROWS: i32 = GRID_TEMPLATE_AREAS.len() as i32;
const COLUMN_WIDTH: i32 = WIDTH / COLUMNS;
const ROW_HEIGHT: i32 = HEIGHT / ROWS;

const PADDING: i32 = 2;
const LINE_HEIGHT: i32 = FONT_6X10.character_size.height as i32 + PADDING;
const ENTRY_TOP: i32 = 18;
const INCIDENT_LINES: i32 = (AREA_A.h - ENTRY_TOP) / LINE_HEIGHT;
const ID_RIGHT: i32 = 30;
const MESSAGE_LEFT: i32 = 34;
const ICON_SIZE: i32 = 24;

const INCIDENT_SLOTS: usize = 3;

#[derive(Default, Clone, PartialEq)]
pub struct View {
    incidents: Vec<(u64, String)>,
    failure_rate: Option<String>,
    tool_version: Option<String>,
    average_time: Option<String>,
    service_status: Option<String>,
}

const UNAVAILABLE: &str = "---";

impl View {
    pub fn from_model(model: &Model) -> Self {
        match &model.report {
            Some(report) if !DEBUG_PLACEHOLDER => Self::from_report(report, &model.service_status),
            _ => Self::default(),
        }
    }

    fn from_report(report: &UpdateReport, service_status: &str) -> Self {
        let incidents = report
            .runs
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, run)| !run.passed)
            .take(INCIDENT_SLOTS)
            .map(|(index, run)| {
                let reason = run
                    .reason
                    .clone()
                    .unwrap_or_else(|| "run failed".to_string());
                (index as u64 + 1, reason)
            })
            .collect();
        let passed_runs = report.runs.iter().filter(|run| run.passed);
        let passed_count = passed_runs.clone().count() as i32;
        let total_time: time::Duration = passed_runs
            .map(|run| run.update_end - run.update_start)
            .sum();
        let average_time = total_time.checked_div(passed_count).map(format_minutes_seconds);
        let tool_version = report
            .fingerprint
            .split_whitespace()
            .nth(1)
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| report.fingerprint.clone());
        Self {
            incidents,
            failure_rate: Some(format!(
                "{}/{}",
                report.failed,
                report.failed + report.passed
            )),
            tool_version: Some(tool_version),
            average_time,
            service_status: Some(service_status.to_string()),
        }
    }

    fn as_items(&self) -> Vec<Item<'_>> {
        let mut items = Vec::new();
        items.extend(incident_items(&self.incidents));

        let failure_rate = self.failure_rate.as_deref().unwrap_or(UNAVAILABLE);
        let x = center_x(AREA_B, failure_rate, BIG_FONT);
        items.push(item(
            failure_rate,
            x,
            center_baseline(AREA_B, BIG_FONT),
            BIG_FONT,
        ));

        let stacks: [(&str, Option<&str>, Rect); 3] = [
            ("VERSION", self.tool_version.as_deref(), AREA_C),
            ("AVG TIME", self.average_time.as_deref(), AREA_D),
            ("SERVICE", self.service_status.as_deref(), AREA_E),
        ];
        for (label, value, rect) in stacks {
            let value = value.unwrap_or(UNAVAILABLE);
            items.push(item(
                label,
                center_x(rect, label, &FONT_6X10),
                stack_label_baseline(rect),
                &FONT_6X10,
            ));
            let x = center_x(rect, value, VALUE_FONT);
            items.push(item(value, x, stack_value_baseline(rect), VALUE_FONT));
        }
        items
    }

    pub fn render(&self) -> Vec<u8> {
        let mut frame = vec![0xFFu8; FRAME_LEN];
        let mut canvas = Canvas { frame: &mut frame };
        if DEBUG_GRID {
            for rect in [AREA_A, AREA_B, AREA_C, AREA_D, AREA_E] {
                let outline = Rectangle::new(
                    Point::new(rect.x, rect.y),
                    Size::new(rect.w as u32, rect.h as u32),
                );
                let _ = outline
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                    .draw(&mut canvas);
            }
        }
        for item in self.as_items() {
            draw_text(&mut canvas, &item.text, item.x, item.y, item.font);
        }
        if self.failure_rate.is_none() {
            let icon = EmojiPuzzled::new(BinaryColor::On);
            let x = AREA_A.x + (AREA_A.w - ICON_SIZE) / 2;
            let y = AREA_A.y + (AREA_A.h - ICON_SIZE) / 2;
            let _ = Image::new(&icon, Point::new(x, y)).draw(&mut canvas);
        }
        if self.failure_rate.is_some() && self.incidents.is_empty() {
            let icon = CheckCircle::new(BinaryColor::On);
            let x = AREA_A.x + (AREA_A.w - ICON_SIZE) / 2;
            let y = AREA_A.y + (AREA_A.h - ICON_SIZE) / 2;
            let _ = Image::new(&icon, Point::new(x, y)).draw(&mut canvas);
        }
        frame
    }
}

fn incident_items(incidents: &[(u64, String)]) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let max_lines_per_entry = INCIDENT_LINES / incidents.len().max(1) as i32;
    let mut next_line = 0;
    for (run, reason) in incidents {
        let id = format!("#{:02}", run);
        let x = right_aligned_x(&id, AREA_A.x + ID_RIGHT);
        items.push(item(id, x, entry_baseline(next_line, 0), &FONT_6X10));
        let lines = fit_lines(wrap(reason, MESSAGE_CHARS), max_lines_per_entry as usize);
        let used = lines.len() as i32;
        for (line, text) in lines.into_iter().enumerate() {
            items.push(item(
                text,
                AREA_A.x + MESSAGE_LEFT,
                entry_baseline(next_line, line as i32),
                &FONT_6X10,
            ));
        }
        next_line += used;
    }
    items
}

fn format_minutes_seconds(duration: time::Duration) -> String {
    let secs = duration.whole_seconds().max(0);
    format!("{}m{}s", secs / 60, secs % 60)
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

const fn area(name: u8) -> Rect {
    let mut top = ROWS;
    let mut bottom = -1;
    let mut left = COLUMNS;
    let mut right = -1;
    let mut row = 0;
    while row < ROWS {
        let cells = GRID_TEMPLATE_AREAS[row as usize].as_bytes();
        let mut col = 0;
        while col < COLUMNS {
            if cells[col as usize] == name {
                if row < top {
                    top = row;
                }
                if row > bottom {
                    bottom = row;
                }
                if col < left {
                    left = col;
                }
                if col > right {
                    right = col;
                }
            }
            col += 1;
        }
        row += 1;
    }
    Rect {
        x: left * COLUMN_WIDTH,
        y: top * ROW_HEIGHT,
        w: (right - left + 1) * COLUMN_WIDTH,
        h: (bottom - top + 1) * ROW_HEIGHT,
    }
}

const AREA_A: Rect = area(b'a');
const AREA_B: Rect = area(b'b');
const AREA_C: Rect = area(b'c');
const AREA_D: Rect = area(b'd');
const AREA_E: Rect = area(b'e');

const MESSAGE_CHARS: usize =
    ((AREA_A.w - MESSAGE_LEFT) / FONT_6X10.character_size.width as i32) as usize;

const VALUE_FONT: &MonoFont<'static> = &FONT_7X13_BOLD;
const BIG_FONT: &MonoFont<'static> = &FONT_10X20;

const fn text_width(text: &str, font: &MonoFont<'_>) -> i32 {
    text.len() as i32 * font.character_size.width as i32
}

const fn center_x(rect: Rect, text: &str, font: &MonoFont<'_>) -> i32 {
    rect.x + (rect.w - text_width(text, font)) / 2
}

const fn center_baseline(rect: Rect, font: &MonoFont<'_>) -> i32 {
    rect.y + (rect.h - font.character_size.height as i32) / 2 + font.baseline as i32
}

const fn stack_label_baseline(rect: Rect) -> i32 {
    let block = LINE_HEIGHT + VALUE_FONT.character_size.height as i32;
    rect.y + (rect.h - block) / 2 + FONT_6X10.baseline as i32
}

const fn stack_value_baseline(rect: Rect) -> i32 {
    stack_label_baseline(rect) + LINE_HEIGHT
}

const fn entry_baseline(first_line: i32, line: i32) -> i32 {
    AREA_A.y + ENTRY_TOP + (first_line + line) * LINE_HEIGHT
}

const fn right_aligned_x(text: &str, right: i32) -> i32 {
    right - text_width(text, &FONT_6X10)
}

struct Item<'a> {
    text: Cow<'a, str>,
    x: i32,
    y: i32,
    font: &'static MonoFont<'static>,
}

fn item<'a>(
    text: impl Into<Cow<'a, str>>,
    x: i32,
    y: i32,
    font: &'static MonoFont<'static>,
) -> Item<'a> {
    Item {
        text: text.into(),
        x,
        y,
        font,
    }
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

fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > max_chars {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

// Keeps at most `max_lines`; a cut last line ends with ".." and still fits MESSAGE_CHARS.
fn fit_lines(mut lines: Vec<String>, max_lines: usize) -> Vec<String> {
    if lines.len() <= max_lines {
        return lines;
    }
    lines.truncate(max_lines);
    if let Some(last) = lines.last_mut() {
        let kept: String = last.chars().take(MESSAGE_CHARS.saturating_sub(2)).collect();
        *last = format!("{kept}..");
    }
    lines
}

fn draw_text(canvas: &mut Canvas, text: &str, x: i32, y: i32, font: &MonoFont<'_>) {
    let style = MonoTextStyle::new(font, BinaryColor::On);
    let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Alphabetic).draw(canvas);
}
