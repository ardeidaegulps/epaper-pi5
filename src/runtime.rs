//! Runtime: runs the MVU loop, executes commands, runs timers and file watches, and drives the display.
use futures_util::{StreamExt, TryStreamExt, future};
use inotify::{EventMask, Inotify, WatchMask};
use rtnetlink::packet_route::link::LinkFlags;
use rtnetlink::packet_route::neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourState};
use rtnetlink::{Handle, MulticastGroup};
use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io::Error;
use std::net::Ipv4Addr;
use std::path::Path;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use zbus::Connection;

use crate::epd::Epd;
use crate::model::{Model, REPORT_PATHS, UpdateReport};
use crate::update::{self, Command, Message};
use crate::view;

const MESSAGE_CAPACITY: usize = 64;
const SERVICE_UNIT: &str = "offline-update.service";
const ETHERNET_IF: &str = "eth0";
const ETHERNET_HOST: Ipv4Addr = Ipv4Addr::new(192, 168, 225, 12);
const ETHERNET_DESTINATION: NeighbourAttribute =
    NeighbourAttribute::Destination(NeighbourAddress::Inet(ETHERNET_HOST));
const NETLINK_GROUPS: [MulticastGroup; 2] = [MulticastGroup::Link, MulticastGroup::Neigh];
// Force to read state without waiting for an event, because netlink drops
// events when its buffer overflows (ENOBUFS), and systemd may unload an
// inactive unit without ending its D-Bus signal stream. Neither case closes the
// stream, so no reconnect happens.
const RESYNC_DEADLINE: Duration = Duration::from_secs(30);
const RETRY_DELAY: Duration = Duration::from_secs(5);

pub async fn run(epd: Epd) {
    let model = Model::default();
    let (view_tx, view_rx) = watch::channel(view::view(&model));
    let (message_tx, message_rx) = mpsc::channel(MESSAGE_CAPACITY);
    let _ = message_tx.send(Message::Bootstrap).await;

    // These sources are fixed, so the runtime starts them directly without a Subscription layer.
    let handle_report_changes = handle_report_changes(message_tx.clone());
    let handle_service_status = handle_service_status(message_tx.clone());
    let handle_netlink_status = handle_netlink_status(message_tx.clone());
    let handle_view_rendering = handle_view_rendering(epd, view_rx);
    let handle_model_updates = handle_model_updates(model, message_rx, view_tx);

    tokio::select! {
        _ = handle_report_changes => eprintln!("report watcher stopped"),
        _ = handle_service_status => eprintln!("service watcher stopped"),
        _ = handle_netlink_status => eprintln!("netlink watcher stopped"),
        _ = handle_view_rendering => eprintln!("render stopped"),
        _ = handle_model_updates => eprintln!("update stopped"),
    }
}

/// Sends `ReportFileChanged` for report file events, after inotify overflow, and after re-watching a removed directory.
async fn handle_report_changes(tx: mpsc::Sender<Message>) {
    let target_names: BTreeSet<OsString> = REPORT_PATHS
        .iter()
        .filter_map(|path| Path::new(path).file_name().map(ToOwned::to_owned))
        .collect();
    let mut inotify = create_inotify_retrying().await;
    loop {
        let names = target_names.clone();
        let blocking_read = tokio::task::spawn_blocking(move || {
            let mut buffer = [0u8; 4096];
            let changes = inotify.read_events_blocking(&mut buffer).map(|events| {
                events.fold((false, false), |(rewatch, changed), event| {
                    let is_ignored = event.mask.contains(EventMask::IGNORED);
                    let is_target = event.mask.contains(EventMask::Q_OVERFLOW)
                        || event.name.is_some_and(|name| names.contains(name));
                    (rewatch || is_ignored, changed || is_ignored || is_target)
                })
            });
            (inotify, changes)
        })
        .await;
        let Ok((returned_inotify, changes)) = blocking_read else {
            eprintln!("report watch read task failed");
            return;
        };
        inotify = returned_inotify;
        let (need_rewatch, has_changed) = changes.unwrap_or_else(|error| {
            eprintln!("report watch read failed due to {error}, re-watching");
            (true, true)
        });
        if need_rewatch {
            inotify = create_inotify_retrying().await;
        }
        if has_changed && tx.send(Message::ReportFileChanged).await.is_err() {
            return;
        }
    }
}

