use crate::service::{FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult};

mod batches {
    use super::{GitFault, create_oid, create_service};
    use crate::{
        domain::{SourceUri, TreeOid},
        service::{
            DocumentErrorKind, ProvenanceIssue, ProvenanceRequest, SearchFlag, SnapshotState,
            SourceProvenance,
        },
    };
    use std::collections::BTreeSet;

    #[test]
    fn batch_reuses_exact_history_but_reads_each_live_binding() {
        // Given: two bindings of exactly the same historical address.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a#L1-L1").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: both bindings are observed together.
        let results = service.observe_sources_batch(&[request, request]).unwrap();
        // Then: equal clean observations share history but capture live bytes twice.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], results[1]);
        assert!(results[0].flags.is_empty());
        assert!(results[0].issues.is_empty());
        assert_eq!(service.git.verified.borrow().as_slice(), &[tree]);
        assert_eq!(service.git.reads.get(), 1);
        assert_eq!(service.filesystem.reads.get(), 2);
    }

    #[test]
    fn batch_observation_detects_edit_between_live_reads() {
        // Given: the live file changes after the first capture.
        let service = create_service(GitFault::None);
        *service.filesystem.changes.borrow_mut() =
            vec![Some(b"basis\n".to_vec()), Some(b"edited\n".to_vec())];
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: the two bindings are observed in order.
        let results = service.observe_sources_batch(&[request, request]).unwrap();
        // Then: only the second binding is changed; the historical read is shared.
        assert!(results[0].flags.is_empty());
        assert_eq!(
            results[1].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert_eq!(service.git.reads.get(), 1);
        assert_eq!(service.filesystem.reads.get(), 2);
    }

    #[test]
    fn batch_preserves_first_error_and_result_order() {
        // Given: the first binding has a changed source; the next has a missing basis.
        let mut service = create_service(GitFault::None);
        let tree = create_oid('a');
        let missing = create_oid('b');
        service
            .git
            .states
            .borrow_mut()
            .insert(missing.clone(), SnapshotState::Missing);
        *service.filesystem.changes.borrow_mut() = vec![Some(b"edit\n".to_vec()), None];
        let source = SourceUri::parse("a").unwrap();
        let sources = BTreeSet::from([source.clone()]);
        let requests = [
            ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            },
            ProvenanceRequest {
                source_snapshot: Some(&missing),
                sources: &sources,
            },
        ];
        // When: both observations succeed with diagnostic findings.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: owner order and independent missing causes are preserved.
        assert_eq!(
            results[0].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert_eq!(
            results[1].issues,
            BTreeSet::from([
                ProvenanceIssue::SnapshotUnavailable {
                    snapshot: missing.clone()
                },
                ProvenanceIssue::SourceNotFound { source }
            ])
        );
        // Given: a live I/O failure at the first binding.
        service.filesystem.is_failing = true;
        service.git.verified.borrow_mut().clear();
        // When: the batch is attempted again.
        let error = service.observe_sources_batch(&requests).unwrap_err();
        // Then: no later root is demanded.
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert_eq!(service.git.verified.borrow().as_slice(), &[tree]);
    }

    #[test]
    fn batch_does_not_cache_policy_or_io_failure() {
        for fault in [GitFault::Verify, GitFault::Read, GitFault::ReadInvalid] {
            // Given: a failed observation from Git policy, I/O or addressed type checks.
            let service = create_service(fault);
            let tree = create_oid('a');
            let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
            let request = ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            };
            // When: the first batch encounters the configured failure.
            let error = service.observe_sources_batch(&[request]).unwrap_err();
            // Then: the failure propagates as an internal error.
            assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
            // When: the failure is repaired before a new operation.
            service.git.fault.set(GitFault::None);
            let results = service.observe_sources_batch(&[request]).unwrap();
            // Then: successful current observation is not hidden by an error cache.
            assert!(results[0].flags.is_empty());
            assert_eq!(service.git.verified.borrow().len(), 2);
        }
    }

    #[test]
    fn batch_releases_state_between_operations() {
        // Given: two equal bindings within each batch.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: two batches and one singleton run on the same service.
        service.observe_sources_batch(&[request, request]).unwrap();
        service.observe_sources_batch(&[request, request]).unwrap();
        service.observe_sources(request).unwrap();
        // Then: each operation reads history anew and every binding captures live bytes.
        assert_eq!(service.git.verified.borrow().len(), 3);
        assert_eq!(service.git.reads.get(), 3);
        assert_eq!(service.filesystem.reads.get(), 5);
    }

