//! search_offers: calls Duffel offer search API and returns available flights.
//!
//! Duffel search is two calls:
//!   1. POST /air/offer-requests → returns offer_request_id
//!   2. GET /air/offers?offer_request_id=<id> → returns offer list

#[cfg(target_arch = "wasm32")]
use serde_json::json;

#[cfg(target_arch = "wasm32")]
use crate::{
    exports::z::tenant_flight::contracts::{Offer, SearchOffersReq, SearchOffersResp},
    host::interfaces::{http as http_iface, kv_store, logging},
};

#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;

#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #[derive(Debug, Clone)]
    pub struct SearchOffersReq {
        pub origin: String,
        pub destination: String,
        pub departure_date: String,
        pub cabin_class: String,
        pub adult_count: u32,
    }
    #[derive(Debug, Clone)]
    pub struct Offer {
        pub id: String,
        pub total_amount: String,
        pub total_currency: String,
        pub expires_at: String,
    }
    #[derive(Debug, Clone)]
    pub struct SearchOffersResp {
        pub offers: Vec<Offer>,
    }
}

const DUFFEL_BASE: &str = "https://api.duffel.com";
const DUFFEL_VERSION: &str = "v2";

#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub fn search_offers(req: SearchOffersReq) -> Result<SearchOffersResp, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let api_key = get_api_key()?;

        // Step 1: create offer request
        let offer_request_body = json!({
            "data": {
                "slices": [{
                    "origin": req.origin,
                    "destination": req.destination,
                    "departure_date": req.departure_date,
                }],
                "passengers": build_passenger_count(req.adult_count),
                "cabin_class": req.cabin_class,
            }
        });

        let offer_req_resp = http_iface::call(http_iface::Request {
            method: http_iface::Verb::Post,
            url: alloc::format!("{DUFFEL_BASE}/air/offer-requests"),
            headers: Some(duffel_headers(&api_key)),
            payload: Some(serde_json::to_vec(&offer_request_body).map_err(|e| e.to_string())?),
        })
        .map_err(|e| alloc::format!("duffel offer-request: {e}"))?;

        if offer_req_resp.code != 201 {
            return Err(alloc::format!(
                "Duffel offer-request failed: HTTP {}",
                offer_req_resp.code
            ));
        }

        let offer_req_json: serde_json::Value =
            serde_json::from_slice(&offer_req_resp.payload).map_err(|e| e.to_string())?;
        let offer_request_id = offer_req_json["data"]["id"]
            .as_str()
            .ok_or("missing offer_request_id")?
            .to_string();

        let _ = logging::info(&alloc::format!(
            "Duffel offer request created: {offer_request_id}"
        ));

        // Step 2: fetch offers
        let offers_resp = http_iface::call(http_iface::Request {
            method: http_iface::Verb::Get,
            url: alloc::format!(
                "{DUFFEL_BASE}/air/offers?offer_request_id={offer_request_id}&max_connections=0"
            ),
            headers: Some(duffel_headers(&api_key)),
            payload: None,
        })
        .map_err(|e| alloc::format!("duffel offers: {e}"))?;

        if offers_resp.code != 200 {
            return Err(alloc::format!(
                "Duffel offers fetch failed: HTTP {}",
                offers_resp.code
            ));
        }

        let offers_json: serde_json::Value =
            serde_json::from_slice(&offers_resp.payload).map_err(|e| e.to_string())?;

        let offers: Result<alloc::vec::Vec<Offer>, alloc::string::String> = offers_json["data"]
            .as_array()
            .ok_or("missing offers array")?
            .iter()
            .map(|o| {
                let id = o["id"].as_str().ok_or("offer missing id")?.to_string();
                let total_amount = o["total_amount"]
                    .as_str()
                    .ok_or("offer missing total_amount")?
                    .to_string();
                let total_currency = o["total_currency"]
                    .as_str()
                    .ok_or("offer missing total_currency")?
                    .to_string();
                let expires_at = o["expires_at"].as_str().unwrap_or("").to_string();
                Ok(Offer {
                    id,
                    total_amount,
                    total_currency,
                    expires_at,
                })
            })
            .collect();
        let offers = offers?;

        return Ok(SearchOffersResp { offers });
    }

    #[cfg(not(target_arch = "wasm32"))]
    Err("search_offers is only implemented on the wasm32 target".to_string())
}

#[cfg(target_arch = "wasm32")]
fn get_api_key() -> Result<alloc::string::String, alloc::string::String> {
    let bytes = kv_store::get("secrets", b"duffel_api_key")
        .map_err(|e| alloc::format!("kv read: {e}"))?
        .ok_or("duffel_api_key not found in secrets KV map — populate it via the tenant SDK before use")?;
    alloc::string::String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn duffel_headers(
    api_key: &str,
) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
    alloc::vec![
        (
            "Authorization".to_string(),
            alloc::format!("Bearer {api_key}"),
        ),
        ("Duffel-Version".to_string(), DUFFEL_VERSION.to_string()),
        ("Content-Type".to_string(), "application/json".to_string(),),
        ("Accept".to_string(), "application/json".to_string()),
    ]
}

#[cfg(target_arch = "wasm32")]
fn build_passenger_count(adult_count: u32) -> alloc::vec::Vec<serde_json::Value> {
    (0..adult_count)
        .map(|_| json!({ "type": "adult" }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_offers_non_wasm_returns_err() {
        let req = SearchOffersReq {
            origin: "LHR".to_string(),
            destination: "JFK".to_string(),
            departure_date: "2026-07-15".to_string(),
            cabin_class: "economy".to_string(),
            adult_count: 1,
        };
        let result = search_offers(req);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }
}
