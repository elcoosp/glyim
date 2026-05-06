use sha2::{Digest, Sha256};
use std::fmt;

/// A content-hash fingerprint: SHA-256 of some data.
/// Used as the cache key for memoized query results.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The all-zero fingerprint (used as a sentinel for "no data").
    pub const ZERO: Self = Self([0u8; 32]);

    /// Compute the fingerprint of arbitrary bytes.
    pub fn of(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }

    /// Compute the fingerprint of a string.
    pub fn of_str(s: &str) -> Self {
        Self::of(s.as_bytes())
    }

    /// Combine two fingerprints into one (order-dependent).
    /// This is used to compute a composite fingerprint from multiple inputs.
    pub fn combine(a: Self, b: Self) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(a.0);
        hasher.update(b.0);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }

    /// Combine a list of fingerprints in order.
    /// Returns `ZERO` for an empty list.
    pub fn combine_all(fps: &[Self]) -> Self {
        if fps.is_empty() {
            return Self::ZERO;
        }
        let mut acc = fps[0];
        for fp in &fps[1..] {
            acc = Self::combine(acc, *fp);
        }
        acc
    }

    /// Return the raw 32-byte hash.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to lowercase hex string (64 chars).
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Parse from a 64-character hex string.
    pub fn from_hex(hex: &str) -> Result<Self, ParseFingerprintError> {
        if hex.len() != 64 {
            return Err(ParseFingerprintError::WrongLength(hex.len()));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| ParseFingerprintError::InvalidHex(i * 2))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FP({}..{})", &self.to_hex()[..8], &self.to_hex()[56..])
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseFingerprintError {
    WrongLength(usize),
    InvalidHex(usize),
}

impl std::fmt::Display for ParseFingerprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLength(n) => write!(f, "expected 64 hex chars, got {n}"),
            Self::InvalidHex(p) => write!(f, "invalid hex char at position {p}"),
        }
    }
}

impl std::error::Error for ParseFingerprintError {}
