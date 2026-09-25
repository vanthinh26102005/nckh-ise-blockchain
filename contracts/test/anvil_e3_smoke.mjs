import { readFile } from "node:fs/promises";
import process from "node:process";
import { ContractFactory, JsonRpcProvider, Wallet, NonceManager } from "ethers";
import solc from "solc";

const fixturePath = process.env.E3_FIXTURE;
if (!fixturePath) throw new Error("E3_FIXTURE must point to a real aggregate proof fixture");
const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const sources = ["EpochAggregateAnchor.sol", "sp1/ISP1Verifier.sol", "sp1/Groth16Verifier.sol", "sp1/SP1VerifierGroth16.sol"];
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
const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const anchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor",
  await verifier.getAddress(), fixture.vkey, fixture.leafVKeyDigest, fixture.aggregateVKeyDigest, fixture.oldNullifierRoot);
const args = [fixture.epochId, fixture.oldNullifierRoot, fixture.publicValues, fixture.proof];
const mutate = (hex) => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustReject = async (name, values) => {
  try { await anchor.verifyAndAnchor.staticCall(...values); }
  catch { return; }
  throw new Error(`${name} was accepted`);
};
await mustReject("tampered aggregate proof", [args[0], args[1], args[2], mutate(args[3])]);
await mustReject("tampered public values", [args[0], args[1], mutate(args[2]), args[3]]);
await mustReject("wrong old root", [args[0], `0x${"ff".repeat(32)}`, args[2], args[3]]);
const wrongKeyAnchor = await deploy("EpochAggregateAnchor.sol", "EpochAggregateAnchor",
  await verifier.getAddress(), fixture.vkey, `0x${"ff".repeat(32)}`, fixture.aggregateVKeyDigest, fixture.oldNullifierRoot);
try { await wrongKeyAnchor.verifyAndAnchor.staticCall(...args); throw new Error("wrong leaf key was accepted"); }
catch (error) { if (error.message === "wrong leaf key was accepted") throw error; }
const tx = await anchor.verifyAndAnchor(...args);
const receipt = await tx.wait();
if (receipt.status !== 1 || (await anchor.nullifierRoot()).toLowerCase() !== fixture.newNullifierRoot.toLowerCase()) {
  throw new Error("E3 aggregate transaction did not update the proved root");
}
await mustReject("duplicate epoch", args);
console.log(JSON.stringify({
  transactionHash: receipt.hash,
  status: receipt.status,
  gasUsed: receipt.gasUsed.toString(),
  calldataBytes: (tx.data.length - 2) / 2,
  newNullifierRoot: await anchor.nullifierRoot(),
  verifier: await verifier.getAddress(),
  anchor: await anchor.getAddress(),
}, null, 2));
