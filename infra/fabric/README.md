# Fabric EPCIS smoke environment

`e2epcis` stores the exact base64 representation of one 86-byte `EpcisEventV1`
payload with its SHA-256 digest. The Fabric gateway only returns `201` after the
Fabric Gateway `Submit` call receives commit status.

This repository intentionally uses Hyperledger's maintained `fabric-samples`
test network rather than copying its generated crypto material or Docker
configuration. The smoke topology is two organisations, two peers and one
orderer. It is **not** the proposed three-Raft-orderer deployment topology.

## Start the smoke network

Use Fabric 2.5.8 binaries and Docker images, then set the two anchor peers so
the Org1 gateway can collect the required endorsements from Org1 and Org2:

```bash
export FABRIC_SAMPLES=/path/to/fabric-samples
export PATH="$FABRIC_SAMPLES/bin:$PATH"
export FABRIC_CFG_PATH="$FABRIC_SAMPLES/config"

cd "$FABRIC_SAMPLES/test-network"
./network.sh up createChannel -c mychannel
./scripts/setAnchorPeer.sh 1 mychannel
./scripts/setAnchorPeer.sh 2 mychannel
./network.sh deployCC -c mychannel -ccn e2epcis \
  -ccp /absolute/path/to/nckh-ise-blockchain/infra/fabric/chaincode/e2epcis \
  -ccl go -ccep "AND('Org1MSP.peer','Org2MSP.peer')"
```

Start the gateway from this repository in a second terminal:

```bash
cd infra/fabric/gateway
FABRIC_CRYPTO_ROOT="$FABRIC_SAMPLES/test-network/organizations/peerOrganizations/org1.example.com" \
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

Generated Fabric crypto, Docker volumes, vendored deployment dependencies and
ledger data are not versioned. A three-Raft-orderer topology is deliberately
left as a deployment follow-up, not represented as a passing local smoke test.
