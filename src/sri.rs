//! Custom SRI (Subresource Integrity) parsing and verification
//!
//! This module provides a lightweight implementation of W3C Subresource Integrity
//! hash parsing and verification for the three standard algorithms: SHA-256, SHA-384, and SHA-512.
//!
//! Benefits over using the `ssri` crate:
//! - Smaller bundle size (~23KB savings, removes miette, xxhash-rust, hex, thiserror)
//! - Full control over implementation
//! - Only implements what we need

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha2::{Digest, Sha256, Sha384, Sha512};

/// SRI hash with algorithm-specific type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SriHash {
    /// SHA-256 hash (32 bytes)
    Sha256([u8; 32]),
    /// SHA-384 hash (48 bytes) - recommended
    Sha384([u8; 48]),
    /// SHA-512 hash (64 bytes)
    Sha512([u8; 64]),
}

/// Error type for SRI operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SriError {
    /// Invalid format (missing algorithm prefix or hash)
    InvalidFormat,
    /// Unsupported algorithm
    UnsupportedAlgorithm,
    /// Base64 decoding failed
    InvalidBase64,
    /// Hash length doesn't match algorithm
    InvalidHashLength,
}

impl SriError {
    /// Get a human-readable description of the error
    pub fn description(&self) -> &'static str {
        match self {
            Self::InvalidFormat => "Invalid SRI format (expected 'algorithm-base64hash')",
            Self::UnsupportedAlgorithm => {
                "Unsupported algorithm (supported: sha256, sha384, sha512)"
            }
            Self::InvalidBase64 => "Invalid base64 encoding in hash",
            Self::InvalidHashLength => "Hash length doesn't match algorithm",
        }
    }
}

