use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::RwLock;
use world_model::WorldModel;

/// Real-time thresholds for missed-heartbeat detection. Real time, not
/// tick count, because game speed (and therefore tick rate) can vary --
/// see docs/OPENRCT2_INTEGRATION.md on `interval.tick`.
pub const DEGRADED_AFTER: Duration = Duration::from_secs(3);
pub const LOST_AFTER: Duration = Duration::from_secs(10);

/// Connection health state machine: CONNECTING -> LIVE -> DEGRADED (missed
/// heartbeats) -> LOST. A fresh connection starts CONNECTING; `hello`
/// moves it to LIVE; heartbeats keep it there; a gap moves it to DEGRADED
/// then LOST; a further heartbeat recovers it back to LIVE from either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Live,
    Degraded,
    Lost,
}

#[derive(Debug)]
pub struct ConnectionHealth {
    pub state: ConnectionState,
    pub last_heartbeat_at: Option<Instant>,
    pub last_heartbeat_tick: Option<u64>,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            state: ConnectionState::Connecting,
            last_heartbeat_at: None,
            last_heartbeat_tick: None,
        }
    }
}

impl ConnectionHealth {
    /// Called when a new TCP connection is accepted.
    pub fn on_connect(&mut self) {
        *self = ConnectionHealth::default();
    }

    /// Called on `hello`.
    pub fn on_hello(&mut self) {
        self.state = ConnectionState::Live;
        self.last_heartbeat_at = Some(Instant::now());
    }

    /// Called on `heartbeat { tick }`. Recovers from DEGRADED/LOST to LIVE.
    pub fn on_heartbeat(&mut self, tick: u64) {
        self.state = ConnectionState::Live;
        self.last_heartbeat_at = Some(Instant::now());
        self.last_heartbeat_tick = Some(tick);
    }

    /// Called when the socket closes or errors -- known-bad, not just
    /// "haven't heard from it in a while".
    pub fn on_disconnect(&mut self) {
        self.state = ConnectionState::Lost;
    }

    /// Re-evaluates DEGRADED/LOST based on elapsed time since the last
    /// heartbeat. Called periodically by the health-check task. Never
    /// transitions out of CONNECTING (no heartbeat has ever arrived) or
    /// out of LOST-from-disconnect on its own -- only a new connection or
    /// heartbeat recovers those.
    pub fn tick_check(&mut self) {
        if self.state == ConnectionState::Lost {
            // Only a new connection (on_connect) or a heartbeat on an
            // existing one (on_heartbeat) leaves LOST -- elapsed-time
            // recalculation must not resurrect a state we know is gone
            // just because the last heartbeat happens to still be recent.
            return;
        }
        let Some(last) = self.last_heartbeat_at else {
            return;
        };
        let elapsed = last.elapsed();
        self.state = if elapsed >= LOST_AFTER {
            ConnectionState::Lost
        } else if elapsed >= DEGRADED_AFTER {
            ConnectionState::Degraded
        } else {
            ConnectionState::Live
        };
    }
}

/// Database reachability, tracked separately from the bridge connection's
/// [`ConnectionState`] -- see db.rs. CAUTIOUS mirrors the vocabulary
/// docs/VISION.md's fuller AFK safety-state machine (0.1 only needs this
/// one degraded state; NORMAL/CONSERVATION/QUARANTINE/etc. are later
/// milestones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DbState {
    #[default]
    Connected,
    Cautious,
}

pub struct SharedState {
    pub health: ConnectionHealth,
    pub world: WorldModel,
    pub db_state: DbState,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            health: ConnectionHealth::default(),
            world: WorldModel::new(),
            db_state: DbState::default(),
        }
    }
}

pub type Shared = Arc<RwLock<SharedState>>;

pub fn new_shared() -> Shared {
    Arc::new(RwLock::new(SharedState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_connecting() {
        let health = ConnectionHealth::default();
        assert_eq!(health.state, ConnectionState::Connecting);
    }

    #[test]
    fn hello_then_heartbeat_goes_live() {
        let mut health = ConnectionHealth::default();
        health.on_hello();
        assert_eq!(health.state, ConnectionState::Live);
        health.on_heartbeat(120);
        assert_eq!(health.state, ConnectionState::Live);
        assert_eq!(health.last_heartbeat_tick, Some(120));
    }

    #[test]
    fn stale_heartbeat_degrades_then_is_lost() {
        let mut health = ConnectionHealth::default();
        health.on_hello();
        health.last_heartbeat_at = Some(Instant::now() - DEGRADED_AFTER - Duration::from_millis(1));
        health.tick_check();
        assert_eq!(health.state, ConnectionState::Degraded);

        health.last_heartbeat_at = Some(Instant::now() - LOST_AFTER - Duration::from_millis(1));
        health.tick_check();
        assert_eq!(health.state, ConnectionState::Lost);
    }

    #[test]
    fn heartbeat_recovers_from_degraded() {
        let mut health = ConnectionHealth::default();
        health.on_hello();
        health.last_heartbeat_at = Some(Instant::now() - DEGRADED_AFTER - Duration::from_millis(1));
        health.tick_check();
        assert_eq!(health.state, ConnectionState::Degraded);

        health.on_heartbeat(1);
        assert_eq!(health.state, ConnectionState::Live);
    }

    #[test]
    fn disconnect_is_lost_immediately() {
        let mut health = ConnectionHealth::default();
        health.on_hello();
        health.on_disconnect();
        assert_eq!(health.state, ConnectionState::Lost);
    }

    #[test]
    fn tick_check_does_not_resurrect_a_disconnected_state() {
        // Regression: tick_check() recomputes LIVE/DEGRADED/LOST purely
        // from elapsed time since the last heartbeat. A disconnect right
        // after a heartbeat must not get overwritten back to LIVE just
        // because that heartbeat is still recent.
        let mut health = ConnectionHealth::default();
        health.on_hello();
        health.on_heartbeat(1);
        health.on_disconnect();
        assert_eq!(health.state, ConnectionState::Lost);

        health.tick_check();
        assert_eq!(health.state, ConnectionState::Lost);
    }

    #[test]
    fn new_connection_resets_to_connecting() {
        let mut health = ConnectionHealth::default();
        health.on_hello();
        health.on_disconnect();
        health.on_connect();
        assert_eq!(health.state, ConnectionState::Connecting);
        assert_eq!(health.last_heartbeat_tick, None);
    }
}
