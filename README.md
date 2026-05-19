# z-tenant-flight

An example [Trinity](https://terminal3.io) tenant contract that books Duffel flights with full PII privacy inside the TEE (Trusted Execution Environment).

## What this is

A working Rust WASM contract that your agent can fork and deploy as its own Trinity z-space contract. The privacy guarantee: the agent hands in a Duffel `offer_id` and receives a PNR — the passenger's passport number, date-of-birth, and full name **never leave the TEE**.

## Prerequisites

- A [Duffel](https://duffel.com) sandbox account and API key.
- Access to a Trinity staging cluster (contact [Terminal-3](https://terminal3.io) for access).
- A tenant DID and signing key.
- Rust toolchain with `wasm32-wasip2` target:
  ```bash
  rustup target add wasm32-wasip2
  ```

## Quick start

```bash
# 1. Fork this repo.
gh repo fork Terminal-3/z-tenant-flight --clone

# 2. Store your Duffel sandbox API key in the KV config map.
#    (Done via the Trinity admin API, not in source code.)
trinity admin kv put z:<your_tid>:travel/contracts config duffel_api_key <your_duffel_key>

# 3. Build the WASM artefact.
cargo build --target wasm32-wasip2 --release

# 4. Register the contract with your tenant.
trinity contract register \
  --name "z:<your_tid>:travel/contracts" \
  --wasm target/wasm32-wasip2/release/z_tenant_flight.wasm \
  --manifest capability-manifest.json

# 5. Invoke the contract from your agent.
trinity invoke store-offer --offer-id off_abc123 --amount 12345 --currency GBP
trinity invoke start-booking \
  --offer-id off_abc123 \
  --passenger-did 0x<user_did> \
  --delegation "$(cat delegation.json)"

# 6. Poll for the PNR once the Duffel webhook fires.
trinity invoke get-booking --booking-id bk-<...>
```

## Architecture

The data flow through the TEE:

```
  agent              z:<tid>:travel/contracts              Duffel sandbox
    │                         │                                 │
    │   store_offer(offer_id) │                                 │
    ├────────────────────────▶│  write cached_offers[offer_id]  │
    ◀──── { ok: offer_id } ───┤                                 │
    │                         │                                 │
    │   start_booking({...})  │                                 │
    ├────────────────────────▶│  1. verify delegation (6 checks)│
    │                         │  2. user-profile.get-fields()   │
    │                         │     [PII never leaves TEE]      │
    │                         │  3. outbox.enqueue Duffel       │
    ◀── { booking_id: Pending}─┤     POST /air/orders            │
    │                         │──────────────────────────────▶  │
    │                         │◀── webhook order.created ───────┤
    │   confirm_booking(pnr)  │                                 │
    │ (webhook relay call) ───▶  write bookings[booking_id].pnr │
    │                         │                                 │
    │   get_booking(id)       │                                 │
    ◀── { pnr, status } ──────┤                                 │
```

The key privacy property is step 2: passenger PII is read from the TEE's
`user-profile` store and injected directly into the outbox request body.
The contract never returns PII across the WIT boundary — the agent only
ever sees `booking_id` and the eventual `pnr`.

## KV maps declared

| Constant               | Map name              | Purpose                                            |
|------------------------|-----------------------|----------------------------------------------------|
| `KV_CACHED_OFFERS`     | `cached_offers`       | Duffel offer → amount / currency / expiry          |
| `KV_BOOKINGS`          | `bookings`            | booking_id → `BookingRow` (status + PNR)           |
| `KV_DELEGATION_NONCES` | `delegation_nonces`   | 16-byte nonce → `"1"` (one-time replay guard)      |
| `KV_CONFIG`            | `config`              | Operator-supplied config (Duffel API key, etc.)    |

All maps are scoped to `z:<tid>:*` by the Trinity KV governor. Cross-tenant
reads/writes are denied at the policy layer — the contract never needs to
enforce that itself.

## Capability manifest

The manifest JSON you pass to `trinity contract register`:

```json
{
  "version": "0.1.0",
  "capabilities": {
    "kv_store": {
      "maps": ["cached_offers", "bookings", "delegation_nonces", "config"]
    },
    "outbox": {
      "allowed_upstream_hosts": ["api.duffel.com"]
    },
    "user_profile": {
      "scopes": ["ProfileRead"],
      "fields": ["passport_number", "date_of_birth", "full_name"]
    },
    "logging": true
  }
}
```

The `user_profile.scopes: ["ProfileRead"]` declaration is required by the
Trinity host to allow the contract to read PII fields. Without it, the
`user-profile` interface returns a permission-denied error.

## Delegation credentials

The `start-booking` function requires the agent to present a
`BookingDelegationCredential` — a JSON object signed by the user's Ethereum
wallet — proving that the user explicitly authorised this specific booking.

The contract verifies six conditions in order:

1. **EIP-191 signature** — the recovered Ethereum address matches `user_did`.
2. **Expiry** — `expires_at_sec > cluster_timestamp_secs`.
3. **Amount cap** — `max_amount_minor >= offer.amount_minor`.
4. **Offer binding** — `offer_id` in the credential matches the request.
5. **Agent DID** — `agent_did` matches the calling session DID.
6. **Nonce replay** — the 16-byte nonce has not been used before.

Credential shape:

```json
{
  "user_did": "0x<20-byte ETH address>",
  "agent_did": "0x<20-byte agent DID>",
  "action": "book-flight",
  "offer_id": "off_abc123",
  "max_amount_minor": 12345,
  "currency": "GBP",
  "expires_at_sec": 1999999999,
  "nonce": [222, 173, 190, 239, 0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187]
}
```

Sign the compact JSON with EIP-191 (`personal_sign`) and include the
65-byte `r||s||v` hex signature in the delegation envelope:

```json
{
  "credential_json": "{...}",
  "signature_hex": "0x<65-byte hex>"
}
```

> **Note:** the EIP-191 signature recovery in `src/delegation.rs` is
> intentionally a stub in this scaffold. Wire in a `secp256k1` crate
> (e.g. `k256`) before deploying. The code comment in `delegation.rs`
> contains the full implementation template.

## Tests

### Unit tests (native, no WASM runtime needed)

```bash
cargo test --lib
```

Tests cover:
- All 6 `DelegationDenyReason` variants.
- `OfferRow` and `BookingRow` serialisation round-trips.
- Stub paths for `confirm_booking` and `get_booking`.
- `CONTRACT_VERSION` semver format.
- Function name constants.

### WASM build

```bash
cargo build --target wasm32-wasip2 --release
```

### Duffel sandbox integration test

Once deployed to Trinity staging:

1. Create a Duffel sandbox offer via the Duffel dashboard or API.
2. Call `store-offer` with the offer_id.
3. Sign a `BookingDelegationCredential` with a test wallet.
4. Call `start-booking` — verify the contract returns `{ status: "Pending" }`.
5. Trigger a Duffel webhook (or wait for the sandbox order to land).
6. Call `get-booking` — verify `pnr` is populated and `status` is `"Confirmed"`.

## License

MIT
