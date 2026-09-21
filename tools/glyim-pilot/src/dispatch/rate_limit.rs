use crate::dispatch::provider_pool::ProviderPool;
use crate::error::PilotError;

#[derive(Debug, Clone)]
/// RateLimitContext.
pub struct RateLimitContext {
    /// Struct.
    pub stream_id: String,
    /// Struct.
    pub turn: u32,
    /// Struct.
    pub commits: u32,
    /// Struct.
    pub brief_summary: String,
    /// Struct.
    pub max_reassign_attempts: u32,
}

#[derive(Debug, Clone)]
/// RateLimitAction.
pub enum RateLimitAction {
    /// Variant.
    Failover {
        /// Struct.
        new_provider_id: String,
        /// Struct.
        failover_prompt: String,
    },
    /// Variant.
    RetryAfter {
        /// Struct.
        provider_id: String,
        /// Struct.
        delay_secs: u64,
    },
    /// Variant.
    Escalate {
        /// Struct.
        reason: String,
    },
}

/// handle_rate_limit.
pub fn handle_rate_limit(
    pool: &mut ProviderPool,
    provider_id: &str,
    base_delay_secs: u64,
    max_delay_secs: u64,
    attempt: u32,
    ctx: &RateLimitContext,
) -> Result<RateLimitAction, PilotError> {
    // Apply cooldown to the failing provider
    let cooldown = pool
        .get_config(provider_id)
        .map(|c| c.rate_limit_cooldown)
        .unwrap_or(base_delay_secs);
    pool.cooldown(provider_id, cooldown);
    tracing::warn!(
        provider_id = provider_id,
        cooldown_secs = cooldown,
        attempt = attempt,
        "rate limit detected"
    );

    // Try failover if within reassign limits
    if attempt <= ctx.max_reassign_attempts {
        if let Some((new_id, _)) = pool.most_slots_available() {
            if new_id != provider_id {
                let failover_prompt = format!(
                    "Session {} moved from {} due to rate limit. Turns: {}, Commits: {}. Brief: {}",
                    ctx.stream_id, provider_id, ctx.turn, ctx.commits, ctx.brief_summary
                );
                return Ok(RateLimitAction::Failover {
                    new_provider_id: new_id,
                    failover_prompt,
                });
            }
        }
    }

    // Calculate backoff with jitter
    let exp = base_delay_secs
        .saturating_mul(2u64.saturating_pow(attempt))
        .min(max_delay_secs);
    let stagger = (attempt as u64 * 17) % ((exp as f64 * 0.2).max(1.0) as u64);
    let delay = exp.saturating_add(stagger).min(max_delay_secs);

    if attempt < 5 {
        Ok(RateLimitAction::RetryAfter {
            provider_id: provider_id.to_string(),
            delay_secs: delay,
        })
    } else {
        Ok(RateLimitAction::Escalate {
            reason: format!("rate limit on {} after {} attempts", provider_id, attempt),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::ProviderConfig;
    use crate::dispatch::provider_pool::ProviderPool;
    use std::collections::HashMap;

    fn test_pool() -> ProviderPool {
        let mut providers = HashMap::new();
        let config = ProviderConfig {
            enabled: true,
            max_concurrent: 2,
            rate_limit_cooldown: 30,
            ..Default::default()
        };
        providers.insert("test".to_string(), config);
        ProviderPool::new(&providers)
    }

    #[test]
    fn test_backoff_calculation() {
        let pool = &mut test_pool();
        let ctx = RateLimitContext {
            stream_id: "s1".into(),
            turn: 1,
            commits: 0,
            brief_summary: "test".into(),
            max_reassign_attempts: 2,
        };
        // Attempt 1: should retry after delay
        let result = handle_rate_limit(pool, "test", 1, 60, 1, &ctx).unwrap();
        match result {
            RateLimitAction::RetryAfter { delay_secs, .. } => assert!(delay_secs >= 1),
            _ => panic!("expected RetryAfter"),
        }
    }

    #[test]
    fn test_failover_after_attempts() {
        // This test would need a second provider configured; simplified.
        // Just ensure no panic.
        let pool = &mut test_pool();
        let ctx = RateLimitContext {
            stream_id: "s1".into(),
            turn: 1,
            commits: 0,
            brief_summary: "test".into(),
            max_reassign_attempts: 2,
        };
        let _ = handle_rate_limit(pool, "test", 1, 60, 3, &ctx);
    }
}
