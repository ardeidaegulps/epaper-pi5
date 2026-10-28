//! Turns messages into a new model and commands, with no I/O.
//!
//! All I/O and async tasks (file reads, D-Bus, netlink, timers) belong in the
//! runtime. Update only describes side effects as `Command` values; keeping it
//! free of I/O avoids drifting into MVC.
use crate::model::{Model, UpdateReport};
use rtnetlink::packet_route::link::LinkFlags;
use rtnetlink::packet_route::neighbour::NeighbourState;

/// Represents runtime events triggered by core service and network state changes.
#[derive(Clone, Debug)]
pub enum Message {
    Bootstrap,
    ReportFileChanged,
    ReportLoaded(Option<UpdateReport>),
    ServiceChanged(String),
    EthernetChanged(LinkFlags, NeighbourState),
}

/// Side effect requested by update, executed by the runtime.
#[derive(Debug)]
pub enum Command {
    ReadReport,
}

pub fn update(message: Message, mut model: Model) -> (Model, Option<Command>) {
    let command = match message {
        Message::Bootstrap | Message::ReportFileChanged => Some(Command::ReadReport),
        Message::ReportLoaded(report) => {
            model.report = report;
            None
        }
        Message::ServiceChanged(service_status) => {
            model.service_status = service_status;
            None
        }
        Message::EthernetChanged(flags, arp) => {
            model.ethernet_link_up = flags.contains(LinkFlags::LowerUp);
            model.ethernet_arp = arp;
            None
        }
    };
    (model, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> UpdateReport {
        UpdateReport {
            fingerprint: "abc".to_string(),
            passed: 1,
            failed: 0,
            runs: Vec::new(),
        }
    }

    #[test]
    fn bootstrap_requests_report_read() {
        let (_, command) = update(Message::Bootstrap, Model::default());
        assert!(matches!(command, Some(Command::ReadReport)));
    }

    #[test]
    fn report_file_change_requests_report_read() {
        let (_, command) = update(Message::ReportFileChanged, Model::default());
        assert!(matches!(command, Some(Command::ReadReport)));
    }

    #[test]
    fn report_change_stores_report() {
        let (model, command) = update(Message::ReportLoaded(Some(report())), Model::default());
        assert_eq!(model.report, Some(report()));
        assert!(command.is_none());
    }

    #[test]
    fn report_removal_clears_report() {
        let (model, _) = update(Message::ReportLoaded(Some(report())), Model::default());
        let (model, _) = update(Message::ReportLoaded(None), model);
        assert!(model.report.is_none());
    }

    #[test]
    fn ethernet_change_updates_link_and_arp() {
        let (model, _) = update(
            Message::EthernetChanged(LinkFlags::LowerUp, NeighbourState::Reachable),
            Model::default(),
        );
        assert!(model.ethernet_link_up);
        assert_eq!(model.ethernet_arp, NeighbourState::Reachable);
    }

    #[test]
    fn service_change_updates_status() {
        let (model, _) = update(
            Message::ServiceChanged("ACTIVE".to_string()),
            Model::default(),
        );
        assert_eq!(model.service_status, "ACTIVE");
    }
}
