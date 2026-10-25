use super::ValueError;

/// A lexically normalized path relative to the Git root; no filesystem access.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoPath(String);
impl RepoPath {
    /// Resolve dot components and reject absolute paths, C0 controls and root escapes.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidPath`] for absolute paths, C0 control characters,
    /// `#`, root escapes, or a path that normalizes to an empty string.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if raw.starts_with('/') || raw.chars().any(|c| c <= '\u{1f}' || c == '#') {
            return Err(ValueError::InvalidPath);
        }
        let mut parts = Vec::new();
        for part in raw.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop().ok_or(ValueError::InvalidPath)?;
                }
                _ => parts.push(part),
            }
        }
        if parts.is_empty() {
            return Err(ValueError::InvalidPath);
        }
        Ok(Self(parts.join("/")))
    }
    /// Normalized path preserving case and Unicode.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An explicit X-4 heading anchor, excluding source line-range syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor(String);
impl Anchor {
    /// Validate ASCII anchor grammar and its reserved line-range form.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidAnchor`] unless the anchor starts with an
    /// ASCII letter and contains only ASCII letters, digits, `_` or `-`.
    /// The source line-range form `L<digits>-L<digits>` is also rejected.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        let reserved = raw
            .strip_prefix('L')
            .and_then(|r| r.split_once("-L"))
            .is_some_and(|(a, b)| contains_only_ascii_digits(a) && contains_only_ascii_digits(b));
        if !raw.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            || !raw
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || reserved
        {
            return Err(ValueError::InvalidAnchor);
        }
        Ok(Self(raw.into()))
    }
    /// Validated anchor without the hash separator.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// Identity of a whole Markdown file or an anchored knowledge section.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KnowledgeId(String);
impl KnowledgeId {
    /// Parse a root-relative Markdown path and optional explicit anchor.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidPath`] for an invalid path or a path without
    /// a `.md` or `.markdown` suffix, and [`ValueError::InvalidAnchor`] for an
    /// invalid explicit anchor.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        let (path, anchor) = match raw.split_once('#') {
            Some((p, a)) => (p, Some(Anchor::parse(a)?)),
            None => (raw, None),
        };
        Self::new(RepoPath::parse(path)?, anchor)
    }
    /// Build a knowledge identity from validated components.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidPath`] when the path does not end in `.md`
    /// or `.markdown`.
    pub fn new(path: RepoPath, anchor: Option<Anchor>) -> Result<Self, ValueError> {
        if !path.as_str().ends_with(".md") && !path.as_str().ends_with(".markdown") {
            return Err(ValueError::InvalidPath);
        }
        let mut result = path.0;
        if let Some(a) = anchor {
            result.push('#');
            result.push_str(a.as_str());
        }
        Ok(Self(result))
    }
    /// Canonical identity, including its optional anchor.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn contains_only_ascii_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}
