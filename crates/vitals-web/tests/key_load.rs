//! **A ward key that cannot be read is a fault, not an absence.**
//!
//! `ward_signer()` used to resolve once and keep the answer — including a `None` from a keypair file
//! it could not read yet on a cold start, which then looked exactly like a host that holds no key at
//! all. The two mean opposite things (a fault to fix, a choice to respect), and the loader now keeps
//! them apart so the caller can refuse to cache the first.

use vitals_web::ward_chain::load_ward_signer;

#[test]
fn a_ward_key_that_cannot_be_read_is_a_fault_not_an_absence() {
    // Holding neither is a keyless ward, and that is a legitimate answer.
    assert_eq!(load_ward_signer(None, None), Ok(None));

    // A valid operator key resolves.
    assert!(matches!(
        load_ward_signer(Some("11111111111111111111111111111111".into()), None),
        Ok(Some(_))
    ));

    // An operator key that is not a key is a fault — never silently "no key".
    assert!(load_ward_signer(Some("not-a-key".into()), None).is_err(),
            "a malformed operator key read as no key at all");

    // **The cold-start case:** a keypair path that cannot be read is a fault, never silently none.
    // This is the answer that used to be cached for an instance's whole life.
    assert!(load_ward_signer(None, Some("/no/such/keypair.json".into())).is_err(),
            "an unreadable keypair file read as no key at all");
}