    #[test]
    fn batch_reclassifies_missing_and_invalid_roots() {
        for initial in [SnapshotState::Missing, SnapshotState::Invalid] {
            // Given: a previously unusable tree becomes available during the batch.
            let service = create_service(GitFault::None);
            *service.git.states_sequence.borrow_mut() = vec![initial, SnapshotState::Available];
            let tree = create_oid('a');
            let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
            let request = ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            };
            // When: the tree is demanded twice.
            let results = service.observe_sources_batch(&[request, request]).unwrap();
            // Then: the second binding sees the available tree.
            assert_eq!(
                results[0].flags,
                BTreeSet::from([SearchFlag::SourceChanged])
            );
            assert!(results[1].flags.is_empty());
            assert_eq!(service.git.verified.borrow().len(), 2);
            assert_eq!(service.git.reads.get(), 1);
        }
    }

    #[test]
    fn batch_keys_history_by_exact_tree_and_range() {
        // Given: distinct historical trees and line ranges have different bytes.
        let service = create_service(GitFault::None);
        let first = create_oid('a');
        let second = create_oid('b');
        service
            .git
            .bytes
            .borrow_mut()
            .insert(first.clone(), b"basis\nold\n".to_vec());
        service
            .git
            .bytes
            .borrow_mut()
            .insert(second.clone(), b"other\nbasis\n".to_vec());
        *service.filesystem.changes.borrow_mut() = vec![Some(b"basis\nbasis\n".to_vec()); 4];
        let line1 = BTreeSet::from([SourceUri::parse("a#L1-L1").unwrap()]);
        let line2 = BTreeSet::from([SourceUri::parse("a#L2-L2").unwrap()]);
        let requests = [
            ProvenanceRequest {
                source_snapshot: Some(&first),
                sources: &line1,
            },
            ProvenanceRequest {
                source_snapshot: Some(&second),
                sources: &line1,
            },
            ProvenanceRequest {
                source_snapshot: Some(&first),
                sources: &line2,
            },
            ProvenanceRequest {
                source_snapshot: Some(&second),
                sources: &line2,
            },
        ];
        // When: each exact tree/range is observed.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: the independently chosen equal ranges are first/last, never the middle two.
        assert!(results[0].flags.is_empty());
        assert_eq!(
            results[1].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert_eq!(
            results[2].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert!(results[3].flags.is_empty());
        assert_eq!(service.git.verified.borrow().len(), 2);
        assert_eq!(service.git.reads.get(), 2);
    }

    #[test]
    fn batch_single_request_reuses_history_across_distinct_ranges() {
        // Given: one request binds two distinct ranges of one unchanged file.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        service
            .git
            .bytes
            .borrow_mut()
            .insert(tree.clone(), b"basis\nold\n".to_vec());
        *service.filesystem.changes.borrow_mut() = vec![Some(b"basis\nold\n".to_vec()); 4];
        let sources = BTreeSet::from([
            SourceUri::parse("a#L1-L1").unwrap(),
            SourceUri::parse("a#L2-L2").unwrap(),
        ]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: the single-request batch observes both selectors.
        let results = service.observe_sources_batch(&[request]).unwrap();
        // Then: one historical file read supplies both ranges, with separate live captures.
        assert_eq!(results.len(), 1);
        assert!(results[0].flags.is_empty());
        assert!(results[0].issues.is_empty());
        assert_eq!(service.git.reads.get(), 1);
        assert_eq!(service.filesystem.reads.get(), 2);
        // When: the singleton API independently observes the same selectors.
        let result = service.observe_sources(request).unwrap();
        // Then: its unchanged sequential contract reads both historical addresses afresh.
        assert!(result.flags.is_empty());
        assert!(result.issues.is_empty());
        assert_eq!(service.git.reads.get(), 3);
        assert_eq!(service.filesystem.reads.get(), 4);
    }

    #[test]
    fn batch_reuses_history_across_distinct_ranges() {
        // Given: repeated and distinct selectors share one file with a changed second line.
        let service = create_service(GitFault::None);
        let singleton = create_service(GitFault::None);
        let tree = create_oid('a');
        for recorder in [&service, &singleton] {
            recorder
                .git
                .bytes
                .borrow_mut()
                .insert(tree.clone(), b"basis\nold\n".to_vec());
            *recorder.filesystem.changes.borrow_mut() = vec![Some(b"basis\nnew\n".to_vec()); 4];
        }
        let sources = ["a#L1-L1", "a#L2-L2", "a#L1-L1", "a"]
            .map(|source| BTreeSet::from([SourceUri::parse(source).unwrap()]));
        let requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        // When: batch and sequential singleton calls observe the same stable inputs.
        let results = service.observe_sources_batch(&requests).unwrap();
        let sequential: Vec<_> = requests
            .iter()
            .map(|request| singleton.observe_sources(*request).unwrap())
            .collect();
        // Then: both paths agree with literal flags and live capture remains per binding.
        assert_eq!(results, sequential);
        assert!(results[0].flags.is_empty());
        assert_eq!(
            results[1].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert!(results[2].flags.is_empty());
        assert_eq!(
            results[3].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert_eq!(service.git.reads.get(), 1);
        assert_eq!(service.filesystem.reads.get(), 4);
    }

    #[test]
    fn batch_defers_historical_damage_until_next_read() {
        for fault in [
            GitFault::Read,
            GitFault::ReadInvalid,
            GitFault::ReadMissing,
            GitFault::Verify,
        ] {
            // Given: the first live capture damages history after a successful grouped read.
            let service = create_service(GitFault::None);
            service.filesystem.next_git_fault.set(Some(fault));
            let tree = create_oid('a');
            let sources =
                ["a#L1-L1", "a"].map(|source| BTreeSet::from([SourceUri::parse(source).unwrap()]));
            let requests: Vec<_> = sources
                .iter()
                .map(|sources| ProvenanceRequest {
                    source_snapshot: Some(&tree),
                    sources,
                })
                .collect();
            // When: the second selector consumes the first successful contents.
            let results = service.observe_sources_batch(&requests).unwrap();
            // Then: both remain clean; a new call observes the injected damage.
            assert!(results.iter().all(|result| result.flags.is_empty()));
            assert_eq!(service.git.reads.get(), 1);
            if matches!(fault, GitFault::ReadMissing) {
                let later = service.observe_sources_batch(&requests).unwrap();
                assert_eq!(later[0].flags, BTreeSet::from([SearchFlag::SourceNotFound]));
            } else {
                assert_eq!(
                    service
                        .observe_sources_batch(&requests)
                        .unwrap_err()
                        .get_kind(),
                    DocumentErrorKind::InternalError
                );
            }
        }
    }

    #[test]
    fn batch_preserves_exact_missing_result_after_group_recovers() {
        // Given: the first address is missing, then the same historical file recovers.
        let service = create_service(GitFault::ReadMissing);
        service.filesystem.next_git_fault.set(Some(GitFault::None));
        let tree = create_oid('a');
        service
            .git
            .bytes
            .borrow_mut()
            .insert(tree.clone(), b"basis\nsecond\n".to_vec());
        *service.filesystem.changes.borrow_mut() = vec![Some(b"basis\nsecond\n".to_vec()); 3];
        let sources = ["a#L1-L1", "a#L2-L2", "a#L1-L1"]
            .map(|source| BTreeSet::from([SourceUri::parse(source).unwrap()]));
        let requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        // When: another selector retries the file before the exact missing address repeats.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: group success never overwrites the earlier exact absence.
        assert_eq!(
            results[0].flags,
            BTreeSet::from([SearchFlag::SourceNotFound])
        );
        assert!(results[1].flags.is_empty());
        assert_eq!(
            results[2].flags,
            BTreeSet::from([SearchFlag::SourceNotFound])
        );
        assert_eq!(service.git.reads.get(), 2);
        assert_eq!(service.filesystem.reads.get(), 3);
    }

    #[test]
    fn batch_live_failure_precedes_future_group_failure() {
        for (next_tree, next_path) in [('a', "b"), ('b', "a")] {
            // Given: the first live read fails while the next group's historical read would fail.
            let mut service = create_service(GitFault::None);
            service.filesystem.is_failing = true;
            service.filesystem.next_git_fault.set(Some(GitFault::Read));
            let first = create_oid('a');
            let second = create_oid(next_tree);
            let first_sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
            let second_sources = BTreeSet::from([SourceUri::parse(next_path).unwrap()]);
            let requests = [
                ProvenanceRequest {
                    source_snapshot: Some(&first),
                    sources: &first_sources,
                },
                ProvenanceRequest {
                    source_snapshot: Some(&second),
                    sources: &second_sources,
                },
            ];
            // When: the ordered batch stops at its first live capture failure.
            let error = service.observe_sources_batch(&requests).unwrap_err();
            // Then: no later tree/path is demanded and the filesystem cause is preserved.
            assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
            assert_eq!(service.git.verified.borrow().as_slice(), &[first]);
            assert_eq!(service.git.paths.borrow().as_slice(), &["a".to_owned()]);
            assert_eq!(service.git.reads.get(), 1);
            assert_eq!(service.filesystem.reads.get(), 1);
        }
    }

    #[test]
    fn batch_missing_range_does_not_hide_valid_neighbor() {
        // Given: an incomplete selector and a valid selector address one nonempty file.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources = ["a#L1-L2", "a#L1-L1"]
            .map(|source| BTreeSet::from([SourceUri::parse(source).unwrap()]));
        let requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        // When: the unavailable range triggers preparation before its valid neighbor.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: the invalid range is missing and its neighbor is clean from one read.
        assert_eq!(
            results[0].flags,
            BTreeSet::from([SearchFlag::SourceNotFound])
        );
        assert!(results[1].flags.is_empty());
        assert_eq!(service.git.reads.get(), 1);
    }

    #[test]
    fn batch_does_not_group_distinct_paths_or_read_unbound_and_remote_sources() {
        // Given: two paths share a root alongside an unbound local and a remote source.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources = ["a", "b", "a", "https://example.test/a"]
            .map(|source| BTreeSet::from([SourceUri::parse(source).unwrap()]));
        let requests: Vec<_> = sources
            .iter()
            .enumerate()
            .map(|(index, sources)| ProvenanceRequest {
                source_snapshot: if index == 2 { None } else { Some(&tree) },
                sources,
            })
            .collect();
        // When: all bindings are observed together.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: history reads both local paths only; live captures include the unbound local.
        assert!(results[0].flags.is_empty());
        assert!(results[1].flags.is_empty());
        assert_eq!(
            results[2].flags,
            BTreeSet::from([SearchFlag::SourceChanged])
        );
        assert!(results[3].flags.is_empty());
        assert_eq!(
            service.git.paths.borrow().as_slice(),
            &["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(service.git.reads.get(), 2);
        assert_eq!(service.filesystem.reads.get(), 3);
    }

    #[test]
    fn plan_entry_limit_counts_unique_roots_and_exact_selectors() {
        // Given: one declared root and 4095 distinct local selectors exactly fill the budget.
        let tree = create_oid('a');
        let sources: Vec<_> = (1..=4096)
            .map(|line| BTreeSet::from([SourceUri::parse(&format!("a#L{line}-L{line}")).unwrap()]))
            .collect();
        let requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        // When / Then: exact capacity is accepted and the next selector rejects the whole plan.
        assert!(super::super::HistoricalPlan::build_plan(&requests[..4095]).is_some());
        assert!(super::super::HistoricalPlan::build_plan(&requests).is_none());
        // Given: repeated bindings introduce no new addresses.
        let duplicates = vec![requests[0]; 5000];
        // When / Then: duplicates remain eligible for grouped reuse.
        assert!(super::super::HistoricalPlan::build_plan(&duplicates).is_some());
    }

    #[test]
    fn plan_address_limit_counts_full_source_and_root_strings() {
        // Given: a forty-byte root plus its selector root and path exactly total one MiB.
        let tree = create_oid('a');
        let exact = BTreeSet::from([SourceUri::parse(&"a".repeat(1_048_496)).unwrap()]);
        let overflow = BTreeSet::from([SourceUri::parse(&"a".repeat(1_048_497)).unwrap()]);
        let exact_request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &exact,
        };
        let overflow_request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &overflow,
        };
        // When / Then: a single extra address byte rejects the complete plan.
        assert!(super::super::HistoricalPlan::build_plan(&[exact_request]).is_some());
        assert!(super::super::HistoricalPlan::build_plan(&[overflow_request]).is_none());
    }

    #[test]
    fn plan_counts_roots_without_local_sources_and_repeated_path_strings() {
        // Given: two selectors repeat a long path and a separate root has only remote sources.
        let tree = create_oid('a');
        let remote_tree = create_oid('b');
        let path = "a".repeat(524_220);
        let sources = BTreeSet::from([
            SourceUri::parse(&format!("{path}#L1-L1")).unwrap(),
            SourceUri::parse(&format!("{path}#L2-L2")).unwrap(),
        ]);
        let remote = BTreeSet::from([SourceUri::parse("https://example.test/a").unwrap()]);
        let requests = [
            ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            },
            ProvenanceRequest {
                source_snapshot: Some(&remote_tree),
                sources: &remote,
            },
        ];
        // When / Then: 1,048,572 bytes fit, but the extra declared root exceeds one MiB.
        assert!(super::super::HistoricalPlan::build_plan(&requests[..1]).is_some());
        assert!(super::super::HistoricalPlan::build_plan(&requests).is_none());
    }

    #[test]
    fn plan_address_arithmetic_overflow_rejects_without_allocating() {
        // Given: impossible accumulated address lengths can overflow either addition.
        // When / Then: checked arithmetic and the exact byte limit reject safely.
        assert_eq!(super::super::get_address_byte_count(usize::MAX, 1, 0), None);
        assert_eq!(super::super::get_address_byte_count(0, usize::MAX, 1), None);
        assert_eq!(
            super::super::get_address_byte_count(1_048_575, 1, 0),
            Some(1_048_576)
        );
        assert_eq!(super::super::get_address_byte_count(1_048_576, 0, 1), None);
    }

    #[test]
    fn plan_overflow_uses_whole_call_fallback() {
        // Given: an early shared group precedes a source whose address exceeds the plan budget.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources = ["a#L1-L1".to_owned(), "a".to_owned(), "b".repeat(1_048_577)]
            .map(|source| BTreeSet::from([SourceUri::parse(&source).unwrap()]));
        let requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        // When: one over-budget address disables grouping for the whole call.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: even the first group retains separate legacy reads and clean observations.
        assert!(results.iter().all(|result| result.flags.is_empty()));
        assert_eq!(service.git.reads.get(), 3);
        assert_eq!(service.filesystem.reads.get(), 3);
    }

    #[test]
    fn historical_cache_overflow_recomputes_without_rejecting() {
        // Given: one root plus more distinct addresses than the combined 4096-entry limit.
        let service = create_service(GitFault::None);
        let tree = create_oid('a');
        let sources: Vec<_> = (0..4096)
            .map(|n| BTreeSet::from([SourceUri::parse(&format!("a{n}")).unwrap()]))
            .collect();
        let mut requests: Vec<_> = sources
            .iter()
            .map(|sources| ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources,
            })
            .collect();
        requests.push(requests[4094]);
        requests.push(requests[4095]);
        // When: the last cached and first uncached addresses are demanded again.
        let results = service.observe_sources_batch(&requests).unwrap();
        // Then: the cache stays bounded, overflow recomputes and all results remain valid.
        assert_eq!(results.len(), 4098);
        assert!(results.iter().all(|result| result.flags.is_empty()));
        assert_eq!(service.git.verified.borrow().len(), 1);
        assert_eq!(service.git.reads.get(), 4097);
        assert_eq!(service.filesystem.reads.get(), 4098);
    }

    #[test]
    fn singleton_and_batch_observe_the_same_sequence_of_live_edits() {
        // Given: independent captures with identical literal changes and a disappearance.
        let batch = create_service(GitFault::None);
        let singleton = create_service(GitFault::None);
        let changes = vec![Some(b"basis\n".to_vec()), Some(b"edited\n".to_vec()), None];
        *batch.filesystem.changes.borrow_mut() = changes.clone();
        *singleton.filesystem.changes.borrow_mut() = changes;
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        let request = ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        };
        // When: the same sequence runs through the batch and singleton interfaces.
        let actual = batch.observe_sources_batch(&[request; 3]).unwrap();
        let sequential: Vec<_> = (0..3)
            .map(|_| singleton.observe_sources(request).unwrap())
            .collect();
        // Then: literal outcomes and both interfaces agree, without caching live bytes.
        assert!(actual[0].flags.is_empty());
        assert_eq!(actual[1].flags, BTreeSet::from([SearchFlag::SourceChanged]));
        assert_eq!(
            actual[2].flags,
            BTreeSet::from([SearchFlag::SourceNotFound])
        );
        assert_eq!(actual, sequential);
        assert_eq!(batch.git.reads.get(), 1);
        assert_eq!(singleton.git.reads.get(), 3);
    }

    #[test]
    fn root_cache_overflow_also_recomputes_without_rejecting() {
        // Given: more available roots than the aggregate cache limit and no source bytes.
        let service = create_service(GitFault::None);
        let roots: Vec<_> = (0..4097)
            .map(|n| TreeOid::parse(&format!("{n:040x}")).unwrap())
            .collect();
        let sources = BTreeSet::new();
        let mut requests: Vec<_> = roots
            .iter()
            .map(|tree| ProvenanceRequest {
                source_snapshot: Some(tree),
                sources: &sources,
            })
            .collect();
        requests.push(requests[4095]);
        requests.push(requests[4096]);
        // When: the last cached and first uncached roots are repeated.
        let observations = service.observe_sources_batch(&requests).unwrap();
        // Then: root overflow recomputes while clean observations remain complete.
        assert_eq!(observations.len(), 4099);
        assert!(observations.iter().all(|result| result.flags.is_empty()));
        assert_eq!(service.git.verified.borrow().len(), 4098);
        assert_eq!(service.git.reads.get(), 0);
        assert_eq!(service.filesystem.reads.get(), 0);
    }
}

use super::ProvenanceService;
use crate::{
    domain::{BlobOid, RepoPath, SourceUri, TreeOid},
    service::{
        CapturedFile, DocumentError, DocumentErrorKind, FileStat, GitSnapshots, ProvenanceRequest,
        SnapshotEntry, SnapshotError, SnapshotErrorKind, SnapshotFile, SnapshotRefsUpdater,
        SnapshotState, SnapshotVerifier, SourceProvenance, SourceRevisionComparison,
        SourceRevisionRequest,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
    ops::{Deref, DerefMut},
    rc::Rc,
};

#[test]
fn full_audit_still_pins_available_roots() {
    // Given: one available author root.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    // When: exhaustive verification is explicitly requested.
    service
        .snapshot_sync_by_oids(&BTreeSet::from([tree.clone()]))
        .unwrap();
    // Then: strong pin is used instead of lightweight retention.
    assert_eq!(*service.git.strong_pins.borrow(), [tree]);
    assert!(service.git.pins.borrow().is_empty());
}

#[test]
fn leave_remote_unknown_without_file_reads_or_retention() {
    // Given: a remote source bound to an available historical tree.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let source = SourceUri::parse("https://host/path#L1-L2").unwrap();
    let sources = BTreeSet::from([source.clone()]);
    // When: the source is observed and compared with its declared revision.
    let observation = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap();
    let comparison = service
        .compare_revision(SourceRevisionRequest {
            source_snapshot: Some(&tree),
            source: &source,
            revision: &tree,
        })
        .unwrap();
    // Then: remote bytes remain unknown without live reads or retention.
    assert!(observation.flags.is_empty());
    assert!(observation.issues.is_empty());
    assert_eq!(comparison, SourceRevisionComparison::Unfetched);
    assert_eq!(service.filesystem.reads.get(), 0);
    assert!(service.git.pins.borrow().is_empty());
}

#[test]
fn verify_each_root_once_and_never_pin_during_read() {
    // Given: two local sources share one available historical root.
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let source = SourceUri::parse("a#L1-L1").unwrap();
    let sources = BTreeSet::from([source.clone(), SourceUri::parse("b").unwrap()]);
    // When: the sources are observed, then one exact revision is compared.
    service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap();
    // Then: the shared root was verified once for the observation.
    assert_eq!(
        service.git.verified.borrow().as_slice(),
        std::slice::from_ref(&tree)
    );
    // Given: the recorder starts empty for an independent revision comparison.
    service.git.verified.borrow_mut().clear();
    // When: the source is compared with its exact declared revision.
    let comparison = service
        .compare_revision(SourceRevisionRequest {
            source_snapshot: Some(&tree),
            source: &source,
            revision: &tree,
        })
        .unwrap();
    // Then: each operation verifies the root once and neither retains it.
    assert_eq!(comparison, SourceRevisionComparison::Equal);
    assert_eq!(*service.git.verified.borrow(), [tree]);
    assert!(service.git.pins.borrow().is_empty());
}

#[test]
fn retain_each_unique_available_root_and_skip_missing_or_invalid() {
    // Given: duplicate available roots and missing or invalid roots.
    let service = create_service(GitFault::None);
    let available = create_oid('a');
    let missing = create_oid('b');
    let invalid = create_oid('c');
    service
        .git
        .states
        .borrow_mut()
        .insert(missing.clone(), SnapshotState::Missing);
    service
        .git
        .states
        .borrow_mut()
        .insert(invalid.clone(), SnapshotState::Invalid);
    let roots = BTreeSet::from([
        available.clone(),
        available.clone(),
        missing.clone(),
        invalid.clone(),
    ]);
    // When: the unique roots are retained.
    service.snapshot_refs_update_by_oids(&roots).unwrap();
    // Then: all unique roots are verified and only the available root is retained.
    assert_eq!(
        *service.git.verified.borrow(),
        [available.clone(), missing, invalid]
    );
    assert_eq!(*service.git.pins.borrow(), [available]);
    assert_eq!(service.filesystem.reads.get(), 0);
}

#[test]
fn propagate_retention_and_verification_failures_with_private_causes() {
    for fault in [GitFault::Verify, GitFault::Pin] {
        // Given: Git verification or retention fails with a private cause.
        let service = create_service(fault);
        let roots = BTreeSet::from([create_oid('a')]);
        // When: the author roots are retained.
        let error = service.snapshot_refs_update_by_oids(&roots).unwrap_err();
        // Then: the public error preserves its private cause without exposing it.
        assert_private_document_error(&error);
    }
}

#[test]
fn propagate_corruption_and_invalid_blob_instead_of_reporting_missing() {
    for fault in [GitFault::Verify, GitFault::Read, GitFault::ReadInvalid] {
        // Given: Git root verification or addressed blob reads fail.
        let service = create_service(fault);
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        // When: the local source is observed.
        let error = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            })
            .unwrap_err();
        // Then: the failure remains an internal error with a private cause.
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert!(error.source().is_some());
        assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
    }
}

