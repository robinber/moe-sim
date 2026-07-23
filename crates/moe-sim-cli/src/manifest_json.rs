//! Strict v1 JSON codec for model manifests.
//!
//! The wire document is `{"experts": [{"layer_id", "expert_id",
//! "size_bytes"}, ...]}`. Every field is required and unknown fields are
//! rejected. An empty `experts` array is a valid empty manifest. Encoding is
//! deterministic: compact JSON with entries in ascending `(layer_id,
//! expert_id)` order, terminated by one LF.

use moe_sim_core::{ExpertKey, ExpertSizeEntry, ManifestError, ModelManifest};
use serde::{Deserialize, Serialize};

/// Wire shape of the manifest document (strict v1).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestWire {
    experts: Vec<ExpertEntryWire>,
}

/// Wire shape of one expert size entry (strict v1).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ExpertEntryWire {
    layer_id: u32,
    expert_id: u32,
    size_bytes: u64,
}

/// Errors returned when parsing a strict v1 manifest document.
///
/// Domain violations keep the underlying [`ManifestError`] as `source`.
#[derive(Debug, thiserror::Error)]
pub enum ManifestParseError {
    /// The document was not a valid v1 manifest object.
    #[error("invalid model manifest JSON: {source}")]
    Json {
        /// Underlying JSON error; it reports line and column in the document.
        source: serde_json::Error,
    },
    /// A wire-valid document violated a manifest invariant.
    #[error("invalid model manifest entries: {source}")]
    Manifest {
        /// Underlying domain error from manifest construction.
        source: ManifestError,
    },
}

/// Error returned when encoding a manifest as strict v1 JSON.
#[derive(Debug, thiserror::Error)]
#[error("failed to encode model manifest as JSON: {source}")]
pub struct ManifestEncodeError {
    source: serde_json::Error,
}

/// Parses a strict v1 manifest document into a [`ModelManifest`].
///
/// # Errors
///
/// Returns [`ManifestParseError::Json`] when the document is not a valid v1
/// manifest object (missing fields, unknown fields, wrong types, trailing
/// content), and [`ManifestParseError::Manifest`] when wire-valid entries are
/// rejected by [`ModelManifest::try_from_entries`] (zero sizes, duplicate
/// keys).
pub fn parse_manifest_json(input: &str) -> Result<ModelManifest, ManifestParseError> {
    let wire: ManifestWire =
        serde_json::from_str(input).map_err(|source| ManifestParseError::Json { source })?;
    let entries = wire.experts.into_iter().map(|entry| ExpertSizeEntry {
        key: ExpertKey::new(entry.layer_id, entry.expert_id),
        size_bytes: entry.size_bytes,
    });
    ModelManifest::try_from_entries(entries)
        .map_err(|source| ManifestParseError::Manifest { source })
}

/// Encodes a manifest as strict v1 JSON: compact, entries in ascending
/// `(layer_id, expert_id)` order, terminated by one LF.
///
/// Equal manifests produce byte-identical output.
///
/// # Errors
///
/// Returns [`ManifestEncodeError`] when JSON serialization fails; with the
/// v1 wire shape this is not expected to occur.
pub fn encode_manifest_json(manifest: &ModelManifest) -> Result<String, ManifestEncodeError> {
    let wire = ManifestWire {
        experts: manifest
            .entries()
            .map(|entry| ExpertEntryWire {
                layer_id: entry.key.layer_id(),
                expert_id: entry.key.expert_id(),
                size_bytes: entry.size_bytes,
            })
            .collect(),
    };
    let mut out = serde_json::to_string(&wire).map_err(|source| ManifestEncodeError { source })?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests exercise the fallible codec directly; direct unwraps keep failure diagnostics close to the fixture"
)]
mod tests {
    use super::*;

    fn manifest_from(entries: &[(u32, u32, u64)]) -> ModelManifest {
        ModelManifest::try_from_entries(entries.iter().map(|&(layer_id, expert_id, size_bytes)| {
            ExpertSizeEntry {
                key: ExpertKey::new(layer_id, expert_id),
                size_bytes,
            }
        }))
        .unwrap()
    }

    #[test]
    fn round_trip_encodes_entries_in_sorted_key_order() {
        // Insertion order is deliberately unsorted.
        let manifest = manifest_from(&[(1, 0, 300), (0, 7, 100), (0, 2, 200)]);

        let encoded = encode_manifest_json(&manifest).unwrap();
        assert_eq!(
            encoded,
            "{\"experts\":[\
             {\"layer_id\":0,\"expert_id\":2,\"size_bytes\":200},\
             {\"layer_id\":0,\"expert_id\":7,\"size_bytes\":100},\
             {\"layer_id\":1,\"expert_id\":0,\"size_bytes\":300}]}\n"
        );

        assert_eq!(parse_manifest_json(&encoded).unwrap(), manifest);
    }

