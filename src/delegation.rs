//! Delegation credential for the Duffel booking flow.
//!
//! Before the TEE calls Duffel, the contract verifies that the user
//! *explicitly* authorised this specific booking by checking a signed
//! `BookingDelegationCredential`. The credential is produced off-chain by
//! the user's wallet and carried into the contract by the agent.
//!
//! # Six-check verification order
//!
//! 1. EIP-191 signature recovery → extracted Ethereum address must match
//!    the `user_did` field (the user's ETH address).
//! 2. `expires_at_sec > cluster_timestamp` — not expired.
//! 3. `max_amount_minor >= offer.amount_minor` — amount within cap.
//! 4. `offer_id == req.offer_id` — credential is for this specific offer.
//! 5. `agent_did` matches the calling session DID from `tenant-context`.
//! 6. `nonce` not already present in `KV_DELEGATION_NONCES` — no replay.
//!
//! Checks are performed in this order so the cheapest (no I/O) checks
//! run before the KV nonce lookup.

use serde::{Deserialize, Serialize};

/// A delegation credential the user signs off-chain to authorise one
/// specific flight booking by one specific agent.
///
/// The credential is serialised as compact JSON, then EIP-191 signed:
///   `"\x19Ethereum Signed Message:\n" + len(json) + json`
///
/// The resulting 65-byte `{ r || s || v }` signature is carried in the
/// `delegation_envelope` field of `start-booking-req` alongside the JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookingDelegationCredential {
    /// ETH address of the user authorising this booking (20 bytes, hex-encoded).
    pub user_did: String,
    /// DID of the agent that may execute this booking.
    pub agent_did: String,
    /// Action selector — MUST equal `"book-flight"`.
    pub action: String,
    /// Duffel offer_id this credential is bound to.
    pub offer_id: String,
    /// Maximum booking amount the user is willing to pay (in minor currency units).
    pub max_amount_minor: u64,
    /// ISO 4217 currency code the `max_amount_minor` is denominated in.
    pub currency: String,
    /// Unix timestamp (seconds) after which this credential is void.
    pub expires_at_sec: u64,
    /// 16-byte cryptographic nonce — prevents replay. Stored in
    /// `KV_DELEGATION_NONCES` on first use.
    pub nonce: [u8; 16],
}

/// Typed reasons `verify` returns `Err`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationDenyReason {
    /// EIP-191 recovery failed or the recovered address does not match `user_did`.
    InvalidSignature,
    /// `expires_at_sec <= cluster_timestamp_secs`.
    Expired,
    /// The nonce is already present in `KV_DELEGATION_NONCES`.
    NonceAlreadyUsed,
    /// `credential.offer_id != req.offer_id`.
    OfferIdMismatch,
    /// `credential.max_amount_minor < offer.amount_minor`.
    AmountExceeded,
    /// `credential.agent_did` does not match the calling session DID from
    /// `tenant-context.calling-user-did()`.
    AgentDidMismatch,
}

impl core::fmt::Display for DelegationDenyReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "invalid-signature"),
            Self::Expired => write!(f, "expired"),
            Self::NonceAlreadyUsed => write!(f, "nonce-already-used"),
            Self::OfferIdMismatch => write!(f, "offer-id-mismatch"),
            Self::AmountExceeded => write!(f, "amount-exceeded"),
            Self::AgentDidMismatch => write!(f, "agent-did-mismatch"),
        }
    }
}

/// EIP-191 signature recovery stub.
///
/// The real implementation must use a `secp256k1` crate (e.g.
/// `k256` / `libsecp256k1`). That crate is intentionally omitted from
/// the scaffold so the repo compiles to native for tests without pulling
/// in a heavy WASM-specific crypto dep tree.
///
/// Wire-in the real implementation like this:
/// ```rust,no_run,ignore
/// use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
/// use sha3::{Digest, Keccak256};
///
/// pub fn recover_eth_address(sig: &[u8], msg: &[u8]) -> Option<[u8; 20]> {
///     // 1. Hash the EIP-191 prefixed message with keccak-256.
///     let prefix = format!("\x19Ethereum Signed Message:\n{}", msg.len());
///     let mut hasher = Keccak256::new();
///     hasher.update(prefix.as_bytes());
///     hasher.update(msg);
///     let hash = hasher.finalize();
///
///     // 2. Split r||s||v (65 bytes).
///     if sig.len() != 65 { return None; }
///     let v = sig[64];
///     let recovery_id = RecoveryId::from_byte(v % 2)?;
///     let signature = Signature::from_slice(&sig[..64]).ok()?;
///
///     // 3. Recover the public key and derive the Ethereum address.
///     let key = VerifyingKey::recover_from_prehash(&hash, &signature, recovery_id).ok()?;
///     let encoded = key.to_encoded_point(false);
///     let pub_bytes = &encoded.as_bytes()[1..]; // drop 0x04 prefix
///     let mut addr_hash = Keccak256::new();
///     addr_hash.update(pub_bytes);
///     let full = addr_hash.finalize();
///     let mut addr = [0u8; 20];
///     addr.copy_from_slice(&full[12..]);
///     Some(addr)
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::missing_panics_doc)]
pub fn recover_eth_address(_sig: &[u8], _msg: &[u8]) -> Option<[u8; 20]> {
    unimplemented!("EIP-191 recovery requires secp256k1 — wire actual impl for WASM")
}

