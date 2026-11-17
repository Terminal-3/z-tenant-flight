# z-tenant-flight

Let an AI agent book flights for a customer through the [Duffel](https://duffel.com) API, without the agent ever seeing who is flying.

A Rust contract that runs inside the Terminal 3 (T3N) enclave under your own tenant. The agent asks for a search and a booking; the customer's name, email, phone and passport details stay inside the enclave and only go to Duffel. It builds on the [Terminal 3 Get Started guide](https://docs.terminal3.io/developers/adk/get-started/quickstart): same key, same organisation, same agent.

## At a glance

- **What it does:** your agent searches Duffel for flights and books one for a customer. It only ever sees offers and the booking reference; the customer's details go from the enclave straight to Duffel.
- **What it adds to the Get Started setup:** outbound HTTP to a real API, an API key sealed in the enclave (`z:<tid>:secrets`), a grant that allows exactly one host (`api.duffel.com`), and `{{profile.*}}` placeholders for the customer's name and email. It's a second contract, `z:<tid>:flight`, in the same tenant as the walkthrough's `vault`, called by the same organisation agent.
- **What you need:** the `.env` from Get Started (`T3N_API_KEY`, `AGENT_DID`, `AGENT_API_KEY`), a free Duffel **test** token, Rust with `wasm32-wasip2`, Node.js 22, and test credits (registering costs about as much as the walkthrough's `vault` did). The passport comes from the [Private Doc Scanner](https://github.com/Terminal-3/private-doc-scanner) (recommended) or a JSON file the customer fills in; sample data is in [`examples/`](examples/).
- **Build ideas:**
  - the same shape for hotels, rail or car hire: another API, another host in the grant;
  - the agent searches, the customer approves, then the agent books;
  - a per-agent spend limit checked in your own map before `book-offer`;
  - loyalty numbers or seat preferences stored in the traveller record.
- **Docs:** [Flight Booking](https://docs.terminal3.io/developers/adk/use-cases/flight-booking).

| Function | Who calls it | What it does |
|---|---|---|
| `save-traveller` | The customer, for themselves | Saves their traveller record (title, phone, and their passport unless it comes from the scanner) in `z:<tid>:travellers` |
| `forget-traveller` | The customer, for themselves | Deletes their traveller record |
| `search-offers` | The agent | Searches Duffel for flights. No personal data involved |
| `book-offer` | The agent | Books an offer for the customer and returns `{ id, pnr, status }` |

The agent's `book-offer` input carries no personal data, only the offer it chose:

```json
{ "offer_id": "off_…", "passenger_id": "pas_…", "total_amount": "199.00", "total_currency": "GBP" }
```

## Where the passenger's details come from

| Duffel needs | Comes from |
|---|---|
| Given name, family name, email | The customer's **T3N profile**. The contract sends `{{profile.first_name}}`, `{{profile.last_name}}` and `{{profile.verified_contacts.email.value}}`, and the node fills them in on the way to Duffel, so these values never enter the contract. |
| Title, phone number | The customer's **traveller record**, which they save for themselves with `save-traveller`. |
| Passport number, issuing country, expiry, date of birth, sex | **Recommended:** the passport the customer scanned with the [Private Doc Scanner](https://github.com/Terminal-3/private-doc-scanner). **Or** the `passport` block of their traveller record. |

The T3N profile holds only the customer's name and email, so the customer supplies everything else. Date of birth and sex (Duffel's `born_on` and `gender`) are read from the passport. If the profile is missing a name or a verified email, `book-offer` fails with `user profile missing field: <field>`.

The contract refuses an expired passport, a scanned passport whose number, expiry, issuing country, date of birth or sex failed the scanner's checks or was edited by hand, a sex other than M or F (Duffel accepts only those), and a scanned record that belongs to someone else.

### Recommended: use the Private Doc Scanner for the passport

The scanner reads the passport on the customer's phone, checks its machine-readable zone and seals the reviewed details to their vault. Nobody types the number, and the contract only accepts fields the scanner verified.

1. Scan the passport with the [Private Doc Scanner](https://github.com/Terminal-3/private-doc-scanner), signed in with the **same Terminal 3 account**, so its vault (`z:<tid>:doc-intake` and its `intake` map) is in the same tenant as this contract.
2. Run `npm run flight -- setup --with-scanner`, which lets this contract read the vault's `intake` map. If you later re-run the scanner's own setup and it registers a new version, it resets that map's readers: run this again afterwards.
3. Save a traveller record without a passport: [`examples/traveller-scanner.json`](examples/traveller-scanner.json).
4. Book with `scanner_request_id` set to the scanner's reference for that passport.

[`examples/scanner-record.json`](examples/scanner-record.json) shows what the scanner stores, if you want to see the fields the contract reads.

### Or enter the passport yourself

Without the scanner, the customer types their details into a JSON file and seals it to the contract. [`examples/traveller.json`](examples/traveller.json) is sample seed data (fictional values) you can seal as-is to try the flow:

```json
{
  "title": "ms",
  "phone_number": "+442071234567",
  "passport": {
    "number": "000000000",
    "issuing_country": "GB",
    "expires_on": "2031-04-11",
    "date_of_birth": "1990-04-12",
    "sex": "F"
  }
}
```

To use real details:

1. Copy the sample into the starter folder: `cp examples/traveller.json starter/my-traveller.json`. The starter's `.gitignore` covers `my-traveller*.json`, so it can't be committed.
2. Open `starter/my-traveller.json` in your own editor and replace the values, using the formats below. **Do it yourself, not through an AI assistant:** anything an assistant opens or writes ends up in its chat.
3. From `starter/`, run `npm run flight save-traveller my-traveller.json`. It signs in with your key and calls `save-traveller`, which checks the fields and seals the record in `z:<tid>:travellers` under your DID.
4. **Delete the file** once you see `saved: true`: `rm my-traveller.json`. The sealed copy is the one the contract uses, and a copy left on disk is one an AI assistant working in this folder could read. To change anything later, write a new file, save it again (it replaces the old record), and delete it again. `forget-traveller` deletes the sealed record.

| Field | Required | Format |
|---|---|---|
| `title` | yes | `mr`, `ms`, `mrs`, `miss` or `dr` |
| `phone_number` | yes | International format, e.g. `+442071234567` |
| `passport.number` | unless you use the scanner | As printed |
| `passport.issuing_country` | unless you use the scanner | ISO 3166 code, 2 or 3 letters (`GB` or `GBR`) |
| `passport.expires_on` | unless you use the scanner | `YYYY-MM-DD` |
| `passport.date_of_birth` | unless you use the scanner | `YYYY-MM-DD` |
| `passport.sex` | unless you use the scanner | `M` or `F`, as printed |

No other fields are accepted: name and email always come from the T3N profile.

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
npm run flight save-traveller              # as the customer: ../examples/traveller.json (or traveller-scanner.json)
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
