#[path = "support/git.rs"]
mod fixture;

#[test]
#[should_panic(expected = "supervised test did not run exactly the requested case")]
fn rejects_a_successful_child_with_an_unmatched_exact_test_filter() {
    // Given an unmatched exact test filter.
    // When / Then
    fixture::supervise_test("this_test_name_is_intentionally_absent", &[]);
}
