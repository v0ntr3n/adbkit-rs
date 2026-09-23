//! Device tracking (`adb host:track-devices`).
//!
//! Ported from `src/adb/tracker.ts`. The TypeScript tracker is an
//! `EventEmitter` that keeps a connection open and emits `add`, `remove`,
//! `change`, `changeSet`, `end` and `error` events; here the events are sent
//! over a channel, which can be consumed as a [`futures::Stream`].

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, Notify};

use crate::command;
use crate::connection::{AdbStream, Connection};
use crate::error::Error;
use crate::types::{Device, TrackerChangeSet};

/// An event emitted by a [`TrackerHandle`].
#[derive(Debug)]
pub enum TrackerEvent {
    /// A new device was connected.
    Add(Device),
    /// A device was unplugged. This does not include `offline` devices.
    Remove(Device),
    /// The type of a device changed.
    Change {
        /// The device, with its new type.
        device: Device,
        /// The device, with its previous type.
        old_device: Device,
    },
    /// All changes reported by ADB in a single run.
    ChangeSet(TrackerChangeSet),
    /// The underlying connection ended.
    End,
    /// An error occurred.
    Error(Error),
}

/// The tracker state machine.
///
/// Kept separate from the event source so that it can be unit tested, just
/// like the TypeScript original.
pub struct Tracker {
    device_list: Vec<Device>,
    device_map: BTreeMap<String, Device>,
    tx: mpsc::Sender<TrackerEvent>,
}

impl Tracker {
    /// Creates a tracker that emits events on the given channel.
    pub fn new(tx: mpsc::Sender<TrackerEvent>) -> Self {
        Tracker {
            device_list: Vec::new(),
            device_map: BTreeMap::new(),
            tx,
        }
    }

    /// Processes a new device list, emitting the resulting events.
    ///
    /// Ported from `Tracker.update()`.
    pub fn update(&mut self, new_list: Vec<Device>) -> TrackerChangeSet {
        let mut change_set = TrackerChangeSet::default();
        let mut new_map = BTreeMap::new();

        for device in &new_list {
            match self.device_map.get(&device.id) {
                Some(old_device) if old_device.r#type != device.r#type => {
                    change_set.changed.push(device.clone());
                    let _ = self.tx.try_send(TrackerEvent::Change {
                        device: device.clone(),
                        old_device: old_device.clone(),
                    });
                }
                Some(_) => {}
                None => {
                    change_set.added.push(device.clone());
                    let _ = self.tx.try_send(TrackerEvent::Add(device.clone()));
                }
            }
            new_map.insert(device.id.clone(), device.clone());
        }

        for device in &self.device_list {
            if !new_map.contains_key(&device.id) {
                change_set.removed.push(device.clone());
                let _ = self.tx.try_send(TrackerEvent::Remove(device.clone()));
            }
        }

        let _ = self
            .tx
            .try_send(TrackerEvent::ChangeSet(change_set.clone()));
        self.device_list = new_list;
        self.device_map = new_map;
        change_set
    }

    /// The devices seen in the last update.
    pub fn devices(&self) -> &[Device] {
        &self.device_list
    }
}

/// A device tracker.
///
/// Events are emitted as devices are added, removed, or change their type. Note
/// that the same events will be emitted for the initially connected devices
/// also, so you don't need to use both `Client::listDevices()` and
/// `Client::trackDevices()`.
///
/// The tracker keeps a connection open; drop it (or call
/// [`TrackerHandle::end`]) when you wish to stop tracking devices.
pub struct TrackerHandle {
    rx: mpsc::Receiver<TrackerEvent>,
    stop: Arc<Notify>,
    ended: Arc<AtomicBool>,
}

impl TrackerHandle {
    /// Starts tracking devices on a connection that has already entered
    /// `host:track-devices` mode.
    pub fn spawn<S: AdbStream>(mut connection: Connection<S>) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let stop = Arc::new(Notify::new());
        let stop_clone = stop.clone();

