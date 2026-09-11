import { readFile } from "node:fs/promises";
import process from "node:process";
import { ContractFactory, JsonRpcProvider, Wallet, NonceManager, AbiCoder } from "ethers";
import solc from "solc";

const fixturePath = process.env.SP1_FIXTURE;
if (!fixturePath) throw new Error("SP1_FIXTURE must point to a generated proof JSON file");

const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
const compilerInput = {
  language: "Solidity",
  sources: Object.fromEntries(
    await Promise.all(
      [
        "EpochAnchor.sol",
        "sp1/ISP1Verifier.sol",
        "sp1/Groth16Verifier.sol",
        "sp1/SP1VerifierGroth16.sol",
      ].map(async (path) => [`contracts/${path}`, { content: await readFile(new URL(`../${path}`, import.meta.url), "utf8") }]),
    ),
  ),
  settings: { outputSelection: { "*": { "*": ["abi", "evm.bytecode.object"] } } },
};
const compilerOutput = JSON.parse(solc.compile(JSON.stringify(compilerInput)));
const errors = compilerOutput.errors?.filter((entry) => entry.severity === "error") ?? [];
if (errors.length) throw new Error(errors.map((entry) => entry.formattedMessage).join("\n"));

const artifact = (source, contract) => compilerOutput.contracts[source][contract];
const provider = new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:8545");
const signer = new NonceManager(new Wallet(
  process.env.ANVIL_PRIVATE_KEY ?? "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  provider,
));
const deploy = async (source, contract, ...args) => {
  const compiled = artifact(source, contract);
  const instance = await new ContractFactory(compiled.abi, compiled.evm.bytecode.object, signer).deploy(...args);
  await instance.waitForDeployment();
  return instance;
};

const verifier = await deploy("contracts/sp1/SP1VerifierGroth16.sol", "SP1Verifier");
const [epochId, oldRoot, newRoot] = AbiCoder.defaultAbiCoder().decode(
  ["uint64", "bytes32", "bytes32"],
  fixture.publicValues,
);
const anchor = await deploy(
  "contracts/EpochAnchor.sol",
  "EpochAnchor",
  await verifier.getAddress(),
  fixture.vkey,
  oldRoot,
);

const receipt = await (await anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, fixture.proof)).wait();
if ((await anchor.nullifierRoot()).toLowerCase() !== newRoot.toLowerCase()) {
  throw new Error("Anvil state root was not updated by the SP1-verified proof");
}

const mutate = (hex) => `${hex.slice(0, -2)}${(Number.parseInt(hex.slice(-2), 16) ^ 1).toString(16).padStart(2, "0")}`;
const mustRevert = async (label, call) => {
  try {
    await call();
  } catch {
    return;
  }
  throw new Error(`${label} was accepted`);
};
await mustRevert("tampered proof", () => anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, mutate(fixture.proof)));
await mustRevert("tampered public values", () => anchor.verifyAndAnchor(epochId, oldRoot, mutate(fixture.publicValues), fixture.proof));
await mustRevert("duplicate epoch", () => anchor.verifyAndAnchor(epochId, oldRoot, fixture.publicValues, fixture.proof));

console.log(JSON.stringify({
  verifier: await verifier.getAddress(),
  anchor: await anchor.getAddress(),
  transactionHash: receipt.hash,
  status: receipt.status,
  gasUsed: receipt.gasUsed.toString(),
  proofBytes: (fixture.proof.length - 2) / 2,
}, null, 2));
