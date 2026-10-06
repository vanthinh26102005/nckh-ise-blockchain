import { createServer } from "node:http";
import process from "node:process";
import { JsonRpcProvider } from "ethers";
import { bytes32, compileContracts, deployer, localSigner, readPolicyManifest, readTrustedProgramKeys, registerPolicies,
  decodeValues, requireAllowed, validationMetadata, anchorEvent } from "./policy_v2.mjs";

const policies = await readPolicyManifest();
const trustedKeys = await readTrustedProgramKeys();
const { leaf: trustedLeafKey, aggregate: trustedEpochKey, leafDigest: trustedLeafDigest, aggregateDigest: trustedAggregateDigest } = trustedKeys;
const requireLeafKey = value => {
  if (!trustedLeafKey) throw new Error("proof endpoints require a trusted startup leaf program key");
  if (value !== trustedLeafKey) throw new Error("programVKey does not match the trusted startup leaf program key");
};

const listen = process.env.E2_ANCHOR_LISTEN ?? "127.0.0.1:8546";
const splitAt = listen.lastIndexOf(":");
if (splitAt <= 0) throw new Error("E2_ANCHOR_LISTEN must be host:port");
const host = listen.slice(0, splitAt);
const port = Number.parseInt(listen.slice(splitAt + 1), 10);
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("E2_ANCHOR_LISTEN has an invalid port");

const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const deploy = deployer(await compileContracts(), await localSigner(provider));
// Only trusted startup configuration registers roots, before any request is served.
const registry = await registerPolicies(deploy, policies);
const registryAddress = await registry.getAddress();
let verifier;
let anchor;
let anchorVKey;
let epochAnchor;
let epochVKey;
let shipmentHashAnchor;
let adaptedTokenAnchor;
let adaptedPolicyVKey;

const deployAnchor = async (initialRoot, programVKey) => {
  const root = bytes32(initialRoot, "initialNullifierRoot");
  const vkey = bytes32(programVKey, "programVKey");
  requireLeafKey(vkey);
  verifier ??= await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
  anchor = await deploy("EpochAnchor.sol", "EpochAnchor", await verifier.getAddress(), vkey, root, registryAddress);
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
  const body = `${JSON.stringify(value)}\n`;
  response.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(body) });
  response.end(body);
};

const anchorProof = async (body) => {
  const epochId = BigInt(String(body.epochId));
  const expectedOldRoot = bytes32(body.oldNullifierRoot, "oldNullifierRoot");
  const programVKey = bytes32(body.programVKey, "programVKey");
  if (typeof body.publicValues !== "string" || typeof body.proof !== "string") {
    throw new Error("publicValues and proof must be hex strings");
  }
  const values = decodeValues(body.publicValues);
  requireAllowed(values, "leaf", policies);
  const [proofEpochId, proofOldRoot, proofNewRoot] = values;
  if (proofEpochId !== epochId || proofOldRoot.toLowerCase() !== expectedOldRoot) {
    throw new Error("requested epoch or old root disagrees with SP1 public values");
  }
  if (!anchor) throw new Error("anchor has not been reset before proof submission");
  if (anchorVKey !== programVKey) throw new Error("programVKey changed after the anchor was deployed");
  const currentRoot = (await anchor.nullifierRoot()).toLowerCase();
  if (currentRoot !== expectedOldRoot) throw new Error("anchor nullifier root does not match the next proof");

  const tx = await anchor.verifyAndAnchor(epochId, expectedOldRoot, body.publicValues, body.proof);
  const receipt = await tx.wait();
  if (receipt.status !== 1) throw new Error("Anvil transaction was not mined successfully");
  const newRoot = (await anchor.nullifierRoot()).toLowerCase();
  if (newRoot !== proofNewRoot.toLowerCase()) throw new Error("Anvil did not apply the proved nullifier root");
  return {
    ...validationMetadata(values), programVKey, status: receipt.status, registry: registryAddress,
    anchor: await anchor.getAddress(), anchorEvent: anchorEvent(anchor, receipt, "EpochAnchored"),
    epochId: epochId.toString(),
    calldataBytes: (tx.data.length - 2) / 2,
    transactionHash: receipt.hash,
    gasUsed: receipt.gasUsed.toString(),
    newNullifierRoot: newRoot,
  };
};