        tokio::spawn(async move {
            let mut tracker = Tracker::new(tx.clone());
            loop {
                tokio::select! {
                    result = command::host::read_devices(connection.parser()) => {
                        match result {
                            Ok(list) => {
                                tracker.update(list);
                            }
                            Err(Error::PrematureEof(_)) => {
                                let _ = tx
                                    .send(TrackerEvent::Error(Error::Device(
                                        "Connection closed".to_string(),
                                    )))
                                    .await;
                                break;
                            }
                            Err(err) => {
                                let _ = tx.send(TrackerEvent::Error(err)).await;
                                break;
                            }
                        }
                    }
                    _ = stop_clone.notified() => break,
                }
            }
            let _ = connection.parser().end().await;
            let _ = tx.send(TrackerEvent::End).await;
        });

        TrackerHandle {
            rx,
            stop,
            ended: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Waits for the next event. Returns `None` once the tracker has ended.
    pub async fn next(&mut self) -> Option<TrackerEvent> {
        self.rx.recv().await
    }

    /// Stops tracking devices.
    pub fn end(self) {
        self.ended.store(true, Ordering::SeqCst);
        self.stop.notify_waiters();
    }
}

impl futures::Stream for TrackerHandle {
    type Item = TrackerEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

impl std::fmt::Debug for TrackerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackerHandle").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, r#type: crate::types::DeviceType) -> Device {
        Device::new(id, r#type)
    }

    async fn drain(rx: &mut mpsc::Receiver<TrackerEvent>) -> Vec<TrackerEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn test_emits_add() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut tracker = Tracker::new(tx);
        let d1 = device("a", crate::types::DeviceType::Device);
        let d2 = device("b", crate::types::DeviceType::Device);
        tracker.update(vec![d1.clone(), d2.clone()]);
        let events = drain(&mut rx).await;
        assert!(matches!(&events[0], TrackerEvent::Add(d) if *d == d1));
        assert!(matches!(&events[1], TrackerEvent::Add(d) if *d == d2));
        assert!(matches!(&events[2], TrackerEvent::ChangeSet(_)));
    }

    #[tokio::test]
    async fn test_emits_remove() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut tracker = Tracker::new(tx);
        let d1 = device("a", crate::types::DeviceType::Device);
        let d2 = device("b", crate::types::DeviceType::Device);
        tracker.update(vec![d1.clone(), d2.clone()]);
        drain(&mut rx).await;
        tracker.update(vec![d1]);
        let events = drain(&mut rx).await;
        assert!(matches!(&events[0], TrackerEvent::Remove(d) if *d == d2));
    }

    #[tokio::test]
    async fn test_emits_change() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut tracker = Tracker::new(tx);
        let old = device("a", crate::types::DeviceType::Device);
        let new = device("a", crate::types::DeviceType::Offline);
        tracker.update(vec![old.clone()]);
        drain(&mut rx).await;
        tracker.update(vec![new.clone()]);
        let events = drain(&mut rx).await;
        match &events[0] {
            TrackerEvent::Change { device, old_device } => {
                assert_eq!(device, &new);
                assert_eq!(old_device, &old);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_emits_change_set() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut tracker = Tracker::new(tx);
        let d1 = device("a", crate::types::DeviceType::Device);
        let d2 = device("b", crate::types::DeviceType::Device);
        let d3 = device("c", crate::types::DeviceType::Device);
        tracker.update(vec![d1.clone(), d2.clone(), d3.clone()]);
        drain(&mut rx).await;

        let d3_new = device("c", crate::types::DeviceType::Offline);
        let d4 = device("d", crate::types::DeviceType::Offline);
        let change_set = tracker.update(vec![d1, d3_new.clone(), d4.clone()]);
        assert_eq!(change_set.added, vec![d4]);
        assert_eq!(change_set.changed, vec![d3_new]);
        assert_eq!(change_set.removed, vec![d2]);
    }
}
