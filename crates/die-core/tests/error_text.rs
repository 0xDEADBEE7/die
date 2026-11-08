//! Errors as a person reads them, on the command line or stderr (contract §15).

use die_test_cap as _;

use die_core::{Checkout, Error};

#[test]
fn an_error_reads_as_text_not_as_rust() {
    let msg = Error::Msg("/a/b: not a regular file".into());
    assert_eq!(msg.to_string(), "/a/b: not a regular file");
    assert_eq!(
        Error::NoCommand("x".into()).to_string(),
        "no command named x"
    );
    assert_eq!(
        Error::from(Checkout::Gone).to_string(),
        "that buffer is gone"
    );
}
