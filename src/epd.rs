//! E-paper panel driver over SPI and GPIO.
use crate::{FRAME_LEN, HEIGHT, WIDTH_BYTES};
use gpiocdev::Request;
use gpiocdev::line::Value;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::io::{self, Write};
use std::thread::sleep;
use std::time::{Duration, Instant};

const SPI_PATH: &str = "/dev/spidev0.0";
const GPIO_CHIP: &str = "/dev/gpiochip4";
const PIN_RST: u32 = 17;
const PIN_DC: u32 = 25;
const PIN_BUSY: u32 = 24;

/// Logical landscape canvas that views render into, packed one bit per pixel in rows.
pub const LOGICAL_WIDTH: usize = HEIGHT;
pub const LOGICAL_HEIGHT: usize = WIDTH_BYTES * 8;
pub const LOGICAL_ROW_BYTES: usize = LOGICAL_WIDTH.div_ceil(8);
pub const LOGICAL_FRAME_LEN: usize = LOGICAL_ROW_BYTES * LOGICAL_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    #[allow(dead_code)]
    Deg90,
    Deg270,
}

type Result<T> = std::io::Result<T>;

pub struct Epd {
    spi: Spidev,
    rst: Request,
    dc: Request,
    busy: Request,
    pub orientation: Orientation,
}

impl Epd {
    pub fn open() -> Result<Self> {
        let mut spi = Spidev::open(SPI_PATH)?;
        let opts = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(4_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spi.configure(&opts)?;

        let output = |line: u32| {
            Request::builder()
                .on_chip(GPIO_CHIP)
                .with_consumer("epaper-test")
                .with_line(line)
                .as_output(Value::Inactive)
                .request()
                .map_err(io::Error::other)
        };
        let rst = output(PIN_RST)?;
        let dc = output(PIN_DC)?;
        let busy = Request::builder()
            .on_chip(GPIO_CHIP)
            .with_consumer("epaper-test")
            .with_line(PIN_BUSY)
            .as_input()
            .request()
            .map_err(io::Error::other)?;

        Ok(Self {
            spi,
            rst,
            dc,
            busy,
            orientation: Orientation::Deg270,
        })
    }

    fn reset(&mut self) -> Result<()> {
        self.rst
            .set_value(PIN_RST, Value::Active)
            .map_err(io::Error::other)?;
        sleep(Duration::from_millis(20));
        self.rst
            .set_value(PIN_RST, Value::Inactive)
            .map_err(io::Error::other)?;
        sleep(Duration::from_millis(2));
        self.rst
            .set_value(PIN_RST, Value::Active)
            .map_err(io::Error::other)?;
        sleep(Duration::from_millis(20));
        Ok(())
    }

    // BUSY is high while the controller is working.
    fn wait_busy(&self) -> Result<()> {
        let start = Instant::now();
        while self.busy.value(PIN_BUSY).map_err(io::Error::other)? == Value::Active {
            if start.elapsed() > Duration::from_secs(10) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for BUSY",
                ));
            }
            sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn command(&mut self, cmd: u8) -> Result<()> {
        self.dc
            .set_value(PIN_DC, Value::Inactive)
            .map_err(io::Error::other)?;
        self.spi.write_all(&[cmd])?;
        Ok(())
    }

    fn data(&mut self, data: &[u8]) -> Result<()> {
        self.dc
            .set_value(PIN_DC, Value::Active)
            .map_err(io::Error::other)?;
        for chunk in data.chunks(4096) {
            self.spi.write_all(chunk)?;
        }
        Ok(())
    }

    fn init(&mut self) -> Result<()> {
        self.reset()?;
        self.wait_busy()?;

        self.command(0x12)?;
        self.wait_busy()?;

        self.command(0x01)?;
        self.data(&[(HEIGHT - 1) as u8, 0x00, 0x00])?;
        self.command(0x11)?;
        self.data(&[0x03])?;
        self.command(0x44)?;
        self.data(&[0x00, (WIDTH_BYTES - 1) as u8])?;
        self.command(0x45)?;
        self.data(&[0x00, 0x00, (HEIGHT - 1) as u8, 0x00])?;
        self.command(0x3C)?;
        self.data(&[0x05])?;
        self.command(0x21)?;
        self.data(&[0x00, 0x80])?;
        self.command(0x18)?;
        self.data(&[0x80])?;
        self.command(0x4E)?;
        self.data(&[0x00])?;
        self.command(0x4F)?;
        self.data(&[0x00, 0x00])?;
        self.wait_busy()?;
        Ok(())
    }

    fn show(&mut self, frame: &[u8]) -> Result<()> {
        self.command(0x24)?;
        self.data(frame)?;
        self.command(0x22)?;
        self.data(&[0xF7])?;
        self.command(0x20)?;
        self.wait_busy()?;
        Ok(())
    }

    /// Rotates a logical landscape frame into the panel's portrait layout using `self.orientation`.
    pub fn show_frame(&mut self, logical: &[u8]) -> Result<()> {
        let frame = to_portrait(logical, self.orientation);
        self.init()?;
        self.show(&frame)?;
        self.sleep_mode()
    }

    fn sleep_mode(&mut self) -> Result<()> {
        self.command(0x10)?;
        self.data(&[0x01])?;
        Ok(())
    }
}

impl Drop for Epd {
    fn drop(&mut self) {
        if let Err(error) = self.show_frame(&[0xFF; LOGICAL_FRAME_LEN]) {
            eprintln!("wipe on drop failed due to {error}");
        }
    }
}

/// Converts the logical landscape frame into the panel's portrait byte layout (1 = white, 0 = black).
fn to_portrait(logical: &[u8], orientation: Orientation) -> Vec<u8> {
    let mut frame = vec![0xFF; FRAME_LEN];
    for y in 0..LOGICAL_HEIGHT {
        for x in 0..LOGICAL_WIDTH {
            let on = logical[y * LOGICAL_ROW_BYTES + x / 8] & (0x80 >> (x % 8)) == 0;
            if !on {
                continue;
            }
            let (px, py) = match orientation {
                Orientation::Deg90 => ((LOGICAL_HEIGHT - 1) - y, x),
                Orientation::Deg270 => (y, (LOGICAL_WIDTH - 1) - x),
            };
            frame[py * WIDTH_BYTES + px / 8] &= !(0x80 >> (px % 8));
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_portrait_rotates_top_left_pixel() {
        let mut logical = vec![0xFF; LOGICAL_FRAME_LEN];
        logical[0] &= !0x80;

        let deg90 = to_portrait(&logical, Orientation::Deg90);
        assert_eq!(deg90[WIDTH_BYTES - 1], 0xFE);
        assert_eq!(deg90.iter().filter(|&&byte| byte != 0xFF).count(), 1);

        let deg270 = to_portrait(&logical, Orientation::Deg270);
        assert_eq!(deg270[(LOGICAL_WIDTH - 1) * WIDTH_BYTES], 0x7F);
        assert_eq!(deg270.iter().filter(|&&byte| byte != 0xFF).count(), 1);
    }
}
