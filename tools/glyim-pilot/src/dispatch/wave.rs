use crate::dispatch::provider_pool::ProviderPool;
use crate::error::PilotError;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
/// DispatchStrategy.
pub enum DispatchStrategy {
    /// Variant.
    MostSlotsFirst,
    /// Variant.
    RoundRobin,
    /// Variant.
    LeastLoaded,
}

impl std::str::FromStr for DispatchStrategy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "most_slots_first" => Ok(Self::MostSlotsFirst),
            "round_robin" => Ok(Self::RoundRobin),
            "least_loaded" => Ok(Self::LeastLoaded),
            _ => Err(format!("unknown strategy: {s}")),
        }
    }
}

#[derive(Debug, Clone)]
/// StreamAssignment.
pub struct StreamAssignment {
    /// Struct.
    pub stream_id: String,
    /// Struct.
    pub provider_id: String,
}

/// dispatch_wave.
pub fn dispatch_wave(
    stream_ids: &[String],
    pool: &mut ProviderPool,
    strategy: &DispatchStrategy,
) -> Result<Vec<StreamAssignment>, PilotError> {
    let mut unassigned: VecDeque<String> = stream_ids.iter().cloned().collect();
    let mut assignments = Vec::new();

    match strategy {
        DispatchStrategy::MostSlotsFirst => {
            while let Some((best_id, _)) = pool.most_slots_available() {
                if pool.allocate(&best_id).is_ok() {
                    if let Some(id) = unassigned.pop_front() {
                        assignments.push(StreamAssignment {
                            stream_id: id,
                            provider_id: best_id,
                        });
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        DispatchStrategy::RoundRobin => {
            let providers = pool.provider_ids();
            if providers.is_empty() {
                return Ok(assignments);
            }
            let mut idx = 0;
            let mut consecutive_fails = 0;
            while let Some(id) = unassigned.pop_front() {
                let pid = &providers[idx % providers.len()];
                if pool.allocate(pid).is_ok() {
                    assignments.push(StreamAssignment {
                        stream_id: id,
                        provider_id: pid.clone(),
                    });
                    consecutive_fails = 0;
                } else {
                    unassigned.push_front(id);
                    consecutive_fails += 1;
                    if consecutive_fails > providers.len() * 2 {
                        break;
                    }
                }
                idx += 1;
            }
        }
        DispatchStrategy::LeastLoaded => {
            while let Some(id) = unassigned.pop_front() {
                let mut providers = pool.provider_ids();
                // Sort by available slots descending (most free first)
                providers.sort_by_key(|b| std::cmp::Reverse(pool.available_slots(b)));
                let mut assigned = false;
                for pid in providers {
                    if pool.allocate(&pid).is_ok() {
                        assignments.push(StreamAssignment {
                            stream_id: id,
                            provider_id: pid,
                        });
                        assigned = true;
                        break;
                    }
                }
                if !assigned {
                    break;
                }
            }
        }
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ProviderConfig;
    use crate::dispatch::provider_pool::ProviderPool;
    use std::collections::HashMap;

    fn setup_pool() -> ProviderPool {
        let mut providers = HashMap::new();
        let config1 = ProviderConfig {
            enabled: true,
            max_concurrent: 2,
            ..Default::default()
        };
        let config2 = ProviderConfig {
            enabled: true,
            max_concurrent: 3,
            ..Default::default()
        };
        providers.insert("p1".to_string(), config1);
        providers.insert("p2".to_string(), config2);
        ProviderPool::new(&providers)
    }

    #[test]
    fn test_most_slots_first() {
        let mut pool = setup_pool();
        let streams = vec!["s1".to_string(), "s2".to_string()];
        let assignments =
            dispatch_wave(&streams, &mut pool, &DispatchStrategy::MostSlotsFirst).unwrap();
        assert_eq!(assignments.len(), 2);
        // The provider with most slots (p2 has 3 slots) should get first assignment
        assert_eq!(assignments[0].provider_id, "p2");
    }

    #[test]
    fn test_round_robin() {
        let mut pool = setup_pool();
        let streams = vec![
            "s1".to_string(),
            "s2".to_string(),
            "s3".to_string(),
            "s4".to_string(),
        ];
        let assignments =
            dispatch_wave(&streams, &mut pool, &DispatchStrategy::RoundRobin).unwrap();
        assert_eq!(assignments.len(), 4);
        // Since provider iteration order is arbitrary, we just check that assignments are alternating between providers
        let ids: Vec<&str> = assignments.iter().map(|a| a.provider_id.as_str()).collect();
        // The two possible patterns: ["p1","p2","p1","p2"] or ["p2","p1","p2","p1"]
        assert!(ids == vec!["p1", "p2", "p1", "p2"] || ids == vec!["p2", "p1", "p2", "p1"]);
    }

    #[test]
    fn test_least_loaded() {
        let mut pool = setup_pool();
        // Initially both have 0 slots used, so any order
        let streams = vec!["s1".to_string()];
        let assignments =
            dispatch_wave(&streams, &mut pool, &DispatchStrategy::LeastLoaded).unwrap();
        assert_eq!(assignments.len(), 1);
        // Should pick the one with most available slots (p2 has 3 > p1's 2)
        assert_eq!(assignments[0].provider_id, "p2");
    }
}
