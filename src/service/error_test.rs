use super::{DocumentError, DocumentErrorKind, ProcessError, ServiceError, ServiceErrorKind};
use std::{error::Error, io};

#[test]
fn process_error_preserves_its_io_cause_without_exposing_private_payload() {
    // Given: process creation reports an I/O failure containing private details.
    let error = ProcessError::new(io::Error::other("PRIVATE_PROCESS_CAUSE"));
    // When: callers format the public failure.
    let display = error.to_string();
    let debug = format!("{error:?}");
    // Then: public diagnostics are fixed, while the original cause remains inspectable.
    assert_eq!(display, "cannot spawn process");
    assert!(!debug.contains("PRIVATE_PROCESS_CAUSE"));
    let cause = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::Other);
    assert_eq!(cause.to_string(), "PRIVATE_PROCESS_CAUSE");
}

#[test]
fn preserves_document_context_without_exposing_its_private_cause() {
    // Given: callers supply safe operation context and a separate raw cause.
    let context = "safe document operation context";
    let error = DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        context,
        io::Error::other("PRIVATE_DOCUMENT_CAUSE"),
    );
    // When
    let message = error.get_message();
    let display = error.to_string();
    let debug = format!("{error:?}");
    // Then: public context remains useful while the cause stays internal.
    assert_eq!(message, context);
    assert_eq!(display, context);
    assert!(debug.contains(context));
    assert!(!debug.contains("PRIVATE_DOCUMENT_CAUSE"));
    assert!(
        error
            .source()
            .unwrap()
            .to_string()
            .contains("PRIVATE_DOCUMENT_CAUSE")
    );
}

#[test]
fn service_error_keeps_kind_without_exposing_private_cause() {
    // Given
    let error = ServiceError::new_with_cause(
        ServiceErrorKind::Initialization,
        io::Error::other("PRIVATE_SERVICE_CAUSE"),
    );
    // When
    let diagnostic = format!("{error:?}");
    // Then
    assert_eq!(error.get_kind(), ServiceErrorKind::Initialization);
    assert!(!diagnostic.contains("PRIVATE_SERVICE_CAUSE"));
    assert!(
        error
            .source()
            .unwrap()
            .to_string()
            .contains("PRIVATE_SERVICE_CAUSE")
    );
}
