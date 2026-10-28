mod area;
mod epd;
mod model;
mod runtime;
mod update;
mod view;

const WIDTH_BYTES: usize = 16;
const HEIGHT: usize = 250;
const FRAME_LEN: usize = WIDTH_BYTES * HEIGHT;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let epd = match epd::Epd::open() {
        Ok(epd) => epd,
        Err(error) => {
            eprintln!("display unavailable due to {error}");
            return;
        }
    };
    runtime::run(epd).await;
}