#[test]
fn propagate_unsafe_live_capture_without_exposing_private_cause() {
    // Given: live capture fails with a private cause.
    let mut service = create_service(GitFault::None);
    service.filesystem.is_failing = true;
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
    // When: the local source is observed.
    let error = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap_err();
    // Then: the public error preserves its private cause without exposing it.
    assert_private_document_error(&error);
}

#[test]
fn preserve_requested_revision_corruption_and_invalid_blob_classifications() {
    for (fault, expected) in [
        (GitFault::Verify, SnapshotErrorKind::InternalError),
        (GitFault::Read, SnapshotErrorKind::InternalError),
        (GitFault::ReadInvalid, SnapshotErrorKind::InvalidRequest),
    ] {
        // Given: Git verification or blob reads fail with a known classification.
        let service = create_service(fault);
        let tree = create_oid('a');
        let source = SourceUri::parse("a").unwrap();
        // When: the source is compared with its declared revision.
        let error = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            })
            .unwrap_err();
        // Then: the error classification survives without exposing the private cause.
        assert_eq!(error.get_kind(), expected);
        assert!(!error.get_message().contains("PRIVATE_PROVENANCE_CAUSE"));
        if expected == SnapshotErrorKind::InternalError {
            assert!(error.source().is_some());
        }
    }
}

