# Policy-v2 anchoring

All proof anchors receive the `PolicyRegistry` address as their final constructor
argument. Its immutable owner is its deployer. Only that owner may call
`setPolicyRoots(certificateRoot, polygonCommitment, allowed)`. Revocation affects
all anchors that use that registry. Registration occurs before measurements.

Leaf public values remain exactly 352 canonical ABI bytes. They must report
role 4, threshold 900, 8–64 events and `valid=true`. Aggregate public values are
352 bytes: the previous nine words followed by certificate root and polygon
commitment at words 9 and 10. Every transition must advance the current root,
use an approved exact root pair, and respect replay guards.

## Explicit trusted configuration

Pass `POLICY_MANIFEST=/absolute/path/policy.json` to the server and real-fixture
smoke scripts. Configure roots independently of submitted proofs:

```json
{
  "protocolVersion": 2,
  "acceptedPolicies": [
    {
      "certificateRoot": "0x<64 hex characters>",
      "polygonCommitment": "0x<64 hex characters>"
    }
  ]
}
```

All proof smoke scripts require the same trusted program keys described below;
the fixture itself cannot select a different program.

A single pair may instead appear as top-level `certificateRoot` and
`polygonCommitment`. The server registers only this startup configuration;
reset/proof requests cannot register roots. Proof endpoints also require fixed
startup keys: `leafProgramVKey` (or `programVKey`) for leaf/VeCroToken;
`aggregateProgramVKey`, `leafVKeyDigest` and `aggregateVKeyDigest` for aggregates.
Public environment settings `E2_POLICY_VKEY`, `E3_EPOCH_VKEY`,
`LEAF_VKEY_DIGEST`, and `AGGREGATE_VKEY_DIGEST` may configure the same keys.
Reset requests must match those trusted keys. A manifest without keys supports
the hash-only baseline and cannot deploy a proof anchor. Reset each experiment's anchor
before submitting proofs. The existing request schema remains supported with
canonical v2 public values. Real fixtures require `protocolVersion: 2` and an
SP1 Groth16 proof for the pinned verifier. Historical fixtures remain historical.

## Checks and smoke scripts

```sh
npm test --prefix contracts
ANVIL_RPC=http://127.0.0.1:18545 npm run --prefix contracts anvil:policy-v2-test
ANVIL_RPC=http://127.0.0.1:18545 node contracts/test/anvil_policy_server_test.mjs
POLICY_MANIFEST=/path/policy.json SP1_FIXTURE=/path/leaf.json npm run --prefix contracts anvil:smoke
POLICY_MANIFEST=/path/policy.json E3_FIXTURE=/path/epoch.json npm run --prefix contracts anvil:e3-smoke
POLICY_MANIFEST=/path/policy.json node contracts/test/five_epoch_smoke.mjs /path/epoch1.json /path/epoch2.json /path/epoch3.json /path/epoch4.json /path/epoch5.json
```

The Anvil scripts use its unlocked account 0 on chain 31337. They do not read
private keys. The contract test uses an accepting verifier only inside its test
compiler input. Its output is `contract-stub-test`, never formal proof evidence.
Real smoke receipts record `protocolVersion: 2`, `verificationKind:
sp1-groth16`, the validated policy, and the applied roots. Aggregate receipts
also carry the decoded mined `EpochAggregated` event. The hash-only baseline is
explicitly labeled `hash-on-chain`.

The five-epoch script accepts exactly five existing real aggregate fixtures with
fixed program/child keys and consecutive epochs whose roots form one chain. It
does not create proofs.

## Sepolia preparation

Add a `deployment` object to the manifest:

```json
{
  "anchorKind": "aggregate",
  "programVKey": "0x<64 hex characters>",
  "initialNullifierRoot": "0x<64 hex characters>",
  "leafVKeyDigest": "0x<64 hex characters>",
  "aggregateVKeyDigest": "0x<64 hex characters>"
}
```

`anchorKind` also accepts `leaf` and `vecro`, which omit child keys.

```sh
POLICY_MANIFEST=/path/deployment.json node contracts/test/sepolia_deploy.mjs
```

The default compiles and prints reviewable constructor arguments and creation
calldata using labeled placeholder verifier/registry addresses. It makes no RPC
requests, accesses no keys and broadcasts nothing. Actual deployment requires
`--broadcast`, `SEPOLIA_RPC`, and an explicit `SEPOLIA_SIGNER` address managed
by that RPC. It checks Sepolia chain 11155111, deploys the owner registry and
real verifier, registers the configured pairs, then deploys the selected anchor.
This script does not load a private key or generate a proof.
