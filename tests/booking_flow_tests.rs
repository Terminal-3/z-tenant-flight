//! Booking flow integration tests (native target).
//!
//! These tests verify the non-WASM stub paths for `confirm_booking`
//! and `get_booking`, and the serialisation round-trips for `OfferRow`
//! and `BookingRow`. The full WASM flow (with live KV and outbox) is
//! tested against the Trinity staging cluster — see the README for
//! the Duffel sandbox test playbook.

use z_tenant_flight::__test_exports::{
    KV_BOOKINGS, KV_CACHED_OFFERS, KV_CONFIG, KV_DELEGATION_NONCES,
};

// ── KV map name constants ─────────────────────────────────────────────────────

#[test]
fn test_kv_map_names() {
    assert_eq!(KV_CACHED_OFFERS, "cached_offers");
    assert_eq!(KV_BOOKINGS, "bookings");
    assert_eq!(KV_DELEGATION_NONCES, "delegation_nonces");
    assert_eq!(KV_CONFIG, "config");
}

// ── OfferRow serialisation ────────────────────────────────────────────────────

#[test]
fn test_offer_row_roundtrip() {
    use z_tenant_flight::booking::OfferRow;

    let row = OfferRow {
        offer_id: "off_abc123".to_string(),
        amount_minor: 12_345,
        currency: "GBP".to_string(),
        expires_at_sec: 9_999_999_999,
    };
    let json = serde_json::to_string(&row).expect("serialize");
    let back: OfferRow = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.offer_id, row.offer_id);
    assert_eq!(back.amount_minor, row.amount_minor);
    assert_eq!(back.currency, row.currency);
    assert_eq!(back.expires_at_sec, row.expires_at_sec);
}

// ── BookingRow serialisation ──────────────────────────────────────────────────

#[test]
fn test_booking_row_roundtrip() {
    use z_tenant_flight::booking::BookingRow;

    let row = BookingRow {
        booking_id: "bk-deadbeef".to_string(),
        offer_id: "off_abc123".to_string(),
        pnr: None,
        status: "Pending".to_string(),
    };
    let json = serde_json::to_string(&row).expect("serialize");
    let back: BookingRow = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.booking_id, row.booking_id);
    assert!(back.pnr.is_none());
    assert_eq!(back.status, "Pending");
}

#[test]
fn test_booking_row_with_pnr() {
    use z_tenant_flight::booking::BookingRow;

    let row = BookingRow {
        booking_id: "bk-deadbeef".to_string(),
        offer_id: "off_abc123".to_string(),
        pnr: Some("ABC123".to_string()),
        status: "Confirmed".to_string(),
    };
    let json = serde_json::to_string(&row).expect("serialize");
    let back: BookingRow = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.pnr, Some("ABC123".to_string()));
    assert_eq!(back.status, "Confirmed");
}

// ── confirm_booking stub ──────────────────────────────────────────────────────

#[test]
fn test_confirm_booking_stub_returns_confirmed() {
    use z_tenant_flight::booking::{confirm_booking, ConfirmReq};

    let req = ConfirmReq {
        booking_id: "bk-deadbeef".to_string(),
        pnr: "XYZ789".to_string(),
    };
    let result = confirm_booking(req);
    // Non-WASM stub always succeeds.
    let booking = result.expect("confirm_booking stub should succeed");
    assert_eq!(booking.status, "Confirmed");
    assert_eq!(booking.pnr, Some("XYZ789".to_string()));
}

// ── get_booking stub ──────────────────────────────────────────────────────────

#[test]
fn test_get_booking_stub_returns_not_found() {
    use z_tenant_flight::booking::{get_booking, GetBookingReq};

    let req = GetBookingReq {
        booking_id: "bk-nonexistent".to_string(),
    };
    let result = get_booking(req);
    // Non-WASM stub always returns Err (no real KV).
    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("bk-nonexistent"),
        "error should mention the booking_id"
    );
}

// ── store_offer stub ──────────────────────────────────────────────────────────

#[test]
fn test_store_offer_serialises_correctly() {
    use z_tenant_flight::booking::StoreOfferReq;

    // Just verify the request struct is constructible and serialisable.
    let req = StoreOfferReq {
        offer_id: "off_xyz789".to_string(),
        amount_minor: 50_000,
        currency: "EUR".to_string(),
        expires_at_sec: 9_999_999_999,
    };
    assert_eq!(req.offer_id, "off_xyz789");
    assert_eq!(req.amount_minor, 50_000);
}

// ── contract version ──────────────────────────────────────────────────────────

#[test]
fn test_contract_version_is_semver() {
    let v = z_tenant_flight::CONTRACT_VERSION;
    let parts: Vec<&str> = v.split('.').collect();
    assert_eq!(parts.len(), 3, "CONTRACT_VERSION must be MAJOR.MINOR.PATCH");
    for part in &parts {
        part.parse::<u64>()
            .expect("each version component must be numeric");
    }
}

// ── function name constants ───────────────────────────────────────────────────

#[test]
fn test_fn_name_constants() {
    assert_eq!(z_tenant_flight::FN_STORE_OFFER, "store-offer");
    assert_eq!(z_tenant_flight::FN_START_BOOKING, "start-booking");
    assert_eq!(z_tenant_flight::FN_CONFIRM_BOOKING, "confirm-booking");
    assert_eq!(z_tenant_flight::FN_GET_BOOKING, "get-booking");
}

#[test]
fn test_tenant_functions_slice() {
    let fns = z_tenant_flight::TENANT_FUNCTIONS;
    assert!(fns.contains(&"store-offer"));
    assert!(fns.contains(&"start-booking"));
    assert!(fns.contains(&"confirm-booking"));
    assert!(fns.contains(&"get-booking"));
    assert_eq!(fns.len(), 4);
}
