use super::{LineRange, LocalSource, RepoPath};

#[test]
fn batch_ranges_preserve_exact_line_bytes() {
    // Given
    let selectors = [
        None,
        Some(LineRange::new(1, 1).unwrap()),
        Some(LineRange::new(2, 2).unwrap()),
        Some(LineRange::new(2, 3).unwrap()),
        Some(LineRange::new(3, 4).unwrap()),
    ];

    // When / Then
    assert_eq!(
        LocalSource::get_byte_ranges(b"a\r\nb\nlast", &selectors),
        vec![Some(0..9), Some(0..3), Some(3..5), Some(3..9), None]
    );
}

#[test]
fn batch_ranges_distinguish_empty_files_from_missing_lines() {
    // Given
    let selectors = [None, Some(LineRange::new(1, 1).unwrap())];

    // When / Then
    assert_eq!(
        LocalSource::get_byte_ranges(b"", &selectors),
        vec![Some(0..0), None]
    );
}

#[test]
fn batch_ranges_do_not_add_an_empty_line_after_trailing_lf() {
    // Given
    let selectors = [
        Some(LineRange::new(1, 1).unwrap()),
        Some(LineRange::new(2, 2).unwrap()),
    ];

    // When / Then
    assert_eq!(
        LocalSource::get_byte_ranges(b"a\n", &selectors),
        vec![Some(0..2), None]
    );
}

#[test]
fn batch_ranges_preserve_binary_bytes_and_selector_order_with_duplicates() {
    // Given
    let selectors = [
        Some(LineRange::new(3, 3).unwrap()),
        Some(LineRange::new(1, 2).unwrap()),
        None,
        Some(LineRange::new(3, 3).unwrap()),
        Some(LineRange::new(1, 1).unwrap()),
        None,
    ];

    // When / Then
    assert_eq!(
        LocalSource::get_byte_ranges(b"\xff\0\r\n\n\x80", &selectors),
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
    // Given
    let selectors = [
        Some(LineRange::new(1, u64::MAX).unwrap()),
        Some(LineRange::new(u64::MAX, u64::MAX).unwrap()),
        Some(LineRange::new(1, 1).unwrap()),
    ];

    // When / Then
    assert_eq!(
        LocalSource::get_byte_ranges(b"a\n", &selectors),
        vec![None, None, Some(0..2)]
    );
}

#[test]
fn batch_ranges_accept_empty_selector_lists() {
    // When / Then
    assert!(LocalSource::get_byte_ranges(b"a\nb\n", &[]).is_empty());
    assert!(LocalSource::get_byte_ranges(b"", &[]).is_empty());
}

#[test]
fn batch_ranges_match_independent_selection_for_small_binary_arrays() {
    // Given
    let mut selectors = vec![None];
    for start in 1..=7 {
        for end in start..=8 {
            selectors.push(Some(LineRange::new(start, end).unwrap()));
        }
    }
    selectors.reverse();
    let alphabet = [0, b'\n', b'\r', 0xff];
    for length in 0..=6 {
        for mut encoded in 0..alphabet.len().pow(length) {
            let mut bytes = Vec::new();
            for _ in 0..length {
                bytes.push(alphabet[encoded % alphabet.len()]);
                encoded /= alphabet.len();
            }

            // When / Then
            verify_batch_selection(&bytes, &selectors);
        }
    }
}

fn verify_batch_selection(bytes: &[u8], selectors: &[Option<LineRange>]) {
    let ranges = LocalSource::get_byte_ranges(bytes, selectors);
    assert_eq!(ranges.len(), selectors.len());
    for (range, lines) in ranges.into_iter().zip(selectors) {
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
