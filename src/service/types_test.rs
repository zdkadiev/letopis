mod model_tests {
    use super::super::{EmbeddingConfig, QueryEmbeddingRequest};
    use std::num::NonZeroUsize;

    #[test]
    fn query_request_debug_omits_text_and_provider_parameters() {
        // Given: each private field has an independent recognizable sentinel.
        let config = EmbeddingConfig::parse(
            br#"{"provider":"ollama","model":"PRIVATE_MODEL","parameters":{"base_url":"http://PRIVATE_ENDPOINT"},"max_retries":0}"#,
        )
        .unwrap();
        let request = QueryEmbeddingRequest {
            text: "PRIVATE_QUERY",
            config: &config,
            dimensions: NonZeroUsize::new(2),
        };
        // When
        let diagnostic = format!("{request:?}");
        // Then
        for private in ["PRIVATE_QUERY", "PRIVATE_MODEL", "PRIVATE_ENDPOINT"] {
            assert!(!diagnostic.contains(private));
        }
        assert!(diagnostic.starts_with("QueryEmbeddingRequest {"));
        assert!(diagnostic.contains("dimensions: Some(2)"));
    }
}

mod command_tests {
    use super::super::{CommandError, CommandErrorKind, ElementAddress};
    use std::error::Error;

    #[test]
    fn preserve_local_anchor_and_range_normalization() {
        // Given
        for (raw, expected) in [
            ("./x.md", "x.md"),
            ("a/../x.md#A", "x.md#A"),
            ("./source#L01-L02", "source#L1-L2"),
            ("résumé.MD", "résumé.MD"),
            ("x.md#Lfoo-Lbar", "x.md#Lfoo-Lbar"),
        ] {
            // When / Then
            assert_eq!(ElementAddress::parse(raw).unwrap().as_str(), expected);
        }
        for raw in [
            "",
            "/x",
            "../x",
            "x#L0-L1",
            "x#L2-L1",
            "x.md#",
            "x#anchor",
            "x#L1-L18446744073709551616",
            "x\u{0}",
        ] {
            // When / Then
            assert_eq!(
                ElementAddress::parse(raw).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn preserve_opaque_http_addresses() {
        // Given
        for raw in [
            "https://Host/a?q=1#L0-L2",
            "HTTP://Host/a#Anchor",
            "https://Host",
            "https://Host/a.md",
            "HtTp://Host/a.markdown#A",
        ] {
            // When / Then
            assert_eq!(ElementAddress::parse(raw).unwrap().as_str(), raw);
        }
    }

    #[test]
    fn round_trip_http_prefix_knowledge_addresses() {
        // Given
        for (raw, expected) in [
            ("http:notes.md", "http:notes.md"),
            ("./http:notes.md#A", "http:notes.md#A"),
            ("dir/../HTTP:notes.markdown#A", "HTTP:notes.markdown#A"),
            ("https:/Host/a.md", "https:/Host/a.md"),
        ] {
            // When
            let parsed = ElementAddress::parse(raw).unwrap();
            // Then
            assert_eq!(parsed.as_str(), expected);
            assert_eq!(ElementAddress::parse(expected).unwrap(), parsed);
        }
    }

    #[test]
    fn reject_malformed_absolute_urls_and_source_http_fallbacks() {
        // Given
        for raw in [
            "http:notes",
            "https:/Host/a",
            "HTTP:///a",
            "HTTP:///a.md",
            "https://?q=a.md",
            "http://#A.md",
            "https://",
            "./https:notes",
            "a/../HTTP:notes",
            "https://Host/a b",
        ] {
            // When / Then
            assert!(ElementAddress::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn retain_error_cause_without_exposing_it_in_diagnostics() {
        // Given / When
        let error = CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "safe failure",
            std::io::Error::other("SECRET_SENTINEL"),
        );
        // Then
        assert_eq!(error.get_message(), "safe failure");
        assert!(
            error
                .source()
                .unwrap()
                .to_string()
                .contains("SECRET_SENTINEL")
        );
        assert_eq!(error.to_string(), "safe failure");
        let debug = format!("{error:?}");
        assert!(debug.contains(error.get_message()));
        assert!(!debug.contains("SECRET_SENTINEL"));
    }
}

mod search_filters_tests {
    use super::super::{RecheckFilter, SearchFilters};
    use crate::service::SearchFlag;
    use std::collections::BTreeSet;

    #[test]
    fn default_filters_impose_no_restrictions() {
        // Given
        let filters = SearchFilters::default();
        let observations = [
            BTreeSet::new(),
            BTreeSet::from([SearchFlag::Stale]),
            BTreeSet::from([SearchFlag::SourceChanged]),
            BTreeSet::from([SearchFlag::SourceNotFound]),
        ];
        // When / Then
        assert_eq!(filters.needs_recheck, RecheckFilter::Any);
        assert!(filters.sources.is_empty());
        assert!(filters.source_revisions.is_empty());
        for flags in observations {
            assert!(filters.is_matched_by_flags(&flags), "{flags:?}");
        }
    }

    #[test]
    fn required_recheck_matches_each_current_flag_and_rejects_no_flags() {
        // Given
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::Required,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), false),
            (BTreeSet::from([SearchFlag::Stale]), true),
            (BTreeSet::from([SearchFlag::SourceChanged]), true),
            (BTreeSet::from([SearchFlag::SourceNotFound]), true),
        ];
        // When / Then
        for (flags, expected) in cases {
            assert_eq!(filters.is_matched_by_flags(&flags), expected, "{flags:?}");
        }
    }

    #[test]
    fn not_required_recheck_matches_no_flags_and_rejects_each_current_flag() {
        // Given
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::NotRequired,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), true),
            (BTreeSet::from([SearchFlag::Stale]), false),
            (BTreeSet::from([SearchFlag::SourceChanged]), false),
            (BTreeSet::from([SearchFlag::SourceNotFound]), false),
        ];
        // When / Then
        for (flags, expected) in cases {
            assert_eq!(filters.is_matched_by_flags(&flags), expected, "{flags:?}");
        }
    }
}

