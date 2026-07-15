//! Explicit expert sizes for capacity accounting.
//!
//! Policies and simulators never invent expert sizes. Every `(layer, expert)`
//! pair used by a run must appear in a [`ModelManifest`] with a positive size
//! in bytes.

use std::collections::HashMap;

use crate::trace::Event;

/// Identifies one expert instance in the model: layer and expert index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExpertKey {
    layer_id: u32,
    expert_id: u32,
}

impl ExpertKey {
    /// Builds a key for `expert_id` on `layer_id`.
    #[must_use]
    pub const fn new(layer_id: u32, expert_id: u32) -> Self {
        Self {
            layer_id,
            expert_id,
        }
    }

    /// Layer index.
    #[must_use]
    pub const fn layer_id(self) -> u32 {
        self.layer_id
    }

    /// Expert index within the layer.
    #[must_use]
    pub const fn expert_id(self) -> u32 {
        self.expert_id
    }
}

/// One stored size entry used to build a [`ModelManifest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpertSizeEntry {
    /// Expert identity.
    pub key: ExpertKey,
    /// Stored size of the expert in bytes. Must be strictly positive.
    pub size_bytes: u64,
}

/// Map of expert sizes used for byte-accurate capacity accounting.
///
/// Construction is fallible: zero sizes and duplicate keys are rejected so
/// invalid model data cannot enter the simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    sizes: HashMap<ExpertKey, u64>,
}

/// Errors returned when building a [`ModelManifest`] or looking up sizes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// An expert was declared with a zero byte size.
    #[error("expert size must be positive: layer {layer_id} expert {expert_id} has size 0")]
    ZeroSize {
        /// Layer of the invalid entry.
        layer_id: u32,
        /// Expert of the invalid entry.
        expert_id: u32,
    },
    /// The same expert key appeared more than once while building the manifest.
    #[error("duplicate expert size entry for layer {layer_id} expert {expert_id}")]
    DuplicateKey {
        /// Layer of the duplicated key.
        layer_id: u32,
        /// Expert of the duplicated key.
        expert_id: u32,
    },
    /// An event referenced an expert that is not present in the manifest.
    #[error("unknown expert in model manifest: layer {layer_id} expert {expert_id}")]
    UnknownExpert {
        /// Layer of the missing expert.
        layer_id: u32,
        /// Expert missing from the manifest.
        expert_id: u32,
    },
    /// Summing the active-set sizes would overflow `u64`.
    #[error("active-set byte total overflowed u64 for layer {layer_id}")]
    ActiveSetBytesOverflow {
        /// Layer of the event whose active set overflowed.
        layer_id: u32,
    },
}

impl ModelManifest {
    /// Builds a manifest from explicit size entries.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::ZeroSize`] when any entry has `size_bytes ==
    /// 0`. Returns [`ManifestError::DuplicateKey`] when the same
    /// [`ExpertKey`] appears more than once.
    pub fn try_from_entries(
        entries: impl IntoIterator<Item = ExpertSizeEntry>,
    ) -> Result<Self, ManifestError> {
        let mut sizes = HashMap::new();
        for entry in entries {
            if entry.size_bytes == 0 {
                return Err(ManifestError::ZeroSize {
                    layer_id: entry.key.layer_id(),
                    expert_id: entry.key.expert_id(),
                });
            }
            if sizes.insert(entry.key, entry.size_bytes).is_some() {
                return Err(ManifestError::DuplicateKey {
                    layer_id: entry.key.layer_id(),
                    expert_id: entry.key.expert_id(),
                });
            }
        }
        Ok(Self { sizes })
    }

    /// Number of expert size entries in the manifest.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// Returns `true` when the manifest contains no experts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    /// Returns `true` when `key` has an entry in the manifest.
    #[must_use]
    pub fn contains(&self, key: ExpertKey) -> bool {
        self.sizes.contains_key(&key)
    }

    /// Stored size in bytes for `key`.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::UnknownExpert`] when `key` is absent.
    pub fn size_bytes(&self, key: ExpertKey) -> Result<u64, ManifestError> {
        self.sizes
            .get(&key)
            .copied()
            .ok_or(ManifestError::UnknownExpert {
                layer_id: key.layer_id(),
                expert_id: key.expert_id(),
            })
    }

