import { readFile } from "node:fs/promises";
import { JsonRpcProvider } from "ethers";
import { compileContracts, deployer, localSigner, readPolicyManifest, readTrustedProgramKeys, registerPolicies, validateFixture, validationMetadata, anchorEvent } from "./policy_v2.mjs";

if (!process.env.SP1_FIXTURE) throw new Error("SP1_FIXTURE must point to a real policy-v2 SP1 policy proof");
const policies = await readPolicyManifest();
const trustedKeys = await readTrustedProgramKeys();
const fixture = JSON.parse(await readFile(process.env.SP1_FIXTURE, "utf8"));
const values = validateFixture(fixture, "leaf", policies, trustedKeys);
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const deploy = deployer(await compileContracts(), await localSigner(provider));
const registry = await registerPolicies(deploy, policies);
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy("VeCroTokenAdaptedAnchor.sol", "VeCroTokenAdaptedAnchor", await verifier.getAddress(), fixture.vkey, values[1], await registry.getAddress());
const mutate = hex => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustReject = async (name, args) => {
  try { await anchor.verifyAndMint.staticCall(...args); } catch { return; }
  throw new Error(`${name} was accepted`);
};
await mustReject("tampered proof", [fixture.publicValues, mutate(fixture.proof)]);
await mustReject("tampered public values", [mutate(fixture.publicValues), fixture.proof]);
const tx = await anchor.verifyAndMint(fixture.publicValues, fixture.proof);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== values[2].toLowerCase()) throw new Error("adapted token anchor did not store the proved root");
await mustReject("duplicate token/root transition", [fixture.publicValues, fixture.proof]);
console.log(JSON.stringify({ ...validationMetadata(values), programVKey: fixture.vkey, status: receipt.status, transactionHash: receipt.hash,
  gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length-2)/2, eventBatchDigest: values[3],
  verifier: await verifier.getAddress(), registry: await registry.getAddress(), anchor: await anchor.getAddress(), anchorEvent: anchorEvent(anchor, receipt, "TokenMinted") }, null, 2));
