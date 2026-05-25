//! book_offer: calls Duffel create-order API and returns the PNR.

#[derive(serde::Deserialize)]
pub struct Passenger {
    pub title: String,
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

#[derive(serde::Deserialize)]
pub struct BookOfferReq {
    pub offer_id: String,
    pub passengers: Vec<Passenger>,
    pub total_amount: String,
    pub total_currency: String,
}

#[derive(serde::Serialize)]
pub struct Booking {
    pub id: String,
    pub pnr: String,
    pub status: String,
}

const DUFFEL_BASE: &str = "https://api.duffel.com";
const DUFFEL_VERSION: &str = "v2";

/// Entry point called from `lib.rs`. `input` is the raw JSON bytes from the
/// node's `generic-input.input` field.
pub fn book_offer(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: BookOfferReq =
        serde_json::from_slice(input).map_err(|e| alloc::format!("book-offer: bad input: {e}"))?;

    #[cfg(target_arch = "wasm32")]
    {
        let booking = book_offer_wasm(req)?;
        return serde_json::to_vec(&booking).map_err(|e| e.to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = req;
        Err("book_offer is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http as http_iface, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn book_offer_wasm(req: BookOfferReq) -> Result<Booking, String> {
    use serde_json::json;

    let api_key = get_api_key()?;

    let passengers_payload: alloc::vec::Vec<serde_json::Value> = req
        .passengers
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "id": alloc::format!("passenger_{i}"),
                "title": p.title,
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

    let resp = http_iface::call(&http_iface::Request {
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

    Ok(Booking {
        id: booking_id,
        pnr,
        status,
    })
}

#[cfg(target_arch = "wasm32")]
fn get_api_key() -> Result<alloc::string::String, alloc::string::String> {
    let tid = tenant_context::tenant_did();
    let map_name = alloc::format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, b"duffel_api_key")
        .map_err(|e| alloc::format!("kv read: {e}"))?
        .ok_or("duffel_api_key not found in z:<tid>:secrets — populate it via the tenant SDK before use")?;
    alloc::string::String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn duffel_headers(
    api_key: &str,
) -> alloc::vec::Vec<(alloc::string::String, alloc::string::String)> {
    // Content-Type is set automatically by the host HTTP function via
    // .json() — sending it explicitly creates a duplicate that Duffel rejects.
    alloc::vec![
        (
            "Authorization".to_string(),
            alloc::format!("Bearer {api_key}"),
        ),
        ("Duffel-Version".to_string(), DUFFEL_VERSION.to_string()),
        ("Accept".to_string(), "application/json".to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_offer_non_wasm_returns_err() {
        let input = serde_json::to_vec(&serde_json::json!({
            "offer_id": "off_abc123",
            "passengers": [{
                "title": "ms",
                "given_name": "Jane",
                "family_name": "Smith",
                "date_of_birth": "1990-01-15",
                "passport_number": "AB1234567",
                "nationality": "GB",
                "passport_expiry": "2030-06-01",
                "gender": "f",
                "email": "jane@example.com",
                "phone": "+441234567890",
            }],
            "total_amount": "199.00",
            "total_currency": "GBP",
        }))
        .unwrap();
        let result = book_offer(&input);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("only implemented on the wasm32 target"));
    }

    #[test]
    fn book_offer_bad_input_returns_err() {
        let result = book_offer(b"not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }
}
