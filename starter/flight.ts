/**
 * Run z-tenant-flight on your own T3N tenant, on top of the Terminal 3 Get Started setup.
 *
 *   npm run flight setup [--with-scanner]       register the contract, create its maps, seal the Duffel key
 *   npm run flight save-traveller [file.json]   save your traveller record (default ../examples/traveller.json)
 *   npm run flight grant                        let your org agent call search-offers and book-offer
 *   npm run flight search LHR JFK 2026-12-15    search as the agent
 *   npm run flight book <offer_id> <passenger_id> <amount> <currency> [scanner_request_id]
 *
 * Keys come from .env (git-ignored) or the environment and are never printed.
 */
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import {
  T3nClient,
  TenantClient,
  setEnvironment,
  loadWasmComponent,
  resolveTrustAnchor,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  getNodeUrl,
  getContractVersion,
  fanOutGrant,
  invoke,
  type BoundGrant,
} from "@terminal3/t3n-sdk";

const TAIL = "flight";
const VERSION = "0.6.0";
const WASM = new URL("../target/wasm32-wasip2/release/z_tenant_flight.wasm", import.meta.url);
const DUFFEL_HOST = "api.duffel.com";

function need(name: string): string {
  const v = process.env[name]?.trim();
  if (!v) throw new Error(`${name} is not set. Add it to starter/.env (see .env.example); never paste it into a chat.`);
  return v;
}

async function signIn() {
  setEnvironment("testnet");
  const key = need("T3N_API_KEY");
  const address = eth_get_address(key);
  const t3n = new T3nClient({
    trustAnchor: await resolveTrustAnchor("testnet"),
    wasmComponent: await loadWasmComponent(),
    handlers: { EthSign: metamask_sign(address, undefined, key) },
  });
  await t3n.handshake();
  const tenantDid = (await t3n.authenticate(createEthAuthInput(address))).value;
  const tenant = new TenantClient({ t3n, baseUrl: getNodeUrl(), tenantDid });
  const contract = `z:${tenantDid.slice("did:t3n:".length)}:${TAIL}`;
  return { t3n, tenant, tenantDid, contract };
}

async function deployedVersion(contract: string): Promise<string | null> {
  try {
    return await getContractVersion(getNodeUrl(), contract);
  } catch {
    return null; // not registered yet
  }
}

async function createMap(tenant: TenantClient, tail: string, contractIds: number[]) {
  const acl = { readers: { only: contractIds }, writers: { only: contractIds } };
  try {
    await tenant.maps.create({ tail, visibility: "private", ...acl });
  } catch (err) {
    if (!/already.?exists/i.test(String((err as Error).message))) throw err;
    await tenant.maps.update(tail, acl);
  }
  console.log(`map ${tenant.canonicalName(tail)} → contract ${contractIds.join(", ")}`);
}

async function setup(withScanner: boolean) {
  const { tenant, contract } = await signIn();
  let id = Number(process.env.FLIGHT_CONTRACT_ID) || undefined;
  const deployed = await deployedVersion(contract);
  if (deployed !== VERSION || id === undefined) {
    if (!existsSync(WASM)) throw new Error("Build the contract first: cargo build --release (from the repo root).");
    const result = await tenant.contracts.register({ tail: TAIL, version: VERSION, wasm: readFileSync(WASM) });
    id = result.contract_id;
    console.log(`registered ${contract} ${VERSION} as contract id ${id}`);
    if (existsSync(".env")) appendFileSync(".env", `\nFLIGHT_CONTRACT_ID=${id}\n`);
  } else {
    console.log(`${contract} ${VERSION} already registered (contract id ${id})`);
  }

  await createMap(tenant, "secrets", [id]);
  await createMap(tenant, "travellers", [id]);
  await tenant.maps.entrySet("secrets", "duffel_api_key", need("DUFFEL_API_KEY"));
  console.log("Duffel key sealed in secrets (not shown)");

  if (withScanner) {
    // The Private Doc Scanner records its own contract id in its config map.
    const scannerId = Number(await tenant.maps.entryGet("config", "contract_id"));
    if (!scannerId) throw new Error("No Private Doc Scanner vault found in this tenant: run its setup-vault first.");
    await tenant.maps.update("intake", { readers: { only: [scannerId, id] } });
    console.log(`scanner vault ${tenant.canonicalName("intake")} readable by contract ${id}`);
  }
}

async function saveTraveller(file: string) {
  const { t3n, contract } = await signIn();
  const out = await t3n.executeAndDecode({
    contract_id: contract,
    contract_version: VERSION,
    function_name: "save-traveller",
    input: JSON.parse(readFileSync(file, "utf8")),
  });
  console.log("save-traveller:", out);
}

async function grant() {
  const { t3n, contract } = await signIn();
  const agentDid = need("AGENT_DID");
  await t3n.updateMemberDelegation(
    fanOutGrant<BoundGrant>(
      { grantee: agentDid, contract_id: contract, scopes: [], allowed_hosts: [DUFFEL_HOST] },
      ["search-offers", "book-offer"],
    ),
  );
  console.log(`granted ${agentDid} → ${contract}: search-offers, book-offer (egress ${DUFFEL_HOST})`);
}

async function asAgent(functionName: string, input: unknown) {
  const { tenantDid, contract } = await signIn();
  return invoke({
    baseUrl: getNodeUrl(),
    apiKey: need("AGENT_API_KEY"),
    request: { contract_id: contract, contract_version: VERSION, function_name: functionName, pii_did: tenantDid, input },
  });
}

const [cmd, ...args] = process.argv.slice(2);
switch (cmd) {
  case "setup":
    await setup(args.includes("--with-scanner"));
    break;
  case "save-traveller":
    await saveTraveller(args[0] ?? "../examples/traveller.json");
    break;
  case "grant":
    await grant();
    break;
  case "search": {
    const [origin, destination, departure_date] = args;
    if (!departure_date) throw new Error("usage: search <ORIGIN> <DESTINATION> <YYYY-MM-DD>");
    const out = await asAgent("search-offers", { origin, destination, departure_date, cabin_class: "economy", adult_count: 1 });
    console.log(JSON.stringify(out, null, 2));
    break;
  }
  case "book": {
    const [offer_id, passenger_id, total_amount, total_currency, scanner_request_id] = args;
    if (!total_currency) throw new Error("usage: book <offer_id> <passenger_id> <amount> <currency> [scanner_request_id]");
    const input = { offer_id, passenger_id, total_amount, total_currency, ...(scanner_request_id && { scanner_request_id }) };
    console.log(JSON.stringify(await asAgent("book-offer", input), null, 2));
    break;
  }
  default:
    console.log("commands: setup [--with-scanner] | save-traveller [file] | grant | search <from> <to> <date> | book <offer> <passenger> <amount> <currency> [scanner_request_id]");
    process.exitCode = cmd ? 2 : 0;
}
