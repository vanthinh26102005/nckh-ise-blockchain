import { readFile } from "node:fs/promises";
import process from "node:process";
import { AbiCoder, ContractFactory, JsonRpcProvider, Wallet, NonceManager } from "ethers";
import solc from "solc";

const fixturePath = process.env.SP1_FIXTURE;
if (!fixturePath) throw new Error("SP1_FIXTURE must point to a real SP1 policy proof");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const sources = ["VeCroTokenAdaptedAnchor.sol", "sp1/ISP1Verifier.sol", "sp1/Groth16Verifier.sol", "sp1/SP1VerifierGroth16.sol"];
const input = {
  language: "Solidity",
  sources: Object.fromEntries(await Promise.all(sources.map(async (path) =>
    [`contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") }]))),
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } },
};
const compiled = JSON.parse(solc.compile(JSON.stringify(input)));
const errors = compiled.errors?.filter((entry) => entry.severity === "error") ?? [];
if (errors.length) throw new Error(errors.map((entry) => entry.formattedMessage).join("\n"));
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const signer = new NonceManager(new Wallet(
  process.env.ANVIL_PRIVATE_KEY ?? "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  provider,
));
const deploy = async (source, contract, ...args) => {
  const artifact = compiled.contracts[`contracts/${source}`][contract];
  const instance = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object, signer).deploy(...args);
  await instance.waitForDeployment();
  return instance;
};
const mutate = (hex) => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustReject = async (name, values) => {
  try { await anchor.verifyAndMint.staticCall(...values); }
  catch { return; }
  throw new Error(`${name} was accepted`);
};
const [, oldRoot, newRoot, batchDigest] = AbiCoder.defaultAbiCoder().decode(
  ["uint64", "bytes32", "bytes32", "bytes32"], fixture.publicValues,
);
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy("VeCroTokenAdaptedAnchor.sol", "VeCroTokenAdaptedAnchor",
  await verifier.getAddress(), fixture.vkey, oldRoot);
await mustReject("tampered proof", [fixture.publicValues, mutate(fixture.proof)]);
await mustReject("tampered public values", [mutate(fixture.publicValues), fixture.proof]);
const tx = await anchor.verifyAndMint(fixture.publicValues, fixture.proof);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== newRoot.toLowerCase()) {
  throw new Error("adapted token anchor did not store the proved root");
}
await mustReject("duplicate token/root transition", [fixture.publicValues, fixture.proof]);
console.log(JSON.stringify({ status: receipt.status, transactionHash: receipt.hash,
  gasUsed: receipt.gasUsed.toString(), calldataBytes: (tx.data.length - 2) / 2,
  newNullifierRoot: newRoot, eventBatchDigest: batchDigest, anchor: await anchor.getAddress() }, null, 2));