#[derive(Clone, Copy)]
enum GitFault {
    None,
    Verify,
    Read,
    ReadInvalid,
    ReadMissing,
    Pin,
}

struct RecordedGit {
    states: RefCell<BTreeMap<TreeOid, SnapshotState>>,
    verified: RefCell<Vec<TreeOid>>,
    pins: RefCell<Vec<TreeOid>>,
    strong_pins: RefCell<Vec<TreeOid>>,
    fault: Cell<GitFault>,
    reads: Cell<usize>,
    paths: RefCell<Vec<String>>,
    bytes: RefCell<BTreeMap<TreeOid, Vec<u8>>>,
    states_sequence: RefCell<Vec<SnapshotState>>,
}

struct RecordedFiles {
    reads: Cell<usize>,
    is_failing: bool,
    changes: RefCell<Vec<Option<Vec<u8>>>>,
    git: Rc<RecordedGit>,
    next_git_fault: Cell<Option<GitFault>>,
}

// Share the recorder between the boxed collaborator and independent test probes.

struct RecordedProvenance {
    service: ProvenanceService<RecordedFiles>,
    git: Rc<RecordedGit>,
}

impl SnapshotVerifier for Rc<RecordedGit> {
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.verified.borrow_mut().push(tree.clone());
        if matches!(self.fault.get(), GitFault::Verify) {
            return Err(create_git_failure());
        }
        if !self.states_sequence.borrow().is_empty() {
            return Ok(self.states_sequence.borrow_mut().remove(0));
        }
        Ok(self
            .states
            .borrow()
            .get(tree)
            .copied()
            .unwrap_or(SnapshotState::Available))
    }
}

