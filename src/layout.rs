use crate::{FRAME_LEN, WIDTH_BYTES};
use embedded_graphics::image::Image;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_7X13_BOLD, FONT_10X20};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use embedded_iconoir::icons::size24px::actions::CheckCircle;
use embedded_iconoir::prelude::IconoirNewIcon;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::fs;
use std::process::Command;
use std::time::Duration;
use time::OffsetDateTime;

const WIDTH: i32 = crate::HEIGHT as i32;
const HEIGHT: i32 = (WIDTH_BYTES * 8) as i32;

const GRID_TEMPLATE_AREAS: [&str; 4] = ["aab", "aab", "aab", "cde"];
const DEBUG_GRID: bool = false;
const COLUMNS: i32 = GRID_TEMPLATE_AREAS[0].len() as i32;
const ROWS: i32 = GRID_TEMPLATE_AREAS.len() as i32;
const COLUMN_WIDTH: i32 = WIDTH / COLUMNS;
const ROW_HEIGHT: i32 = HEIGHT / ROWS;

const PADDING: i32 = 2;
const LINE_HEIGHT: i32 = FONT_6X10.character_size.height as i32 + PADDING;
const ENTRY_HEIGHT: i32 = 2 * LINE_HEIGHT + PADDING;
const ENTRY_TOP: i32 = 18;
const ID_RIGHT: i32 = 30;
const MESSAGE_LEFT: i32 = 34;
const ICON_SIZE: i32 = 24;

pub const REPORT_PATH: &str = "/massdata/ota/update-report-pending.toml.tmp";
const INCIDENT_SLOTS: usize = 3;

#[derive(Deserialize)]
struct UpdateReport {
    fingerprint: String,
    passed: u64,
    failed: u64,
    runs: Vec<UpdateRun>,
    incidents: Vec<UpdateIncident>,
}

#[derive(Deserialize)]
struct UpdateRun {
    #[serde(with = "toml_datetime_compat")]
    update_start: OffsetDateTime,
    #[serde(with = "toml_datetime_compat")]
    update_end: OffsetDateTime,
}

#[derive(Deserialize)]
struct UpdateIncident {
    reason: String,
}

struct Layout {
    incidents: VecDeque<(u64, String)>,
    failed: u64,
    passed: u64,
    tool_version: String,
    average_time: Duration,
    service_status: String,
}

