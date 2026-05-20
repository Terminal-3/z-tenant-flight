//! z-tenant-flight v0.2.0 — Duffel flight booking showcase (MAT-1572).
//!
//! Demonstrates the z-space tenant model:
//!   - `search-offers`: calls Duffel offer search API inside the TEE.
//!   - `book-offer`: calls Duffel create-order API inside the TEE.
//!
//! The Duffel API key is read from the z: KV map `credentials` (key:
//! `duffel_api_key`). This map is created and populated by the tenant SDK
//! before the contract runs. PII is passed in by the agent, used inside the
//! enclave to call Duffel, and never returned to the agent. Only the booking
//! ID and PNR cross the WIT boundary back to the caller.
//!
//! # Host-capability requirements
//!
//! Declare in manifest:
//! ```json
//! { "host_capabilities": ["kv_store", "logging", "tenant_context", "http"] }
//! ```
//!
//! # Setup
//!
//! Before first use, the tenant SDK must create the `credentials` KV map and
//! write the Duffel API key:
//! ```text
//! // Via the tenant SDK (before contract first use):
//! z_sdk.kv("credentials").set("duffel_api_key", "duffel_test_your_key_here")
//! ```
#![warn(clippy::style, missing_debug_implementations)]
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

extern crate alloc;

pub const CONTRACT_VERSION: &str = "0.2.0";

wit_bindgen::generate!({
    world: "tenant-flight",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

mod booking;
mod search;

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_flight::contracts::Guest for Component {
    fn search_offers(
        req: exports::z::tenant_flight::contracts::SearchOffersReq,
    ) -> Result<exports::z::tenant_flight::contracts::SearchOffersResp, String> {
        search::search_offers(req)
    }

    fn book_offer(
        req: exports::z::tenant_flight::contracts::BookOfferReq,
    ) -> Result<exports::z::tenant_flight::contracts::Booking, String> {
        booking::book_offer(req)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;

    #[test]
    fn contract_version_is_semver() {
        let parts: Vec<&str> = CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "CONTRACT_VERSION must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "each part must be a number");
        }
    }

    #[test]
    fn contract_version_is_v0_2_0() {
        assert_eq!(CONTRACT_VERSION, "0.2.0");
    }
}
