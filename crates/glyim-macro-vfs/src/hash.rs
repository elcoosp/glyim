use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }
    pub fn of_str(s: &str) -> Self {
        Self::of(s.as_bytes())
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    #[must_use]
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{:02x}", b)).collect()
    }
    pub fn from_hex(hex: &str) -> Result<Self, ParseHexError> {
        if hex.len() != 64 {
            return Err(ParseHexError(HexErrorKind::WrongLength(hex.len())));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| ParseHexError(HexErrorKind::InvalidHex(i * 2)))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", self.to_hex())
    }
}
impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_hex().fmt(f)
    }
}

impl FromStr for ContentHash {
    type Err = ParseHexError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseHexError(HexErrorKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum HexErrorKind {
    WrongLength(usize),
    InvalidHex(usize),
}

impl fmt::Display for ParseHexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            HexErrorKind::WrongLength(n) => write!(f, "expected 64 hex chars, got {}", n),
            HexErrorKind::InvalidHex(p) => write!(f, "invalid hex char at position {}", p),
        }
    }
}
impl std::error::Error for ParseHexError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_hash() {
        assert_eq!(
            ContentHash::of_str("").to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
    #[test]
    fn round_trip() {
        let h = ContentHash::of_str("glyim");
        assert_eq!(ContentHash::from_hex(&h.to_hex()).unwrap(), h);
    }
    #[test]
    fn reject_short() {
        assert!(ContentHash::from_hex("abcd").is_err());
    }
    #[test]
    fn parse_trait() {
        let h = ContentHash::of_str("test");
        let s: ContentHash = h.to_hex().parse().unwrap();
        assert_eq!(s, h);
    }
}
