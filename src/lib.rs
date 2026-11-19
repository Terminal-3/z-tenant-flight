//! z-tenant-flight — book flights through the Duffel API for a customer,
//! without the agent that books ever seeing their details.
//!
//! - `save-traveller` / `forget-traveller`: the customer saves or deletes
//!   their own traveller record (title, phone, and optionally a passport) in
//!   `z:<tid>:travellers`. Refused on a delegated call.
//! - `search-offers`: Duffel offer search over plain `http` (no PII).
//! - `book-offer`: Duffel order over `http-with-placeholders`. Name and email
//!   are `{{profile.*}}` markers the host fills from the customer's T3N
//!   profile; title and phone come from the traveller record; the passport,
//!   with date of birth and sex, from that record or from a passport sealed
//!   with the Private Doc Scanner (`z:<tid>:intake`).
//!
//! Maps (create before first use, readable and writable by this contract):
//! `secrets` (`duffel_api_key`, seeded by the tenant) and `travellers`.
//! Using the scanner's passport also needs this contract added to the
//! readers of the scanner's `intake` map.
#![warn(clippy::style, missing_debug_implementations)]
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

extern crate alloc;

/// Scanned passports: sex may read Male / Female, and fields the traveller
/// corrected in the scanner are accepted. MINOR.
pub const CONTRACT_VERSION: &str = "0.7.0";

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
mod countries;
mod search;
pub mod traveller;

struct Component;

#[cfg(target_arch = "wasm32")]
mod store {
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    use crate::host::interfaces::{kv_store, logging};
    use crate::host::tenant::tenant_context;
    use crate::traveller;

    /// The caller's own record key. Refused on a delegated call, so an agent
    /// can never write or delete a customer's traveller record.
    fn self_key(op: &str) -> Result<Vec<u8>, String> {
        let user = tenant_context::calling_user_did()
            .ok_or_else(|| format!("{op}: no calling customer; call through an authenticated T3N session"))?;
        if tenant_context::is_delegated() {
            return Err(format!("{op}: only the customer can do this for themselves; an agent cannot"));
        }
        Ok(hex::encode(user).into_bytes())
    }

    fn map() -> String {
        format!("z:{}:{}", hex::encode(tenant_context::tenant_did()), traveller::TRAVELLERS_MAP_TAIL)
    }

    pub fn save(input: &[u8]) -> Result<Vec<u8>, String> {
        let key = self_key("save-traveller")?;
        let t = traveller::parse_traveller(input)?;
        let bytes = serde_json::to_vec(&t).map_err(|e| e.to_string())?;
        kv_store::put(&map(), &key, &bytes)?;
        logging::info("save-traveller: record saved")?;
        serde_json::to_vec(&serde_json::json!({ "saved": true, "has_passport": t.passport.is_some() }))
            .map_err(|e| e.to_string())
    }

    pub fn forget() -> Result<Vec<u8>, String> {
        let key = self_key("forget-traveller")?;
        let deleted = kv_store::delete(&map(), &key)?;
        logging::info("forget-traveller: record deleted")?;
        serde_json::to_vec(&serde_json::json!({ "deleted": deleted })).map_err(|e| e.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_flight::contracts::Guest for Component {
    fn save_traveller(
        req: exports::z::tenant_flight::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("save-traveller: missing input")?;
        store::save(&input)
    }

    fn forget_traveller(
        _req: exports::z::tenant_flight::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        store::forget()
    }

    fn search_offers(
        req: exports::z::tenant_flight::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("search-offers: missing input")?;
        search::search_offers(&input)
    }

    fn book_offer(
        req: exports::z::tenant_flight::contracts::GenericInput,
    ) -> Result<alloc::vec::Vec<u8>, alloc::string::String> {
        let input = req.input.ok_or("book-offer: missing input")?;
        booking::book_offer(&input)
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
}
