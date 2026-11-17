//! book_offer: books a Duffel offer for the calling customer and returns the PNR.
//!
//! The input carries no passenger details. Name and email are `{{profile.*}}`
//! markers the host fills from the customer's T3N profile, so those values
//! never enter this contract. Title and phone come from the customer's
//! traveller record (`save-traveller`); the passport, with date of birth and
//! sex, from that record or from a passport they sealed with the Private Doc
//! Scanner. Only the booking id, PNR and status come back.

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookOfferReq {
    pub offer_id: String,
    /// Opaque Duffel passenger id from search-offers. Not PII.
    pub passenger_id: String,
    pub total_amount: String,
    pub total_currency: String,
    /// The Private Doc Scanner reference of a sealed passport to use instead
    /// of the passport in the traveller record.
    #[serde(default)]
    pub scanner_request_id: Option<String>,
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
        serde_json::to_vec(&booking).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        drop(req);
        Err("book_offer is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http_with_placeholders as hwp, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn book_offer_wasm(req: BookOfferReq) -> Result<Booking, String> {
    use crate::traveller;
    use serde_json::json;

    let caller = tenant_context::calling_user_did()
        .ok_or("book-offer: no calling customer; call through an authenticated T3N session")?;
    let caller_hex = hex::encode(&caller);
    let tid = hex::encode(tenant_context::tenant_did());

    let traveller_bytes = kv_store::get(
        &alloc::format!("z:{tid}:{}", traveller::TRAVELLERS_MAP_TAIL),
        caller_hex.as_bytes(),
    )?
    .ok_or("book-offer: no traveller record for this customer; the customer saves one with save-traveller")?;
    let t: traveller::Traveller = serde_json::from_slice(&traveller_bytes)
        .map_err(|_| "book-offer: stored traveller record could not be read".to_string())?;

    let passport = match req.scanner_request_id.as_deref() {
        Some(id) => {
            let key = alloc::format!("req:{id}");
            let record = kv_store::get(
                &alloc::format!("z:{tid}:{}", traveller::SCANNER_INTAKE_MAP_TAIL),
                key.as_bytes(),
            )
            .map_err(|e| alloc::format!("book-offer: reading the scanner vault: {e}"))?
            .ok_or("book-offer: no scanned passport found for this reference")?;
            traveller::passport_from_scanner(&record, &caller_hex)?
        }
        None => t.passport.clone().ok_or(
            "book-offer: the traveller record has no passport; save one with save-traveller or pass scanner_request_id",
        )?,
    };
    let passenger = traveller::passenger(
        &req.passenger_id,
        &t,
        &passport,
        traveller::today(tenant_context::cluster_timestamp_secs()),
    )?;

    let api_key = get_api_key()?;
    let order_body = json!({
        "data": {
            "type": "instant",
            "selected_offers": [req.offer_id],
            "passengers": [passenger],
            "payments": [{
                "type": "balance",
                "amount": req.total_amount,
                "currency": req.total_currency,
            }]
        }
    });

    logging::info("book-offer: calling Duffel POST /air/orders")?;

    let resp = hwp::call(&hwp::Request {
        method: hwp::Verb::Post,
        url: alloc::format!("{DUFFEL_BASE}/air/orders"),
        headers: Some(duffel_headers(&api_key)),
        payload: Some(serde_json::to_vec(&order_body).map_err(|e| e.to_string())?),
    })
    .map_err(|e| alloc::format!("duffel create-order: {}", format_http_error(e)))?;

    if resp.code != 200 && resp.code != 201 {
        // The response body can echo passenger details, so only the status
        // is logged or returned.
        logging::error(&alloc::format!("book-offer: Duffel create-order HTTP {}", resp.code))?;
        return Err(alloc::format!("Duffel create-order failed: HTTP {}", resp.code));
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

    logging::info("book-offer: Duffel order created")?;

    Ok(Booking {
        id: booking_id,
        pnr,
        status,
    })
}

/// Render a typed `http-with-placeholders` error as a contract-facing string.
/// Never includes resolved PII — only field names and host-side reasons.
#[cfg(target_arch = "wasm32")]
fn format_http_error(e: hwp::HttpError) -> alloc::string::String {
    match e {
        hwp::HttpError::EgressDenied(host) => alloc::format!("egress denied for host {host}"),
        hwp::HttpError::PlaceholderDenied(marker) => {
            alloc::format!("placeholder not permitted: {marker}")
        }
        hwp::HttpError::PlaceholderUnknown(field) => {
            alloc::format!("user profile missing field: {field}")
        }
        hwp::HttpError::PlaceholderNoUserContext => {
            "no user context bound for placeholder resolution".to_string()
        }
        hwp::HttpError::UpstreamError(reason) => alloc::format!("upstream: {reason}"),
    }
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
            "passenger_id": "pas_abc123",
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

    #[test]
    fn book_offer_rejects_inline_pii_fields() {
        let input = serde_json::to_vec(&serde_json::json!({
            "offer_id": "off_abc123",
            "passengers": [{ "given_name": "Jane" }],
            "total_amount": "199.00",
            "total_currency": "GBP",
        }))
        .unwrap();
        let result = book_offer(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad input"));
    }
}