mod embedding_config_tests {
    use super::super::EmbeddingConfig;
    use crate::utils::JsonValue;

    #[test]
    fn bounds_raw_token_traversal_before_canonicalization() {
        // Given
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        // When
        let result = serde_json::from_str::<JsonValue<true>>(&nested);
        // Then
        assert!(result.is_err());
    }
    #[test]
    fn rejects_trailing_json_after_configuration() {
        // Given
        let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0} null"#;
        // When
        let result = EmbeddingConfig::parse(bytes);
        // Then
        assert!(result.is_err());
    }
}

mod discovery_configuration_tests {
    use super::super::{Configuration, DiscoveryConfig};

    #[test]
    fn discovery_defaults_preserve_embedding_identity() {
        // Given: fixed provider settings and three discovery representations.
        for discovery in [
            "",
            ",\"discovery\":{}",
            ",\"discovery\":{\"respect_gitignore\":false}",
        ] {
            let bytes = format!(
                r#"{{"embedding_model":{{"enabled":true,"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://127.0.0.1:9/api"}},"max_retries":2}}{discovery}}}"#
            );
            // When: the complete configuration is parsed.
            let configuration = Configuration::parse(bytes.as_bytes()).unwrap();
            // Then: only the independently specified embedding projection controls identity.
            assert_eq!(
                configuration
                    .get_embedding_config()
                    .unwrap()
                    .get_identity()
                    .to_hex(),
                "c713040f35c46c21c7389aa69b25910ad51fb1ce6ceb26b378513ea5dec8748b"
            );
            assert_eq!(
                configuration
                    .get_discovery_config()
                    .is_gitignore_respected(),
                !discovery.contains("false")
            );
        }
        assert!(DiscoveryConfig::default().is_gitignore_respected());
    }

    #[test]
    fn discovery_projection_ignores_invalid_embedding_settings() {
        // Given: valid discovery objects alongside absent or invalid embedding settings.
        for bytes in [br#"{"discovery":{"respect_gitignore":false}}"#.as_slice(),
            br#"{"embedding_model":null,"discovery":{"respect_gitignore":false}}"#,
            br#"{"embedding_model":{"provider":"unsupported"},"discovery":{"respect_gitignore":false}}"#] {
            // When: discovery and complete validation are requested separately.
            let discovery = DiscoveryConfig::parse(bytes).unwrap();
            let complete = Configuration::parse(bytes);
            // Then: FTS can select its corpus without accepting invalid embedding settings.
            assert!(!discovery.is_gitignore_respected());
            assert!(complete.is_err());
        }
    }

