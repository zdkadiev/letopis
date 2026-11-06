use super::VectorStorage;
use crate::service::TransactionError;
use crate::{
    domain::Sha256,
    service::{StorageError, VectorDatabase, VectorOutcome},
};
use rusqlite::Connection;

#[test]
fn malformed_identity_skips_vector_scopes_without_decoding_it() {
    // Given: persisted identity text is malformed rather than a valid SHA-256.
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("create table meta(id integer primary key,config_identity text); insert into meta values(1,'malformed')").unwrap();
    let storage = VectorStorage::new(connection);
    let expected = Sha256::digest(b"current");
    // When: both scope kinds compare the persisted identity before entering callbacks.
    let read = storage
        .with_read_tx(&expected, || -> Result<(), StorageError> {
            panic!("mismatched read must skip callback")
        })
        .unwrap();
    let write = storage
        .with_write_tx(&expected, || -> Result<(), StorageError> {
            panic!("mismatched write must skip callback")
        })
        .unwrap();
    // Then: malformed text remains a configuration mismatch, not a decoding failure.
    assert_eq!(read, VectorOutcome::ConfigChanged);
    assert_eq!(write, VectorOutcome::ConfigChanged);
}

#[test]
fn failed_reset_cleanup_reports_failure_and_disables_the_connection() {
    use super::super::utils::set_rollback_denial;
    // Given: SQLite refuses rollback of a reset with an obsolete observation.
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("create table meta(id integer primary key,config_identity text)")
        .unwrap();
    let storage = VectorStorage::new(connection);
    let observed = Sha256::digest(b"obsolete observation");
    let desired = Sha256::digest(b"desired configuration");
    set_rollback_denial(&storage.connection, true);
    // When: reset skips its mutations but SQLite cannot close its transaction.
    let result = storage.reset_vectors(Some(&observed), &desired);
    set_rollback_denial(&storage.connection, false);
    storage.connection.execute_batch("rollback").unwrap();
    // Then: cleanup failure is an error and external repair cannot reenable this owner.
    assert!(result.is_err());
    let retry = storage.with_read_tx(&desired, || -> Result<(), StorageError> {
        panic!("disabled storage must not execute callbacks")
    });
    assert!(matches!(retry, Err(TransactionError::Storage(_))));
}

#[test]
fn reset_is_rejected_inside_an_active_vector_scope() {
    // Given: a vector transaction for the current configuration.
    let connection = Connection::open_in_memory().unwrap();
    let identity = Sha256::digest(b"current");
    connection
        .execute_batch("create table meta(id integer primary key,config_identity text)")
        .unwrap();
    connection
        .execute("insert into meta values(1,?1)", [identity.to_hex()])
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: a service tries to reset identity from within its write scope.
    let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
        assert!(
            storage
                .reset_vectors(Some(&identity), &Sha256::digest(b"other"))
                .is_err()
        );
        Ok(())
    });
    // Then: the reset is rejected and the enclosing scope cannot commit.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
}

#[test]
fn reads_embedding_failure_from_vector_database_inside_scope() {
    // Given: a primary vector database with a recorded failed attempt.
    let connection = Connection::open_in_memory().unwrap();
    let identity = Sha256::digest(b"current");
    let hash = Sha256::digest(b"text");
    connection.execute_batch("create table meta(id integer primary key,config_identity text); create table embed_failures(search_hash text primary key,attempts integer,error text)").unwrap();
    connection
        .execute("insert into meta values(1,?1)", [identity.to_hex()])
        .unwrap();
    connection
        .execute(
            "insert into embed_failures values(?1,2,'safe failure')",
            [hash.to_hex()],
        )
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: a service reads the failure in a matching scope.
    let outcome = storage
        .with_read_tx(&identity, || {
            storage.embed_failures_get_by_search_hash(&hash)
        })
        .unwrap();
    // Then: the stored hash, attempt count and safe error are returned.
    let VectorOutcome::Current(Some(failure)) = outcome else {
        panic!("failure missing")
    };
    assert_eq!(failure.search_hash, hash);
    assert_eq!(failure.attempts, 2);
    assert_eq!(failure.error.as_deref(), Some("safe failure"));
}

#[test]
fn reads_query_error_from_vector_database_inside_scope() {
    // Given: the primary vector database contains a query failure.
    let connection = Connection::open_in_memory().unwrap();
    let identity = Sha256::digest(b"current");
    connection.execute_batch("create table meta(id integer primary key,config_identity text); create table query_error(id integer primary key,at integer,error text); insert into query_error values(1,42,'safe failure')").unwrap();
    connection
        .execute("insert into meta values(1,?1)", [identity.to_hex()])
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: a service reads within the matching vector scope.
    let outcome = storage
        .with_read_tx(&identity, || storage.query_error_get_by_id())
        .unwrap();
    // Then: it receives the stored query error from the primary database.
    let VectorOutcome::Current(Some(error)) = outcome else {
        panic!("query error missing")
    };
    assert_eq!(error.at, 42);
    assert_eq!(error.error, "safe failure");
}

#[test]
fn vector_read_scope_rejects_mismatched_identity_without_calling_operation() {
    // Given: a vector database configured for one identity.
    let connection = Connection::open_in_memory().unwrap();
    let current = Sha256::digest(b"current");
    connection
        .execute_batch("create table meta(id integer primary key,config_identity text)")
        .unwrap();
    connection
        .execute("insert into meta values(1,?1)", [current.to_hex()])
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: a read is requested for another configuration.
    let result = storage.with_read_tx(&Sha256::digest(b"other"), || -> Result<(), StorageError> {
        panic!("mismatched read callback ran")
    });
    // Then: the caller receives the ordinary changed-configuration outcome.
    assert!(matches!(result, Ok(VectorOutcome::ConfigChanged)));
}

