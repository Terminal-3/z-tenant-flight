//! Booking flow — the four exported contract functions.
//!
//! Each function maps to one WIT export in `wit/world.wit`.
//!
//! # KV map layout
//!
//! | Constant                | Map name              | Purpose                                   |
//! |-------------------------|-----------------------|-------------------------------------------|
//! | `KV_CACHED_OFFERS`      | `cached_offers`       | Duffel offer → amount/currency/expiry     |
//! | `KV_BOOKINGS`           | `bookings`            | booking_id → `BookingRow` (status + PNR)  |
//! | `KV_DELEGATION_NONCES`  | `delegation_nonces`   | 16-byte nonce → "1" (replay guard)        |
//! | `KV_CONFIG`             | `config`              | operator-supplied config (Duffel API key) |
//!
//! All map names are relative; the host prepends `z:<tid>:` automatically.

use serde::{Deserialize, Serialize};

pub const KV_CACHED_OFFERS: &str = "cached_offers";
pub const KV_BOOKINGS: &str = "bookings";
pub const KV_DELEGATION_NONCES: &str = "delegation_nonces";
pub const KV_CONFIG: &str = "config";

/// Stored representation of a Duffel offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferRow {
    pub offer_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub expires_at_sec: u64,
}

/// Stored representation of a booking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookingRow {
    pub booking_id: String,
    pub offer_id: String,
    pub pnr: Option<String>,
    pub status: String,
}

/// Booking status string constants.
pub mod status {
    pub const PENDING: &str = "Pending";
    pub const CONFIRMED: &str = "Confirmed";
}

/// Hardcoded PNR returned in demo/showcase mode.
///
/// Production would receive the real PNR via the Duffel webhook relay
/// (`confirm-booking`). For this showcase the outbox is fully wired but
/// the drain worker does not hit live Duffel — the booking is written as
/// Confirmed immediately so the demo flow runs end-to-end.
const MOCK_PNR: &str = "T3DEMO1";

// ── WASM target: use WIT-generated bindings ──────────────────────────────────

#[cfg(target_arch = "wasm32")]
use crate::exports::z::tenant_flight::contracts::{
    Booking, ConfirmReq, GetBookingReq, OfferId, StartBookingReq, StartBookingResp, StoreOfferReq,
};
#[cfg(target_arch = "wasm32")]
use crate::host::interfaces::kv_store as kv;
#[cfg(target_arch = "wasm32")]
use crate::host::interfaces::logging;
#[cfg(target_arch = "wasm32")]
use crate::host::outbox::outbox as outbox_iface;
#[cfg(target_arch = "wasm32")]
use crate::host::tenant::tenant_context as ctx;

// ── non-WASM target: stub types for tests / clippy ───────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;

#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #[derive(Debug, Clone)]
    pub struct StoreOfferReq {
        pub offer_id: String,
        pub amount_minor: u64,
        pub currency: String,
        pub expires_at_sec: u64,
    }
    #[derive(Debug, Clone)]
    pub struct OfferId {
        pub id: String,
    }
    #[derive(Debug, Clone)]
    pub struct StartBookingReq {
        pub offer_id: String,
        pub passenger_did: String,
        pub delegation_envelope: Vec<u8>,
    }
    #[derive(Debug, Clone)]
    pub struct StartBookingResp {
        pub booking_id: String,
        pub status: String,
    }
    #[derive(Debug, Clone)]
    pub struct ConfirmReq {
        pub booking_id: String,
        pub pnr: String,
    }
    #[derive(Debug, Clone)]
    pub struct Booking {
        pub booking_id: String,
        pub offer_id: String,
        pub pnr: Option<String>,
        pub status: String,
    }
    #[derive(Debug, Clone)]
    pub struct GetBookingReq {
        pub booking_id: String,
    }
}

// ── store_offer ───────────────────────────────────────────────────────────────

/// Cache a Duffel offer for later booking validation.
///
/// Flow:
///   1. Validate that `expires_at_sec` is in the future.
///   2. Serialise `OfferRow` and write to `KV_CACHED_OFFERS[offer_id]`.
///   3. Return `{ id: offer_id }`.
pub fn store_offer(req: StoreOfferReq) -> Result<OfferId, String> {
    // ── 1. validate expiry ──────────────────────────────────────────────────
    #[cfg(target_arch = "wasm32")]
    {
        let now = ctx::cluster_timestamp_secs();
        if req.expires_at_sec <= now {
            return Err("offer already expired".to_string());
        }
    }

    // ── 2. write to KV ─────────────────────────────────────────────────────
    #[cfg(target_arch = "wasm32")]
    {
        let row = OfferRow {
            offer_id: req.offer_id.clone(),
            amount_minor: req.amount_minor,
            currency: req.currency.clone(),
            expires_at_sec: req.expires_at_sec,
        };
        let value = serde_json::to_vec(&row).map_err(|e| e.to_string())?;
        kv::put(KV_CACHED_OFFERS, req.offer_id.as_bytes(), &value)
            .map_err(|e| format!("kv put failed: {e}"))?;
    }

    // ── 3. return ───────────────────────────────────────────────────────────
    Ok(OfferId { id: req.offer_id })
}

