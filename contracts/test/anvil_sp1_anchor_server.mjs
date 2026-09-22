import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import process from "node:process";
import { AbiCoder, ContractFactory, JsonRpcProvider, Wallet, NonceManager } from "ethers";
import solc from "solc";

const listen = process.env.E2_ANCHOR_LISTEN ?? "127.0.0.1:8546";
const splitAt = listen.lastIndexOf(":");
if (splitAt <= 0) throw new Error("E2_ANCHOR_LISTEN must be host:port");
const host = listen.slice(0, splitAt);
const port = Number.parseInt(listen.slice(splitAt + 1), 10);
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("E2_ANCHOR_LISTEN has an invalid port");

const compilerInput = {
  language: "Solidity",
  sources: Object.fromEntries(await Promise.all([
    "EpochAnchor.sol",
    "sp1/ISP1Verifier.sol",
    "sp1/Groth16Verifier.sol",
    "sp1/SP1VerifierGroth16.sol",
  ].map(async (path) => [`contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") }]))),
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } },
};
const compilerOutput = JSON.parse(solc.compile(JSON.stringify(compilerInput)));
const errors = compilerOutput.errors?.filter((entry) => entry.severity === "error") ?? [];
if (errors.length) throw new Error(errors.map((entry) => entry.formattedMessage).join("\n"));
const artifact = (source, contract) => compilerOutput.contracts[source][contract];

const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const signer = new NonceManager(new Wallet(
  process.env.ANVIL_PRIVATE_KEY ?? "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  provider,
));
const coder = AbiCoder.defaultAbiCoder();
let verifier;
let anchor;
let anchorVKey;

const deploy = async (source, contract, ...args) => {
  const compiled = artifact(source, contract);
  const instance = await new ContractFactory(compiled.abi, compiled.evm.bytecode.object, signer).deploy(...args);
  await instance.waitForDeployment();
  return instance;
};

const bytes32 = (value, name) => {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) throw new Error(`${name} must be bytes32 hex`);
  return value.toLowerCase();
};

const deployAnchor = async (initialRoot, programVKey) => {
  const root = bytes32(initialRoot, "initialNullifierRoot");
  const vkey = bytes32(programVKey, "programVKey");
  const configuredVKey = process.env.E2_POLICY_VKEY && bytes32(process.env.E2_POLICY_VKEY, "E2_POLICY_VKEY");
  if (configuredVKey && configuredVKey !== vkey) throw new Error("programVKey does not match E2_POLICY_VKEY");
  verifier ??= await deploy("contracts/sp1/SP1VerifierGroth16.sol", "SP1Verifier");
  anchor = await deploy("contracts/EpochAnchor.sol", "EpochAnchor", await verifier.getAddress(), vkey, root);
  anchorVKey = vkey;
  return anchor;
};

const readJSON = async (request) => {
  let body = "";
  for await (const chunk of request) {
    body += chunk;
    if (body.length > 1_000_000) throw new Error("request body is too large");
  }
  return JSON.parse(body || "{}");
};

const send = (response, status, value) => {
  response.writeHead(status, { "content-type": "application/json" });
  response.end(`${JSON.stringify(value)}\n`);
};

const anchorProof = async (body) => {
  const epochId = BigInt(String(body.epochId));
  const expectedOldRoot = bytes32(body.oldNullifierRoot, "oldNullifierRoot");
  const programVKey = bytes32(body.programVKey, "programVKey");
  if (typeof body.publicValues !== "string" || typeof body.proof !== "string") {
    throw new Error("publicValues and proof must be hex strings");
  }
  const [proofEpochId, proofOldRoot, proofNewRoot] = coder.decode(
    ["uint64", "bytes32", "bytes32"],
    body.publicValues,
  );
  if (proofEpochId !== epochId || proofOldRoot.toLowerCase() !== expectedOldRoot) {
    throw new Error("requested epoch or old root disagrees with SP1 public values");
  }
  if (!anchor) await deployAnchor(proofOldRoot, programVKey);
  if (anchorVKey !== programVKey) throw new Error("programVKey changed after the anchor was deployed");
  const currentRoot = (await anchor.nullifierRoot()).toLowerCase();
  if (currentRoot !== expectedOldRoot) throw new Error("anchor nullifier root does not match the next proof");

  const receipt = await (await anchor.verifyAndAnchor(epochId, expectedOldRoot, body.publicValues, body.proof)).wait();
  if (receipt.status !== 1) throw new Error("Anvil transaction was not mined successfully");
  const newRoot = (await anchor.nullifierRoot()).toLowerCase();
  if (newRoot !== proofNewRoot.toLowerCase()) throw new Error("Anvil did not apply the proved nullifier root");
  return {
    epochId: epochId.toString(),
    transactionHash: receipt.hash,
    gasUsed: receipt.gasUsed.toString(),
    newNullifierRoot: newRoot,
  };
};

const server = createServer(async (request, response) => {
  try {
    if (request.method === "GET" && request.url === "/health") {
      send(response, 200, { status: "ok", anchor: anchor ? await anchor.getAddress() : null });
    } else if (request.method === "POST" && request.url === "/reset") {
      const body = await readJSON(request);
      await deployAnchor(body.initialNullifierRoot, body.programVKey);
      send(response, 201, { status: "reset", anchor: await anchor.getAddress() });
    } else if (request.method === "POST" && request.url === "/anchor") {
      send(response, 201, await anchorProof(await readJSON(request)));
    } else {
      send(response, 404, { error: "not found" });
    }
  } catch (error) {
    send(response, 422, { error: error instanceof Error ? error.message : String(error) });
  }
});

server.listen(port, host, () => console.log(`E2 Anvil anchor server listening on http://${listen}`));
process.on("SIGTERM", () => server.close());
process.on("SIGINT", () => server.close());
