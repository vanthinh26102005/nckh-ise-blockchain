# Policy-v2 selective disclosure prototype

This standalone crate reuses `eudr-policy`. It opens selected witness commitments
and authenticates them through ordered public aggregation children to a mined
aggregate transaction. It does not generate proofs, broadcast transactions,
read signing keys, or turn fixture tests into formal experiment evidence.

```sh
CARGO_BUILD_JOBS=1 cargo test --manifest-path crates/disclosure/Cargo.toml --offline
cargo run --manifest-path crates/disclosure/Cargo.toml --bin view-open -- \
  --lot 17 --fields 'polygon,events[0..3],credential' \
  --witness /private/policy-input-v2.bin \
  --aggregation-path /archive/aggregation-path.json --out /private/view.json
cargo run --manifest-path crates/disclosure/Cargo.toml --bin view-verify -- \
  --bundle /private/view.json --trust /trusted/expected-anchor.json \
  --rpc https://independently-selected-chain-rpc
```

`--fields` supports `polygon`, `credential`, `events[index]`, and inclusive
`events[first..last]` ranges. At least one event is required to authenticate the
claimed lot. A polygon-only or credential-only request fails and exports
nothing. Credentials link to the first selected event; the verifier requires
that event's authenticated actor, certificate, role and lot to match.

`--preset` replaces `--fields`:

| Preset | Fields |
| --- | --- |
| regulator | polygon, events[0..3], credential |
| importer | events[0..3] |
| certifier | events[0], credential |

The output must be a new file. On Unix it is created with mode 0600.

## Private witness input

`--witness` is the exact binary output of `eudr_policy::PolicyInput::encode()`
with input version 2. It contains the full private witness and must stay outside
Git and public archives. It is decoded and evaluated locally without logging
its contents. The bundle exports only the selected event records, their own
per-index salts and Merkle sibling paths. It never exports the nullifier key,
batch nonce, signatures, nullifier path or unselected event bytes. An explicitly
selected polygon includes its nonce; an explicitly selected credential includes
owner identity, key commitment, registry index and membership path.

## Public authentication path archive

`--aggregation-path` uses this exact camelCase schema. Every hex public-values
field contains 352 bytes, not the shortened illustrative values below:

```json
{
  "protocolVersion": 2,
  "transactionHash": "0x<32-byte mined aggregate transaction hash>",
  "levels": [
    {
      "leafVKeyDigest": "0x<32 bytes>",
      "aggregateVKeyDigest": "0x<32 bytes>",
      "childIndex": 0,
      "children": [
        {"kind": "leaf", "publicValues": "0x<352-byte target leaf>"},
        {"kind": "leaf", "publicValues": "0x<352-byte next sibling>"}
      ]
    },
    {
      "leafVKeyDigest": "0x<same 32 bytes>",
      "aggregateVKeyDigest": "0x<same 32 bytes>",
      "childIndex": 1,
      "children": [
        {"kind": "aggregate", "publicValues": "0x<352-byte earlier sibling>"},
        {"kind": "aggregate", "publicValues": "0x<352-byte previous level output>"}
      ]
    }
  ]
}
```

Levels run from the target leaf's immediate parent to the anchored root. Each
level contains all that parent's ordered public children and the selected
child's index, with fan-in 1–8. The first selected child must equal the locally
evaluated witness leaf. Subsequent selected children must exactly equal the
preceding level's evaluated aggregate output. A one-level path is valid. The
archive contains no child proofs or private witness data. It must be produced
alongside the real proof tree and associated with its actual mined transaction;
this tool does not invent the archive for historical proofs.

## Independent anchor trust

`--trust` is required separately from the bundle. Obtain its addresses, keys,
root pairs and verifier code hash from the reviewed deployment/configuration;
do not accept a trust file supplied by the bundle's sender.

```json
{
  "protocolVersion": 2,
  "chainId": 11155111,
  "anchorAddress": "0x<20 bytes>",
  "registryAddress": "0x<20 bytes>",
  "verifierAddress": "0x<20 bytes>",
  "verifierCodeSha256": "0x<SHA-256 of independently compiled real verifier runtime bytes>",
  "programVKey": "0x<32-byte aggregate program key>",
  "leafVKeyDigest": "0x<32 bytes>",
  "aggregateVKeyDigest": "0x<32 bytes>",
  "acceptedPolicies": [
    {"certificateRoot": "0x<32 bytes>", "polygonCommitment": "0x<32 bytes>"}
  ]
}
```

The Sepolia dry-run script in `contracts/test/sepolia_deploy.mjs` emits
`verifierCodeSha256` from the locally compiled pinned official verifier runtime.
Compiler/settings/source changes alter this hash; prepare it from the exact
reviewed artifact that was deployed. Hashing arbitrary code reported by the
bundle's RPC and treating it as trusted would defeat this check.

Verification uses the explicitly selected HTTP or HTTPS RPC to check:

- Expected chain ID, successful receipt, destination anchor, canonical block
  hash and matching transaction hash.
- Exact aggregate call ABI, epoch/old root, the complete authenticated root
  public values and the pinned Groth16 proof selector.
- One `EpochAggregated` log from the expected anchor, with matching transaction,
  block, epoch, root commitment, roots and counts.
- Anchor program/child keys, registry and verifier addresses at that mined
  block, deployed contract code, independently trusted verifier code hash and
  the pinned official `VERIFIER_HASH()`.
- The exact approved root pair via `allowedPolicies` at that mined block.

This binds to the RPC's current canonical mined block; it does not claim a
particular confirmation/finality policy. Archival RPC support is needed for
historical contract calls. It checks on-chain evidence rather than running an
SP1 proof verifier locally.

## Measurement boundary

Successful CLI verification reports bundle bytes, local opening verification
microseconds and RPC verification microseconds. Reports explicitly say
`measurementKind: selective-disclosure-prototype`, `formal: false`, and
`official30Repetitions: pending`. RPC fixture tests and deterministic synthetic
witnesses exercise rejection behavior only. Real GPU proofs, a real recorded
proof-tree archive, and the official 30 repetitions remain separate work.