impl SriHash {
    /// Parse an SRI string like "sha384-v5A9WpDBhOK..."
    ///
    /// # Format
    /// `algorithm-base64hash` where:
    /// - algorithm: sha256, sha384, or sha512
    /// - base64hash: Standard base64-encoded hash
    ///
    /// # Examples
    /// ```
    /// use linkkivahti::sri::SriHash;
    ///
    /// let sri = SriHash::parse("sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC");
    /// assert!(sri.is_ok());
    /// ```
    pub fn parse(s: &str) -> Result<Self, SriError> {
        // Split on first '-' to separate algorithm from hash
        let (algorithm, base64_hash) = s.split_once('-').ok_or(SriError::InvalidFormat)?;

        // Decode base64
        let decoded = BASE64
            .decode(base64_hash)
            .map_err(|_| SriError::InvalidBase64)?;

        // Match algorithm and verify hash length
        match algorithm {
            "sha256" => {
                if decoded.len() != 32 {
                    return Err(SriError::InvalidHashLength);
                }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&decoded);
                Ok(SriHash::Sha256(hash))
            }
            "sha384" => {
                if decoded.len() != 48 {
                    return Err(SriError::InvalidHashLength);
                }
                let mut hash = [0u8; 48];
                hash.copy_from_slice(&decoded);
                Ok(SriHash::Sha384(hash))
            }
            "sha512" => {
                if decoded.len() != 64 {
                    return Err(SriError::InvalidHashLength);
                }
                let mut hash = [0u8; 64];
                hash.copy_from_slice(&decoded);
                Ok(SriHash::Sha512(hash))
            }
            _ => Err(SriError::UnsupportedAlgorithm),
        }
    }

    /// Verify content against this SRI hash
    ///
    /// Computes the appropriate hash of the content and compares it
    /// against the expected hash.
    ///
    /// Note: Uses regular comparison since we're checking public artifacts
    /// and timing attacks are not a concern in this use case.
    ///
    /// # Examples
    /// ```
    /// use linkkivahti::sri::SriHash;
    ///
    /// let content = b"hello world";
    /// let sri = SriHash::parse("sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=").unwrap();
    /// assert!(sri.verify(content));
    /// ```
    pub fn verify(&self, content: &[u8]) -> bool {
        match self {
            SriHash::Sha256(expected) => {
                let mut hasher = Sha256::new();
                hasher.update(content);
                let computed = hasher.finalize();
                computed.as_slice() == expected
            }
            SriHash::Sha384(expected) => {
                let mut hasher = Sha384::new();
                hasher.update(content);
                let computed = hasher.finalize();
                computed.as_slice() == expected
            }
            SriHash::Sha512(expected) => {
                let mut hasher = Sha512::new();
                hasher.update(content);
                let computed = hasher.finalize();
                computed.as_slice() == expected
            }
        }
    }

    /// Get the algorithm name as a string
    pub fn algorithm(&self) -> &'static str {
        match self {
            SriHash::Sha256(_) => "sha256",
            SriHash::Sha384(_) => "sha384",
            SriHash::Sha512(_) => "sha512",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sha256() {
        let sri = "sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
        let parsed = SriHash::parse(sri).unwrap();
        assert_eq!(parsed.algorithm(), "sha256");
        assert!(matches!(parsed, SriHash::Sha256(_)));
    }

    #[test]
    fn test_parse_sha384() {
        let sri = "sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC";
        let parsed = SriHash::parse(sri).unwrap();
        assert_eq!(parsed.algorithm(), "sha384");
        assert!(matches!(parsed, SriHash::Sha384(_)));
    }

    #[test]
    fn test_parse_sha512() {
        let sri = "sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==";
        let parsed = SriHash::parse(sri).unwrap();
        assert_eq!(parsed.algorithm(), "sha512");
        assert!(matches!(parsed, SriHash::Sha512(_)));
    }

    #[test]
    fn test_parse_invalid_format() {
        // Missing dash separator
        assert_eq!(SriHash::parse("sha384"), Err(SriError::InvalidFormat));
    }

    #[test]
    fn test_parse_unsupported_algorithm() {
        // Valid format but unsupported algorithm
        // Note: "abc123" is valid base64 but wrong length, so we get UnsupportedAlgorithm first
        let result = SriHash::parse("md5-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
        assert_eq!(result, Err(SriError::UnsupportedAlgorithm));
    }

    #[test]
    fn test_parse_invalid_base64() {
        let result = SriHash::parse("sha256-!!!invalid!!!");
        assert_eq!(result, Err(SriError::InvalidBase64));
    }

    #[test]
    fn test_parse_wrong_length() {
        // SHA-256 hash with only 16 bytes instead of 32
        let result = SriHash::parse("sha256-dGVzdA=="); // "test" in base64
        assert_eq!(result, Err(SriError::InvalidHashLength));
    }

    #[test]
    fn test_verify_sha256_success() {
        let content = b"hello world";
        // Pre-computed SHA-256 of "hello world"
        let sri = SriHash::parse("sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=").unwrap();
        assert!(sri.verify(content));
    }

    #[test]
    fn test_verify_sha256_failure() {
        let content = b"hello world";
        // Different hash
        let sri = SriHash::parse("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        assert!(!sri.verify(content));
    }

    #[test]
    fn test_verify_sha384_success() {
        let content = b"hello world";
        // Pre-computed SHA-384 of "hello world"
        let sri = SriHash::parse(
            "sha384-/b2OdaZ/KfcBpOBAOF4uI5hjA+oQI5IRr5B/y7g1eLPkF8txzmRu/QgZ3YwIjeG9",
        )
        .unwrap();
        assert!(sri.verify(content));
    }

    #[test]
    fn test_verify_sha512_success() {
        let content = b"hello world";
        // Pre-computed SHA-512 of "hello world"
        let sri = SriHash::parse("sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==").unwrap();
        assert!(sri.verify(content));
    }

    #[test]
    fn test_verify_different_content() {
        let sri = SriHash::parse(
            "sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC",
        )
        .unwrap();
        assert!(!sri.verify(b"wrong content"));
    }

    #[test]
    fn test_error_descriptions() {
        assert!(!SriError::InvalidFormat.description().is_empty());
        assert!(!SriError::UnsupportedAlgorithm.description().is_empty());
        assert!(!SriError::InvalidBase64.description().is_empty());
        assert!(!SriError::InvalidHashLength.description().is_empty());
    }
}
