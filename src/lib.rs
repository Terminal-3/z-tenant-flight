//! z-tenant-flight — example Trinity tenant contract (MAT-1572).
//!
//! Demonstrates the z-space model for a Duffel flight booking workflow:
//!   - An agent calls `store-offer` to cache a Duffel offer_id.
//!   - The agent calls `start-booking` with a signed delegation credential.
//!   - The contract verifies the credential (6 checks), reads PII from
//!     `user-profile` **inside the TEE**, enqueues the Duffel `POST
//!     /orders` call via the outbox, and returns a `Pending` booking.
//!   - For this showcase the drain worker does not hit live Duffel — the
//!     booking is written as Confirmed immediately with a hardcoded demo PNR.
//!
//! Privacy guarantee: passport number, date-of-birth, and full name
//! are read from the TEE's user-profile store and forwarded to Duffel
//! through the durable outbox. They never appear in any WIT return value
//! — the agent only sees `booking_id` and `pnr`.
//!
//! # Host-contract invariants
//!
//! 1. **OCC transaction per invocation.** All `kv-store` writes are
//!    committed atomically on `Ok` and rolled back on `Err`. The
//!    contract never needs to undo a partial write.
//! 2. **`z:`-anchored KV scope.** The governor gates every `kv-store`
//!    call to `z:<tid>:*`; cross-tenant reads/writes are denied at the
//!    policy layer.
//! 3. **Outbox at-most-once.** The drain worker emits the Duffel request
//!    exactly once (idempotency-key-gated); the contract does not retry.
//! 4. **`tenant-context` is host-verified.** `tenant-did`,
//!    `cluster-timestamp-secs`, and `calling-user-did` are trusted
//!    inputs from the dispatcher.
#![warn(clippy::style, missing_debug_implementations)]
// Contract entry points are only reachable via the WASM `export!` macro;
// on host builds (tests, clippy) nothing references the `Guest` impl, so
// dead-code lints fire. Scope the suppression to non-wasm so wasm builds
// remain strict.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

extern crate alloc;

/// Bump on every source change. Semver: patch=fix, minor=add, major=break.
/// 0.1.0 — initial scaffold (MAT-1572).
pub const CONTRACT_VERSION: &str = "0.1.0";

// ── WIT function name constants ─────────────────────────────────────────────
// Canonical kebab-case export names for every WIT function in `wit/world.wit`.

/// `store-offer` — cache a Duffel offer for later booking.
pub const FN_STORE_OFFER: &str = "store-offer";
/// `start-booking` — verify delegation, enqueue Duffel order, write Pending row.
pub const FN_START_BOOKING: &str = "start-booking";
/// `confirm-booking` — write the PNR from the Duffel webhook relay.
pub const FN_CONFIRM_BOOKING: &str = "confirm-booking";
/// `get-booking` — point-read of a booking row.
pub const FN_GET_BOOKING: &str = "get-booking";

/// All dispatchable functions (used by the host's tenant dispatch layer).
pub const TENANT_FUNCTIONS: &[&str] = &[
    FN_STORE_OFFER,
    FN_START_BOOKING,
    FN_CONFIRM_BOOKING,
    FN_GET_BOOKING,
];

wit_bindgen::generate!({
    world: "tenant-flight",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

pub mod booking;
mod delegation;

// Re-export types for integration tests.
#[doc(hidden)]
pub mod __test_exports {
    pub use crate::booking::{KV_BOOKINGS, KV_CACHED_OFFERS, KV_CONFIG, KV_DELEGATION_NONCES};
    pub use crate::delegation::{
        parse_envelope, verify as verify_raw, BookingDelegationCredential, DelegationDenyReason,
        DelegationEnvelope, VerifyCtx,
    };
}

// ── WASM export ─────────────────────────────────────────────────────────────

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_flight::contracts::Guest for Component {
    fn store_offer(
        req: exports::z::tenant_flight::contracts::StoreOfferReq,
    ) -> Result<exports::z::tenant_flight::contracts::OfferId, String> {
        booking::store_offer(req)
    }

    fn start_booking(
        req: exports::z::tenant_flight::contracts::StartBookingReq,
    ) -> Result<exports::z::tenant_flight::contracts::StartBookingResp, String> {
        booking::start_booking(req)
    }

    fn confirm_booking(
        req: exports::z::tenant_flight::contracts::ConfirmReq,
    ) -> Result<exports::z::tenant_flight::contracts::Booking, String> {
        booking::confirm_booking(req)
    }

    fn get_booking(
        req: exports::z::tenant_flight::contracts::GetBookingReq,
    ) -> Result<exports::z::tenant_flight::contracts::Booking, String> {
        booking::get_booking(req)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);
