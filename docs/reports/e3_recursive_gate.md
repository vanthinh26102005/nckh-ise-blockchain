# E3 recursive epoch proof gate

This is the implementation gate for E3/E4, not a benchmark result. The existing
E2 `EpochAnchor.sol` is unchanged.

The SP1 epoch guest verifies each compressed child proof against its program
verification key and the SHA-256 digest of its public values. It then requires
one epoch ID, an ordered old-root/new-root chain, unique child commitments and
positive event/lot counts. Fan-in is at most eight; larger epochs are reduced
recursively. The Solidity anchor verifies the final Groth16 proof, expected
program keys and old root, and rejects a second anchor for the same epoch.

The ABI contains `(epochId, oldNullifierRoot, newNullifierRoot,
epochCommitment, lotCount, eventCount, valid)` plus the leaf and aggregate
verification-key digests. Those two extra words bind the child programs to the
on-chain configuration; they are not additional research metrics.

The gate binary commits two eight-event lots to Fabric, checks the ledger
queries, proves and locally verifies each leaf, verifies that a modified child
public value is rejected, then proves and locally verifies the aggregate.
`anvil_e3_smoke.mjs` deploys a fresh verifier and anchor to Anvil, rejects
tampered aggregate proof/public values, wrong root/key and duplicate epoch,
and records the real receipt. `e3_gate_manifest.py` only writes a passed
manifest when the fixture and successful receipt agree on the new root.

Raw proof fixtures and receipts must be stored outside Git. The example assumes
Fabric Gateway and Anvil are already running on the same host:

```sh
mkdir -p /datastore/uitchain/e3/gate
cargo run --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e \
  --bin e3_epoch_gate -- --out /datastore/uitchain/e3/gate/epoch-proof.json
cd contracts
E3_FIXTURE=/datastore/uitchain/e3/gate/epoch-proof.json \
  node test/anvil_e3_smoke.mjs > /datastore/uitchain/e3/gate/receipt.json
cd ..
python3 scripts/e3_gate_manifest.py \
  --fixture /datastore/uitchain/e3/gate/epoch-proof.json \
  --receipt /datastore/uitchain/e3/gate/receipt.json \
  --out results/e3/gate_manifest.json --git-commit "$(git rev-parse HEAD)"
```

Use a fresh Fabric ledger/epoch ID and output path for each gate run. The Git
commit in the manifest must refer to the exact source that was built, with a
clean worktree; a local dirty-worktree smoke is useful for debugging but does
not establish a reproducible formal gate.
