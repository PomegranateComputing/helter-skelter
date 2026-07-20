//! In-memory model of the running simulation as observed through
//! `observation.snapshot` messages: the latest snapshot, the current tick,
//! and metrics derived from a bounded window of recent history. Every
//! derived-metric function here is pure (no I/O, no shared state) so it
//! can be unit-tested directly against constructed snapshots.

use std::collections::VecDeque;

use common::protocol::ObservationSnapshot;

/// How many snapshots of history `WorldModel` retains. Bounded so a
/// long-running orchestrator doesn't grow memory unboundedly; large enough
/// to cover the trend windows callers are likely to ask for.
pub const MAX_HISTORY: usize = 64;

/// Latest known state of the simulation, plus a bounded window of history
/// for trend calculations.
#[derive(Debug, Default)]
pub struct WorldModel {
    history: VecDeque<ObservationSnapshot>,
    tick: Option<u64>,
}

impl WorldModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a new snapshot as the latest state, evicting the oldest
    /// entry once history exceeds [`MAX_HISTORY`].
    pub fn record_snapshot(&mut self, snapshot: ObservationSnapshot) {
        self.history.push_back(snapshot);
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }

    /// Records the tick a message was observed at (from `heartbeat` or any
    /// other envelope carrying tick information).
    pub fn record_tick(&mut self, tick: u64) {
        self.tick = Some(tick);
    }

    pub fn tick(&self) -> Option<u64> {
        self.tick
    }

    pub fn latest(&self) -> Option<&ObservationSnapshot> {
        self.history.back()
    }

    pub fn history(&self) -> &VecDeque<ObservationSnapshot> {
        &self.history
    }

    /// Change in `cash` between the latest snapshot and the snapshot `n`
    /// positions before it. `None` if fewer than `n + 1` snapshots exist.
    pub fn cash_trend(&self, n: usize) -> Option<i64> {
        cash_trend(&self.history, n)
    }

    /// Sum of `queue_length` across every ride in the latest snapshot.
    pub fn total_queue_length(&self) -> Option<u32> {
        self.latest().map(total_queue_length)
    }
}

/// Pure: change in `cash` between the most recent snapshot in `history`
/// and the snapshot `n` positions before it (`n = 1` means "since the
/// previous snapshot"). `None` if `history` has fewer than `n + 1` entries
/// or `n == 0`.
pub fn cash_trend(history: &VecDeque<ObservationSnapshot>, n: usize) -> Option<i64> {
    if n == 0 {
        return None;
    }
    let len = history.len();
    if len <= n {
        return None;
    }
    let latest = history.get(len - 1)?;
    let earlier = history.get(len - 1 - n)?;
    Some(latest.cash - earlier.cash)
}

/// Pure: sum of `queue_length` across every ride in a single snapshot.
pub fn total_queue_length(snapshot: &ObservationSnapshot) -> u32 {
    snapshot.rides.iter().map(|ride| ride.queue_length).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::protocol::{ParkDate, Ride, RideStatus, StaffCounts, Weather};

    fn snapshot(cash: i64, queue_lengths: &[u32]) -> ObservationSnapshot {
        ObservationSnapshot {
            park_date: ParkDate {
                year: 1,
                month: 1,
                day: 1,
            },
            cash,
            loan: 0,
            park_rating: 500,
            guest_count: 0,
            rides: queue_lengths
                .iter()
                .enumerate()
                .map(|(i, &queue_length)| Ride {
                    id: i as u32,
                    name: format!("Ride {i}"),
                    kind: "test".to_string(),
                    status: RideStatus::Open,
                    price: 0,
                    queue_length,
                    downtime: 0,
                })
                .collect(),
            staff_counts: StaffCounts {
                handyman: 0,
                mechanic: 0,
                security: 0,
                entertainer: 0,
            },
            weather: Weather::Sunny,
        }
    }

    #[test]
    fn cash_trend_none_without_enough_history() {
        let mut history = VecDeque::new();
        history.push_back(snapshot(100, &[]));
        assert_eq!(cash_trend(&history, 1), None);
        assert_eq!(cash_trend(&history, 0), None);
    }

    #[test]
    fn cash_trend_computes_difference_n_back() {
        let mut history = VecDeque::new();
        history.push_back(snapshot(100, &[]));
        history.push_back(snapshot(150, &[]));
        history.push_back(snapshot(90, &[]));
        assert_eq!(cash_trend(&history, 1), Some(90 - 150));
        assert_eq!(cash_trend(&history, 2), Some(90 - 100));
    }

    #[test]
    fn total_queue_length_sums_all_rides() {
        let snap = snapshot(0, &[3, 0, 12]);
        assert_eq!(total_queue_length(&snap), 15);
    }

    #[test]
    fn total_queue_length_zero_with_no_rides() {
        let snap = snapshot(0, &[]);
        assert_eq!(total_queue_length(&snap), 0);
    }

    #[test]
    fn world_model_tracks_latest_and_tick() {
        let mut model = WorldModel::new();
        assert!(model.latest().is_none());
        assert_eq!(model.tick(), None);

        model.record_snapshot(snapshot(100, &[1, 2]));
        model.record_tick(40);
        assert_eq!(model.latest().unwrap().cash, 100);
        assert_eq!(model.tick(), Some(40));
        assert_eq!(model.total_queue_length(), Some(3));

        model.record_snapshot(snapshot(250, &[]));
        assert_eq!(model.cash_trend(1), Some(150));
    }

    #[test]
    fn world_model_history_is_bounded() {
        let mut model = WorldModel::new();
        for i in 0..(MAX_HISTORY + 10) {
            model.record_snapshot(snapshot(i as i64, &[]));
        }
        assert_eq!(model.history().len(), MAX_HISTORY);
        // Oldest entries should have been evicted; the latest retained
        // entry's cash should reflect the last MAX_HISTORY pushes.
        assert_eq!(model.history().front().unwrap().cash, 10);
    }
}
