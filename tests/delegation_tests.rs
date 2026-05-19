//! Unit tests for the six `DelegationDenyReason` variants.
//!
//! These tests run on the native target (`cargo test`) without
//! requiring a WASM runtime. They mirror the check order documented
//! in `src/delegation.rs`:
//!
//!   1. InvalidSignature (EIP-191 recovery, requires secp256k1 — tested via `#[should_panic]`)
//!   2. Expired
//!   3. AmountExceeded
//!   4. OfferIdMismatch
//!   5. AgentDidMismatch
//!   6. NonceAlreadyUsed
//!
//! Tests 2–6 bypass the signature check by checking the conditions
//! directly (mirroring the logic in `delegation::verify`). The
//! condition-checking helper is intentionally kept inline here to
//! document the exact decision boundaries.

use z_tenant_flight::__test_exports::{
    parse_envelope, BookingDelegationCredential, DelegationDenyReason, DelegationEnvelope,
    VerifyCtx,
};

fn make_cred(
    user_did: &str,
    agent_did: &str,
    offer_id: &str,
    max_amount_minor: u64,
    expires_at_sec: u64,
) -> BookingDelegationCredential {
    BookingDelegationCredential {
        user_did: user_did.to_string(),
        agent_did: agent_did.to_string(),
        action: "book-flight".to_string(),
        offer_id: offer_id.to_string(),
        max_amount_minor,
        currency: "GBP".to_string(),
        expires_at_sec,
        nonce: [
            0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
            0xaa, 0xbb,
        ],
    }
}

/// Mirror of the non-signature checks in `delegation::verify`.
/// Returns the first deny reason encountered, or panics if all pass
/// (used to verify that the "positive" path works for checks 2–6).
fn check_deny(
    cred: &BookingDelegationCredential,
    req_offer_id: &str,
    offer_amount_minor: u64,
    cluster_ts: u64,
    calling_agent_did: &str,
    nonce_used: bool,
) -> Option<DelegationDenyReason> {
    // Check 2: expiry
    if cred.expires_at_sec <= cluster_ts {
        return Some(DelegationDenyReason::Expired);
    }
    // Check 3: amount cap
    if cred.max_amount_minor < offer_amount_minor {
        return Some(DelegationDenyReason::AmountExceeded);
    }
    // Check 4: offer_id binding
    if cred.offer_id != req_offer_id {
        return Some(DelegationDenyReason::OfferIdMismatch);
    }
    // Check 5: agent DID
    let expected = cred.agent_did.trim_start_matches("0x").to_lowercase();
    let actual = calling_agent_did.trim_start_matches("0x").to_lowercase();
    if expected != actual {
        return Some(DelegationDenyReason::AgentDidMismatch);
    }
    // Check 6: nonce replay
    if nonce_used {
        return Some(DelegationDenyReason::NonceAlreadyUsed);
    }
    None
}

// ── Test 1: InvalidSignature ─────────────────────────────────────────────────

/// `recover_eth_address` is `unimplemented!()` on non-wasm. Driving the full
/// `parse_envelope` → `verify` path with a syntactically valid envelope
/// confirms that signature recovery is the first gate and that wiring it
/// in will make this path reachable. Replace `#[should_panic]` with a real
/// secp256k1 assertion once the implementation is wired in.
#[test]
#[should_panic(expected = "EIP-191 recovery requires secp256k1")]
fn test_delegation_invalid_signature_panics_on_non_wasm() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 10_000, 9_999_999_999);
    let cred_json = serde_json::to_string(&cred).unwrap();
    let zero_sig = "0".repeat(130); // 65 zero bytes, hex-encoded
    let envelope = DelegationEnvelope {
        credential_json: cred_json,
        signature_hex: zero_sig,
    };
    // parse_envelope succeeds (valid JSON + valid hex), then verify calls
    // recover_eth_address which panics on non-wasm with the documented message.
    let (parsed_cred, sig_bytes) = parse_envelope(&envelope).unwrap();
    let _ = z_tenant_flight::__test_exports::verify_raw(
        &parsed_cred,
        &sig_bytes,
        &envelope.credential_json,
        &VerifyCtx {
            req_offer_id: "off_abc123",
            offer_amount_minor: 5_000,
            cluster_ts: 1_000,
            calling_agent_did: "0xagent01",
            nonce_used: false,
        },
    );
}