impl Layout {
    fn load() -> Self {
        match fs::read_to_string(REPORT_PATH)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str::<UpdateReport>(&text).map_err(|e| e.to_string()))
        {
            Ok(report) => Self::from_report(&report),
            Err(error) => {
                eprintln!("layout: {REPORT_PATH}: {error}, using placeholder");
                Self::new()
            }
        }
    }

    fn from_report(report: &UpdateReport) -> Self {
        let incidents = report
            .incidents
            .iter()
            .enumerate()
            .rev()
            .take(INCIDENT_SLOTS)
            .map(|(index, incident)| ((index + 1) as u64, incident.reason.clone()))
            .collect();
        let total_ns: i128 = report
            .runs
            .iter()
            .map(|run| (run.update_end - run.update_start).whole_nanoseconds())
            .sum();
        let average_secs = match report.runs.len() {
            0 => 0,
            count => (total_ns / count as i128 / 1_000_000_000).max(0) as u64,
        };
        let tool_version = report
            .fingerprint
            .split_whitespace()
            .nth(1)
            .map(|version| format!("v{version}"))
            .unwrap_or_else(|| report.fingerprint.clone());
        Self {
            incidents,
            failed: report.failed,
            passed: report.passed,
            tool_version,
            average_time: Duration::from_secs(average_secs),
            service_status: read_service_status(),
        }
    }

    fn new() -> Self {
        Self {
            incidents: [(10, FIRST_MESSAGE), (9, SECOND_MESSAGE), (7, THIRD_MESSAGE)]
                .into_iter()
                .map(|(run, reason)| (run, reason.to_string()))
                .collect(),
            failed: 3,
            passed: 12,
            tool_version: "v0.1.0".to_string(),
            average_time: Duration::from_secs(90),
            service_status: read_service_status(),
        }
    }

    fn as_items(&self) -> Vec<Item<'_>> {
        let mut items = Vec::new();
        for (entry, (run, reason)) in self.incidents.iter().enumerate() {
            let entry = entry as i32;
            let id = format!("#{:02}", run);
            let x = right_aligned_x(&id, AREA_A.x + ID_RIGHT);
            items.push(item(id, x, entry_baseline(entry, 0), &FONT_6X10));
            for (line, text) in wrap(reason, MESSAGE_CHARS).into_iter().enumerate() {
                items.push(item(
                    text,
                    AREA_A.x + MESSAGE_LEFT,
                    entry_baseline(entry, line as i32),
                    &FONT_6X10,
                ));
            }
        }

        let failure_rate = format!("{}/{}", self.failed, self.failed + self.passed);
        let x = center_x(AREA_B, &failure_rate, BIG_FONT);
        items.push(item(
            failure_rate,
            x,
            center_baseline(AREA_B, BIG_FONT),
            BIG_FONT,
        ));

        let stacks: [(&str, Cow<'_, str>, Rect); 3] = [
            ("VERSION", Cow::Borrowed(&self.tool_version), AREA_C),
            (
                "AVG TIME",
                Cow::Owned(format_minutes_seconds(self.average_time)),
                AREA_D,
            ),
            ("SERVICE", Cow::Borrowed(&self.service_status), AREA_E),
        ];
        for (label, value, rect) in stacks {
            items.push(item(
                label,
                center_x(rect, label, &FONT_6X10),
                stack_label_baseline(rect),
                &FONT_6X10,
            ));
            let x = center_x(rect, &value, VALUE_FONT);
            items.push(item(value, x, stack_value_baseline(rect), VALUE_FONT));
        }
        items
    }
}

fn format_minutes_seconds(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{}m{}s", secs / 60, secs % 60)
}

fn read_service_status() -> String {
    let output = match Command::new("systemctl")
        .args(["status", "offline-update", "--no-pager"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return "UNKNOWN".to_string(),
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("Active:"))
        .map(|state| {
            let state = state.split(" since ").next().unwrap_or(state).trim();
            let word = match (state.find('('), state.rfind(')')) {
                (Some(open), Some(close)) if open < close => &state[open + 1..close],
                _ => state.split_whitespace().next().unwrap_or(state),
            };
            word.to_uppercase()
        })
        .unwrap_or_else(|| "UNKNOWN".to_string())
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

const FIRST_MESSAGE: &str = "Checksum mismatch expected 0x01 but...";
const SECOND_MESSAGE: &str = "Download timeout at 4 hour deadline...";
const THIRD_MESSAGE: &str = "Invalid signature in bundle file...";

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

const fn entry_baseline(entry: i32, line: i32) -> i32 {
    AREA_A.y + ENTRY_TOP + entry * ENTRY_HEIGHT + line * LINE_HEIGHT
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

fn draw_text(canvas: &mut Canvas, text: &str, x: i32, y: i32, font: &MonoFont<'_>) {
    let style = MonoTextStyle::new(font, BinaryColor::On);
    let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Alphabetic).draw(canvas);
}

pub fn render() -> Vec<u8> {
    let mut frame = vec![0xFFu8; FRAME_LEN];
    let mut canvas = Canvas { frame: &mut frame };
    let layout = Layout::load();
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
    for item in layout.as_items() {
        draw_text(&mut canvas, &item.text, item.x, item.y, item.font);
    }
    if layout.incidents.is_empty() {
        let icon = CheckCircle::new(BinaryColor::On);
        let x = AREA_A.x + (AREA_A.w - ICON_SIZE) / 2;
        let y = AREA_A.y + (AREA_A.h - ICON_SIZE) / 2;
        let _ = Image::new(&icon, Point::new(x, y)).draw(&mut canvas);
    }
    frame
}
