import { readFile } from "node:fs/promises";
import { JsonRpcProvider } from "ethers";
import { compileContracts, deployer, localSigner, readPolicyManifest, readTrustedProgramKeys, registerPolicies, validateFixture, validationMetadata, anchorEvent } from "./policy_v2.mjs";

if (!process.env.SP1_FIXTURE) throw new Error("SP1_FIXTURE must point to a generated real policy-v2 proof JSON file");
const policies = await readPolicyManifest();
const trustedKeys = await readTrustedProgramKeys();
const fixture = JSON.parse(await readFile(process.env.SP1_FIXTURE, "utf8"));
const values = validateFixture(fixture, "leaf", policies, trustedKeys);
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const deploy = deployer(await compileContracts(), await localSigner(provider));
const registry = await registerPolicies(deploy, policies);
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy("EpochAnchor.sol", "EpochAnchor", await verifier.getAddress(), fixture.vkey, values[1], await registry.getAddress());
const args = [values[0], values[1], fixture.publicValues, fixture.proof];
const mutate = hex => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustReject = async (label, call) => {
  try { await call(); } catch { return; }
  throw new Error(`${label} was accepted`);
};
await mustReject("tampered proof", () => anchor.verifyAndAnchor.staticCall(args[0], args[1], args[2], mutate(args[3])));
await mustReject("tampered public values", () => anchor.verifyAndAnchor.staticCall(args[0], args[1], mutate(args[2]), args[3]));
const tx = await anchor.verifyAndAnchor(...args);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== values[2].toLowerCase()) throw new Error("Anvil did not apply the SP1-verified root");
await mustReject("duplicate epoch", () => anchor.verifyAndAnchor.staticCall(...args));
console.log(JSON.stringify({ ...validationMetadata(values), programVKey: fixture.vkey, verifier: await verifier.getAddress(), registry: await registry.getAddress(),
  anchor: await anchor.getAddress(), anchorEvent: anchorEvent(anchor, receipt, "EpochAnchored"), transactionHash: receipt.hash, status: receipt.status,
  gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length-2)/2, proofBytes: (fixture.proof.length-2)/2 }, null, 2));
