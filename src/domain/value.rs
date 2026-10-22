use sha2::Digest;
use std::fmt;

/// A violation of an identity or graph value's local invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueError {
    InvalidPath,
    InvalidAnchor,
    InvalidRange,
    InvalidOid,
    InvalidType,
    InvalidTarget,
    SelfLoop,
}
impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "invalid root-relative path",
            Self::InvalidAnchor => "invalid anchor",
            Self::InvalidRange => "invalid range",
            Self::InvalidOid => "invalid tree OID",
            Self::InvalidType => "invalid edge type",
            Self::InvalidTarget => "invalid edge target",
            Self::SelfLoop => "self-loop",
        })
    }
}
impl std::error::Error for ValueError {}

/// A half-open byte range. Its caller verifies bounds against the input file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    start: usize,
    end: usize,
}
impl ByteSpan {
    /// Reject reversed ranges; empty ranges are allowed.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidRange`] when `start > end`.
    pub fn new(start: usize, end: usize) -> Result<Self, ValueError> {
        if start > end {
            return Err(ValueError::InvalidRange);
        }
        Ok(Self { start, end })
    }
    /// First included byte offset.
    pub fn get_start(&self) -> usize {
        self.start
    }
    /// First excluded byte offset.
    pub fn get_end(&self) -> usize {
        self.end
    }
}

/// An inclusive, one-based source line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    start: u64,
    end: u64,
}
impl LineRange {
    /// Reject line zero and reversed ranges.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidRange`] when `start` is zero or greater
    /// than `end`.
    pub fn new(start: u64, end: u64) -> Result<Self, ValueError> {
        if start == 0 || start > end {
            return Err(ValueError::InvalidRange);
        }
        Ok(Self { start, end })
    }
    /// First included line.
    pub fn get_start(&self) -> u64 {
        self.start
    }
    /// Last included line.
    pub fn get_end(&self) -> u64 {
        self.end
    }
}

pub(crate) const SHA1_OID_HEX_LEN: usize = 40;
pub(crate) const SHA256_OID_HEX_LEN: usize = 64;

/// A full SHA-1 or SHA-256 Git tree OID; existence/type require a Git adapter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TreeOid(String);
impl TreeOid {
    /// Accept exactly 40 or 64 lowercase hexadecimal characters.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidOid`] for any other length or for a
    /// non-lowercase-hexadecimal character. Repository existence is not checked.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if ![SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN].contains(&raw.len())
            || !raw
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(ValueError::InvalidOid);
        }
        Ok(Self(raw.into()))
    }
    /// The unmodified full OID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// SHA-256 of exact input bytes, without normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sha256([u8; 32]);
impl Sha256 {
    /// Hash the supplied bytes exactly.
    pub fn digest(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }
    /// Encode lowercase hex without adding a codec dependency.
    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for b in self.0 {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char);
        }
        out
    }
}
