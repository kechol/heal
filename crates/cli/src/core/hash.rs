//! Stable FNV-1a 64-bit hashing.
//!
//! HEAL persists hashes to disk in three places — `Finding.id`,
//! `FindingsRecord.config_hash`, and the plugin fingerprint manifest — and
//! all three must stay valid across processes and Rust toolchain
//! upgrades. `std::hash::DefaultHasher` is explicitly unstable across
//! releases (see CLAUDE.md §Hashing), so we hand-roll FNV-1a with the
//! published constants. The hot path inside `observer::duplication`
//! keeps its own copy on purpose so the per-token loop stays inlined;
//! everything else routes through here to avoid drift.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// FNV-1a 64-bit over a flat byte slice.
#[must_use]
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64-bit over multiple chunks separated by a `0xff` byte. The
/// separator prevents `("ab", "c")` and `("a", "bc")` from colliding —
/// every persistent caller in HEAL needs that property.
#[must_use]
pub fn fnv1a_64_chunked(chunks: &[&[u8]]) -> u64 {
    let mut h = FNV_OFFSET;
    for chunk in chunks {
        for b in *chunk {
            h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
        }
        h = (h ^ 0xff).wrapping_mul(FNV_PRIME);
    }
    h
}

/// Format a 64-bit digest as zero-padded 16 hex chars. Used wherever
/// the digest leaks into a user-visible identifier (`Finding.id`,
/// `FindingsRecord.config_hash`, plugin fingerprint manifest).
#[must_use]
pub fn fnv1a_hex(h: u64) -> String {
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_and_chunked_diverge_for_same_bytes() {
        // Without the separator, the chunked variant equals the flat
        // one. The separator is what makes (`"ab","c"`) ≠ (`"a","bc"`)
        // — verify both halves of that property.
        assert_ne!(fnv1a_64(b"abc"), fnv1a_64_chunked(&[b"abc"]));
        assert_ne!(
            fnv1a_64_chunked(&[b"ab", b"c"]),
            fnv1a_64_chunked(&[b"a", b"bc"]),
        );
    }

    #[test]
    fn flat_is_stable_across_calls() {
        assert_eq!(fnv1a_64(b"heal"), fnv1a_64(b"heal"));
        assert_ne!(fnv1a_64(b"heal"), fnv1a_64(b"HEAL"));
    }

    #[test]
    fn hex_is_zero_padded_to_16_chars() {
        assert_eq!(fnv1a_hex(0).len(), 16);
        assert_eq!(fnv1a_hex(0), "0000000000000000");
        assert_eq!(fnv1a_hex(0xff).len(), 16);
    }
}
