# Fabric EPCIS environment

`e2epcis` stores the exact base64 representation of one 86-byte `EpcisEventV1`
payload with its SHA-256 digest. The Fabric gateway only returns `201` after the
Fabric Gateway `Submit` call receives commit status.

The benchmark topology is two organisations, two peers and three EtcdRaft
orderers. Its certificates, channel block, Docker volumes and ledger live under
`infra/fabric/raft/runtime/` and are ignored by Git.

## Start the E2 Raft topology

Docker pulls the pinned `hyperledger/fabric-tools:2.5.8` image once; no Fabric
binary or generated crypto is committed. Bootstrap joins all three orderers to
`e2channel`, sets the two anchor peers, and commits `e2epcis` only after both
Org1 and Org2 approve its endorsement policy.

```bash
cd /absolute/path/to/nckh-ise-blockchain
bash infra/fabric/raft/bootstrap.sh up
```

Start the gateway from this repository in a second terminal:

```bash
cd infra/fabric/gateway
FABRIC_CRYPTO_ROOT=/absolute/path/to/nckh-ise-blockchain/infra/fabric/raft/runtime/organizations/peerOrganizations/org1.example.com \
FABRIC_CHANNEL=e2channel \
GATEWAY_LISTEN=127.0.0.1:8080 \
go run .
```

The API is `GET /health`, `POST /events`, and `GET /events/{id}`. `POST /events`
requires JSON with `canonicalEvent` (base64 of exactly 86 canonical bytes) and
`digest` (hex SHA-256 of those bytes). Invalid version/length/digest returns
`400`; duplicate event IDs return `409`.

## Checks

```bash
cd infra/fabric/chaincode/e2epcis && go test ./...
cd ../../gateway && go test ./...
curl http://127.0.0.1:8080/health
```

Stop the containers with `bash infra/fabric/raft/bootstrap.sh down`. This leaves
the ignored generated runtime in place, preventing accidental overwrite; inspect
it and remove that exact directory only when you intentionally need a clean
network.

## E2 real benchmark

The runner is `crates/sp1-e2e/script/src/bin/e2_real_benchmark.rs`. Keep the
gateway, Anvil and the runner on the same host when collecting RQ2 latency;
otherwise a network hop becomes an unreported part of the metric.

```bash
# terminal 1: Fabric Gateway from the previous section
cd /absolute/path/to/nckh-ise-blockchain/contracts
anvil --port 8545

# terminal 2: deploy one real SP1 verifier and accept one proof transition per lot
npm run anvil:e2-anchor

# terminal 3: accelerated pilot only; it is not paper evidence
cd /absolute/path/to/nckh-ise-blockchain
cargo run --release --manifest-path crates/sp1-e2e/Cargo.toml -p sp1-e2e \
  --bin e2_real_benchmark -- --accelerated --duration-min 1 --seeds 1
```

Use `--prover cuda --features cuda` only after an allocated GPU job has passed
the SP1 CUDA preflight. The formal run omits `--accelerated` and uses
`--lambda-events-per-min 480 --duration-min 60 --seeds 30`. It writes ignored
JSONL raw data per lot, including the Fabric acknowledgement-to-Anvil receipt
latency, proof time, proof bytes, gas and transaction hash.

After a complete 30-seed run, summarize the raw data with
`python3 scripts/e2_real_analyze.py --raw results/e2/real/raw.jsonl --out results/e2/real/summary.json --expected-seeds 30`.
