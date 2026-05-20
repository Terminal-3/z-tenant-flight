//! z-tenant-flight v0.2.0 — Duffel flight booking showcase (MAT-1572).
//!
//! Demonstrates the z-space tenant model:
//!   - `search-offers`: calls Duffel offer search API inside the TEE.
//!   - `book-offer`: calls Duffel create-order API inside the TEE.
//!
//! The Duffel API key is stored in the host secret store (never in
//! contract code or KV maps). PII is passed in by the agent, used
//! inside the enclave to call Duffel, and never returned to the agent.
//! Only the booking ID and PNR cross the WIT boundary back to the
//! caller.
//!
//! # Host-capability requirements
//!
//! Declare in manifest:
//! ```json
//! { "host_capabilities": ["kv_store", "logging", "tenant_context", "http"] }
//! ```
//! The `Http` capability selects the `tenant-http` linker world, which
//! also includes the `state` and `secret` interfaces.
//!
//! # Setup
//!
//! Before first use, store your Duffel API key:
//! ```
//! secret::put_secret("duffel_api_key", b"duffel_test_your_key_here")
//! ```
//! (Call this once from a setup function or directly via admin tooling.)
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
