use super::{LineRange, RepoPath, ValueError};

/// A canonical local source address or an opaque absolute HTTP/HTTPS address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceUri {
    address: String,
    local: Option<LocalSource>,
}

/// Validated local source components with exact, inclusive line selection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalSource {
    path: RepoPath,
    lines: Option<LineRange>,
}

impl SourceUri {
    /// Normalize local components or preserve an HTTP/HTTPS address verbatim.
    ///
    /// Remote fragments are opaque, even when they resemble local line ranges.
    /// This operation performs no I/O or URL equivalence normalization.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidPath`] for invalid local paths or remote
    /// addresses without `://` and a nonempty authority, or with controls,
    /// whitespace or backslashes. Returns [`ValueError::InvalidRange`] for
    /// malformed, zero, reversed or overflowing local line ranges.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if has_remote_scheme(raw) {
            validate_remote_address(raw)?;
            return Ok(Self {
                address: raw.into(),
                local: None,
            });
        }
        let local = LocalSource::parse(raw)?;
        let mut address = local.path.as_str().to_owned();
        if let Some(lines) = local.lines {
            address.push_str(&format!("#L{}-L{}", lines.get_start(), lines.get_end()));
        }
        Ok(Self {
            address,
            local: Some(local),
        })
    }

    /// Return the canonical address, which is also the source identity.
    pub fn as_str(&self) -> &str {
        &self.address
    }

    /// Return validated local components, or `None` for opaque remote addresses.
    pub fn get_local(&self) -> Option<&LocalSource> {
        self.local.as_ref()
    }
}

impl LocalSource {
    /// Return the normalized root-relative source path.
    pub fn get_path(&self) -> &RepoPath {
        &self.path
    }

    /// Return the inclusive line range, or `None` for the whole file.
    pub fn get_lines(&self) -> Option<LineRange> {
        self.lines
    }

    /// Select all bytes or the complete addressed line range without decoding.
    ///
    /// LF terminates a line and remains in its bytes; CR is preserved. An
    /// unterminated final line counts, but a trailing LF adds no empty line.
    /// Whole empty files return `Some`, while empty or incomplete line ranges
    /// return `None`. The returned slice borrows only the supplied bytes.
    pub fn select_bytes<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        let Some(lines) = self.lines else {
            return Some(bytes);
        };
        let mut offset = 0;
        let mut start = 0;
        for (line, number) in bytes.split_inclusive(|byte| *byte == b'\n').zip(1_u64..) {
            if number == lines.get_start() {
                start = offset;
            }
            offset += line.len();
            if number == lines.get_end() {
                return Some(&bytes[start..offset]);
            }
        }
        None
    }

    fn parse(raw: &str) -> Result<Self, ValueError> {
        let (path, fragment) = raw
            .split_once('#')
            .map_or((raw, None), |(path, fragment)| (path, Some(fragment)));
        Ok(Self {
            path: RepoPath::parse(path)?,
            lines: fragment.map(parse_line_range).transpose()?,
        })
    }
}

fn has_remote_scheme(raw: &str) -> bool {
    raw.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

fn validate_remote_address(raw: &str) -> Result<(), ValueError> {
    if raw
        .chars()
        .any(|character| character.is_control() || character.is_whitespace() || character == '\\')
    {
        return Err(ValueError::InvalidPath);
    }
    let (_, suffix) = raw.split_once(':').ok_or(ValueError::InvalidPath)?;
    let remaining = suffix.strip_prefix("//").ok_or(ValueError::InvalidPath)?;
    let authority = remaining.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(ValueError::InvalidPath);
    }
    Ok(())
}

fn parse_line_range(fragment: &str) -> Result<LineRange, ValueError> {
    let (start, end) = fragment
        .strip_prefix('L')
        .and_then(|fragment| fragment.split_once("-L"))
        .ok_or(ValueError::InvalidRange)?;
    if [start, end]
        .iter()
        .any(|number| number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(ValueError::InvalidRange);
    }
    LineRange::new(
        start.parse().map_err(|_| ValueError::InvalidRange)?,
        end.parse().map_err(|_| ValueError::InvalidRange)?,
    )
}