impl GitSnapshots for Rc<RecordedGit> {
    fn ref_update_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.pins.borrow_mut().push(tree.clone());
        if matches!(self.fault.get(), GitFault::Pin) {
            return Err(create_git_failure());
        }
        Ok(())
    }

    fn tree_create(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        panic!("provenance must not create trees")
    }

    fn blob_write(&self, _: &[u8]) -> Result<BlobOid, SnapshotError> {
        panic!("provenance must not write blobs")
    }

    fn tree_assemble(&self, _: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        panic!("provenance must not create trees")
    }

    fn blob_get_by_tree_oid_and_path(
        &self,
        tree: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        self.reads.set(self.reads.get() + 1);
        self.paths.borrow_mut().push(path.as_str().to_owned());
        match self.fault.get() {
            GitFault::Read => Err(create_git_failure()),
            GitFault::ReadMissing => Err(SnapshotError::new(
                SnapshotErrorKind::NotFound,
                "missing blob",
            )),
            GitFault::ReadInvalid => Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "not a blob",
            )),
            _ => Ok(self
                .bytes
                .borrow()
                .get(tree)
                .cloned()
                .unwrap_or_else(|| b"basis\n".to_vec())),
        }
    }

    fn tree_sync_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        self.strong_pins.borrow_mut().push(tree.clone());
        if matches!(self.fault.get(), GitFault::Pin) {
            return Err(create_git_failure());
        }
        Ok(())
    }
}

