//! CPU statistics tracking.
//!
//! Ported from `src/adb/proc/stat.ts`. Tracks `/proc/stat` on the device and
//! emits useful information, such as CPU load.

use std::collections::BTreeMap;

use tokio::sync::mpsc;

use crate::connection::AdbStream;
use crate::error::Error;
use crate::sync::{PullOptions, Sync};
use crate::types::{CpuStats, Loads};

/// An event emitted by a [`ProcStat`] tracker.
#[derive(Debug)]
pub enum ProcStatEvent {
    /// A CPU load calculation is available.
    Load(Loads),
    /// An error occurred.
    Error(Error),
    /// The tracker was ended.
    End,
}

/// Raw statistics for a single CPU, including the line it was parsed from.
#[derive(Debug, Clone, Default, PartialEq)]
struct CpuStatsWithLine {
    stats: CpuStats,
    line: String,
}

/// The raw statistics of the previous update.
///
/// Passed to [`update`] so that the loads can be computed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawStats {
    cpus: BTreeMap<String, CpuStatsWithLine>,
}

impl RawStats {
    /// Creates empty statistics.
    pub fn new() -> Self {
        RawStats::default()
    }
}

/// Tracks `/proc/stat` and emits useful information, such as CPU load.
///
/// A single sync service instance is used to download the `/proc/stat` file for
/// processing. While doing this does consume some resources, it is very light
/// and should not be a problem.
///
/// Ported from `src/adb/proc/stat.ts`.
pub struct ProcStat<S>
where
    S: AdbStream,
{
    events: mpsc::Receiver<ProcStatEvent>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    _marker: std::marker::PhantomData<std::sync::Mutex<S>>,
}

impl<S> ProcStat<S>
where
    S: AdbStream,
{
    /// The polling interval, in milliseconds.
    pub const INTERVAL: u64 = 1000;

    /// Starts tracking with the given sync connection.
    ///
    /// Ported from the `ProcStat` constructor.
    pub fn new(mut sync: Sync<S>) -> Self {
        let (tx, rx) = mpsc::channel(16);
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut raw = RawStats::default();
            let mut ignore: BTreeMap<String, String> = BTreeMap::new();
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(Self::INTERVAL));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                match sync.pull("/proc/stat", PullOptions::default()).await {
                    Ok(transfer) => match transfer.read_to_vec().await {
                        Ok(data) => {
                            let text = String::from_utf8_lossy(&data).into_owned();
                            if let Some(loads) = update(&mut raw, &mut ignore, &text) {
                                if tx.send(ProcStatEvent::Load(loads)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            if tx.send(ProcStatEvent::Error(err)).await.is_err() {
                                break;
                            }
                        }
                    },
                    Err(err) => {
                        if tx.send(ProcStatEvent::Error(err)).await.is_err() {
                            break;
                        }
                        break;
                    }
                }

                tokio::select! {
                    _ = interval.tick() => {}
                    _ = &mut stop_rx => break,
                }
            }

            sync.end().await;
            let _ = tx.send(ProcStatEvent::End).await;
        });

        ProcStat {
            events: rx,
            stop: Some(stop_tx),
            _marker: std::marker::PhantomData,
        }
    }

    /// Waits for the next event. Returns `None` once the tracker has ended.
    pub async fn next(&mut self) -> Option<ProcStatEvent> {
        self.events.recv().await
    }

    /// Stops tracking.
    ///
    /// Ported from `ProcStat.end()`.
    pub fn end(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl<S> futures::Stream for ProcStat<S>
where
    S: AdbStream,
{
    type Item = ProcStatEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.events.poll_recv(cx)
    }
}

impl<S> std::fmt::Debug for ProcStat<S>
where
    S: AdbStream,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcStat").finish_non_exhaustive()
    }
}

