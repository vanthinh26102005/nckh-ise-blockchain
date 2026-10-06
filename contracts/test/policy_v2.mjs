import { readFile } from "node:fs/promises";
import { AbiCoder, ContractFactory, NonceManager } from "ethers";
import solc from "solc";

export const leafTypes = ["uint64", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "bytes32", "uint8", "uint32", "uint8", "bool"];
export const epochTypes = ["uint64", "bytes32", "bytes32", "bytes32", "uint64", "uint64", "bool", "bytes32", "bytes32", "bytes32", "bytes32"];
export const bytes32 = (value, name) => {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) throw new Error(`${name} must be bytes32 hex`);
  return value.toLowerCase();
};
export async function readPolicyManifest(path = process.env.POLICY_MANIFEST) {
  if (!path) throw new Error("POLICY_MANIFEST must point to explicit trusted policy-v2 configuration");
  const manifest = JSON.parse(await readFile(path, "utf8"));
  if (manifest.protocolVersion !== 2) throw new Error("policy manifest protocolVersion must be 2");
  const pairs = manifest.acceptedPolicies ?? [{ certificateRoot: manifest.certificateRoot, polygonCommitment: manifest.polygonCommitment }];
  if (!Array.isArray(pairs) || !pairs.length) throw new Error("acceptedPolicies must be nonempty");
  return pairs.map(pair => ({ certificateRoot: bytes32(pair.certificateRoot, "certificateRoot"),
    polygonCommitment: bytes32(pair.polygonCommitment, "polygonCommitment") }));
}
export async function readTrustedProgramKeys() {
  const manifest = JSON.parse(await readFile(process.env.POLICY_MANIFEST, "utf8"));
  const key = (value, name) => value === undefined ? undefined : bytes32(value, name);
  return { leaf: key(process.env.E2_POLICY_VKEY ?? manifest.leafProgramVKey ?? manifest.programVKey, "trusted leaf program key"),
    aggregate: key(process.env.E3_EPOCH_VKEY ?? manifest.aggregateProgramVKey, "trusted aggregate program key"),
    leafDigest: key(process.env.LEAF_VKEY_DIGEST ?? manifest.leafVKeyDigest, "trusted leaf key digest"),
    aggregateDigest: key(process.env.AGGREGATE_VKEY_DIGEST ?? manifest.aggregateVKeyDigest, "trusted aggregate key digest") };
}
export function decodeValues(publicValues, kind = "leaf") {
  if (typeof publicValues !== "string" || !/^0x[0-9a-fA-F]{704}$/.test(publicValues)) throw new Error("policy-v2 publicValues must be exactly 352 bytes");
  const types = kind === "aggregate" ? epochTypes : leafTypes;
  const coder = AbiCoder.defaultAbiCoder();
  const values = coder.decode(types, publicValues);
  if (coder.encode(types, values).toLowerCase() !== publicValues.toLowerCase()) throw new Error("publicValues are not canonical ABI");
  if (values[1] === values[2]) throw new Error("nullifier root must advance");
  if (kind === "aggregate") {
    if (!values[6] || values[4] === 0n || values[5] < values[4]*8n || values[5] > values[4]*64n) throw new Error("invalid aggregate policy result");
  } else if (!values[10] || values[7] !== 4n || values[8] !== 900n || values[9] < 8n || values[9] > 64n) {
    throw new Error("invalid leaf policy result");
  }
  return values;
}
export function requireAllowed(values, kind, policies) {
  const offset = kind === "aggregate" ? 9 : 4;
  if (!policies.some(pair => pair.certificateRoot === values[offset].toLowerCase() && pair.polygonCommitment === values[offset+1].toLowerCase())) {
    throw new Error("proof policy roots are not configured in the trusted manifest");
  }
}
export function validateFixture(fixture, kind, policies, trustedKeys) {
  const expectedKey = kind === "aggregate" ? trustedKeys?.aggregate : trustedKeys?.leaf;
  if (!expectedKey || bytes32(fixture.vkey, "vkey") !== expectedKey) throw new Error("fixture program key does not match trusted startup configuration");
  if (kind === "aggregate" && (!trustedKeys?.leafDigest || !trustedKeys?.aggregateDigest ||
    bytes32(fixture.leafVKeyDigest, "leafVKeyDigest") !== trustedKeys.leafDigest ||
    bytes32(fixture.aggregateVKeyDigest, "aggregateVKeyDigest") !== trustedKeys.aggregateDigest)) throw new Error("fixture child keys do not match trusted startup configuration");
  if (fixture.protocolVersion !== 2) throw new Error("fixture protocolVersion must be 2; historical fixtures cannot be upgraded by relabeling");
  if (fixture.verificationKind !== "sp1-groth16") throw new Error("fixture verificationKind must be sp1-groth16; stub fixtures cannot be used as proof evidence");
  if (typeof fixture.proof !== "string" || !/^0x4388a21c[0-9a-fA-F]{704}$/.test(fixture.proof)) throw new Error("fixture requires a real SP1 Groth16 proof");
  bytes32(fixture.vkey, "vkey");
  const values = decodeValues(fixture.publicValues, kind);
  requireAllowed(values, kind, policies);
  for (const [field,index] of [["epochId",0],["oldNullifierRoot",1],["newNullifierRoot",2],["certificateRoot",kind === "aggregate" ? 9 : 4],["polygonCommitment",kind === "aggregate" ? 10 : 5]]) {
    if (fixture[field] !== undefined && String(fixture[field]).toLowerCase() !== String(values[index]).toLowerCase()) throw new Error(`fixture ${field} disagrees with publicValues`);
  }
  if (kind === "aggregate") {
    for (const [field,index] of [["leafVKeyDigest",7],["aggregateVKeyDigest",8]]) {
      if (bytes32(fixture[field], field) !== values[index].toLowerCase()) throw new Error(`fixture ${field} disagrees with publicValues`);
    }
  }
  return values;
}
export function validationMetadata(values, kind = "leaf", verificationKind = "sp1-groth16") {
  const offset = kind === "aggregate" ? 9 : 4;
  return { protocolVersion: 2, verificationKind, publicValuesBytes: 352,
    epochId: values[0].toString(), oldNullifierRoot: values[1], newNullifierRoot: values[2],
    certificateRoot: values[offset], polygonCommitment: values[offset+1], policyValid: true,
    ...(kind === "aggregate" ? { epochCommitment: values[3], lotCount: values[4].toString(), eventCount: values[5].toString(), leafVKeyDigest: values[7], aggregateVKeyDigest: values[8] }
      : { eventBatchDigest: values[3], nullifier: values[6], role: Number(values[7]), threshold: Number(values[8]), eventCount: Number(values[9]) }) };
}
export async function compileContracts() {
  const sources = ["PolicyRegistry.sol", "EpochAnchor.sol", "EpochAggregateAnchor.sol", "VeCroTokenAdaptedAnchor.sol", "ShipmentHashAnchor.sol", "sp1/ISP1Verifier.sol", "sp1/Groth16Verifier.sol", "sp1/SP1VerifierGroth16.sol"];
  const compiled = JSON.parse(solc.compile(JSON.stringify({ language: "Solidity",
    sources: Object.fromEntries(await Promise.all(sources.map(async path => [`contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") }]))),
    settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"] } } } })));
  const errors = compiled.errors?.filter(entry => entry.severity === "error") ?? [];
  if (errors.length) throw new Error(errors.map(entry => entry.formattedMessage).join("\n"));
  return (source, name) => compiled.contracts[`contracts/${source}`][name];
}
export function deployer(artifact, signer) {
  return async (source, name, ...args) => {
    const compiled = artifact(source, name);
    const contract = await new ContractFactory(compiled.abi, compiled.evm.bytecode.object, signer).deploy(...args);
    await contract.waitForDeployment();
    return contract;
  };
}
export async function localSigner(provider) {
  if ((await provider.getNetwork()).chainId !== 31337n) throw new Error("Anvil scripts require chainId 31337");
  return new NonceManager(await provider.getSigner(0));
}
export async function registerPolicies(deploy, policies) {
  const registry = await deploy("PolicyRegistry.sol", "PolicyRegistry");
  for (const pair of policies) await (await registry.setPolicyRoots(pair.certificateRoot, pair.polygonCommitment, true)).wait();
  return registry;
}
export function anchorEvent(contract, receipt, name) {
  const address = String(contract.target).toLowerCase();
  const log = receipt.logs.find(log => log.address.toLowerCase() === address && contract.interface.parseLog(log)?.name === name);
  if (!log) throw new Error(`mined receipt is missing ${name}`);
  const parsed = contract.interface.parseLog(log);
  return { name, address, transactionHash: log.transactionHash, blockHash: log.blockHash,
    args: Object.fromEntries(parsed.fragment.inputs.map((input,index) => [input.name, String(parsed.args[index])])) };
}
