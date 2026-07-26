//! Provenance facts recorded in every `moe-sim` success report.
//!
//! A report names the package version that produced it, the input-contract
//! version it parsed against, and the exact bytes of every input document.
//! Digests are lowercase hexadecimal SHA-256 over the input bytes as read, so
//! a reader can reproduce any of them outside this tool with
//! `shasum -a 256 <path>`.
//!
//! The package version is not a full build identity: it pins neither the
//! source revision nor the dependency lock. Recording those is a separate
//! decision, not something this module silently implies.
//!
//! Seed provenance: the one stochastic command is
//! `trace generate --pattern random`, whose generation report records its
//! `seed:` beside the tool version, contract version, and output digests,
//! so a synthetic input is reproducible from its report alone. Replay
//! commands stay seedless because no shipped policy is stochastic.

use sha2::{Digest, Sha256};

/// Input-contract version this build parses.
///
/// Reports name it so a reader knows which document contract was assumed,
/// independently of the tool version that applied it.
pub const INPUT_FORMAT_VERSION: &str = "v1";

const HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";

/// Package version of the binary that produced a report.
///
/// This is `CARGO_PKG_VERSION` alone. It identifies the released version, not
/// the exact build: two binaries from different commits of the same version
/// report the same string.
#[must_use]
pub fn tool_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Lowercase hexadecimal SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical NIST SHA-256 vectors: an implementation that matches these
    // matches every external tool a reader might verify a report with.
    const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ABC_DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn sha256_matches_the_empty_input_vector() {
        assert_eq!(sha256_hex(b""), EMPTY_DIGEST);
    }

    #[test]
    fn sha256_matches_the_abc_vector() {
        assert_eq!(sha256_hex(b"abc"), ABC_DIGEST);
    }

    #[test]
    fn sha256_is_lowercase_hex_of_fixed_width() {
        let digest = sha256_hex(b"moe-sim");
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "digest must be lowercase hex: {digest}"
        );
    }

    #[test]
    fn sha256_is_deterministic_across_calls() {
        assert_eq!(sha256_hex(b"same bytes"), sha256_hex(b"same bytes"));
    }

    #[test]
    fn sha256_separates_different_inputs() {
        assert_ne!(sha256_hex(b"trace"), sha256_hex(b"manifest"));
    }

    #[test]
    fn tool_version_is_the_crate_version() {
        assert_eq!(tool_version(), env!("CARGO_PKG_VERSION"));
    }
}
