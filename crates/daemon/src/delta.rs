use crate::poller::InterfaceSnapshot;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct InterfaceDelta {
    pub ts: i64,
    pub interval_secs: u32,
    pub interface_guid: String,
    pub interface_name: String,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub interface_type: u32,
    pub is_metered: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
struct CounterState {
    sent: u64,
    recv: u64,
}

#[derive(Debug)]
pub struct DeltaEngine {
    previous: HashMap<String, CounterState>,
    max_delta_bytes: u64,
}

impl DeltaEngine {
    pub fn new(max_delta_bytes: u64) -> Self {
        Self {
            previous: HashMap::new(),
            max_delta_bytes,
        }
    }

    pub fn compute(
        &mut self,
        snapshots: &[InterfaceSnapshot],
        ts: i64,
        interval_secs: u32,
        nominal_interval_secs: u32,
    ) -> Vec<InterfaceDelta> {
        let mut deltas = Vec::with_capacity(snapshots.len());
        let allowed_delta = self.allowed_delta_bytes(interval_secs, nominal_interval_secs);

        for snapshot in snapshots {
            let current = CounterState {
                sent: snapshot.bytes_sent,
                recv: snapshot.bytes_recv,
            };

            if let Some(previous) = self.previous.get(&snapshot.interface_guid).copied() {
                let delta_sent = compute_counter_delta(previous.sent, current.sent, allowed_delta);
                let delta_recv = compute_counter_delta(previous.recv, current.recv, allowed_delta);
                let combined = delta_sent.saturating_add(delta_recv);

                if combined > 0 && combined <= allowed_delta {
                    deltas.push(InterfaceDelta {
                        ts,
                        interval_secs,
                        interface_guid: snapshot.interface_guid.clone(),
                        interface_name: snapshot.interface_name.clone(),
                        bytes_sent: delta_sent,
                        bytes_recv: delta_recv,
                        interface_type: snapshot.interface_type,
                        is_metered: snapshot.is_metered,
                    });
                }
            }

            self.previous
                .insert(snapshot.interface_guid.clone(), current);
        }

        deltas
    }

    fn allowed_delta_bytes(&self, interval_secs: u32, nominal_interval_secs: u32) -> u64 {
        let interval = u128::from(interval_secs.max(1));
        let nominal = u128::from(nominal_interval_secs.max(1));
        let base = u128::from(self.max_delta_bytes);

        let scaled = base
            .saturating_mul(interval)
            .saturating_add(nominal.saturating_sub(1))
            / nominal;

        scaled.min(u128::from(u64::MAX)) as u64
    }
}

fn compute_counter_delta(previous: u64, current: u64, allowed_delta: u64) -> u64 {
    if current >= previous {
        current - previous
    } else if current <= allowed_delta {
        current
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(guid: &str, sent: u64, recv: u64) -> InterfaceSnapshot {
        InterfaceSnapshot {
            interface_guid: guid.to_string(),
            interface_name: "Ethernet".to_string(),
            bytes_sent: sent,
            bytes_recv: recv,
            interface_type: 6,
            is_metered: Some(false),
        }
    }

    #[test]
    fn first_observation_has_no_delta() {
        let mut engine = DeltaEngine::new(10_000);
        let deltas = engine.compute(&[snapshot("{if0}", 100, 200)], 1000, 60, 60);
        assert!(deltas.is_empty());
    }

    #[test]
    fn normal_growth_emits_delta() {
        let mut engine = DeltaEngine::new(10_000);
        engine.compute(&[snapshot("{if0}", 100, 200)], 1000, 60, 60);

        let deltas = engine.compute(&[snapshot("{if0}", 180, 260)], 1060, 60, 60);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].bytes_sent, 80);
        assert_eq!(deltas[0].bytes_recv, 60);
    }

    #[test]
    fn long_observed_interval_scales_anomaly_budget() {
        let mut engine = DeltaEngine::new(100);
        engine.compute(&[snapshot("{if0}", 100, 100)], 1000, 10, 10);

        let deltas = engine.compute(&[snapshot("{if0}", 400, 400)], 1060, 60, 10);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].bytes_sent, 300);
        assert_eq!(deltas[0].bytes_recv, 300);
    }

    #[test]
    fn short_observed_interval_tightens_anomaly_budget() {
        let mut engine = DeltaEngine::new(1_000);
        engine.compute(&[snapshot("{if0}", 100, 100)], 1000, 60, 60);

        let deltas = engine.compute(&[snapshot("{if0}", 400, 400)], 1015, 15, 60);
        assert!(deltas.is_empty());
    }

    #[test]
    fn large_regression_is_suppressed() {
        let mut engine = DeltaEngine::new(1_000);
        engine.compute(&[snapshot("{if0}", 4_000, 4_000)], 1000, 60, 60);

        let deltas = engine.compute(&[snapshot("{if0}", 3_000, 3_100)], 1060, 60, 60);
        assert!(deltas.is_empty());
    }

    #[test]
    fn small_regression_is_treated_as_reset() {
        let mut engine = DeltaEngine::new(1_000);
        engine.compute(&[snapshot("{if0}", 4_000, 4_000)], 1000, 60, 60);

        let deltas = engine.compute(&[snapshot("{if0}", 100, 120)], 1060, 60, 60);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].bytes_sent, 100);
        assert_eq!(deltas[0].bytes_recv, 120);
    }
}
