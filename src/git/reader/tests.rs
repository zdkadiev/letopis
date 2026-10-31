use super::ObjectReader;
use crate::git::reader::VerifiedObject;
use crate::service::SnapshotErrorKind;
use std::env;

mod allocation;

#[test]
fn run_owned_reader() {
    // Given: only the integration harness supplies an isolated repository.
    let Some(root) = env::var_os("LETOPIS_READER_ROOT") else {
        return;
    };
    let repo = gix::open(root).unwrap();
    let oid = env::var("LETOPIS_READER_OID").unwrap();
    let kind = env::var("LETOPIS_READER_KIND").unwrap_or_else(|_| "tree".into());
    let outcome = env::var("LETOPIS_READER_OUTCOME").unwrap_or_else(|_| "ok".into());
    let mut reader = ObjectReader::new(&repo, oid.len()).unwrap();
    // When: the raw library reader verifies one object, measuring only that call's allocations.
    allocation::start();
    let result = reader.get_object(
        &oid,
        &kind,
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    );
    let allocations = allocation::stop();
    // Then: identity/type/missing failures are classified and oversized capacity is released.
    match outcome.as_str() {
        "ok" => match (kind.as_str(), result.unwrap()) {
            ("blob", VerifiedObject::Blob) => {}
            ("tree", VerifiedObject::Tree(bytes)) => assert!(bytes.is_empty()),
            unexpected => panic!("unexpected verified object: {unexpected:?}"),
        },
        "missing" => assert_eq!(result.unwrap_err().get_kind(), SnapshotErrorKind::NotFound),
        "wrong_type" => assert_eq!(
            result.unwrap_err().get_kind(),
            SnapshotErrorKind::InvalidRequest
        ),
        "corrupt" => {
            let error = result.unwrap_err();
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            assert!(!format!("{error:?}").contains("CANARY"));
        }
        _ => panic!("unknown outcome"),
    }
    assert!(reader.buffer.capacity() <= 4 * 1024 * 1024);
    println!(
        "READER_RECEIPT {}",
        serde_json::json!({
            "oid": oid, "outcome": outcome, "retained_bytes": reader.buffer.capacity(),
            "largest_allocation_bytes": allocations.0, "total_allocation_bytes": allocations.1,
            "rss_peak_bytes": allocation::get_peak_rss(),
        })
    );
}

#[test]
fn raw_reads_ignore_configured_allocation_caps_replacements_and_payload_caches() {
    // Given: valid raw objects, a one-byte gix cap and a Git replacement for the requested identity.
    let fixture =
        crate::process_test_support::TempRepo::new(crate::process_test_support::FORMATS[0]);
    let original = fixture.get_oid(
        &["hash-object", "-w", "--stdin"],
        Some(b"raw original bytes"),
    );
    let replacement = fixture.get_oid(
        &["hash-object", "-w", "--stdin"],
        Some(b"replacement bytes"),
    );
    fixture.run_git_successfully(&["config", "gitoxide.objects.allocLimit", "1"], None);
    fixture.run_git_successfully(&["replace", &original, &replacement], None);
    let repo = gix::open(fixture.get_root()).unwrap();
    let mut reader = ObjectReader::new(&repo, original.len()).unwrap();
    // When: the reader accesses raw local identity, without gix's optional cap or replacement.
    let result = reader.get_object(
        &original,
        "blob",
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    );
    // Then: canonical bytes pass, and a later corrupt replacement cannot be hidden by a payload cache.
    assert!(result.is_ok(), "{result:?}");
    fixture.replace_loose_object(&original, &replacement);
    let result = reader.get_object(
        &original,
        "blob",
        SnapshotErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest,
    );
    assert_eq!(
        result.unwrap_err().get_kind(),
        SnapshotErrorKind::InternalError
    );
}