// ── start_booking ─────────────────────────────────────────────────────────────

/// Begin a flight booking inside the TEE.
///
/// Flow:
///   1. Deserialise the delegation envelope.
///   2. Load the cached offer from KV.
///   3. Load the nonce-used flag from KV.
///   4. Call `delegation::verify()` (6 checks, cheapest-first).
///   5. Write the nonce to `KV_DELEGATION_NONCES` (replay guard).
///   6. Read passenger PII from `user-profile` (never leaves the TEE).
///   7. Enqueue `POST /orders` to Duffel via the outbox.
///   8. Write a `Confirmed` booking row to `KV_BOOKINGS` (mock PNR for demo).
///   9. Return `{ booking_id, status: "Confirmed" }`.
///
/// # Demo note
/// The outbox enqueue call is fully wired (URL, headers, body) but the drain
/// worker does not hit real Duffel in this showcase. A hardcoded PNR is used
/// so callers see a complete end-to-end flow without a live Duffel account.
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub fn start_booking(req: StartBookingReq) -> Result<StartBookingResp, String> {
    // ── 1. parse delegation envelope (once — nonce extracted here) ───────────
    let envelope: crate::delegation::DelegationEnvelope =
        serde_json::from_slice(&req.delegation_envelope)
            .map_err(|e| format!("bad delegation envelope: {e}"))?;
    let (cred, sig_bytes) =
        crate::delegation::parse_envelope(&envelope).map_err(|r| r.to_string())?;
    // Nonce KV key: hex of the 16-byte nonce array from the parsed credential.
    let nonce_key: alloc::vec::Vec<u8> = crate::delegation::bytes_to_hex(&cred.nonce).into_bytes();

    // ── 2 & 3. load offer + resolve context (WASM only) ──────────────────────
    #[cfg(target_arch = "wasm32")]
    let (offer_amount_minor, cluster_ts, calling_agent_did, nonce_used) = {
        let offer_bytes = kv::get(KV_CACHED_OFFERS, req.offer_id.as_bytes())
            .map_err(|e| format!("kv get offer: {e}"))?
            .ok_or_else(|| format!("offer not found: {}", req.offer_id))?;
        let offer: OfferRow =
            serde_json::from_slice(&offer_bytes).map_err(|e| format!("decode offer: {e}"))?;

        let ts = ctx::cluster_timestamp_secs();

        let agent_did_hex: alloc::string::String =
            crate::delegation::bytes_to_hex(&ctx::calling_user_did().unwrap_or_default());

        let used = kv::get(KV_DELEGATION_NONCES, &nonce_key)
            .map_err(|e| format!("kv get nonce: {e}"))?
            .is_some();

        (offer.amount_minor, ts, agent_did_hex, used)
    };

    // ── 4. verify delegation ────────────────────────────────────────────────
    #[cfg(target_arch = "wasm32")]
    crate::delegation::verify(
        &cred,
        &sig_bytes,
        &envelope.credential_json,
        &crate::delegation::VerifyCtx {
            req_offer_id: &req.offer_id,
            offer_amount_minor,
            cluster_ts,
            calling_agent_did: &calling_agent_did,
            nonce_used,
        },
    )
    .map_err(|r| r.to_string())?;

    // ── 5. mark nonce used ──────────────────────────────────────────────────
    #[cfg(target_arch = "wasm32")]
    kv::put(KV_DELEGATION_NONCES, &nonce_key, b"1").map_err(|e| format!("kv put nonce: {e}"))?;

    // ── 6. read PII from user-profile (stays inside the TEE) ────────────────
    // The `user-profile` interface is write-only from the guest's perspective
    // (T3-TS-033); PII is forwarded directly to the outbox request body below.
    // This comment is load-bearing: it documents the privacy guarantee.
    //
    // TODO: call `host:interfaces/user-profile.get-fields(passenger_did,
    //       ["passport_number", "date_of_birth", "full_name"])` once the
    //       read-path WIT function ships in host:interfaces@2.2.0.
    //       For now the outbox body uses placeholder fields so the scaffold
    //       compiles end-to-end.
    let _ = &req.passenger_did; // consumed below

    // ── 7. enqueue Duffel POST /orders via outbox ────────────────────────────
    #[cfg(target_arch = "wasm32")]
    {
        // Idempotency key: contract-name@booking_id (outbox idk format).
        // nonce_key is already the hex of cred.nonce (computed once above).
        let booking_id =
            alloc::format!("bk-{}", alloc::string::String::from_utf8_lossy(&nonce_key));
        let idk = alloc::format!("z:travel/contracts@{}", booking_id);

        // Read Duffel API key from config.
        let api_key = kv::get(KV_CONFIG, b"duffel_api_key")
            .ok()
            .flatten()
            .and_then(|v| alloc::string::String::from_utf8(v).ok())
            .unwrap_or_default();

        // Build the Duffel create-order payload.
        // In production, include PII read from user-profile above.
        let body_json = serde_json::json!({
            "data": {
                "type": "instant",
                "selected_offers": [req.offer_id],
                "passengers": [{
                    "id": "passenger_0",
                    "passenger_did": req.passenger_did,
                    // TODO: add passport_number, date_of_birth, full_name from user-profile
                }]
            }
        });
        let body = serde_json::to_vec(&body_json).map_err(|e| e.to_string())?;

        let duffel_req = outbox_iface::Request {
            method: outbox_iface::Verb::Post,
            url: "https://api.duffel.com/air/orders".to_string(),
            headers: alloc::vec![
                (
                    "Authorization".to_string(),
                    alloc::format!("Bearer {api_key}")
                ),
                ("Duffel-Version".to_string(), "v2".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body,
        };

        outbox_iface::enqueue(&idk, &duffel_req)
            .map_err(|e| alloc::format!("outbox enqueue: {e:?}"))?;

        // Demo: outbox is wired but drain worker does not hit live Duffel.
        // Log the mock response and skip the webhook-relay step.
        let _ = logging::info(&alloc::format!(
            "Duffel POST /orders enqueued for booking {booking_id} \
             — mock response: order created OK, PNR={MOCK_PNR}"
        ));

        // ── 8. write Confirmed booking row (mock PNR for demo) ───────────────
        let row = BookingRow {
            booking_id: booking_id.clone(),
            offer_id: req.offer_id.clone(),
            pnr: Some(MOCK_PNR.to_string()),
            status: status::CONFIRMED.to_string(),
        };
        let row_bytes = serde_json::to_vec(&row).map_err(|e| e.to_string())?;
        kv::put(KV_BOOKINGS, booking_id.as_bytes(), &row_bytes)
            .map_err(|e| alloc::format!("kv put booking: {e}"))?;

        return Ok(StartBookingResp {
            booking_id,
            status: status::CONFIRMED.to_string(),
        });
    }

    // Non-WASM stub path — unreachable in tests that target this function.
    #[cfg(not(target_arch = "wasm32"))]
    Err("start_booking is only fully implemented on the wasm32 target".to_string())
}

// ── confirm_booking ───────────────────────────────────────────────────────────

/// Write the PNR returned by the Duffel webhook relay.
///
/// Flow:
///   1. Load the booking row from KV.
///   2. Transition status to `Confirmed` and set `pnr`.
///   3. Write back to `KV_BOOKINGS`.
///   4. Return the updated `Booking`.
pub fn confirm_booking(req: ConfirmReq) -> Result<Booking, String> {
    #[cfg(target_arch = "wasm32")]
    {
        // Load existing row.
        let row_bytes = kv::get(KV_BOOKINGS, req.booking_id.as_bytes())
            .map_err(|e| format!("kv get: {e}"))?
            .ok_or_else(|| format!("booking not found: {}", req.booking_id))?;
        let mut row: BookingRow =
            serde_json::from_slice(&row_bytes).map_err(|e| format!("decode: {e}"))?;

        // Transition.
        row.pnr = Some(req.pnr.clone());
        row.status = status::CONFIRMED.to_string();

        // Write back.
        let updated = serde_json::to_vec(&row).map_err(|e| e.to_string())?;
        kv::put(KV_BOOKINGS, req.booking_id.as_bytes(), &updated)
            .map_err(|e| format!("kv put: {e}"))?;

        return Ok(Booking {
            booking_id: row.booking_id,
            offer_id: row.offer_id,
            pnr: row.pnr,
            status: row.status,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Stub for non-WASM builds.
        Ok(Booking {
            booking_id: req.booking_id,
            offer_id: String::new(),
            pnr: Some(req.pnr),
            status: status::CONFIRMED.to_string(),
        })
    }
}

// ── get_booking ───────────────────────────────────────────────────────────────

/// Point-read of a booking row.
pub fn get_booking(req: GetBookingReq) -> Result<Booking, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let row_bytes = kv::get(KV_BOOKINGS, req.booking_id.as_bytes())
            .map_err(|e| format!("kv get: {e}"))?
            .ok_or_else(|| format!("booking not found: {}", req.booking_id))?;
        let row: BookingRow =
            serde_json::from_slice(&row_bytes).map_err(|e| format!("decode: {e}"))?;

        return Ok(Booking {
            booking_id: row.booking_id,
            offer_id: row.offer_id,
            pnr: row.pnr,
            status: row.status,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    Err(format!("booking not found (stub): {}", req.booking_id))
}
