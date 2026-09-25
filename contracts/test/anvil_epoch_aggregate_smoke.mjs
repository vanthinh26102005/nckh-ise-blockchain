import { readFile } from "node:fs/promises";
import process from "node:process";
import { AbiCoder, ContractFactory, JsonRpcProvider, NonceManager, Wallet } from "ethers";
import solc from "solc";

const fixturePath = process.env.E3_FIXTURE;
if (!fixturePath) throw new Error("E3_FIXTURE must point to a real aggregate Groth16 fixture");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const files = [
  "EpochAggregateAnchor.sol",
  "sp1/ISP1Verifier.sol",
  "sp1/Groth16Verifier.sol",
  "sp1/SP1VerifierGroth16.sol",
];
const sources = Object.fromEntries(await Promise.all(files.map(async (path) => [
  `contracts/${path}`,
  { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") },
])));
const compiled = JSON.parse(solc.compile(JSON.stringify({
  language: "Solidity", sources,
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } },
})));
const errors = compiled.errors?.filter((entry) => entry.severity === "error") ?? [];
if (errors.length) throw new Error(errors.map((entry) => entry.formattedMessage).join("\n"));
const artifact = (path, name) => compiled.contracts[`contracts/${path}`][name];
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const signer = new NonceManager(new Wallet(
  process.env.ANVIL_PRIVATE_KEY ?? "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  provider,
));
const deploy = async (path, name, ...args) => {
  const a = artifact(path, name);
  const contract = await new ContractFactory(a.abi, a.evm.bytecode.object, signer).deploy(...args);
  await contract.waitForDeployment();
  return contract;
};
const [epochId, oldRoot, newRoot, commitment, lotCount, eventCount, valid] =
  AbiCoder.defaultAbiCoder().decode(
    ["uint64", "bytes32", "bytes32", "bytes32", "uint32", "uint32", "bool"],
    fixture.publicValues,
  );
const expectedEvents = BigInt(process.env.E3_EXPECTED_EVENTS ?? "128");
if (!valid || lotCount !== 1n || eventCount !== expectedEvents) {
  throw new Error(`fixture must be one valid ${expectedEvents}-event shipment from two leaf proofs`);
}
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy(
  "EpochAggregateAnchor.sol", "EpochAggregateAnchor",
  await verifier.getAddress(), fixture.vkey, oldRoot,
);
const tx = await anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, fixture.proof);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== newRoot.toLowerCase()
  || (await anchor.epochCommitments(epochId)).toLowerCase() !== commitment.toLowerCase()) {
  throw new Error("Anvil did not persist the verified aggregate transition");
}
const mutate = (hex) => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustRevert = async (label, call) => {
  try { await call(); } catch { return; }
  throw new Error(`${label} was accepted`);
};
await mustRevert("tampered proof", () => anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, mutate(fixture.proof)));
await mustRevert("tampered public values", () => anchor.verifyAndAnchor(epochId, oldRoot, mutate(fixture.publicValues), fixture.proof));
await mustRevert("duplicate epoch", () => anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, fixture.proof));
await mustRevert("wrong expected epoch", () => anchor.verifyAndAnchor(epochId + 1n, oldRoot, fixture.publicValues, fixture.proof));
console.log(JSON.stringify({
  chainId: (await provider.getNetwork()).chainId.toString(),
  verifier: await verifier.getAddress(),
  anchor: await anchor.getAddress(),
  transactionHash: receipt.hash,
  status: receipt.status,
  gasUsed: receipt.gasUsed.toString(),
  calldataBytes: (tx.data.length - 2) / 2,
  proofBytes: (fixture.proof.length - 2) / 2,
  lotCount: lotCount.toString(),
  eventCount: eventCount.toString(),
}, null, 2));
