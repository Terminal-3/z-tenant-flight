//! book_offer: calls Duffel create-order API and returns the PNR.

#[cfg(target_arch = "wasm32")]
use serde_json::json;

#[cfg(target_arch = "wasm32")]
use crate::{
    exports::z::tenant_flight::contracts::{BookOfferReq, Booking},
    host::interfaces::{http as http_iface, kv_store, logging},
};

#[cfg(not(target_arch = "wasm32"))]
pub use stubs::*;

#[cfg(not(target_arch = "wasm32"))]
mod stubs {
    #[derive(Debug, Clone)]
    pub struct Passenger {
        pub given_name: String,
        pub family_name: String,
        pub date_of_birth: String,
        pub passport_number: String,
        pub nationality: String,
        pub passport_expiry: String,
        pub gender: String,
        pub email: String,
        pub phone: String,
    }
    #[derive(Debug, Clone)]
    pub struct BookOfferReq {
        pub offer_id: String,
        pub passengers: Vec<Passenger>,
        pub total_amount: String,
        pub total_currency: String,
    }
    #[derive(Debug, Clone)]
    pub struct Booking {
        pub id: String,
        pub pnr: String,
        pub status: String,
    }
}

const DUFFEL_BASE: &str = "https://api.duffel.com";
const DUFFEL_VERSION: &str = "v2";

#[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
pub fn book_offer(req: BookOfferReq) -> Result<Booking, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let api_key = get_api_key()?;

        let passengers_payload: alloc::vec::Vec<serde_json::Value> = req
            .passengers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                json!({
                    "id": alloc::format!("passenger_{i}"),
                    "given_name": p.given_name,
                    "family_name": p.family_name,
                    "born_on": p.date_of_birth,
                    "passport_number": p.passport_number,
                    "passport_country_code": p.nationality,
                    "passport_expiry_date": p.passport_expiry,
                    "gender": p.gender,
                    "email": p.email,
                    "phone_number": p.phone,
                })
            })
            .collect();

        let order_body = json!({
            "data": {
                "type": "instant",
                "selected_offers": [req.offer_id],
                "passengers": passengers_payload,
                "payments": [{
                    "type": "balance",
                    "amount": req.total_amount,
                    "currency": req.total_currency,
                }]
            }
        });

        let _ = logging::info(&alloc::format!(
            "Calling Duffel POST /air/orders for offer {}",
            req.offer_id
        ));

        let resp = http_iface::call(http_iface::Request {
            method: http_iface::Verb::Post,
            url: alloc::format!("{DUFFEL_BASE}/air/orders"),
            headers: Some(duffel_headers(&api_key)),
            payload: Some(serde_json::to_vec(&order_body).map_err(|e| e.to_string())?),
        })
        .map_err(|e| alloc::format!("duffel create-order: {e}"))?;

        if resp.code != 200 && resp.code != 201 {
            // Log full body inside the enclave; do NOT forward it — it may contain PII.
            let _ = logging::error(&alloc::format!(
                "Duffel create-order HTTP {}: {}",
                resp.code,
                alloc::string::String::from_utf8_lossy(&resp.payload)
            ));
            return Err(alloc::format!(
                "Duffel create-order failed: HTTP {}",
                resp.code
            ));
        }

        let order: serde_json::Value =
            serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;

        let booking_id = order["data"]["id"]
            .as_str()
            .ok_or("Duffel response missing order id")?
            .to_string();
        let pnr = order["data"]["booking_reference"]
            .as_str()
            .ok_or("Duffel response missing booking_reference")?
            .to_string();
        let status = order["data"]["payment_status"]["awaiting_payment"]
            .as_bool()
            .map(|b| if b { "awaiting_payment" } else { "confirmed" })
            .ok_or("Duffel response missing payment_status.awaiting_payment")?
            .to_string();

        let _ = logging::info(&alloc::format!(
            "Duffel order created: id={booking_id} pnr={pnr}"
        ));

        return Ok(Booking {
            id: booking_id,
            pnr,
            status,
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    Err("book_offer is only implemented on the wasm32 target".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_offer_non_wasm_returns_err() {
        let req = BookOfferReq {
            offer_id: "off_abc123".to_string(),
            passengers: vec![Passenger {
                given_name: "Jane".to_string(),
                family_name: "Smith".to_string(),
                date_of_birth: "1990-01-15".to_string(),
                passport_number: "AB1234567".to_string(),
                nationality: "GB".to_string(),
                passport_expiry: "2030-06-01".to_string(),
                gender: "f".to_string(),
                email: "jane@example.com".to_string(),
                phone: "+441234567890".to_string(),
            }],
            total_amount: "199.00".to_string(),
            total_currency: "GBP".to_string(),
        };
        let result = book_offer(req);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }
}
