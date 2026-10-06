// Requires five existing real fixtures; this script does not generate proofs.
import { readFile } from "node:fs/promises";
import { JsonRpcProvider } from "ethers";
import { compileContracts, deployer, localSigner, readPolicyManifest, readTrustedProgramKeys, registerPolicies, validateFixture, validationMetadata, anchorEvent } from "./policy_v2.mjs";

const paths = process.argv.slice(2);
if (paths.length !== 5) throw new Error("usage: POLICY_MANIFEST=trusted.json node five_epoch_smoke.mjs epoch1.json epoch2.json epoch3.json epoch4.json epoch5.json");
const policies = await readPolicyManifest();
const trustedKeys = await readTrustedProgramKeys();
const fixtures = await Promise.all(paths.map(async path => JSON.parse(await readFile(path, "utf8"))));
const values = fixtures.map(fixture => validateFixture(fixture, "aggregate", policies, trustedKeys));
for (let i = 1; i < 5; i++) {
  if (values[i][0] !== values[i-1][0]+1n || values[i][1] !== values[i-1][2]) throw new Error("five fixtures must form consecutive epochs and one continuous root chain");
  if (fixtures[i].vkey !== fixtures[0].vkey || values[i][7] !== values[0][7] || values[i][8] !== values[0][8]) throw new Error("program and child keys must remain fixed for all five epochs");
}
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const deploy = deployer(await compileContracts(), await localSigner(provider));
const registry = await registerPolicies(deploy, policies);
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor", await verifier.getAddress(), fixtures[0].vkey,
  values[0][7], values[0][8], values[0][1], await registry.getAddress());
const receipts = [];
for (let i = 0; i < 5; i++) {
  const tx = await anchor.verifyAndAnchor(values[i][0], values[i][1], fixtures[i].publicValues, fixtures[i].proof);
  const receipt = await tx.wait();
  if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== values[i][2].toLowerCase()) throw new Error(`epoch ${values[i][0]} did not apply its proved root`);
  receipts.push({ ...validationMetadata(values[i], "aggregate"), programVKey: fixtures[i].vkey, status: receipt.status, transactionHash: receipt.hash, anchorEvent: anchorEvent(anchor, receipt, "EpochAggregated"),
    gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length-2)/2 });
}
console.log(JSON.stringify({ protocolVersion: 2, verificationKind: "sp1-groth16", registry: await registry.getAddress(),
  verifier: await verifier.getAddress(), anchor: await anchor.getAddress(), receipts }, null, 2));