    /// Total stored size in bytes of the atomic active set of `event`.
    ///
    /// Uses `event.layer_id()` for every expert in the set. An empty active
    /// set contributes `0` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError::UnknownExpert`] when any expert in the set is
    /// missing from the manifest.
    /// Returns [`ManifestError::ActiveSetBytesOverflow`] when the sum overflows
    /// `u64`.
    pub fn active_set_bytes(&self, event: &Event) -> Result<u64, ManifestError> {
        let layer_id = event.layer_id();
        let mut total: u64 = 0;
        for &expert_id in event.expert_ids() {
            let key = ExpertKey::new(layer_id, expert_id);
            let size = self.size_bytes(key)?;
            total = total
                .checked_add(size)
                .ok_or(ManifestError::ActiveSetBytesOverflow { layer_id })?;
        }
        Ok(total)
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests exercise fallible constructors and lookups directly"
)]
mod tests {
    use super::*;
    use crate::trace::{Event, EventParts, Phase};

    fn entry(layer_id: u32, expert_id: u32, size_bytes: u64) -> ExpertSizeEntry {
        ExpertSizeEntry {
            key: ExpertKey::new(layer_id, expert_id),
            size_bytes,
        }
    }

    fn sample_event(layer_id: u32, expert_ids: Vec<u32>) -> Event {
        Event::new(EventParts {
            request_id: 1,
            phase: Phase::Decode,
            step_id: 0,
            token_position: 0,
            layer_id,
            expert_ids,
        })
        .unwrap()
    }

    #[test]
    fn manifest_accepts_unique_positive_sizes() {
        let manifest = ModelManifest::try_from_entries([
            entry(0, 0, 1_024),
            entry(0, 1, 2_048),
            entry(1, 0, 512),
        ])
        .unwrap();

        assert_eq!(manifest.len(), 3);
        assert!(!manifest.is_empty());
        assert!(manifest.contains(ExpertKey::new(0, 1)));
        assert_eq!(manifest.size_bytes(ExpertKey::new(0, 0)).unwrap(), 1_024);
        assert_eq!(manifest.size_bytes(ExpertKey::new(1, 0)).unwrap(), 512);
    }

    #[test]
    fn manifest_rejects_zero_size() {
        let err = ModelManifest::try_from_entries([entry(2, 5, 0)]).unwrap_err();
        assert_eq!(
            err,
            ManifestError::ZeroSize {
                layer_id: 2,
                expert_id: 5,
            }
        );
    }

    #[test]
    fn manifest_rejects_duplicate_keys() {
        let err =
            ModelManifest::try_from_entries([entry(0, 1, 100), entry(0, 1, 200)]).unwrap_err();
        assert_eq!(
            err,
            ManifestError::DuplicateKey {
                layer_id: 0,
                expert_id: 1,
            }
        );
    }

    #[test]
    fn size_bytes_rejects_unknown_expert() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let err = manifest.size_bytes(ExpertKey::new(0, 9)).unwrap_err();
        assert_eq!(
            err,
            ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 9,
            }
        );
    }

    #[test]
    fn active_set_bytes_matches_hand_calculated_fixture() {
        // Hand fixture:
        // layer 1: expert 0 = 100 B, expert 2 = 250 B, expert 7 = 50 B
        // event active set {0, 2, 7} => 100 + 250 + 50 = 400 B
        let manifest = ModelManifest::try_from_entries([
            entry(1, 0, 100),
            entry(1, 2, 250),
            entry(1, 7, 50),
            entry(0, 0, 9_999), // other layer must not contribute
        ])
        .unwrap();
        let event = sample_event(1, vec![0, 2, 7]);

        assert_eq!(manifest.active_set_bytes(&event).unwrap(), 400);
    }

    #[test]
    fn active_set_bytes_empty_set_is_zero() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let event = sample_event(0, vec![]);
        assert_eq!(manifest.active_set_bytes(&event).unwrap(), 0);
    }

    #[test]
    fn active_set_bytes_rejects_unknown_expert_in_event() {
        let manifest = ModelManifest::try_from_entries([entry(0, 0, 64)]).unwrap();
        let event = sample_event(0, vec![0, 3]);
        let err = manifest.active_set_bytes(&event).unwrap_err();
        assert_eq!(
            err,
            ManifestError::UnknownExpert {
                layer_id: 0,
                expert_id: 3,
            }
        );
    }

    #[test]
    fn active_set_bytes_detects_overflow() {
        let manifest =
            ModelManifest::try_from_entries([entry(0, 0, u64::MAX), entry(0, 1, 1)]).unwrap();
        let event = sample_event(0, vec![0, 1]);
        let err = manifest.active_set_bytes(&event).unwrap_err();
        assert_eq!(err, ManifestError::ActiveSetBytesOverflow { layer_id: 0 });
    }
}
