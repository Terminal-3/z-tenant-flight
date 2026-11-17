# z-tenant-flight

Let an AI agent book flights for a customer through the [Duffel](https://duffel.com) API, without the agent ever seeing who is flying.

A Rust contract that runs inside the Terminal 3 (T3N) enclave under your own tenant. The agent asks for a search and a booking; the customer's name, date of birth, contact details and passport stay inside the enclave and only go to Duffel. It builds on the [Terminal 3 Get Started guide](https://docs.terminal3.io/developers/adk/get-started/quickstart): same key, same organisation, same agent.

| Function | Who calls it | What it does |
|---|---|---|
| `save-traveller` | The customer, for themselves | Saves their traveller record (title, phone, passport, and optionally name, date of birth, gender, email) in `z:<tid>:travellers` |
| `forget-traveller` | The customer, for themselves | Deletes their traveller record |
| `search-offers` | The agent | Searches Duffel for flights. No personal data involved |
| `book-offer` | The agent | Books an offer for the customer and returns `{ id, pnr, status }` |

The agent's `book-offer` input carries no personal data, only the offer it chose:

```json
{ "offer_id": "off_…", "passenger_id": "pas_…", "total_amount": "199.00", "total_currency": "GBP" }
```

## Where the passenger's details come from

`book-offer` builds the Duffel passenger from three sources, all inside the enclave:

1. **The traveller record** the customer saved with `save-traveller`: title, phone number and, normally, the passport.
2. **The customer's T3N profile**, for any of name, date of birth, gender and email the record leaves out. The contract sends `{{profile.*}}` markers and the node fills them in on the way to Duffel, so those values never enter the contract at all.
3. **Optionally, a passport sealed with the [Private Doc Scanner](https://github.com/Terminal-3/private-doc-scanner)**: pass `"scanner_request_id": "<the scanner reference>"` to `book-offer` and the passport comes from the customer's scanned, MRZ-verified passport instead of the record.

The contract refuses an expired passport, a scanned passport whose number, expiry or issuing country failed the scanner's checks or was edited by hand, and a scanned record that belongs to someone else.

### Bring your own data

You don't need the scanner. Save a traveller record in this format ([`examples/traveller.json`](examples/traveller.json), all values fictional):

```json
{
  "title": "ms",
  "phone_number": "+442071234567",
  "passport": { "number": "000000000", "issuing_country": "GB", "expires_on": "2031-04-11" },
  "given_name": "Alex",
  "family_name": "Example",
  "born_on": "1990-04-12",
  "gender": "f",
  "email": "alex@example.com"
}
```

| Field | Required | Format |
|---|---|---|
| `title` | yes | `mr`, `ms`, `mrs`, `miss` or `dr` |
| `phone_number` | yes | International format, e.g. `+442071234567` |
| `passport.number` | unless you use the scanner | As printed |
| `passport.issuing_country` | unless you use the scanner | ISO 3166 code, 2 or 3 letters (`GB` or `GBR`) |
| `passport.expires_on` | unless you use the scanner | `YYYY-MM-DD` |
| `given_name`, `family_name` | no | Left out: taken from your T3N profile |
| `born_on` | no | `YYYY-MM-DD`. Left out: taken from your T3N profile |
| `gender` | no | `m` or `f`. Left out: taken from your T3N profile |
| `email` | no | Left out: your T3N profile's verified email |

To rely on your T3N profile for the personal fields, use [`examples/traveller-profile.json`](examples/traveller-profile.json), which has only title, phone and passport. If your profile lacks one of those fields, `book-offer` fails with `user profile missing field: <field>`; add the field to the record instead.

### Use the Private Doc Scanner

Scan your passport with the [Private Doc Scanner](https://github.com/Terminal-3/private-doc-scanner), signed in with the same Terminal 3 account, so its vault (`z:<tid>:doc-intake` and its `intake` map) sits in the same tenant as this contract. Run setup with `--with-scanner` so this contract may read the vault, then book with `scanner_request_id`. If you re-run the scanner's own setup and it registers a new version, it resets the vault's readers: run `npm run flight setup --with-scanner` again afterwards. You still need a traveller record for title and phone; [`examples/traveller-profile.json`](examples/traveller-profile.json) without the `passport` block is enough.

[`examples/scanner-record.json`](examples/scanner-record.json) shows the shape the scanner stores, if you want to test this path with sample data.

## Run it

You need:
- the [Get Started](https://docs.terminal3.io/developers/adk/get-started/quickstart) setup: your `T3N_API_KEY`, and an organisation agent's `AGENT_DID` and `AGENT_API_KEY` ([Create Your Organisation](https://docs.terminal3.io/developers/adk/get-started/create-organisation));
- a Duffel **test** token (`duffel_test_…`) from your Duffel dashboard in test mode;
- Rust with the `wasm32-wasip2` target, and Node.js 22.

```bash
cargo build --release                      # target/wasm32-wasip2/release/z_tenant_flight.wasm
cd starter && npm install
cp .env.example .env                       # then fill it in, in your editor, never in a chat
npm run flight setup                       # register, create maps, seal the Duffel key
npm run flight save-traveller              # as the customer: ../examples/traveller.json
npm run flight grant                       # let your agent search and book (egress: api.duffel.com)
npm run flight search LHR JFK 2026-12-15   # as the agent
npm run flight book <offer_id> <passenger_id> <total_amount> <total_currency>
```

`setup` registers the contract at the tail `flight`, creates the private `secrets` and `travellers` maps for it, and seals your Duffel key in `secrets`; it's safe to re-run. In this walkthrough you are both the customer and the developer, as in the Terminal 3 guide; in production the customer signs in with their own key and runs `save-traveller` themselves.

Take `offer_id`, `passenger_ids[0]`, `total_amount` and `total_currency` from a search result. Duffel test mode creates test orders only.

## Build and test

```bash
cargo test --target x86_64-unknown-linux-gnu   # .cargo/config.toml defaults the target to wasm32-wasip2
cargo build --release
```

The host interfaces in `wit/deps/` are vendored from the Trinity release `testnet-v1.0.12` (`host:interfaces@2.4.0`, `host:tenant@2.0.0`). Importing `http-with-placeholders` puts the contract in the `tenant-http-placeholders` world.
