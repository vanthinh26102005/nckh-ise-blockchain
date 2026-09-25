import process from "node:process";

const server = process.env.E3_ANCHOR_URL ?? "http://127.0.0.1:8547";
const digest = `0x${"11".repeat(32)}`;
const post = async (path, body, expectedStatus = 201) => {
  const response = await fetch(`${server}${path}`, {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
  });
  const value = await response.json();
  if (response.status !== expectedStatus) {
    throw new Error(`${path}: expected ${expectedStatus}, received ${response.status}: ${JSON.stringify(value)}`);
  }
  return value;
};
const reset = await post("/reset", {});
if (!reset.chainId || !reset.hashAnchor || !reset.token) throw new Error("missing Anvil deployment metadata");
const hash = await post("/hash", { shipmentId: "1", digest });
const token = await post("/token", { shipmentId: "1", digest });
if (!hash.transactionHash || BigInt(hash.gasUsed) <= 0n || hash.calldataBytes <= 0
  || token.transactionHashes.length !== 2 || BigInt(token.gasUsed) <= BigInt(hash.gasUsed)) {
  throw new Error("missing real hash/token receipt data");
}
await post("/hash", { shipmentId: "1", digest }, 422);
const afterRejection = await post("/hash", { shipmentId: "2", digest });
if (!afterRejection.transactionHash) throw new Error("nonce did not recover after rejection");
console.log(JSON.stringify({ chainId: reset.chainId, hash, token, afterRejection }, null, 2));