impl FileReader for RecordedFiles {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        panic!("provenance must not stat sources")
    }

    fn file_get_bytes_by_path(&self, _: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        panic!("unexpected file_get_bytes_by_path")
    }

    fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        self.reads.set(self.reads.get() + 1);
        if let Some(fault) = self.next_git_fault.take() {
            self.git.fault.set(fault);
        }
        if self.is_failing {
            return Err(FileError::new_with_cause(
                FileErrorKind::Io,
                io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
            ));
        }
        let bytes = if self.changes.borrow().is_empty() {
            Some(b"basis\n".to_vec())
        } else {
            self.changes.borrow_mut().remove(0)
        };
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        Ok(Some(CapturedFile {
            bytes,
            stat: FileStat {
                size: 6,
                mtime_ns: 1,
                inode: 1,
                device: 1,
            },
            checked_at_ns: 1,
        }))
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unexpected file_get_identity_by_path")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unexpected file_is_exist")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_identity_by_path")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_distinct_by_paths")
    }

    fn directory_get_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::os::fd::OwnedFd>> {
        panic!("unexpected directory_get_by_path")
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        panic!("unexpected lock_get_owner_by_path")
    }
}

impl FileLocker for RecordedFiles {
    type Lock = ();

    fn lock_acquire_by_path(&self, _path: &std::path::Path) -> FileResult<Self::Lock> {
        panic!("caller owns the index lock")
    }

    fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
        panic!("unexpected lock probe")
    }
}

impl Deref for RecordedProvenance {
    type Target = ProvenanceService<RecordedFiles>;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

impl DerefMut for RecordedProvenance {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.service
    }
}

fn create_service(fault: GitFault) -> RecordedProvenance {
    let git = Rc::new(RecordedGit {
        states: RefCell::new(BTreeMap::new()),
        verified: RefCell::new(Vec::new()),
        pins: RefCell::new(Vec::new()),
        strong_pins: RefCell::new(Vec::new()),
        fault: Cell::new(fault),
        reads: Cell::new(0),
        paths: RefCell::new(Vec::new()),
        bytes: RefCell::new(BTreeMap::new()),
        states_sequence: RefCell::new(Vec::new()),
    });
    let service = ProvenanceService::new(
        Box::new(Rc::clone(&git)),
        RecordedFiles {
            reads: Cell::new(0),
            is_failing: false,
            changes: RefCell::new(Vec::new()),
            git: Rc::clone(&git),
            next_git_fault: Cell::new(None),
        },
    );
    RecordedProvenance { service, git }
}

fn create_oid(digit: char) -> TreeOid {
    TreeOid::parse(&digit.to_string().repeat(40)).unwrap()
}

fn create_git_failure() -> SnapshotError {
    SnapshotError::new_with_cause(
        "cannot access provenance tree",
        io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
    )
}

fn assert_private_document_error(error: &DocumentError) {
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
    assert!(!format!("{error:?}").contains("PRIVATE_PROVENANCE_CAUSE"));
    let mut cause = error.source();
    while let Some(current) = cause {
        if current.to_string().contains("PRIVATE_PROVENANCE_CAUSE") {
            return;
        }
        cause = current.source();
    }
    panic!("private cause was discarded");
}
