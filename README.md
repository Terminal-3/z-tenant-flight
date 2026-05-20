# z-tenant-flight

Duffel flight booking showcase for Trinity z-space tenants — v0.2.0.

A Rust WASM contract that runs inside the Trinity TEE (Trusted Execution Environment) and calls the [Duffel](https://duffel.com) API synchronously via `host:interfaces/http`.

## What this is

Two contract functions exposed over WIT:

| Function | What it does |
|---|---|
| `search-offers` | POST to Duffel `/air/offer-requests`, then GET `/air/offers` — returns a list of available flights |
| `book-offer` | POST to Duffel `/air/orders` with full passenger PII — returns the booking ID and PNR |

Privacy guarantee: passenger PII (passport number, date-of-birth, full name) is passed in by the agent and used inside the enclave to call Duffel. Only the booking ID and PNR cross the WIT boundary back to the caller.

## Host-capability manifest

Declare in your contract manifest:

```json
{ "host_capabilities": ["kv_store", "logging", "tenant_context", "http"] }
```

The `http` capability selects the `tenant-http` linker world (MAT-1571), which also includes the `state` and `secret` interfaces used to store the Duffel API key.

## Setup: storing the Duffel API key

Before first use, store your Duffel API key in the TEE secret store. The key is never in source code or KV maps — it lives in the encrypted secret store scoped to your tenant.

```bash
# Via Trinity admin tooling:
trinity admin secret put duffel_api_key duffel_test_your_key_here

# Or via the put-secret WIT function in a one-time setup call.
```

## Building

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

The WASM artefact will be at `target/wasm32-wasip2/release/z_tenant_flight.wasm`.

## Running tests (native)

```bash
cargo test --lib
cargo clippy --all-targets -- -D warnings
```

## Contract functions

### `search-offers`

```wit
search-offers: func(req: search-offers-req) -> result<search-offers-resp, string>;
```

Input:

```json
{
  "origin": "LHR",
  "destination": "JFK",
  "departure_date": "2026-07-15",
  "cabin_class": "economy",
  "adult_count": 1
}
```

Returns a list of `offer` records, each with `id`, `total_amount`, `total_currency`, and `expires_at`.

### `book-offer`

```wit
book-offer: func(req: book-offer-req) -> result<booking, string>;
```

Input:

```json
{
  "offer_id": "off_abc123",
  "passenger": {
    "given_name": "Jane",
    "family_name": "Smith",
    "date_of_birth": "1990-01-15",
    "passport_number": "AB1234567",
    "nationality": "GB",
    "passport_expiry": "2030-06-01",
    "gender": "f",
    "email": "jane@example.com",
    "phone": "+441234567890"
  }
}
```

Returns `{ "id": "ord_...", "pnr": "ABC123", "status": "confirmed" }`.

## Architecture

```
  agent                       TEE (z-space contract)              Duffel
    |                                  |                             |
    |  search-offers(origin, dest, ...) |                             |
    |----------------------------------------->  POST /air/offer-requests  |
    |                                  |----------------------------> |
    |                                  |<-- { offer_request_id } -----|
    |                                  |  GET /air/offers?id=...      |
    |                                  |----------------------------> |
    |                                  |<-- [ offer, offer, ... ] ----|
    |<--- { offers: [...] } -----------|                             |
    |                                  |                             |
    |  book-offer(offer_id, passenger) |                             |
    |----------------------------------------->  POST /air/orders        |
    |  [PII enters TEE here]           |----------------------------> |
    |  [PII never returned]            |<-- { id, pnr, status } ------|
    |<--- { id, pnr, status } ---------|                             |
```

## Depends on

- MAT-1571: `tenant-http` linker world — provides `host:interfaces/http` and `host:interfaces/secret` to z-space contracts at runtime.