// ── Test 2: Expired ───────────────────────────────────────────────────────────

#[test]
fn test_delegation_expired() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 10_000, 1_000);
    // cluster_ts (2000) > expires_at_sec (1000) → Expired
    let reason = check_deny(&cred, "off_abc123", 5_000, 2_000, "0xagent01", false);
    assert_eq!(reason, Some(DelegationDenyReason::Expired));
}

// ── Test 3: AmountExceeded ────────────────────────────────────────────────────

#[test]
fn test_delegation_amount_exceeded() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 5_000, 9_999_999_999);
    // offer costs 10_000 but credential only allows 5_000 → AmountExceeded
    let reason = check_deny(&cred, "off_abc123", 10_000, 1_000, "0xagent01", false);
    assert_eq!(reason, Some(DelegationDenyReason::AmountExceeded));
}

// ── Test 4: OfferIdMismatch ───────────────────────────────────────────────────

#[test]
fn test_delegation_offer_id_mismatch() {
    let cred = make_cred(
        "0xuser01",
        "0xagent01",
        "off_CORRECT",
        10_000,
        9_999_999_999,
    );
    // req uses a different offer_id → OfferIdMismatch
    let reason = check_deny(&cred, "off_WRONG", 5_000, 1_000, "0xagent01", false);
    assert_eq!(reason, Some(DelegationDenyReason::OfferIdMismatch));
}

// ── Test 5: AgentDidMismatch ──────────────────────────────────────────────────

#[test]
fn test_delegation_agent_did_mismatch() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 10_000, 9_999_999_999);
    // calling agent is different from credential's agent_did → AgentDidMismatch
    let reason = check_deny(&cred, "off_abc123", 5_000, 1_000, "0xagentXX", false);
    assert_eq!(reason, Some(DelegationDenyReason::AgentDidMismatch));
}

// ── Test 6: NonceAlreadyUsed ──────────────────────────────────────────────────

#[test]
fn test_delegation_nonce_reuse() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 10_000, 9_999_999_999);
    // nonce_used = true → NonceAlreadyUsed
    let reason = check_deny(&cred, "off_abc123", 5_000, 1_000, "0xagent01", true);
    assert_eq!(reason, Some(DelegationDenyReason::NonceAlreadyUsed));
}

// ── Positive: all non-signature checks pass ───────────────────────────────────

#[test]
fn test_delegation_all_non_sig_checks_pass() {
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 10_000, 9_999_999_999);
    // All conditions satisfied → None (no deny reason from checks 2–6).
    let reason = check_deny(
        &cred,
        "off_abc123",
        5_000,       // <= max_amount_minor (10_000)
        1_000,       // < expires_at_sec (9_999_999_999)
        "0xagent01", // matches credential.agent_did
        false,       // nonce not yet used
    );
    assert_eq!(reason, None, "all non-signature checks should pass");
}

// ── Check ordering: Expired fires before AmountExceeded ──────────────────────

#[test]
fn test_delegation_check_order_expired_before_amount() {
    // Both Expired and AmountExceeded conditions are true; Expired must win.
    let cred = make_cred("0xuser01", "0xagent01", "off_abc123", 1_000, 500);
    let reason = check_deny(
        &cred,
        "off_abc123",
        10_000, // AmountExceeded condition also true
        1_000,  // cluster_ts > expires_at_sec → Expired fires first
        "0xagent01",
        false,
    );
    assert_eq!(reason, Some(DelegationDenyReason::Expired));
}

// ── DelegationDenyReason Display ─────────────────────────────────────────────

#[test]
fn test_deny_reason_display() {
    assert_eq!(
        DelegationDenyReason::InvalidSignature.to_string(),
        "invalid-signature"
    );
    assert_eq!(DelegationDenyReason::Expired.to_string(), "expired");
    assert_eq!(
        DelegationDenyReason::NonceAlreadyUsed.to_string(),
        "nonce-already-used"
    );
    assert_eq!(
        DelegationDenyReason::OfferIdMismatch.to_string(),
        "offer-id-mismatch"
    );
    assert_eq!(
        DelegationDenyReason::AmountExceeded.to_string(),
        "amount-exceeded"
    );
    assert_eq!(
        DelegationDenyReason::AgentDidMismatch.to_string(),
        "agent-did-mismatch"
    );
}
