//! Update: turns messages into a new model and commands, with no I/O.
use crate::model::{Model, UpdateReport};
use std::time::Duration;

const TICK: Duration = Duration::from_secs(5);

/// Input to update: start-up, timer ticks, and command results.
#[derive(Clone, Debug)]
pub enum Message {
    Bootstrap,
    Tick,
    Refreshed(u64, Option<UpdateReport>, String),
}

/// Side effect requested by update, executed by the runtime.
#[derive(Debug)]
pub enum Command {
    Refresh(u64),
}

/// Ongoing event source kept alive by the runtime, such as a timer.
#[derive(Debug)]
pub enum Subscription {
    Every(Duration, Message),
}

pub fn update(message: Message, mut model: Model) -> (Model, Option<Command>) {
    let command = match message {
        Message::Bootstrap | Message::Tick => request_refresh(&mut model),
        Message::Refreshed(seq, report, service_status) => {
            if model.pending_refresh == Some(seq) {
                model.pending_refresh = None;
                model.report = report;
                model.service_status = service_status;
            }
            None
        }
    };
    (model, command)
}

fn request_refresh(model: &mut Model) -> Option<Command> {
    if model.pending_refresh.is_some() {
        return None;
    }
    let seq = model.next_refresh;
    model.next_refresh += 1;
    model.pending_refresh = Some(seq);
    Some(Command::Refresh(seq))
}

pub fn subscriptions() -> Subscription {
    Subscription::Every(TICK, Message::Tick)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_requests_one_refresh_while_pending() {
        let (model, command) = update(Message::Tick, Model::default());
        assert!(matches!(command, Some(Command::Refresh(0))));
        let (_, command) = update(Message::Tick, model);
        assert!(command.is_none());
    }

    #[test]
    fn stale_result_is_ignored() {
        let (model, _) = update(Message::Bootstrap, Model::default());
        let (model, _) = update(Message::Refreshed(7, None, "ACTIVE".to_string()), model);
        assert_eq!(model.pending_refresh, Some(0));
        assert_eq!(model.service_status, "");
    }

    #[test]
    fn matching_result_clears_pending() {
        let (model, _) = update(Message::Bootstrap, Model::default());
        let (model, command) = update(Message::Refreshed(0, None, "ACTIVE".to_string()), model);
        assert!(command.is_none());
        assert_eq!(model.pending_refresh, None);
        assert_eq!(model.service_status, "ACTIVE");
    }
}
