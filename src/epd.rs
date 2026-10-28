//! E-paper panel driver over SPI and GPIO.
use crate::{HEIGHT, WIDTH_BYTES};
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

type Result<T> = std::io::Result<T>;

pub struct Epd {
    spi: Spidev,
    rst: Request,
    dc: Request,
    busy: Request,
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

        Ok(Self { spi, rst, dc, busy })
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

    pub fn show_frame(&mut self, frame: &[u8]) -> Result<()> {
        self.init()?;
        self.show(frame)?;
        self.sleep_mode()
    }

    fn sleep_mode(&mut self) -> Result<()> {
        self.command(0x10)?;
        self.data(&[0x01])?;
        Ok(())
    }
}
