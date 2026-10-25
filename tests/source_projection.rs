mod support;

use letopis::{
    domain::{LineRange, Sha256},
    service::{NodeMetadata, Refresher},
    wire_document_refresher,
};
use support::documents::{DocumentRepo, build_external};

#[test]
fn publish_opaque_remote_without_local_interpretation() {
    let repo = DocumentRepo::new();
    let address = "https://Host/a?x=1#section";
    repo.write("remote.md", build_external(7, address, "RemoteToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();

    refresher.refresh().unwrap();

    let state = repo.read_state();
    let NodeMetadata::Source(source) = &state.get_node(address).metadata else {
        panic!("expected remote source")
    };
    assert_eq!(source.uri.as_str(), address);
    assert_eq!(
        (source.hash, source.lines, source.created_at),
        (None, None, 7)
    );
    assert_eq!(repo.search("RemoteToken"), ["remote.md"]);
}

#[test]
fn merge_equivalent_local_ranges_with_minimum_reference_time() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write(
        "early.md",
        build_external(3, "./source.txt#L01-L01", "EarlyToken"),
    );
    repo.write(
        "late.md",
        build_external(8, "source.txt#L1-L1", "LateToken"),
    );
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();

    refresher.refresh().unwrap();

    let state = repo.read_state();
    assert_eq!(state.nodes.len(), 3);
    let NodeMetadata::Source(source) = &state.get_node("source.txt#L1-L1").metadata else {
        panic!("expected local source")
    };
    assert_eq!(source.created_at, 3);
    assert_eq!(source.lines, Some(LineRange::new(1, 1).unwrap()));
    assert_eq!(
        source.hash,
        Some(
            Sha256::parse("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
                .unwrap()
        )
    );
}
