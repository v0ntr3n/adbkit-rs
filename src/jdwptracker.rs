//! JDWP process tracking (`adb track-jdwp`).
//!
//! Ported from `src/adb/jdwptracker.ts`.

use std::sync::Arc;

use tokio::sync::{mpsc, Notify};

use crate::connection::{AdbStream, Connection};
use crate::error::Error;

/// All changes reported in a single run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JdwpTrackerChangeSet {
    /// Pids that were removed.
    pub removed: Vec<String>,
    /// Pids that were added.
    pub added: Vec<String>,
}

/// An event emitted by a [`JdwpTracker`].
#[derive(Debug)]
pub enum JdwpTrackerEvent {
    /// A new JDWP process became available.
    Add(String),
    /// A JDWP process became unavailable.
    Remove(String),
    /// All changes in a single event, plus all currently active pids.
    ChangeSet(JdwpTrackerChangeSet, Vec<String>),
    /// The underlying connection ended.
    End,
    /// An error occurred.
    Error(Error),
}

/// A JDWP tracker.
///
/// The tracker keeps a connection open; drop it (or call
/// [`JdwpTracker::end`]) when you wish to stop tracking JDWP processes.
pub struct JdwpTracker {
    rx: mpsc::Receiver<JdwpTrackerEvent>,
    stop: Arc<Notify>,
}

impl JdwpTracker {
    /// Starts tracking JDWP processes on a connection that has already sent
    /// `track-jdwp`.
    pub fn from_connection<S: AdbStream>(mut connection: Connection<S>) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let stop = Arc::new(Notify::new());
        let stop_clone = stop.clone();

        tokio::spawn(async move {
            let mut pids: Vec<String> = Vec::new();
            loop {
                tokio::select! {
                    result = connection.parser().read_value() => {
                        match result {
                            Ok(value) => {
                                let list = parse_pids(&String::from_utf8_lossy(&value));
                                let (change_set, new_pids) = update(&mut pids, list);
                                let _ = tx
                                    .send(JdwpTrackerEvent::ChangeSet(change_set, new_pids))
                                    .await;
                            }
                            Err(Error::PrematureEof(_)) => break,
                            Err(err) => {
                                let _ = tx.send(JdwpTrackerEvent::Error(err)).await;
                                break;
                            }
                        }
                    }
                    _ = stop_clone.notified() => break,
                }
            }
            let _ = connection.parser().end().await;
            let _ = tx.send(JdwpTrackerEvent::End).await;
        });

        JdwpTracker { rx, stop }
    }

    /// Waits for the next event. Returns `None` once the tracker has ended.
    pub async fn next(&mut self) -> Option<JdwpTrackerEvent> {
        self.rx.recv().await
    }

    /// Stops tracking JDWP processes.
    pub fn end(self) {
        self.stop.notify_waiters();
    }
}

impl futures::Stream for JdwpTracker {
    type Item = JdwpTrackerEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Compares a new pid list with the previous one, emitting the resulting
/// events.
///
/// Ported from `JdwpTracker.update()`.
pub fn update(
    pids: &mut Vec<String>,
    new_list: Vec<String>,
) -> (JdwpTrackerChangeSet, Vec<String>) {
    let mut change_set = JdwpTrackerChangeSet::default();
    let mut new_map: Vec<&str> = Vec::new();

    for pid in &new_list {
        if !pids.contains(pid) {
            change_set.added.push(pid.clone());
        }
        new_map.push(pid.as_str());
    }

    for pid in pids.iter() {
        if !new_map.contains(&pid.as_str()) {
            change_set.removed.push(pid.clone());
        }
    }

    *pids = new_list.clone();
    (change_set, new_list)
}

/// Splits the value sent by the ADB server into pids.
///
/// Ported from `JdwpTracker.read()`.
pub fn parse_pids(value: &str) -> Vec<String> {
    let mut pids: Vec<String> = value.split('\n').map(|pid| pid.to_string()).collect();
    let maybe_empty = pids.pop();
    if let Some(maybe_empty) = maybe_empty {
        if !maybe_empty.is_empty() {
            pids.push(maybe_empty);
        }
    }
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pids() {
        assert_eq!(parse_pids("1234\n5678"), vec!["1234", "5678"]);
        assert_eq!(parse_pids("1234\n5678\n"), vec!["1234", "5678"]);
        assert_eq!(parse_pids(""), Vec::<String>::new());
    }

    #[test]
    fn test_update() {
        let mut pids = Vec::new();
        let (change_set, all) = update(&mut pids, parse_pids("1\n2"));
        assert_eq!(change_set.added, vec!["1", "2"]);
        assert!(change_set.removed.is_empty());
        assert_eq!(all, vec!["1", "2"]);

        let (change_set, all) = update(&mut pids, parse_pids("2\n3"));
        assert_eq!(change_set.added, vec!["3"]);
        assert_eq!(change_set.removed, vec!["1"]);
        assert_eq!(all, vec!["2", "3"]);
    }
}
