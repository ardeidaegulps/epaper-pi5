use gpiocdev::Request;
use gpiocdev::line::Value;
use spidev::{SpiModeFlags, Spidev, SpidevOptions};
use std::env;
use std::fs;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

mod layout;

const SPI_PATH: &str = "/dev/spidev0.0";
const GPIO_CHIP: &str = "/dev/gpiochip4";
const PIN_RST: u32 = 17;
const PIN_DC: u32 = 25;
const PIN_BUSY: u32 = 24;

const WIDTH_BYTES: usize = 16;
const HEIGHT: usize = 250;
const FRAME_LEN: usize = WIDTH_BYTES * HEIGHT;
const BLOCK: usize = 16;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Epd {
    spi: Spidev,
    rst: Request,
    dc: Request,
    busy: Request,
}

impl Epd {
    fn open() -> Result<Self> {
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
        };
        let rst = output(PIN_RST)?;
        let dc = output(PIN_DC)?;
        let busy = Request::builder()
            .on_chip(GPIO_CHIP)
            .with_consumer("epaper-test")
            .with_line(PIN_BUSY)
            .as_input()
            .request()?;

        Ok(Self { spi, rst, dc, busy })
    }

    fn reset(&mut self) -> Result<()> {
        self.rst.set_value(PIN_RST, Value::Active)?;
        sleep(Duration::from_millis(20));
        self.rst.set_value(PIN_RST, Value::Inactive)?;
        sleep(Duration::from_millis(2));
        self.rst.set_value(PIN_RST, Value::Active)?;
        sleep(Duration::from_millis(20));
        Ok(())
    }

    // BUSY is high while the controller is working.
    fn wait_busy(&self) -> Result<()> {
        let start = Instant::now();
        while self.busy.value(PIN_BUSY)? == Value::Active {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("timed out waiting for BUSY".into());
            }
            sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn command(&mut self, cmd: u8) -> Result<()> {
        self.dc.set_value(PIN_DC, Value::Inactive)?;
        self.spi.write_all(&[cmd])?;
        Ok(())
    }

    fn data(&mut self, data: &[u8]) -> Result<()> {
        self.dc.set_value(PIN_DC, Value::Active)?;
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

    fn sleep_mode(&mut self) -> Result<()> {
        self.command(0x10)?;
        self.data(&[0x01])?;
        Ok(())
    }
}

fn pattern(name: &str) -> Vec<u8> {
    let mut frame = vec![0u8; FRAME_LEN];
    for y in 0..HEIGHT {
        for x in 0..(WIDTH_BYTES * 8) {
            let white = match name {
                "white" => true,
                "black" => false,
                _ => ((x / BLOCK) + (y / BLOCK)) % 2 == 0,
            };
            if white {
                frame[y * WIDTH_BYTES + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    frame
}

fn load_frame(name: &str) -> Result<Vec<u8>> {
    if name == "layout" {
        return Ok(layout::render());
    }
    if name.ends_with(".bin") {
        let data = fs::read(name)?;
        if data.len() != FRAME_LEN {
            return Err(format!("{name}: expected {FRAME_LEN} bytes, got {}", data.len()).into());
        }
        return Ok(data);
    }
    Ok(pattern(name))
}

fn main() -> Result<()> {
    let name = env::args().nth(1).unwrap_or_else(|| "checker".to_string());
    let frame = load_frame(&name)?;

    let mut epd = Epd::open()?;
    epd.init()?;
    epd.show(&frame)?;
    epd.sleep_mode()?;
    println!("displayed pattern: {name}");
    Ok(())
}
