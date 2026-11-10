use super::{LineRange, RepoPath, ValueError};
use std::collections::BTreeMap;
use std::ops::Range;

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
    /// Parses a source address, normalizing local components and preserving remote addresses.
    ///
    /// Remote fragments are kept as written, even when they resemble local line ranges.
    /// This operation performs no I/O or URL equivalence normalization.
    ///
    /// # Arguments
    ///
    /// * `raw` — the source address, either a local path with optional line range or an
    ///   HTTP/HTTPS address.
    ///
    /// # Returns
    /// The parsed source address.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidPath`] for invalid local paths or remote
    /// addresses without `://` and a nonempty authority, or with controls,
    /// whitespace or backslashes. Local paths whose normalization exposes a
    /// case-insensitive `http:` or `https:` prefix are also invalid: their
    /// serialized identity would otherwise be reclassified as remote.
    /// Returns [`ValueError::InvalidRange`] for
    /// malformed, zero, reversed or overflowing local line ranges.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if is_remote_scheme(raw) {
            validate_remote_address(raw)?;
            return Ok(Self {
                address: raw.into(),
                local: None,
            });
        }
        let local = LocalSource::parse(raw)?;
        if is_remote_scheme(local.path.as_str()) {
            return Err(ValueError::InvalidPath);
        }
        let mut address = local.path.as_str().to_owned();
        if let Some(lines) = local.lines {
            address.push_str(&format!("#L{}-L{}", lines.get_start(), lines.get_end()));
        }
        Ok(Self {
            address,
            local: Some(local),
        })
    }

    /// Returns the canonical address, which is also the source identity.
    pub fn as_str(&self) -> &str {
        &self.address
    }

    /// Returns validated local components, or `None` for remote addresses.
    pub fn get_local(&self) -> Option<&LocalSource> {
        self.local.as_ref()
    }
}

impl LocalSource {
    /// Returns the normalized root-relative source path.
    pub fn get_path(&self) -> &RepoPath {
        &self.path
    }

    /// Returns the inclusive line range, or `None` for the whole file.
    pub fn get_lines(&self) -> Option<LineRange> {
        self.lines
    }

    /// Returns all bytes or the complete addressed line range without decoding.
    ///
    /// LF terminates a line and remains in its bytes; CR is preserved. An
    /// unterminated final line counts, but a trailing LF adds no empty line.
    /// Whole empty files return `Some`, while empty or incomplete line ranges
    /// return `None`. The returned slice borrows only the supplied bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` — the complete file contents from which the selected slice is taken.
    ///
    /// # Returns
    /// The selected bytes, or `None` for an empty or incomplete line range.
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

    /// Gets exact byte ranges for selectors using at most one scan of the bytes.
    ///
    /// A `None` selector addresses the whole file, including an empty file.
    /// Line selection follows [`Self::select_bytes`]: LF remains in each line,
    /// CR and binary bytes are preserved, and a trailing LF adds no empty line.
    /// Only requested line boundaries are retained; no per-line index is built.
    /// This operation performs no I/O.
    ///
    /// # Arguments
    ///
    /// * `bytes` - the complete file contents used to locate the selected ranges.
    /// * `selectors` - inclusive line ranges, or `None` for the whole file, in result order.
    ///
    /// # Returns
    /// One byte range per selector in the original order, including duplicates,
    /// or `None` when the complete selected line range is unavailable.
    pub fn get_byte_ranges(
        bytes: &[u8],
        selectors: &[Option<LineRange>],
    ) -> Vec<Option<Range<usize>>> {
        let line_byte_ranges = get_line_byte_ranges(bytes, selectors);
        let mut byte_ranges = Vec::with_capacity(selectors.len());
        for selector in selectors {
            let Some(lines) = selector else {
                byte_ranges.push(Some(0..bytes.len()));
                continue;
            };
            let start = line_byte_ranges.get(&lines.get_start());
            let end = line_byte_ranges.get(&lines.get_end());
            byte_ranges.push(match (start, end) {
                (Some(start), Some(end)) => Some(start.start..end.end),
                _ => None,
            });
        }
        byte_ranges
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

fn get_line_byte_ranges(
    bytes: &[u8],
    selectors: &[Option<LineRange>],
) -> BTreeMap<u64, Range<usize>> {
    let mut line_numbers = Vec::new();
    for lines in selectors.iter().flatten() {
        line_numbers.push(lines.get_start());
        line_numbers.push(lines.get_end());
    }
    line_numbers.sort_unstable();
    line_numbers.dedup();
    let mut requested_line_numbers = line_numbers.into_iter().peekable();
    let mut line_byte_ranges = BTreeMap::new();
    if requested_line_numbers.peek().is_none() {
        return line_byte_ranges;
    }
    let mut offset = 0;
    for (line, number) in bytes.split_inclusive(|byte| *byte == b'\n').zip(1_u64..) {
        let start = offset;
        offset += line.len();
        if requested_line_numbers.peek() == Some(&number) {
            line_byte_ranges.insert(number, start..offset);
            requested_line_numbers.next();
        }
        if requested_line_numbers.peek().is_none() {
            break;
        }
    }
    line_byte_ranges
}

fn is_remote_scheme(raw: &str) -> bool {
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

#[cfg(test)]
#[path = "source_test.rs"]
mod tests;
