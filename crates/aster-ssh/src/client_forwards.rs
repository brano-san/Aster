use crate::client::{Conn, Tunnel};
use crate::forward::{self, Event, Running};
use aster_proto::ssh_forward::{Command, Forward, ForwardStatus};
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, watch};

pub(super) struct LiveForwards {
    pub commands: mpsc::UnboundedSender<Command>,
    pub statuses: watch::Receiver<Vec<ForwardStatus>>,
}

struct Entry {
    status: ForwardStatus,
    listener: Option<Running>,
}

pub(super) struct ForwardWorker {
    commands: mpsc::UnboundedReceiver<Command>,
    statuses: watch::Sender<Vec<ForwardStatus>>,
    entries: Vec<Entry>,
    next_index: usize,
}

impl ForwardWorker {
    pub fn new() -> (LiveForwards, Self) {
        let (send, commands) = mpsc::unbounded_channel();
        let (statuses, receive) = watch::channel(Vec::new());
        (LiveForwards { commands: send, statuses: receive }, Self { commands, statuses, entries: Vec::new(), next_index: 0 })
    }

    pub async fn add(&mut self, conn: &Arc<Conn>, forward: Forward) -> ForwardStatus {
        let (bound, listener) = if let Forward::Remote { bind, port, host, host_port } = &forward {
            (conn.request_remote_forward(bind, *port, host, *host_port).await, None)
        } else {
            let (events, mut seen) = mpsc::unbounded_channel();
            let listener = forward::start(forward.clone(), Arc::new(Tunnel(Arc::downgrade(conn))), events).await;
            match seen.try_recv() {
                Ok(Event::Listening { port, .. }) => (Ok(u32::from(port)), Some(listener)),
                Ok(Event::Failed { error, .. }) => (Err(error), None),
                _ => (Err("the listener did not report its bind result".into()), None),
            }
        };
        let status = ForwardStatus { index: self.next_index, forward, bound, stop_error: None };
        self.next_index += 1;
        self.entries.push(Entry { status: status.clone(), listener });
        self.publish();
        status
    }

    async fn stop(&mut self, conn: &Conn, index: usize) {
        let Some(at) = self.entries.iter().position(|entry| entry.status.index == index) else { return };
        let entry = &mut self.entries[at];
        if let (Forward::Remote { bind, .. }, Ok(port)) = (&entry.status.forward, &entry.status.bound) {
            if let Err(error) = conn.cancel_remote_forward(bind, *port).await {
                entry.status.stop_error = Some(error);
                self.publish();
                return;
            }
        }
        if let Some(listener) = entry.listener.take() { listener.stop(); }
        self.entries.remove(at);
        self.publish();
    }

    fn publish(&self) {
        self.statuses.send_replace(self.entries.iter().map(|entry| entry.status.clone()).collect());
    }

    pub async fn run(mut self, conn: Weak<Conn>) {
        while let Some(command) = self.commands.recv().await {
            let Some(conn) = conn.upgrade() else { break };
            match command {
                Command::AddForward(forward) => { self.add(&conn, forward).await; }
                Command::StopForward(index) => self.stop(&conn, index).await,
            }
        }
    }
}
