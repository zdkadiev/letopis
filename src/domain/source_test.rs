use super::{LineRange, LocalSource, RepoPath, map_line_ranges_to_byte_ranges};

#[test]
fn batch_ranges_preserve_exact_line_bytes() {
    // Given: whole-file, complete and incomplete line ranges over mixed line endings.
    let line_ranges = [
        None,
        Some(LineRange::new(1, 1).unwrap()),
        Some(LineRange::new(2, 2).unwrap()),
        Some(LineRange::new(2, 3).unwrap()),
        Some(LineRange::new(3, 4).unwrap()),
    ];

    // When / Then: exact byte ranges preserve line endings and reject the incomplete line range.
    assert_eq!(
        map_line_ranges_to_byte_ranges(b"a\r\nb\nlast", &line_ranges),
        vec![Some(0..9), Some(0..3), Some(3..5), Some(3..9), None]
    );
}

#[test]
fn batch_ranges_distinguish_empty_files_from_missing_lines() {
    // Given: ranges address the whole empty file and its first line.
    let line_ranges = [None, Some(LineRange::new(1, 1).unwrap())];

    // When / Then: only the whole file has an empty byte range.
    assert_eq!(
        map_line_ranges_to_byte_ranges(b"", &line_ranges),
        vec![Some(0..0), None]
    );
}

#[test]
fn batch_ranges_do_not_add_an_empty_line_after_trailing_lf() {
    // Given: ranges address the first and second lines of a file ending in a line feed.
    let line_ranges = [
        Some(LineRange::new(1, 1).unwrap()),
        Some(LineRange::new(2, 2).unwrap()),
    ];

    // When / Then: the trailing line feed belongs to the first line and adds no second line.
    assert_eq!(
        map_line_ranges_to_byte_ranges(b"a\n", &line_ranges),
        vec![Some(0..2), None]
    );
}

#[test]
fn batch_ranges_preserve_binary_bytes_and_line_range_order_with_duplicates() {
    // Given: unordered and repeated line ranges over binary bytes and mixed line endings.
    let line_ranges = [
        Some(LineRange::new(3, 3).unwrap()),
        Some(LineRange::new(1, 2).unwrap()),
        None,
        Some(LineRange::new(3, 3).unwrap()),
        Some(LineRange::new(1, 1).unwrap()),
        None,
    ];

    // When / Then: byte ranges preserve binary content, line range order and duplicates.
    assert_eq!(
        map_line_ranges_to_byte_ranges(b"\xff\0\r\n\n\x80", &line_ranges),
        vec![
            Some(5..6),
            Some(0..5),
            Some(0..6),
            Some(5..6),
            Some(0..4),
            Some(0..6),
        ]
    );
}

#[test]
fn batch_ranges_handle_maximum_line_numbers_without_overflow() {
    // Given: line ranges containing maximum line numbers and one available line.
    let line_ranges = [
        Some(LineRange::new(1, u64::MAX).unwrap()),
        Some(LineRange::new(u64::MAX, u64::MAX).unwrap()),
        Some(LineRange::new(1, 1).unwrap()),
    ];

    // When / Then: unavailable ranges return None without overflowing.
    assert_eq!(
        map_line_ranges_to_byte_ranges(b"a\n", &line_ranges),
        vec![None, None, Some(0..2)]
    );
}

#[test]
fn batch_ranges_accept_empty_line_range_lists() {
    // Given: nonempty and empty files with no line ranges.
    // When / Then: both selections return no byte ranges.
    assert!(map_line_ranges_to_byte_ranges(b"a\nb\n", &[]).is_empty());
    assert!(map_line_ranges_to_byte_ranges(b"", &[]).is_empty());
}

#[test]
fn batch_ranges_match_independent_selection_for_small_binary_arrays() {
    // Given: all small binary arrays and ordered line ranges including unavailable lines.
    let mut line_ranges = vec![None];
    for start in 1..=7 {
        for end in start..=8 {
            line_ranges.push(Some(LineRange::new(start, end).unwrap()));
        }
    }
    line_ranges.reverse();
    let alphabet = [0, b'\n', b'\r', 0xff];
    for length in 0..=6 {
        for mut encoded in 0..alphabet.len().pow(length) {
            let mut bytes = Vec::new();
            for _ in 0..length {
                bytes.push(alphabet[encoded % alphabet.len()]);
                encoded /= alphabet.len();
            }

            // When / Then: each batch selection matches the independent single-source selection.
            verify_batch_selection(&bytes, &line_ranges);
        }
    }
}

fn verify_batch_selection(bytes: &[u8], line_ranges: &[Option<LineRange>]) {
    let ranges = map_line_ranges_to_byte_ranges(bytes, line_ranges);
    assert_eq!(ranges.len(), line_ranges.len());
    for (range, lines) in ranges.into_iter().zip(line_ranges) {
        let local_source = LocalSource {
            path: RepoPath::parse("source.bin").unwrap(),
            lines: *lines,
        };
        let map_byte_range_to_bytes = |range| &bytes[range];
        let actual = range.map(map_byte_range_to_bytes);
        assert_eq!(
            actual,
            local_source.select_bytes(bytes),
            "{lines:?} over {bytes:?}"
        );
    }
}