    #[test]
    fn empty_manifest_round_trips() {
        let manifest = manifest_from(&[]);
        let encoded = encode_manifest_json(&manifest).unwrap();
        assert_eq!(encoded, "{\"experts\":[]}\n");

        let parsed = parse_manifest_json(&encoded).unwrap();
        assert!(parsed.is_empty());
        assert_eq!(parsed, manifest);
    }

    #[test]
    fn round_trip_preserves_u64_max_size() {
        let manifest = manifest_from(&[(u32::MAX, u32::MAX, u64::MAX)]);
        let encoded = encode_manifest_json(&manifest).unwrap();
        let parsed = parse_manifest_json(&encoded).unwrap();
        assert_eq!(
            parsed
                .size_bytes(ExpertKey::new(u32::MAX, u32::MAX))
                .unwrap(),
            u64::MAX
        );
        assert_eq!(parsed, manifest);
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let err = parse_manifest_json("{\"experts\":[],\"version\":1}").unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn unknown_entry_field_is_rejected() {
        let input = "{\"experts\":[\
             {\"layer_id\":0,\"expert_id\":0,\"size_bytes\":1,\"name\":\"e0\"}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn missing_layer_id_entry_field_is_rejected() {
        let input = "{\"experts\":[{\"expert_id\":0,\"size_bytes\":1}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("missing field `layer_id`"));
    }

    #[test]
    fn missing_expert_id_entry_field_is_rejected() {
        let input = "{\"experts\":[{\"layer_id\":0,\"size_bytes\":1}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("missing field `expert_id`"));
    }

    #[test]
    fn missing_size_bytes_entry_field_is_rejected() {
        let input = "{\"experts\":[{\"layer_id\":0,\"expert_id\":0}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("missing field `size_bytes`"));
    }

    #[test]
    fn negative_size_bytes_is_rejected() {
        let input = "{\"experts\":[{\"layer_id\":0,\"expert_id\":0,\"size_bytes\":-1}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn fractional_size_bytes_is_rejected() {
        let input = "{\"experts\":[{\"layer_id\":0,\"expert_id\":0,\"size_bytes\":1.5}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn size_bytes_above_u64_max_is_rejected() {
        // 18446744073709551616 == u64::MAX + 1.
        let input = "{\"experts\":[\
             {\"layer_id\":0,\"expert_id\":0,\"size_bytes\":18446744073709551616}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn layer_id_above_u32_max_is_rejected() {
        // 4294967296 == u32::MAX + 1.
        let input = "{\"experts\":[\
             {\"layer_id\":4294967296,\"expert_id\":0,\"size_bytes\":1}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn duplicate_object_member_is_rejected() {
        let input = "{\"experts\":[\
             {\"layer_id\":0,\"expert_id\":0,\"size_bytes\":1,\"size_bytes\":2}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("duplicate field `size_bytes`"));
    }

    #[test]
    fn missing_experts_field_is_rejected() {
        let err = parse_manifest_json("{}").unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
        assert!(err.to_string().contains("missing field"));
    }

    #[test]
    fn non_object_document_is_rejected() {
        let err = parse_manifest_json("[]").unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn trailing_content_after_document_is_rejected() {
        let err = parse_manifest_json("{\"experts\":[]} trailing").unwrap_err();
        assert!(matches!(err, ManifestParseError::Json { .. }));
    }

    #[test]
    fn zero_size_preserves_domain_error() {
        let input = "{\"experts\":[{\"layer_id\":2,\"expert_id\":5,\"size_bytes\":0}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(
            err,
            ManifestParseError::Manifest {
                source: ManifestError::ZeroSize {
                    layer_id: 2,
                    expert_id: 5,
                },
            }
        ));
        // The domain error stays reachable through std::error::Error::source.
        let source = std::error::Error::source(&err);
        assert_eq!(
            source.and_then(|s| s.downcast_ref::<ManifestError>()),
            Some(&ManifestError::ZeroSize {
                layer_id: 2,
                expert_id: 5,
            })
        );
    }

    #[test]
    fn duplicate_key_preserves_domain_error() {
        let input = "{\"experts\":[\
             {\"layer_id\":0,\"expert_id\":1,\"size_bytes\":100},\
             {\"layer_id\":0,\"expert_id\":1,\"size_bytes\":200}]}";
        let err = parse_manifest_json(input).unwrap_err();
        assert!(matches!(
            err,
            ManifestParseError::Manifest {
                source: ManifestError::DuplicateKey {
                    layer_id: 0,
                    expert_id: 1,
                },
            }
        ));
    }
}
