import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import process from "node:process";
import { ContractFactory, JsonRpcProvider, Wallet } from "ethers";
import solc from "solc";

const files = [
  "HashShipmentAnchor.sol", "VeCroTokenAdapted.sol", "EpochAggregateAnchor.sol",
  "sp1/ISP1Verifier.sol", "sp1/Groth16Verifier.sol", "sp1/SP1VerifierGroth16.sol",
];
const sources = Object.fromEntries(await Promise.all(files.map(async (path) => [
  `contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") },
])));
const output = JSON.parse(solc.compile(JSON.stringify({
  language: "Solidity", sources,
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } },
})));
const errors = output.errors?.filter((entry) => entry.severity === "error") ?? [];
if (errors.length) throw new Error(errors.map((entry) => entry.formattedMessage).join("\n"));
const artifact = (path, name) => output.contracts[`contracts/${path}`][name];
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const signer = new Wallet(
  process.env.ANVIL_PRIVATE_KEY ?? "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  provider,
);
// Fetch directly from Anvil for every transaction. ethers' provider and
// NonceManager can cache a stale nonce across rapid receipts or a reverted
// estimateGas; an explicit on-chain nonce keeps the benchmark moving.
const chainNonce = async () => Number(BigInt(await provider.send(
  "eth_getTransactionCount", [await signer.getAddress(), "latest"],
)));
const auditor = "0x000000000000000000000000000000000000dEaD";
let hashAnchor;
let token;
let aggregate;
let aggregateVkey;
let verifier;

const deploy = async (path, name, ...args) => {
  const a = artifact(path, name);
  const c = await new ContractFactory(a.abi, a.evm.bytecode.object, signer).deploy(...args, { nonce: await chainNonce() });
  await c.waitForDeployment();
  return c;
};
const bytes32 = (value, field) => {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) {
    throw new Error(`${field} must be bytes32 hex`);
  }
  return value.toLowerCase();
};
const receiptData = (tx, receipt) => {
  if (receipt.status !== 1) throw new Error("Anvil transaction failed");
  return {
    transactionHash: receipt.hash,
    gasUsed: receipt.gasUsed.toString(),
    calldataBytes: (tx.data.length - 2) / 2,
    blockNumber: receipt.blockNumber,
  };
};
const reset = async (body) => {
  hashAnchor = await deploy("HashShipmentAnchor.sol", "HashShipmentAnchor");
  token = await deploy("VeCroTokenAdapted.sol", "VeCroTokenAdapted");
  if (body.programVKey || body.initialNullifierRoot) {
    aggregateVkey = bytes32(body.programVKey, "programVKey");
    const root = bytes32(body.initialNullifierRoot, "initialNullifierRoot");
    verifier ??= await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
    aggregate = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor",
      await verifier.getAddress(), aggregateVkey, root);
  } else {
    aggregate = undefined;
    aggregateVkey = undefined;
  }
  return {
    chainId: (await provider.getNetwork()).chainId.toString(),
    hashAnchor: await hashAnchor.getAddress(),
    token: await token.getAddress(),
    aggregate: aggregate ? await aggregate.getAddress() : null,
  };
};
const recordHash = async (body) => {
  if (!hashAnchor) throw new Error("call /reset first");
  const id = BigInt(String(body.shipmentId));
  const digest = bytes32(body.digest, "digest");
  const tx = await hashAnchor.anchor(id, digest, { nonce: await chainNonce() });
  const receipt = await tx.wait();
  if ((await hashAnchor.shipmentDigests(id)).toLowerCase() !== digest) throw new Error("hash readback mismatch");
  return receiptData(tx, receipt);
};
const recordToken = async (body) => {
  if (!token) throw new Error("call /reset first");
  const id = BigInt(String(body.shipmentId));
  const digest = bytes32(body.digest, "digest");
  const mintTx = await token.mint(id, digest, { nonce: await chainNonce() });
  const mintReceipt = await mintTx.wait();
  const transferTx = await token.transfer(id, auditor, { nonce: await chainNonce() });
  const transferReceipt = await transferTx.wait();
  const state = await token.tokens(id);
  if (state.digest.toLowerCase() !== digest || state.owner.toLowerCase() !== auditor.toLowerCase()) {
    throw new Error("token audit readback mismatch");
  }
  const mint = receiptData(mintTx, mintReceipt);
  const transfer = receiptData(transferTx, transferReceipt);
  return {
    transactionHashes: [mint.transactionHash, transfer.transactionHash],
    gasUsed: (BigInt(mint.gasUsed) + BigInt(transfer.gasUsed)).toString(),
    calldataBytes: mint.calldataBytes + transfer.calldataBytes,
    blockNumbers: [mint.blockNumber, transfer.blockNumber],
  };
};
const recordAggregate = async (body) => {
  if (!aggregate) throw new Error("aggregate was not deployed at reset");
  const id = BigInt(String(body.epochId));
  const oldRoot = bytes32(body.oldNullifierRoot, "oldNullifierRoot");
  const newRoot = bytes32(body.newNullifierRoot, "newNullifierRoot");
  const commitment = bytes32(body.epochCommitment, "epochCommitment");
  if (bytes32(body.programVKey, "programVKey") !== aggregateVkey) throw new Error("wrong aggregate vkey");
  const tx = await aggregate.verifyAndAnchor(id, oldRoot, body.publicValues, body.proof, { nonce: await chainNonce() });
  const receipt = await tx.wait();
  if ((await aggregate.nullifierRoot()).toLowerCase() !== newRoot
    || (await aggregate.epochCommitments(id)).toLowerCase() !== commitment) {
    throw new Error("aggregate readback mismatch");
  }
  return receiptData(tx, receipt);
};
const readJSON = async (req) => {
  let body = "";
  for await (const chunk of req) {
    body += chunk;
    if (body.length > 4_000_000) throw new Error("request too large");
  }
  return JSON.parse(body || "{}");
};
const send = (res, code, value) => {
  const payload = `${JSON.stringify(value)}\n`;
  res.writeHead(code, { "content-type": "application/json", "content-length": Buffer.byteLength(payload) });
  res.end(payload);
};
const server = createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/health") {
      send(res, 200, { status: "ok" });
    } else if (req.method === "POST" && req.url === "/reset") {
      send(res, 201, await reset(await readJSON(req)));
    } else if (req.method === "POST" && req.url === "/hash") {
      send(res, 201, await recordHash(await readJSON(req)));
    } else if (req.method === "POST" && req.url === "/token") {
      send(res, 201, await recordToken(await readJSON(req)));
    } else if (req.method === "POST" && req.url === "/aggregate") {
      send(res, 201, await recordAggregate(await readJSON(req)));
    } else {
      send(res, 404, { error: "not found" });
    }
  } catch (error) {
    send(res, 422, { error: error instanceof Error ? error.message : String(error) });
  }
});
const listen = process.env.E3_ANCHOR_LISTEN ?? "127.0.0.1:8547";
const at = listen.lastIndexOf(":");
server.listen(Number(listen.slice(at + 1)), listen.slice(0, at),
  () => console.log(`E3 Anvil server listening on http://${listen}`));
process.on("SIGINT", () => server.close());
process.on("SIGTERM", () => server.close());
