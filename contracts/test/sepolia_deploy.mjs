// Dry-run by default. Broadcast uses an explicitly supplied RPC-managed signer;
// this script never reads a private key or a secret file.
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { ContractFactory, JsonRpcProvider, NonceManager, getAddress } from "ethers";
import { bytes32, compileContracts, deployer, readPolicyManifest } from "./policy_v2.mjs";

const args = process.argv.slice(2);
const broadcast = args.includes("--broadcast");
if (args.some(arg => arg !== "--broadcast")) throw new Error("usage: POLICY_MANIFEST=deployment.json node sepolia_deploy.mjs [--broadcast]");
const policies = await readPolicyManifest();
const manifest = JSON.parse(await readFile(process.env.POLICY_MANIFEST, "utf8"));
const config = manifest.deployment;
if (!config) throw new Error("manifest.deployment requires anchorKind, programVKey, initialNullifierRoot and (for aggregate) leafVKeyDigest and aggregateVKeyDigest");
const source = config.anchorKind === "aggregate" ? "EpochAggregateAnchor.sol" : config.anchorKind === "leaf" ? "EpochAnchor.sol" : config.anchorKind === "vecro" ? "VeCroTokenAdaptedAnchor.sol" : null;
if (!source) throw new Error("deployment.anchorKind must be leaf, aggregate or vecro");
const vkey = bytes32(config.programVKey, "deployment.programVKey");
const root = bytes32(config.initialNullifierRoot, "deployment.initialNullifierRoot");
const extra = config.anchorKind === "aggregate" ? [bytes32(config.leafVKeyDigest, "deployment.leafVKeyDigest"), bytes32(config.aggregateVKeyDigest, "deployment.aggregateVKeyDigest")] : [];
const compiled = await compileContracts();
const verifierCodeSha256 = "0x" + createHash("sha256").update(Buffer.from(compiled("sp1/SP1VerifierGroth16.sol", "SP1Verifier").evm.deployedBytecode.object, "hex")).digest("hex");
const anchorArgs = (verifier, registry) => [verifier, vkey, ...extra, root, registry];
if (!broadcast) {
  // Placeholder addresses make the exact constructor ordering reviewable.
  const verifier = "0x0000000000000000000000000000000000000001";
  const registry = "0x0000000000000000000000000000000000000002";
  const artifact = compiled(source, source.slice(0,-4));
  const tx = await new ContractFactory(artifact.abi, artifact.evm.bytecode.object).getDeployTransaction(...anchorArgs(verifier, registry));
  console.log(JSON.stringify({ mode: "dry-run", broadcast: false, protocolVersion: 2, chainId: 11155111, verifierCodeSha256, anchorKind: config.anchorKind,
    owner: "explicit RPC signer selected only with --broadcast", acceptedPolicies: policies,
    constructorArguments: anchorArgs(verifier, registry), placeholderVerifier: verifier, placeholderRegistry: registry,
    anchorCreationCalldata: tx.data }, null, 2));
} else {
  if (!process.env.SEPOLIA_RPC || !process.env.SEPOLIA_SIGNER) throw new Error("--broadcast requires SEPOLIA_RPC and explicit SEPOLIA_SIGNER address managed by that RPC");
  const provider = new JsonRpcProvider(process.env.SEPOLIA_RPC);
  if ((await provider.getNetwork()).chainId !== 11155111n) throw new Error("broadcast requires Sepolia chainId 11155111");
  const signer = new NonceManager(await provider.getSigner(getAddress(process.env.SEPOLIA_SIGNER)));
  const deploy = deployer(compiled, signer);
  const registry = await deploy("PolicyRegistry.sol", "PolicyRegistry");
  for (const pair of policies) await (await registry.setPolicyRoots(pair.certificateRoot, pair.polygonCommitment, true)).wait();
  const verifier = await deploy("sp1/SP1VerifierGroth16.sol", "SP1Verifier");
  const anchor = await deploy(source, source.slice(0,-4), ...anchorArgs(await verifier.getAddress(), await registry.getAddress()));
  console.log(JSON.stringify({ mode: "broadcast", protocolVersion: 2, chainId: 11155111, verifierCodeSha256, owner: await signer.getAddress(),
    registry: await registry.getAddress(), verifier: await verifier.getAddress(), anchor: await anchor.getAddress() }, null, 2));
}