const deployEpochAnchor = async (body) => {
  const root = bytes32(body.initialNullifierRoot, "initialNullifierRoot");
  const vkey = bytes32(body.programVKey, "programVKey");
  const leafDigest = bytes32(body.leafVKeyDigest, "leafVKeyDigest");
  const aggregateDigest = bytes32(body.aggregateVKeyDigest, "aggregateVKeyDigest");
  if (!trustedEpochKey || !trustedLeafDigest || !trustedAggregateDigest) throw new Error("aggregate proof endpoints require trusted startup program and child keys");
  if (vkey !== trustedEpochKey || leafDigest !== trustedLeafDigest || aggregateDigest !== trustedAggregateDigest) throw new Error("aggregate keys do not match trusted startup configuration");
  verifier ??= await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
  epochAnchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor",
    await verifier.getAddress(), vkey, leafDigest, aggregateDigest, root, registryAddress);
  epochVKey = vkey;
  return epochAnchor;
};

const anchorEpochProof = async (body) => {
  const epochId = BigInt(String(body.epochId));
  const expectedRoot = bytes32(body.oldNullifierRoot, "oldNullifierRoot");
  const vkey = bytes32(body.programVKey, "programVKey");
  if (!epochAnchor || epochVKey !== vkey) throw new Error("E3 anchor has not been reset with this programVKey");
  if (typeof body.publicValues !== "string" || typeof body.proof !== "string") {
    throw new Error("publicValues and proof must be hex strings");
  }
  const values = decodeValues(body.publicValues, "aggregate");
  requireAllowed(values, "aggregate", policies);
  const [provedEpoch, provedRoot, newRoot] = values;
  if (provedEpoch !== epochId || provedRoot.toLowerCase() !== expectedRoot) {
    throw new Error("requested epoch or root disagrees with aggregate public values");
  }
  const tx = await epochAnchor.verifyAndAnchor(epochId, expectedRoot, body.publicValues, body.proof);
  const receipt = await tx.wait();
  if (receipt.status !== 1 || (await epochAnchor.nullifierRoot()).toLowerCase() !== newRoot.toLowerCase()) {
    throw new Error("Anvil did not apply the proved epoch root");
  }
  return {
    ...validationMetadata(values, "aggregate"), programVKey: vkey, status: receipt.status, registry: registryAddress,
    anchor: await epochAnchor.getAddress(), anchorEvent: anchorEvent(epochAnchor, receipt, "EpochAggregated"),
    epochId: epochId.toString(), transactionHash: receipt.hash,
    gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length - 2) / 2,
    newNullifierRoot: newRoot.toLowerCase(),
  };
};

const anchorShipmentHash = async (body) => {
  if (!shipmentHashAnchor) throw new Error("hash baseline has not been reset");
  const shipmentId = BigInt(String(body.shipmentId));
  const digest = bytes32(body.digest, "digest");
  const eventCount = Number(body.eventCount);
  if (!Number.isInteger(eventCount) || eventCount < 1 || eventCount > 128) {
    throw new Error("eventCount must be 1..128");
  }
  const tx = await shipmentHashAnchor.anchor(shipmentId, digest, eventCount);
  const receipt = await tx.wait();
  if (receipt.status !== 1 || (await shipmentHashAnchor.shipmentDigests(shipmentId)).toLowerCase() !== digest) {
    throw new Error("Anvil did not store the shipment digest");
  }
  return { protocolVersion: 2, verificationKind: "hash-on-chain", status: receipt.status,
    anchor: await shipmentHashAnchor.getAddress(), anchorEvent: anchorEvent(shipmentHashAnchor, receipt, "ShipmentAnchored"),
    shipmentId: shipmentId.toString(), digest, eventCount, transactionHash: receipt.hash,
    gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length - 2) / 2 };
};

