// Contract logic only: the test verifier deliberately accepts every proof.
// This script never produces formal proof or benchmark evidence.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { AbiCoder, ContractFactory, JsonRpcProvider, NonceManager, id } from "ethers";
import solc from "solc";
import { anchorEvent } from "./policy_v2.mjs";

const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:18545");
const signer = new NonceManager(await provider.getSigner(0));
const sources = ["EpochAnchor.sol", "EpochAggregateAnchor.sol", "VeCroTokenAdaptedAnchor.sol", "sp1/ISP1Verifier.sol"];
let hasRegistry = false;
try { await readFile(new URL("../PolicyRegistry.sol", import.meta.url)); hasRegistry = true; sources.push("PolicyRegistry.sol"); }
catch (error) { if (error.code !== "ENOENT") throw error; }
const input = { language: "Solidity", sources: Object.fromEntries(await Promise.all(sources.map(async path =>
  [`contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") }]))),
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } } };
input.sources["TestOnlyVerifier.sol"] = { content: `pragma solidity ^0.8.20;
contract TestOnlyVerifier { function verifyProof(bytes32, bytes calldata, bytes calldata) external pure {} }` };
const compiled = JSON.parse(solc.compile(JSON.stringify(input)));
assert.equal(compiled.errors?.filter(e => e.severity === "error").length ?? 0, 0, JSON.stringify(compiled.errors));
const deploy = async (source, contract, ...args) => {
  const artifact = compiled.contracts[source][contract];
  const instance = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object, signer).deploy(...args);
  await instance.waitForDeployment();
  return instance;
};
const root = value => `0x${value.repeat(64)}`;
const verifier = await deploy("TestOnlyVerifier.sol", "TestOnlyVerifier");
const registry = hasRegistry ? await deploy("contracts/PolicyRegistry.sol", "PolicyRegistry") : null;
if (registry) await (await registry.setPolicyRoots(root("4"), root("5"), true)).wait();
const registryArgs = registry ? [await registry.getAddress()] : [];
const anchor = await deploy("contracts/EpochAnchor.sol", "EpochAnchor", await verifier.getAddress(), root("a"), root("1"), ...registryArgs);
const values = AbiCoder.defaultAbiCoder().encode(
  ["uint64", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "uint8", "uint32", "uint8", "bool"],
  [1, root("1"), root("2"), root("3"), root("4"), root("5"), root("6"), 3, 900, 8, true]);
await assert.rejects(() => anchor.verifyAndAnchor.staticCall(1, root("1"), values, "0x"),
  e => e.data?.slice(0, 10) === id("InvalidPolicyResult()").slice(0, 10), "non-policy role must be rejected");
const reject = async (errorSignature, action) => {
  await assert.rejects(action, e => e.data?.slice(0, 10) === id(errorSignature).slice(0, 10), errorSignature);
};
const coder = AbiCoder.defaultAbiCoder();
const leafTypes = ["uint64", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "uint8", "uint32", "uint8", "bool"];
const leaf = changes => coder.encode(leafTypes, [1, root("1"), root("2"), root("3"), root("4"), root("5"), root("6"), 4, 900, 8, true].map((v, i) => changes?.[i] ?? v));
const token = await deploy("contracts/VeCroTokenAdaptedAnchor.sol", "VeCroTokenAdaptedAnchor", await verifier.getAddress(), root("a"), root("1"), ...registryArgs);
const aggregate = await deploy("contracts/EpochAggregateAnchor.sol", "EpochAggregateAnchor", await verifier.getAddress(), root("a"), root("b"), root("c"), root("1"), ...registryArgs);
const epoch = changes => coder.encode(["uint64", "bytes32", "bytes32", "bytes32", "uint64", "uint64", "bool", "bytes32", "bytes32", "bytes32", "bytes32"],
  [1, root("1"), root("2"), root("3"), 1, 8, true, root("b"), root("c"), root("4"), root("5")].map((v, i) => changes?.[i] ?? v));
const leafCall = (contract, pv) => contract === token ? () => contract.verifyAndMint.staticCall(pv, "0x") : () => contract.verifyAndAnchor.staticCall(1, root("1"), pv, "0x");
for (const contract of [anchor, token]) {
  for (const changes of [{7:3}, {8:899}, {9:7}, {9:65}, {10:false}]) await reject("InvalidPolicyResult()", leafCall(contract, leaf(changes)));
  await reject("StateRootNotAdvanced()", leafCall(contract, leaf({2:root("1")})));
  for (const changes of [{4:root("f")}, {5:root("f")}, {4:root("5"),5:root("4")}]) await reject("PolicyRootsNotAllowed(bytes32,bytes32)", leafCall(contract, leaf(changes)));
  await reject("InvalidPublicValuesLength(uint256)", leafCall(contract, leaf() + "00"));
  await reject("InvalidPublicValuesLength(uint256)", leafCall(contract, leaf().slice(0,-64)));
  // Dirty uint8/bool words have the right length but are noncanonical ABI.
  await assert.rejects(leafCall(contract, leaf().slice(0, 2 + 7*64) + root("f").slice(2) + leaf().slice(2 + 8*64)));
  await assert.rejects(leafCall(contract, leaf().slice(0, -64) + "2".padStart(64, "0")));
}
const aggregateCall = pv => () => aggregate.verifyAndAnchor.staticCall(1, root("1"), pv, "0x");
for (const changes of [{4:0}, {5:7}, {5:65}, {6:false}]) await reject("InvalidEpochResult()", aggregateCall(epoch(changes)));
await reject("StateRootNotAdvanced()", aggregateCall(epoch({2:root("1")})));
for (const changes of [{9:root("f")}, {10:root("f")}, {9:root("5"),10:root("4")}]) await reject("PolicyRootsNotAllowed(bytes32,bytes32)", aggregateCall(epoch(changes)));
await reject("InvalidPublicValuesLength(uint256)", aggregateCall(epoch() + "00"));
await reject("InvalidPublicValuesLength(uint256)", aggregateCall(epoch().slice(0, 2 + 9*64)));
await reject("LeafVKeyMismatch(bytes32,bytes32)", aggregateCall(epoch({7:root("f")})));
await reject("LeafVKeyMismatch(bytes32,bytes32)", aggregateCall(epoch({8:root("f")})));
await reject("Unauthorized(address)", async () => registry.connect(await provider.getSigner(1)).setPolicyRoots.staticCall(root("4"), root("5"), false));
await (await registry.setPolicyRoots(root("4"), root("5"), false)).wait();
for (const contract of [anchor, token]) await reject("PolicyRootsNotAllowed(bytes32,bytes32)", leafCall(contract, leaf()));
await reject("PolicyRootsNotAllowed(bytes32,bytes32)", aggregateCall(epoch()));
await (await registry.setPolicyRoots(root("4"), root("5"), true)).wait();
await reject("UnexpectedEpoch(uint64,uint64)", () => anchor.verifyAndAnchor.staticCall(2, root("1"), leaf(), "0x"));
await reject("StateRootMismatch(bytes32,bytes32)", () => anchor.verifyAndAnchor.staticCall(1, root("f"), leaf(), "0x"));
for (const contract of [anchor, token]) {
  const tx = contract === token ? await contract.verifyAndMint(leaf(), "0x") : await contract.verifyAndAnchor(1, root("1"), leaf(), "0x");
  const receipt = await tx.wait();
  const emitted = anchorEvent(contract, receipt, contract === token ? "TokenMinted" : "EpochAnchored");
  assert.equal(emitted.transactionHash, receipt.hash);
  assert.equal(emitted.args.epochId, "1"); assert.equal(emitted.args.eventBatchDigest, root("3"));
  if (contract === token) assert.equal(emitted.args.eventCount, "8");
  else { assert.equal(emitted.args.oldNullifierRoot, root("1")); assert.equal(emitted.args.newNullifierRoot, root("2")); assert.equal(emitted.args.nullifier, root("6")); }
  assert.equal(await contract.nullifierRoot(), root("2"));
}
const aggregateReceipt = await (await aggregate.verifyAndAnchor(1, root("1"), epoch(), "0x")).wait();
const emittedAggregate = anchorEvent(aggregate, aggregateReceipt, "EpochAggregated");
assert.equal(emittedAggregate.args.epochCommitment, root("3")); assert.equal(emittedAggregate.args.lotCount, "1"); assert.equal(emittedAggregate.args.eventCount, "8");
assert.equal(await aggregate.nullifierRoot(), root("2"));
await reject("DuplicateEpoch(uint64)", leafCall(anchor, leaf()));
await reject("DuplicateToken(bytes32)", leafCall(token, leaf()));
await reject("DuplicateEpoch(uint64)", aggregateCall(epoch()));
// A zero batch digest must not bypass the epoch replay guard.
const zeroAnchor = await deploy("contracts/EpochAnchor.sol", "EpochAnchor", await verifier.getAddress(), root("a"), root("1"), ...registryArgs);
await (await zeroAnchor.verifyAndAnchor(1, root("1"), leaf({3:root("0")}), "0x")).wait();
await reject("DuplicateEpoch(uint64)", () => zeroAnchor.verifyAndAnchor.staticCall(1, root("2"), leaf({1:root("2"),2:root("3"),3:root("0")}), "0x"));
console.log("contract-stub-test: policy-v2 registry, ABI, root, replay and policy checks passed; no formal evidence produced");
