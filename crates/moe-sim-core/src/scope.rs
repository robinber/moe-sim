//! Cache scope: one global budget envelope, or explicit per-layer quotas.

use std::collections::BTreeMap;
use std::fmt;

/// Cache scope selected for one replay.
///
/// Policy and scope are independent: the same policy decides evictions within
/// whichever scope is selected. A global cache uses the total budget. A
/// per-layer cache holds one independent resident cache per layer, each
/// bounded by its explicit quota; unused quota is not shared in `v0.1`.
///
/// An event's atomic active set never crosses layers, so per-layer caches
/// partition residency cleanly: every event runs entirely inside the cache of
/// its own layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheScope {
    /// One cache bounded by the total budget.
    Global {
        /// Total capacity budget in bytes.
        budget_bytes: u64,
    },
    /// One independent cache per layer, each bounded by its explicit quota.
    ///
    /// Every simulated layer needs an explicit quota, and the quotas must sum
    /// to no more than `total_budget_bytes`.
    PerLayer {
        /// Total budget the quota sum may not exceed.
        total_budget_bytes: u64,
        /// Explicit byte quota for each layer, keyed by layer id.
        layer_quota_bytes: BTreeMap<u32, u64>,
    },
}

impl fmt::Display for CacheScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Global { .. } => "global",
            Self::PerLayer { .. } => "per-layer",
        };
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_names_match_the_report_contract() {
        assert_eq!(CacheScope::Global { budget_bytes: 1 }.to_string(), "global");
        assert_eq!(
            CacheScope::PerLayer {
                total_budget_bytes: 1,
                layer_quota_bytes: BTreeMap::new(),
            }
            .to_string(),
            "per-layer"
        );
    }
}