    #[test]
    fn discovery_rejects_ambiguous_or_invalid_input() {
        // Given: ambiguous roots, wrong types and unknown fields.
        for bytes in [
            "null",
            "[]",
            "{invalid}",
            r#"{"future":true}"#,
            r#"{"discovery":null}"#,
            r#"{"discovery":[]}"#,
            r#"{"discovery":{"respect_gitignore":"false"}}"#,
            r#"{"discovery":{"respect_gitignore":0}}"#,
            r#"{"discovery":{"future":false}}"#,
            r#"{"discovery":{},"discovery":{}}"#,
            r#"{"discovery":{"respect_gitignore":true,"respect_gitignore":false}}"#,
        ] {
            // When: the independent discovery projection is parsed.
            let result = DiscoveryConfig::parse(bytes.as_bytes());
            // Then: invalid input never silently chooses a wider corpus.
            assert!(result.is_err(), "{bytes}");
        }
    }
}

mod configuration_value_tests {
    use crate::service::{CapturedFile, FileIdentity, FileReader, FileResult, FileStat};

    use super::super::{Configuration, GENERATED_CONFIG};
    use crate::{
        domain::Sha256,
        service::configuration::ConfigurationService,
        service::{
            ConfigReader, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider,
            EmbeddingResult,
        },
    };
    use serde::Deserialize;
    use std::num::NonZeroUsize;

    const CORPUS: &str = include_str!("../../tests/fixtures/config/corpus.json");
    const ENABLED: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;
    const DISABLED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;

    #[derive(Deserialize)]
    struct Corpus {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        name: String,
        raw: String,
        parse_valid: bool,
        validate_valid: bool,
    }
    struct Provider;
    struct Files {
        config: Vec<u8>,
    }

    #[test]
    fn parses_generated_defaults_and_preserves_retry_setting() {
        // Given / When: the exact generated default configuration is parsed.
        let configuration = Configuration::parse(GENERATED_CONFIG).unwrap();

        // Then: embeddings remain unconfigured and the retry default is preserved.
        assert!(!configuration.is_enabled());
        assert!(configuration.get_embedding_config().is_none());
        assert_eq!(configuration.get_max_retries(), 0);
    }

    #[test]
    fn identity_excludes_enabled_and_the_root_wrapper() {
        // Given: the same configured projection under both execution states.
        let enabled = Configuration::parse(ENABLED).unwrap();
        let disabled = Configuration::parse(DISABLED).unwrap();
        let expected = Sha256::digest(
            br#"{"max_retries":2,"model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"provider":"ollama"}"#,
        );

        // When / Then: only the four configured settings participate.
        assert_eq!(
            enabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert_eq!(
            disabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert!(enabled.is_enabled());
        assert!(!disabled.is_enabled());
    }

    #[test]
    fn reports_legacy_flat_document_for_migration() {
        // Given
        let raw = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0}"#;

        // When
        let error = Configuration::parse(raw).unwrap_err();

        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::MigrationRequired);
    }

    #[test]
    fn shared_corpus_agrees_with_local_parse_and_validation() {
        // Given: the same corpus the schema checker consumes.
        let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();

        for case in corpus.cases {
            // Given: a corpus document exposed through the configuration service.
            let files = Files {
                config: case.raw.as_bytes().to_vec(),
            };
            let service = ConfigurationService {
                file_reader: &files,
                provider: &Provider,
            };

            // When: structural parsing inspects the document.
            let parsed = Configuration::parse(case.raw.as_bytes());

            // Then: parsing agrees with the independently recorded corpus result.
            assert_eq!(parsed.is_ok(), case.parse_valid, "{}", case.name);
            if parsed.is_ok() {
                // When: the service also validates retained provider settings.
                let validated = service.get_config();

                // Then: validation agrees with the recorded corpus result.
                assert_eq!(validated.is_ok(), case.validate_valid, "{}", case.name);
            }
        }
    }

    impl FileReader for Files {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }

        fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            panic!("unexpected file_get_by_path")
        }

        fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
            assert_eq!(path, std::path::Path::new(".letopis/config.json"));
            Ok(Some(self.config.clone()))
        }

        fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
            panic!("unexpected file_get_stat_by_path")
        }

        fn file_get_identity_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<FileIdentity>> {
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
            Ok(None)
        }

        fn lock_get_owner_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::num::NonZeroU32>> {
            Ok(None)
        }
    }

    impl EmbeddingProvider for Provider {
        fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
            let url = config
                .get_parameters()
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                Ok(())
            } else {
                Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
            }
        }

        fn vectors_compute_by_texts(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            Err(EmbeddingError::new(EmbeddingErrorKind::Input))
        }
    }
}
