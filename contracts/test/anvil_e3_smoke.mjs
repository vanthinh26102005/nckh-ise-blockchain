import { readFile } from "node:fs/promises";
import { JsonRpcProvider } from "ethers";
import { compileContracts, deployer, localSigner, readPolicyManifest, readTrustedProgramKeys, registerPolicies, validateFixture, validationMetadata, anchorEvent } from "./policy_v2.mjs";

if (!process.env.E3_FIXTURE) throw new Error("E3_FIXTURE must point to a real policy-v2 aggregate proof fixture");
const policies = await readPolicyManifest();
const trustedKeys = await readTrustedProgramKeys();
const fixture = JSON.parse(await readFile(process.env.E3_FIXTURE, "utf8"));
const values = validateFixture(fixture, "aggregate", policies, trustedKeys);
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const deploy = deployer(await compileContracts(), await localSigner(provider));
const registry = await registerPolicies(deploy, policies);
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const constructorArgs = [await verifier.getAddress(), fixture.vkey, values[7], values[8], values[1], await registry.getAddress()];
const anchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor", ...constructorArgs);
const args = [values[0], values[1], fixture.publicValues, fixture.proof];
const mutate = hex => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustReject = async (name, contract, values) => {
  try { await contract.verifyAndAnchor.staticCall(...values); } catch { return; }
  throw new Error(`${name} was accepted`);
};
await mustReject("tampered aggregate proof", anchor, [args[0], args[1], args[2], mutate(args[3])]);
await mustReject("tampered public values", anchor, [args[0], args[1], mutate(args[2]), args[3]]);
await mustReject("wrong old root", anchor, [args[0], `0x${"ff".repeat(32)}`, args[2], args[3]]);
const wrongKeyArgs = [...constructorArgs]; wrongKeyArgs[2] = `0x${"ff".repeat(32)}`;
const wrongKeyAnchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor", ...wrongKeyArgs);
await mustReject("wrong leaf key", wrongKeyAnchor, args);
const tx = await anchor.verifyAndAnchor(...args);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== values[2].toLowerCase()) throw new Error("E3 aggregate transaction did not update the proved root");
await mustReject("duplicate epoch", anchor, args);
console.log(JSON.stringify({ ...validationMetadata(values, "aggregate"), sourceRevision: fixture.sourceRevision, tamperedAggregateProofRejected: true, tamperedAggregateValuesRejected: true, programVKey: fixture.vkey, transactionHash: receipt.hash, status: receipt.status, anchorEvent: anchorEvent(anchor, receipt, "EpochAggregated"),
  gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length-2)/2,
  verifier: await verifier.getAddress(), registry: await registry.getAddress(), anchor: await anchor.getAddress() }, null, 2));