/// Sends `ServiceChanged` whenever the service's `ActiveState` changes, re-reading it periodically and re-subscribing after D-Bus failures.
async fn handle_service_status(tx: mpsc::Sender<Message>) {
    let mut last: Option<String> = None;
    loop {
        let unit = match create_service_unit().await {
            Ok(unit) => unit,
            Err(error) => {
                eprintln!("service unit unavailable due to {error}, retrying");
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
        };
        let mut changes = unit.receive_active_state_changed().await;
        loop {
            let state = match unit.active_state().await {
                Ok(state) => state,
                Err(error) => {
                    eprintln!("service state read failed due to {error}, re-subscribing");
                    break;
                }
            };
            if last.as_deref() != Some(&state) {
                if tx
                    .send(Message::ServiceChanged(state.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
                last = Some(state);
            }
            // Re-reads after RESYNC_DEADLINE without a change signal; a closed
            // signal stream re-subscribes.
            if let Ok(None) = tokio::time::timeout(RESYNC_DEADLINE, changes.next()).await {
                break;
            }
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }
}

/// Keeps the Ethernet state current from netlink events, re-syncing periodically and reconnecting when the event stream ends.
async fn handle_netlink_status(tx: mpsc::Sender<Message>) {
    let mut last = None;
    loop {
        let (connection, handle, mut events) =
            match rtnetlink::new_multicast_connection(&NETLINK_GROUPS) {
                Ok(parts) => parts,
                Err(error) => {
                    eprintln!("netlink connection failed due to {error}, retrying");
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
            };
        let connection_task = tokio::spawn(connection);
        loop {
            // A failed query keeps the last known state instead of reporting the link as down.
            if let Some(state) = ethernet_state(&handle).await {
                if last != Some(state) {
                    let (flags, arp) = state;
                    if tx.send(Message::EthernetChanged(flags, arp)).await.is_err() {
                        return;
                    }
                    last = Some(state);
                }
            }
            // Re-queries after RESYNC_DEADLINE without a netlink event; a closed event stream reconnects.
            if let Ok(None) = tokio::time::timeout(RESYNC_DEADLINE, events.next()).await {
                break;
            }
        }
        connection_task.abort();
        tokio::time::sleep(RETRY_DELAY).await;
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

async fn handle_model_updates(
    mut model: Model,
    mut message_rx: mpsc::Receiver<Message>,
    view_tx: watch::Sender<Vec<u8>>,
) {
    let mut follow_ups = VecDeque::new();
    loop {
        let message = match follow_ups.pop_front() {
            Some(message) => message,
            None => match message_rx.recv().await {
                Some(message) => message,
                None => break,
            },
        };
        let (next, command) = update::update(message, model);
        if let Some(command) = command {
            follow_ups.push_back(perform(command).await);
        }
        let frame = view::view(&next);
        view_tx.send_if_modified(|current| {
            let previous = std::mem::replace(current, frame);
            previous != *current
        });
        model = next;
    }
}

/// Executes a command requested by update and returns the message describing its result.
async fn perform(command: Command) -> Message {
    match command {
        Command::ReadReport => Message::ReportLoaded(read_report().await),
    }
}

fn create_inotify() -> Result<Inotify, Error> {
    let dir = REPORT_PATHS
        .first()
        .and_then(|path| Path::new(path).parent())
        .ok_or_else(|| Error::other("report path has no directory"))?;
    let inotify = Inotify::init()?;
    inotify.watches().add(
        dir,
        WatchMask::CLOSE_WRITE
            | WatchMask::MOVED_TO
            | WatchMask::MOVED_FROM
            | WatchMask::CREATE
            | WatchMask::DELETE,
    )?;
    Ok(inotify)
}

/// Loads the service's systemd unit and returns a proxy for its properties, reading them uncached.
async fn create_service_unit() -> zbus::Result<UnitProxy<'static>> {
    let connection = Connection::system().await?;
    let manager = ManagerProxy::new(&connection).await?;
    let path = manager.load_unit(SERVICE_UNIT).await?;
    UnitProxy::builder(&connection)
        .path(path)?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

/// Retries until the report directory can be watched, since it may not exist yet.
async fn create_inotify_retrying() -> Inotify {
    loop {
        match create_inotify() {
            Ok(inotify) => return inotify,
            Err(error) => {
                eprintln!("report watch unavailable due to {error}, retrying");
                tokio::time::sleep(RETRY_DELAY).await;
            }
        }
    }
}

/// Returns the first report that reads and parses, trying `REPORT_PATHS` in order.
async fn read_report() -> Option<UpdateReport> {
    for path in REPORT_PATHS.iter().map(Path::new) {
        match tokio::fs::read_to_string(path).await {
            Ok(text) => match toml::from_str(&text) {
                Ok(report) => return Some(report),
                Err(error) => {
                    eprintln!("failed to parse report {path:?} due to {error}");
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => {
                eprintln!("failed to read report {path:?} due to {error}");
            }
        }
    }
    None
}

/// Reads eth0's carrier and the neighbour state of `ETHERNET_HOST`, or `None` if a netlink query fails.
async fn ethernet_state(handle: &Handle) -> Option<(LinkFlags, NeighbourState)> {
    let mut links = handle
        .link()
        .get()
        .match_name(ETHERNET_IF.to_string())
        .execute();
    let link = match links.next().await {
        Some(Ok(message)) => message,
        Some(Err(error)) => {
            eprintln!("link query failed due to {error}");
            return None;
        }
        None => return Some((LinkFlags::empty(), NeighbourState::None)),
    };

    let target_ifindex = link.header.index;
    let neighbours = handle
        .neighbours()
        .get()
        .execute()
        .try_filter(|neighbour| {
            future::ready(
                neighbour.header.ifindex == target_ifindex
                    && neighbour.attributes.contains(&ETHERNET_DESTINATION),
            )
        })
        .try_collect::<Vec<_>>()
        .await;
    match neighbours {
        Ok(neighbours) => {
            let arp = neighbours
                .last()
                .map_or(NeighbourState::None, |neighbour| neighbour.header.state);
            Some((link.header.flags, arp))
        }
        Err(error) => {
            eprintln!("neighbour query failed due to {error}");
            None
        }
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Manager {
    fn load_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait Unit {
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;
}
