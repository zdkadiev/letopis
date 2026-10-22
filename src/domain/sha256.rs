use super::ValueError;
use sha2::Digest;

/// SHA-256 of exact input bytes, without normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sha256([u8; 32]);
impl Sha256 {
    /// Decode exactly 64 lowercase hexadecimal characters.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidHash`] for a malformed hash.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if raw.len() != 64
            || !raw
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(ValueError::InvalidHash);
        }
        let mut bytes = [0; 32];
        for (target, pair) in bytes.iter_mut().zip(raw.as_bytes().chunks_exact(2)) {
            let decode = |c: u8| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
            *target = (decode(pair[0]) << 4) | decode(pair[1]);
        }
        Ok(Self(bytes))
    }

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