/// Parses `/proc/stat` and computes the loads since the last call.
///
/// Ported from `ProcStat._parse()` and `ProcStat._set()`.
pub fn update(
    raw: &mut RawStats,
    ignore: &mut BTreeMap<String, String>,
    out: &str,
) -> Option<Loads> {
    let parsed = parse(out, ignore);
    let mut loads: Loads = Default::default();
    let mut found = false;

    for (id, current) in &parsed.cpus {
        let old = match raw.cpus.get(id) {
            Some(old) => old,
            None => continue,
        };
        let ticks = current.stats.total - old.stats.total;
        if ticks > 0.0 {
            found = true;
            // Calculate percentages for everything. For ease of formatting,
            // let's do `x / y * 100` as `100 / y * x`.
            let m = 100.0 / ticks;
            let pct = |current: f64, old: f64| (m * (current - old)).floor();
            loads.insert(
                id.clone(),
                CpuStats {
                    user: pct(current.stats.user, old.stats.user),
                    nice: pct(current.stats.nice, old.stats.nice),
                    system: pct(current.stats.system, old.stats.system),
                    idle: pct(current.stats.idle, old.stats.idle),
                    iowait: pct(current.stats.iowait, old.stats.iowait),
                    irq: pct(current.stats.irq, old.stats.irq),
                    softirq: pct(current.stats.softirq, old.stats.softirq),
                    steal: pct(current.stats.steal, old.stats.steal),
                    guest: pct(current.stats.guest, old.stats.guest),
                    guestnice: pct(current.stats.guestnice, old.stats.guestnice),
                    total: 100.0,
                },
            );
        } else {
            // The CPU is either offline (nothing was done) or it mysteriously
            // warped back in time (idle stat dropped significantly), causing
            // the total tick count to be <0. The latter seems to only happen
            // on Galaxy S4 so far. Either way we don't want those anomalies in
            // our stats. We'll also ignore the line in the next cycle. This
            // doesn't completely eliminate the anomalies, but it helps.
            ignore.insert(id.clone(), current.line.clone());
        }
    }

    *raw = parsed;

    if found {
        Some(loads)
    } else {
        None
    }
}

/// Parses the `cpu*` lines of `/proc/stat`.
fn parse(out: &str, ignore: &BTreeMap<String, String>) -> RawStats {
    let re = regex::Regex::new(r"^cpu[0-9]+ .*$").unwrap();
    let mut stats = RawStats::default();
    for line in out.lines() {
        if !re.is_match(line) {
            continue;
        }
        let columns: Vec<&str> = line
            .split([' ', '\t'])
            .filter(|column| !column.is_empty())
            .collect();
        if columns.is_empty() {
            continue;
        }
        let r#type = columns[0];
        if ignore.get(r#type) == Some(&line.to_string()) {
            continue;
        }
        let values: Vec<f64> = columns[1..]
            .iter()
            .map(|value| value.parse::<f64>().unwrap_or(0.0))
            .collect();
        let get = |index: usize| values.get(index).copied().unwrap_or(0.0);
        let total: f64 = values.iter().sum();
        stats.cpus.insert(
            r#type.to_string(),
            CpuStatsWithLine {
                stats: CpuStats {
                    user: get(0),
                    nice: get(1),
                    system: get(2),
                    idle: get(3),
                    iowait: get(4),
                    irq: get(5),
                    softirq: get(6),
                    steal: get(7),
                    guest: get(8),
                    guestnice: get(9),
                    total,
                },
                line: line.to_string(),
            },
        );
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 200 300 400 500 600 700 800 900 1000
cpu0 10 20 30 40 50 60 70 80 90 100
cpu1 1 2 3 4 5 6 7 8 9 10
intr 1234
ctxt 5678
";

    #[test]
    fn test_parse_cpu_lines() {
        let ignore = BTreeMap::new();
        let parsed = parse(STAT, &ignore);
        // The aggregate "cpu" line is skipped, just like the original regex.
        assert_eq!(parsed.cpus.len(), 2);
        assert!(parsed.cpus.contains_key("cpu0"));
        assert!(parsed.cpus.contains_key("cpu1"));
        let cpu0 = parsed.cpus.get("cpu0").unwrap();
        assert_eq!(cpu0.stats.user, 10.0);
        assert_eq!(cpu0.stats.total, 550.0);
    }

    #[test]
    fn test_update_computes_loads() {
        let mut ignore = BTreeMap::new();
        let mut raw = RawStats::default();
        let _ = update(&mut raw, &mut ignore, STAT);

        let next = "cpu  100 200 300 500 500 600 700 800 900 1000
cpu0 20 20 30 40 50 60 70 80 90 100
cpu1 1 2 3 4 5 6 7 8 9 10
";
        let loads = update(&mut raw, &mut ignore, next).expect("should find loads");
        // cpu0 went from 550 to 560 ticks.
        let cpu0 = loads.get("cpu0").expect("cpu0 should have a load");
        assert_eq!(cpu0.total, 100.0);
        // user went from 10 to 20 ticks out of 10 more ticks.
        assert_eq!(cpu0.user, 100.0);
        // cpu1 stayed at the same total, so it is ignored.
        assert!(!loads.contains_key("cpu1"));
    }

    #[test]
    fn test_update_ignores_offline_cpu() {
        let mut ignore = BTreeMap::new();
        let mut raw = RawStats::default();
        let _ = update(&mut raw, &mut ignore, STAT);
        let next = "cpu0 0 0 0 0 0 0 0 0 0 0
";
        let loads = update(&mut raw, &mut ignore, next);
        assert!(!loads.map(|l| l.contains_key("cpu0")).unwrap_or(false));
        assert!(ignore.contains_key("cpu0"));
    }
}
