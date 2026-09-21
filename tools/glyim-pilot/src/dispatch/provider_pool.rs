use crate::config::types::ProviderConfig;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;

/// ProviderPool.
pub struct ProviderPool {
    providers: HashMap<String, ProviderState>,
}

#[derive(Debug, Clone)]
struct ProviderState {
    config: Arc<ProviderConfig>,
    active_slots: usize,
    cooldown_until: Option<DateTime<Utc>>,
}

impl ProviderPool {
    /// new.
    pub fn new(providers: &HashMap<String, ProviderConfig>) -> Self {
        let mut states = HashMap::new();
        for (id, config) in providers {
            if config.enabled {
                states.insert(
                    id.clone(),
                    ProviderState {
                        config: Arc::new(config.clone()),
                        active_slots: 0,
                        cooldown_until: None,
                    },
                );
            }
        }
        Self { providers: states }
    }
    /// allocate.
    pub fn allocate(&mut self, provider_id: &str) -> Result<(), String> {
        let state = self
            .providers
            .get_mut(provider_id)
            .ok_or("provider not found")?;
        if state.active_slots >= state.config.max_concurrent {
            return Err("no slots".into());
        }
        state.active_slots += 1;
        Ok(())
    }
    /// free.
    pub fn free(&mut self, provider_id: &str) {
        if let Some(state) = self.providers.get_mut(provider_id) {
            state.active_slots = state.active_slots.saturating_sub(1);
        }
    }
    /// most_slots_available.
    pub fn most_slots_available(&self) -> Option<(String, usize)> {
        self.providers
            .iter()
            .filter(|(_, s)| s.active_slots < s.config.max_concurrent)
            .max_by_key(|(_, s)| s.config.max_concurrent - s.active_slots)
            .map(|(id, s)| (id.clone(), s.config.max_concurrent - s.active_slots))
    }
}

impl ProviderPool {
    /// get_config.
    pub fn get_config(&self, provider_id: &str) -> Option<Arc<ProviderConfig>> {
        self.providers.get(provider_id).map(|s| s.config.clone())
    }

    /// cooldown.
    pub fn cooldown(&mut self, provider_id: &str, duration_secs: u64) {
        if let Some(state) = self.providers.get_mut(provider_id) {
            state.cooldown_until = Some(Utc::now() + Duration::seconds(duration_secs as i64));
        }
    }
}

impl ProviderPool {
    /// provider_ids.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// available_slots.
    pub fn available_slots(&self, provider_id: &str) -> usize {
        self.providers
            .get(provider_id)
            .map(|s| s.config.max_concurrent.saturating_sub(s.active_slots))
            .unwrap_or(0)
    }
}
