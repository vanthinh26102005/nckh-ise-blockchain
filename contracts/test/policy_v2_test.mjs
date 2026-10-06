import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { AbiCoder } from "ethers";
import { readPolicyManifest, readTrustedProgramKeys, decodeValues, requireAllowed, validateFixture, validationMetadata, leafTypes, epochTypes, compileContracts } from "./policy_v2.mjs";

const root = value => `0x${value.repeat(64)}`;
const coder = AbiCoder.defaultAbiCoder();
const leaf = changes => coder.encode(leafTypes, [1,root("1"),root("2"),root("3"),root("4"),root("5"),root("6"),4,900,8,true].map((value,index) => changes?.[index] ?? value));
const epoch = changes => coder.encode(epochTypes, [1,root("1"),root("2"),root("3"),1,8,true,root("b"),root("c"),root("4"),root("5")].map((value,index) => changes?.[index] ?? value));
const keys = {leaf:root("a"),aggregate:root("a"),leafDigest:root("b"),aggregateDigest:root("c")};
const pair = { certificateRoot:root("4"), polygonCommitment:root("5") };
const dir = await mkdtemp(join(tmpdir(), "policy-v2-check-"));
try {
  const path = join(dir,"manifest.json");
  for (const manifest of [{ protocolVersion:2,...pair }, { protocolVersion:2,acceptedPolicies:[pair] }]) {
    await writeFile(path,JSON.stringify(manifest));
    assert.deepEqual(await readPolicyManifest(path),[pair]);
  }
  for (const manifest of [{protocolVersion:1,...pair},{protocolVersion:2,acceptedPolicies:[]},{protocolVersion:2},{protocolVersion:2,acceptedPolicies:[{...pair,certificateRoot:"not hex"}]}]) {
    await writeFile(path,JSON.stringify(manifest));
    await assert.rejects(() => readPolicyManifest(path));
  }
  assert.throws(() => decodeValues(leaf()+"00"),/exactly 352/);
  assert.throws(() => decodeValues(epoch().slice(0,2+9*64),"aggregate"),/exactly 352/);
  assert.throws(() => decodeValues(leaf({7:3})),/invalid leaf/);
  assert.throws(() => decodeValues(leaf({8:901})),/invalid leaf/);
  for (const count of [7,65]) assert.throws(() => decodeValues(leaf({9:count})),/invalid leaf/);
  for (const changes of [{4:0},{5:7},{5:65},{6:false}]) assert.throws(() => decodeValues(epoch(changes),"aggregate"),/invalid aggregate/);
  assert.throws(() => decodeValues(leaf({2:root("1")})),/must advance/);
  assert.throws(() => decodeValues(leaf().slice(0,-64)+"2".padStart(64,"0")),/canonical ABI/);
  const values=decodeValues(leaf());
  assert.throws(() => requireAllowed(values,"leaf",[{certificateRoot:root("f"),polygonCommitment:root("5")}]),/not configured/);
  requireAllowed(values,"leaf",[pair],keys);
  const fixture={protocolVersion:2,verificationKind:"sp1-groth16",vkey:root("a"),publicValues:leaf(),proof:"0x4388a21c"+"0".repeat(704)};
  assert.throws(() => validateFixture({...fixture,vkey:root("f")},"leaf",[pair],keys),/program key/);
  assert.equal(validateFixture(fixture,"leaf",[pair],keys)[0],1n);
  assert.throws(() => validateFixture({...fixture,protocolVersion:1},"leaf",[pair],keys),/protocolVersion/);
  assert.throws(() => validateFixture({...fixture,oldNullifierRoot:root("f")},"leaf",[pair],keys),/disagrees/);
  assert.throws(() => validateFixture({...fixture,verificationKind:"contract-stub-test"},"leaf",[pair],keys),/verificationKind/);
  assert.throws(() => validateFixture({...fixture,proof:"0x"},"leaf",[pair],keys),/real SP1/);
  const metadata=validationMetadata(values);
  assert.equal(metadata.protocolVersion,2); assert.equal(metadata.verificationKind,"sp1-groth16");
  assert.equal(metadata.eventBatchDigest,root("3"));assert.equal(metadata.nullifier,root("6"));assert.equal(metadata.certificateRoot,pair.certificateRoot); assert.equal(metadata.threshold,900);
  await writeFile(path, JSON.stringify({protocolVersion:2,...pair,deployment:{anchorKind:"aggregate",programVKey:root("a"),initialNullifierRoot:root("1"),leafVKeyDigest:root("b"),aggregateVKeyDigest:root("c")}}));
  const dryRun=spawnSync(process.execPath,[new URL("./sepolia_deploy.mjs",import.meta.url).pathname],{encoding:"utf8",env:{...process.env,POLICY_MANIFEST:path,SEPOLIA_RPC:"http://127.0.0.1:1"}});
  assert.equal(dryRun.status,0,dryRun.stderr);
  const deployment=JSON.parse(dryRun.stdout);assert.equal(deployment.mode,"dry-run");assert.equal(deployment.broadcast,false);assert.equal(deployment.constructorArguments.length,6);
  const noBroadcast=spawnSync(process.execPath,[new URL("./sepolia_deploy.mjs",import.meta.url).pathname,"--broadcast"],{encoding:"utf8",env:{...process.env,POLICY_MANIFEST:path,SEPOLIA_RPC:"",SEPOLIA_SIGNER:""}});
  assert.notEqual(noBroadcast.status,0);assert.match(noBroadcast.stderr,/requires SEPOLIA_RPC/);
  const priorPath=process.env.POLICY_MANIFEST;
  process.env.POLICY_MANIFEST=path;
  await writeFile(path,JSON.stringify({protocolVersion:2,...pair,leafProgramVKey:root("a"),aggregateProgramVKey:root("b"),leafVKeyDigest:root("c"),aggregateVKeyDigest:root("d")}));
  assert.equal((await readTrustedProgramKeys()).leaf,root("a"));
  assert.equal((await readTrustedProgramKeys()).aggregate,root("b"));
  if (priorPath===undefined) delete process.env.POLICY_MANIFEST; else process.env.POLICY_MANIFEST=priorPath;
  const aggregateFixture={...fixture,publicValues:epoch(),leafVKeyDigest:root("b"),aggregateVKeyDigest:root("c")};
  assert.equal(validateFixture(aggregateFixture,"aggregate",[pair],keys)[4],1n);
  assert.throws(() => validateFixture(aggregateFixture,"aggregate",[pair],{...keys,leafDigest:root("f")}),/child keys/);
  assert.throws(() => validateFixture(fixture,"leaf",[pair]),/trusted startup/);
  const artifacts=await compileContracts();
  for (const name of ["EpochAnchor","EpochAggregateAnchor","VeCroTokenAdaptedAnchor"]) assert.equal(artifacts(`${name}.sol`,name).abi.find(entry=>entry.type==="constructor").inputs.at(-1).name,"policyRegistry_");
  console.log("policy-v2: trusted manifest, canonical ABI, fixture consistency and Solidity compilation checks passed");
} finally { await rm(dir,{recursive:true,force:true}); }