#[test]
fn caught_vector_write_error_rolls_back_earlier_changes() {
    // Given: a vector database with no query_error table.
    let connection = Connection::open_in_memory().unwrap();
    let identity = Sha256::digest(b"current");
    connection.execute_batch("create table meta(id integer primary key,config_identity text); create table sample(value integer)").unwrap();
    connection
        .execute("insert into meta values(1,?1)", [identity.to_hex()])
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: the caller catches a real failing vector write.
    let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
        storage
            .get_write_connection()?
            .execute_batch("insert into sample values(9)")
            .unwrap();
        assert!(storage.query_error_upsert_by_id(1, "safe").is_err());
        Ok(())
    });
    // Then: the transaction cannot commit the earlier write.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(
        storage
            .connection
            .query_row("select count(*) from sample", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn incompatible_nested_identity_skips_callback_and_keeps_outer_work() {
    // Given: a vector database with one selected configuration identity.
    let connection = Connection::open_in_memory().unwrap();
    let identity = Sha256::digest(b"current");
    connection.execute_batch("create table meta(id integer primary key,config_identity text); create table sample(value integer)").unwrap();
    connection
        .execute("insert into meta values(1,?1)", [identity.to_hex()])
        .unwrap();
    let storage = VectorStorage::new(connection);
    // When: an incompatible nested request occurs inside compatible work.
    let result = storage.with_write_tx(&identity, || -> Result<(), StorageError> {
        let skipped = storage
            .with_write_tx(&Sha256::digest(b"other"), || -> Result<(), StorageError> {
                panic!("incompatible callback ran")
            })
            .unwrap();
        assert!(matches!(skipped, VectorOutcome::ConfigChanged));
        storage
            .get_write_connection()?
            .execute_batch("insert into sample values(9)")
            .unwrap();
        Ok(())
    });
    // Then: the compatible outer operation commits normally.
    assert!(matches!(result, Ok(VectorOutcome::Current(()))));
    assert_eq!(
        storage
            .connection
            .query_row("select value from sample", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );
}

mod query_tests {
    use super::super::{
        VectorSchema, VectorStorage, decode_vector, meta_get_by_id, vector_index_config_get_by_id,
    };
    use crate::{
        domain::Sha256,
        service::{StorageError, StorageErrorKind, VectorDatabase},
        storage::{native::register_vec1, schema::bootstrap_vectors},
    };
    use rusqlite::Connection;
    use tempfile::TempDir;

    #[test]
    fn gets_identity_and_dimensions_from_selected_schema() {
        // Given: independent main and attached databases have distinct vector metadata.
        let root = TempDir::new().unwrap();
        let path = root.path().join("attached.db");
        let main_identity = Sha256::digest(b"main");
        let attached_identity = Sha256::digest(b"attached");
        register_vec1().unwrap();
        let mut main = Connection::open_in_memory().unwrap();
        let mut attached = Connection::open(&path).unwrap();
        bootstrap_vectors(&mut main).unwrap();
        bootstrap_vectors(&mut attached).unwrap();
        let main = VectorStorage::new(main);
        let attached = VectorStorage::new(attached);
        for (storage, identity, vector) in [
            (&main, &main_identity, vec![1.0, 0.0]),
            (&attached, &attached_identity, vec![1.0, 0.0, 0.0]),
        ] {
            storage.reset_vectors(None, identity).unwrap();
            storage
                .with_write_tx(identity, || -> Result<_, StorageError> {
                    storage.store_vector(identity, &vector, vector.len())
                })
                .unwrap();
        }
        let main = main.get_test_connection();
        main.execute("attach database ?1 as vec", [path.to_str().unwrap()])
            .unwrap();

        // When: metadata is retrieved through each explicit schema selector.
        let main_id = meta_get_by_id(main, VectorSchema::Main).unwrap();
        let attached_id = meta_get_by_id(main, VectorSchema::Attached).unwrap();
        let main_dimensions = vector_index_config_get_by_id(main, VectorSchema::Main).unwrap();
        let attached_dimensions =
            vector_index_config_get_by_id(main, VectorSchema::Attached).unwrap();

        // Then: neither identity nor native dimensions leak across schema locations.
        assert_eq!(main_id, Some(main_identity.to_hex()));
        assert_eq!(attached_id, Some(attached_identity.to_hex()));
        assert_eq!(main_dimensions.unwrap().get(), 2);
        assert_eq!(attached_dimensions.unwrap().get(), 3);
    }

    #[test]
    fn rejects_empty_or_partially_encoded_stored_vectors() {
        // Given: retain a valid component before a truncated trailing component.
        let mut truncated = 1.0_f32.to_ne_bytes().to_vec();
        truncated.push(0);
        for bytes in [Vec::new(), vec![0], truncated] {
            // When: malformed stored bytes are decoded.
            let error = decode_vector(&bytes).unwrap_err();
            // Then: incomplete bytes cannot silently become a shorter vector.
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        }
        // When / Then: complete finite components still decode without loss.
        assert_eq!(decode_vector(&1.0_f32.to_ne_bytes()).unwrap(), vec![1.0]);
    }
}
