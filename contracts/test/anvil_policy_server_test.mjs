// API configuration only: uses fake proof bytes solely for rejection checks.
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { AbiCoder, Contract, JsonRpcProvider } from "ethers";
import { compileContracts, leafTypes } from "./policy_v2.mjs";

const root = value => `0x${value.repeat(64)}`;
const directory = await mkdtemp(join(tmpdir(),"policy-server-check-"));
let child;
try {
  const path=join(directory,"trusted.json");
  await writeFile(path,JSON.stringify({protocolVersion:2,certificateRoot:root("4"),polygonCommitment:root("5"),leafProgramVKey:root("a"),aggregateProgramVKey:root("b"),leafVKeyDigest:root("c"),aggregateVKeyDigest:root("d")}));
  const serverPort=process.env.POLICY_SERVER_TEST_PORT ?? "18546";
  const base=`http://127.0.0.1:${serverPort}`;
  child=spawn(process.execPath,[new URL("./anvil_sp1_anchor_server.mjs",import.meta.url).pathname],{
    env:{...process.env,POLICY_MANIFEST:path,E2_ANCHOR_LISTEN:`127.0.0.1:${serverPort}`,ANVIL_RPC:process.env.ANVIL_RPC ?? "http://127.0.0.1:18545"},stdio:["ignore","pipe","pipe"]});
  let output="";child.stdout.on("data",data=>output+=data);child.stderr.on("data",data=>output+=data);
  let health;
  for(let attempt=0;attempt<100;attempt++) {
    if(child.exitCode!==null) throw new Error(`server exited: ${output}`);
    try { const response=await fetch(`${base}/health`); if(response.ok){health=await response.json();break;} } catch {}
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert.equal(health?.protocolVersion,2,output);
  const artifact=(await compileContracts())("PolicyRegistry.sol","PolicyRegistry");
  const registry=new Contract(health.registry,artifact.abi,new JsonRpcProvider(process.env.ANVIL_RPC ?? "http://127.0.0.1:18545"));
  assert.equal(await registry.allowedPolicies(root("4"),root("5")),true);
  assert.equal(await registry.allowedPolicies(root("f"),root("5")),false);
  const post=async(path,body)=>{
    const response=await fetch(`${base}${path}`,{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify(body)});
    return {status:response.status,body:await response.json()};
  };
  for (const endpoint of ["/reset", "/e3/vecro/reset"]) {
    const wrongKey=await post(endpoint,{initialNullifierRoot:root("1"),programVKey:root("f")});
    assert.equal(wrongKey.status,422);assert.match(wrongKey.body.error,/trusted startup leaf/);
  }
  const wrongAggregate=await post("/e3/reset",{initialNullifierRoot:root("1"),programVKey:root("b"),leafVKeyDigest:root("f"),aggregateVKeyDigest:root("d")});
  assert.equal(wrongAggregate.status,422);assert.match(wrongAggregate.body.error,/trusted startup/);
  const publicValues=AbiCoder.defaultAbiCoder().encode(leafTypes,[1,root("1"),root("2"),root("3"),root("4"),root("5"),root("6"),4,900,8,true]);
  const body={epochId:"1",oldNullifierRoot:root("1"),programVKey:root("a"),publicValues,proof:"0x"};
  const beforeReset=await post("/anchor",body);
  assert.equal(beforeReset.status,422);assert.match(beforeReset.body.error,/has not been reset/);
  const reset=await post("/reset",{initialNullifierRoot:root("1"),programVKey:root("a"),acceptedPolicies:[{certificateRoot:root("f"),polygonCommitment:root("5")}],certificateRoot:root("f"),polygonCommitment:root("5")});
  assert.equal(reset.status,201,JSON.stringify(reset.body));assert.equal(reset.body.registry,health.registry);
  assert.equal(await registry.allowedPolicies(root("f"),root("5")),false,"request must never register roots");
  const unregistered=await post("/anchor",{...body,publicValues:publicValues.slice(0,2+4*64)+root("f").slice(2)+publicValues.slice(2+5*64)});
  assert.equal(unregistered.status,422);assert.match(unregistered.body.error,/not configured/);
  const trailing=await post("/anchor",{...body,publicValues:publicValues+"00"});
  assert.equal(trailing.status,422);assert.match(trailing.body.error,/exactly 352/);
  const hashReset=await post("/e3/hash/reset",{}); assert.equal(hashReset.status,201);
  const hashReceipt=await post("/e3/hash/anchor",{shipmentId:"9",digest:root("8"),eventCount:8});
  assert.equal(hashReceipt.status,201);assert.equal(hashReceipt.body.verificationKind,"hash-on-chain");
  assert.equal(hashReceipt.body.anchorEvent.name,"ShipmentAnchored");
  assert.equal(hashReceipt.body.anchorEvent.address,hashReceipt.body.anchor.toLowerCase());
  assert.equal(hashReceipt.body.anchorEvent.transactionHash,hashReceipt.body.transactionHash);
  assert.equal(hashReceipt.body.anchorEvent.args.shipmentId,"9");assert.equal(hashReceipt.body.anchorEvent.args.digest,root("8"));assert.equal(hashReceipt.body.anchorEvent.args.eventCount,"8");
  console.log("policy-v2 server: startup-only registration, mandatory reset and malformed/untrusted request rejection passed; no proof evidence produced");
} finally {
  if(child && child.exitCode===null) {child.kill("SIGTERM");await new Promise(resolve=>child.once("exit",resolve));}
  await rm(directory,{recursive:true,force:true});
}