const mintAdaptedToken = async (body) => {
  if (!adaptedTokenAnchor) throw new Error("adapted token baseline has not been reset");
  if (bytes32(body.programVKey, "programVKey") !== adaptedPolicyVKey) {
    throw new Error("policy program vkey changed");
  }
  if (typeof body.publicValues !== "string" || typeof body.proof !== "string") {
    throw new Error("publicValues and proof must be hex strings");
  }
  const values = decodeValues(body.publicValues);
  requireAllowed(values, "leaf", policies);
  const tx = await adaptedTokenAnchor.verifyAndMint(body.publicValues, body.proof);
  const receipt = await tx.wait();
  if (receipt.status !== 1) throw new Error("adapted token transaction failed");
  return { ...validationMetadata(values), programVKey: adaptedPolicyVKey, status: receipt.status, registry: registryAddress,
    anchor: await adaptedTokenAnchor.getAddress(), anchorEvent: anchorEvent(adaptedTokenAnchor, receipt, "TokenMinted"),
    transactionHash: receipt.hash, gasUsed: receipt.gasUsed.toString(),
    calldataBytes: (tx.data.length - 2) / 2,
    newNullifierRoot: (await adaptedTokenAnchor.nullifierRoot()).toLowerCase() };
};

const server = createServer(async (request, response) => {
  try {
    if (request.method === "GET" && request.url === "/health") {
      send(response, 200, { status: "ok", protocolVersion: 2, verificationKind: "sp1-groth16", registry: registryAddress, anchor: anchor ? await anchor.getAddress() : null });
    } else if (request.method === "POST" && request.url === "/reset") {
      const body = await readJSON(request);
      await deployAnchor(body.initialNullifierRoot, body.programVKey);
      send(response, 201, { status: "reset", protocolVersion: 2, registry: registryAddress, anchor: await anchor.getAddress() });
    } else if (request.method === "POST" && request.url === "/anchor") {
      send(response, 201, await anchorProof(await readJSON(request)));
    } else if (request.method === "POST" && request.url === "/e3/reset") {
      const anchor = await deployEpochAnchor(await readJSON(request));
      send(response, 201, { status: "reset", protocolVersion: 2, registry: registryAddress, anchor: await anchor.getAddress() });
    } else if (request.method === "POST" && request.url === "/e3/anchor") {
      send(response, 201, await anchorEpochProof(await readJSON(request)));
    } else if (request.method === "POST" && request.url === "/e3/hash/reset") {
      shipmentHashAnchor = await deploy("ShipmentHashAnchor.sol", "ShipmentHashAnchor");
      send(response, 201, { status: "reset", protocolVersion: 2, registry: registryAddress, anchor: await shipmentHashAnchor.getAddress() });
    } else if (request.method === "POST" && request.url === "/e3/hash/anchor") {
      send(response, 201, await anchorShipmentHash(await readJSON(request)));
    } else if (request.method === "POST" && request.url === "/e3/vecro/reset") {
      const body = await readJSON(request);
      const requestedPolicyVKey = bytes32(body.programVKey, "programVKey");
      requireLeafKey(requestedPolicyVKey);
      adaptedPolicyVKey = requestedPolicyVKey;
      verifier ??= await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
      adaptedTokenAnchor = await deploy("VeCroTokenAdaptedAnchor.sol", "VeCroTokenAdaptedAnchor",
        await verifier.getAddress(), adaptedPolicyVKey, bytes32(body.initialNullifierRoot, "initialNullifierRoot"), registryAddress);
      send(response, 201, { status: "reset", protocolVersion: 2, registry: registryAddress, anchor: await adaptedTokenAnchor.getAddress() });
    } else if (request.method === "POST" && request.url === "/e3/vecro/mint") {
      send(response, 201, await mintAdaptedToken(await readJSON(request)));
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