/// EIP-191 recovery placeholder for WASM target.
/// Replace this with the real implementation (see the `cfg(not(wasm32))`
/// variant above for the full code template).
#[cfg(target_arch = "wasm32")]
pub fn recover_eth_address(_sig: &[u8], _msg: &[u8]) -> Option<[u8; 20]> {
    unimplemented!("EIP-191 recovery requires secp256k1 — wire actual impl for WASM")
}

/// Parsed delegation envelope carried in `start-booking-req.delegation_envelope`.
#[derive(Debug, Deserialize)]
pub struct DelegationEnvelope {
    /// Compact JSON of the `BookingDelegationCredential`.
    pub credential_json: String,
    /// 65-byte EIP-191 signature (r||s||v), hex-encoded.
    pub signature_hex: String,
}

/// Parse the raw `DelegationEnvelope` bytes into a `(credential, sig_bytes)` pair.
///
/// Called by `booking::start_booking` before the WASM-only block so the nonce
/// is available for the KV lookup without a second JSON parse inside `verify`.
pub fn parse_envelope(
    envelope: &DelegationEnvelope,
) -> Result<(BookingDelegationCredential, alloc::vec::Vec<u8>), DelegationDenyReason> {
    let cred: BookingDelegationCredential = serde_json::from_str(&envelope.credential_json)
        .map_err(|_| DelegationDenyReason::InvalidSignature)?;
    let sig_bytes =
        hex_decode(&envelope.signature_hex).ok_or(DelegationDenyReason::InvalidSignature)?;
    Ok((cred, sig_bytes))
}

/// Execution context passed to `verify`. Bundles the host-supplied values that
/// are only available at WASM runtime so they can be passed as a single argument.
#[derive(Debug)]
pub struct VerifyCtx<'a> {
    /// offer_id from the `start-booking-req`.
    pub req_offer_id: &'a str,
    /// The cached offer's price in minor currency units.
    pub offer_amount_minor: u64,
    /// `tenant-context.cluster-timestamp-secs()`.
    pub cluster_ts: u64,
    /// `tenant-context.calling-user-did()`, hex-encoded.
    pub calling_agent_did: &'a str,
    /// `true` when the nonce was already found in `KV_DELEGATION_NONCES`.
    pub nonce_used: bool,
}

/// Verify a pre-parsed delegation credential against the offer and execution context.
///
/// Returns `Ok(())` on success; the caller writes the nonce to
/// `KV_DELEGATION_NONCES` after a successful return.
pub fn verify(
    cred: &BookingDelegationCredential,
    sig_bytes: &[u8],
    credential_json: &str,
    ctx: &VerifyCtx<'_>,
) -> Result<(), DelegationDenyReason> {
    // ── Check 1: EIP-191 signature ──────────────────────────────────────────
    let recovered = recover_eth_address(sig_bytes, credential_json.as_bytes())
        .ok_or(DelegationDenyReason::InvalidSignature)?;

    // Compare recovered address to user_did (strip optional 0x prefix).
    let expected_hex = cred.user_did.trim_start_matches("0x").to_lowercase();
    if bytes_to_hex(&recovered) != expected_hex {
        return Err(DelegationDenyReason::InvalidSignature);
    }

    // ── Check 2: expiry ─────────────────────────────────────────────────────
    if cred.expires_at_sec <= ctx.cluster_ts {
        return Err(DelegationDenyReason::Expired);
    }

    // ── Check 3: amount cap ─────────────────────────────────────────────────
    if cred.max_amount_minor < ctx.offer_amount_minor {
        return Err(DelegationDenyReason::AmountExceeded);
    }

    // ── Check 4: offer_id binding ───────────────────────────────────────────
    if cred.offer_id != ctx.req_offer_id {
        return Err(DelegationDenyReason::OfferIdMismatch);
    }

    // ── Check 5: agent DID ──────────────────────────────────────────────────
    let expected_agent = cred.agent_did.trim_start_matches("0x").to_lowercase();
    let actual_agent = ctx
        .calling_agent_did
        .trim_start_matches("0x")
        .to_lowercase();
    if expected_agent != actual_agent {
        return Err(DelegationDenyReason::AgentDidMismatch);
    }

    // ── Check 6: nonce replay ───────────────────────────────────────────────
    if ctx.nonce_used {
        return Err(DelegationDenyReason::NonceAlreadyUsed);
    }

    Ok(())
}

// ── helpers ─────────────────────────────────────────────────────────────────

/// Encode a byte slice as a lowercase hex string. Used for DID comparison and
/// nonce KV keys. Shared with `booking` via `pub(crate)`.
pub(crate) fn bytes_to_hex(b: &[u8]) -> alloc::string::String {
    b.iter().map(|byte| alloc::format!("{byte:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<alloc::vec::Vec<u8>> {
    let s = s.trim_start_matches("0x");
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = alloc::vec::Vec::with_capacity(s.len() / 2);
    let mut chars = s.chars();
    while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
        let hi = h.to_digit(16)? as u8;
        let lo = l.to_digit(16)? as u8;
        out.push((hi << 4) | lo);
    }
    Some(out)
}
