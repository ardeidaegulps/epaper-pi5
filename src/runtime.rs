//! Runtime: runs the MVU loop, executes commands and subscriptions, and drives the display.
use std::io::Error;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::{interval, timeout};

use crate::epd::Epd;
use crate::model::{Model, REPORT_PATHS, UpdateReport};
use crate::update::{self, Command, Message, Subscription};
use crate::view;

const MESSAGE_CAPACITY: usize = 64;
const REFRESH_TIMEOUT: Duration = Duration::from_secs(10);
const SERVICE: &str = "offline-update";
const ETHERNET_HOST: &str = "192.168.225.12";

pub async fn run(epd: Epd) {
    let model = Model::default();
    let (view_tx, view_rx) = watch::channel(view::view(&model));
    let (message_tx, message_rx) = mpsc::channel(MESSAGE_CAPACITY);
    let _ = message_tx.send(Message::Bootstrap).await;
    let Subscription::Every(period, tick) = update::subscriptions();

    let handle_periodic_ticks = handle_periodic_ticks(period, tick, message_tx.clone());
    let handle_view_rendering = handle_view_rendering(epd, view_rx);
    let handle_message_updates = handle_message_updates(model, message_rx, message_tx, view_tx);

    tokio::select! {
        _ = handle_periodic_ticks => eprintln!("ticker stopped"),
        _ = handle_view_rendering => eprintln!("render stopped"),
        _ = handle_message_updates => eprintln!("update stopped"),
    }
}

async fn handle_periodic_ticks(period: Duration, message: Message, tx: mpsc::Sender<Message>) {
    let mut ticker = interval(period);
    loop {
        ticker.tick().await;
        if tx.send(message.clone()).await.is_err() {
            return;
        }
    }
}

async fn handle_view_rendering(mut epd: Epd, mut rx: watch::Receiver<Vec<u8>>) {
    loop {
        if rx.changed().await.is_err() {
            return;
        }
        let buffer = rx.borrow_and_update().clone();
        let handle = tokio::task::spawn_blocking(move || {
            let res = epd.show_frame(&buffer);
            (epd, res)
        });

        let Ok((next_epd, result)) = handle.await else {
            eprintln!("display task failed (worker crashed)");
            return;
        };
        epd = next_epd;

        match result {
            Ok(()) => println!("displayed layout"),
            Err(error) => eprintln!("display failed due to {error}"),
        }
    }
}

async fn handle_message_updates(
    mut model: Model,
    mut message_rx: mpsc::Receiver<Message>,
    message_tx: mpsc::Sender<Message>,
    view_tx: watch::Sender<Vec<u8>>,
) {
    while let Some(message) = message_rx.recv().await {
        let (next, command) = update::update(message, model);
        if let Some(command) = command {
            let tx = message_tx.clone();
            tokio::spawn(async move {
                let message = perform(command).await;
                let _ = tx.send(message).await;
            });
        }
        let frame = view::view(&next);
        view_tx.send_if_modified(|current| {
            let changed = *current != frame;
            *current = frame;
            changed
        });
        model = next;
    }
}

async fn perform(command: Command) -> Message {
    match command {
        Command::Refresh(seq) => {
            let work = async { tokio::join!(read_report(), service_status(), ethernet_status()) };
            match timeout(REFRESH_TIMEOUT, work).await {
                Ok((report, service, ethernet)) => {
                    Message::Refreshed(seq, report, service, ethernet)
                }
                Err(_) => {
                    eprintln!("refresh timed out");
                    Message::Refreshed(seq, None, "UNKNOWN".to_string(), "UNKNOWN".to_string())
                }
            }
        }
    }
}

async fn read_report() -> Option<UpdateReport> {
    for path in REPORT_PATHS {
        let parsed = tokio::fs::read_to_string(path)
            .await
            .and_then(|text| toml::from_str(&text).map_err(Error::other));
        match parsed {
            Ok(report) => return Some(report),
            Err(error) => eprintln!("failed to read report {path} due to {error}"),
        }
    }
    None
}

async fn ethernet_status() -> String {
    let status = tokio::process::Command::new("ping")
        .args(["-c", "1", "-W", "2", ETHERNET_HOST])
        .output()
        .await;
    match status {
        Ok(out) if out.status.success() => "UP".to_string(),
        Ok(_) => "DOWN".to_string(),
        Err(_) => "UNKNOWN".to_string(),
    }
}

async fn service_status() -> String {
    let output = tokio::process::Command::new("systemctl")
        .args(["is-active", SERVICE])
        .output()
        .await;
    output
        .ok()
        .as_ref()
        .map(|out| String::from_utf8_lossy(&out.stdout))
        .map(|text| text.trim().to_ascii_uppercase())
        .filter(|state| !state.is_empty())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}
