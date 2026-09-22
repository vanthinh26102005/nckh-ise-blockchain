#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$ROOT/../../.." && pwd)
RUNTIME="$ROOT/runtime"
TOOLS_IMAGE=hyperledger/fabric-tools:2.5.8
CHANNEL=e2channel
CHAINCODE=e2epcis
ORDERER_CA=/workspace/infra/fabric/raft/runtime/organizations/ordererOrganizations/example.com/tlsca/tlsca.example.com-cert.pem
RUNTIME_WORKSPACE=/workspace/infra/fabric/raft/runtime

compose() {
  docker compose --project-name e2raft --file "$ROOT/compose.yaml" "$@"
}

tool() {
  docker run --rm --user "$(id -u):$(id -g)" --env HOME=/tmp --env GOCACHE=/tmp/go-build \
    --env GOMODCACHE=/tmp/go-mod -v "$REPO:/workspace" \
    --workdir /workspace/infra/fabric/raft "$TOOLS_IMAGE" "$@"
}

network_tool() {
  docker run --rm --network e2-fabric --user "$(id -u):$(id -g)" --env HOME=/tmp \
    --env GOCACHE=/tmp/go-build --env GOMODCACHE=/tmp/go-mod -v "$REPO:/workspace" \
    --workdir /workspace/infra/fabric/raft "$TOOLS_IMAGE" "$@"
}

retry() {
  local attempt
  for attempt in $(seq 1 45); do
    "$@" && return 0
    sleep 2
  done
  return 1
}

peer_org1() {
  network_tool env CORE_PEER_TLS_ENABLED=true CORE_PEER_LOCALMSPID=Org1MSP \
    CORE_PEER_TLS_ROOTCERT_FILE=/workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org1.example.com/tlsca/tlsca.org1.example.com-cert.pem \
    CORE_PEER_MSPCONFIGPATH=/workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org1.example.com/users/Admin@org1.example.com/msp \
    CORE_PEER_ADDRESS=peer0.org1.example.com:7051 peer "$@"
}

peer_org2() {
  network_tool env CORE_PEER_TLS_ENABLED=true CORE_PEER_LOCALMSPID=Org2MSP \
    CORE_PEER_TLS_ROOTCERT_FILE=/workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org2.example.com/tlsca/tlsca.org2.example.com-cert.pem \
    CORE_PEER_MSPCONFIGPATH=/workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org2.example.com/users/Admin@org2.example.com/msp \
    CORE_PEER_ADDRESS=peer0.org2.example.com:7051 peer "$@"
}

join_orderer() {
  local orderer=$1
  network_tool osnadmin channel join --channelID "$CHANNEL" --config-block "runtime/$CHANNEL.block" \
    -o "$orderer:7053" --ca-file "$ORDERER_CA" \
    --client-cert "runtime/organizations/ordererOrganizations/example.com/orderers/$orderer/tls/server.crt" \
    --client-key "runtime/organizations/ordererOrganizations/example.com/orderers/$orderer/tls/server.key"
}

install_if_missing() {
  local peer_function=$1
  if "$peer_function" lifecycle chaincode queryinstalled --output json | grep -Fq "$package_id"; then
    return 0
  fi
  "$peer_function" lifecycle chaincode install "runtime/$CHAINCODE.tar.gz"
}

set_anchor_peer() {
  local peer_function=$1 msp=$2 host=$3 port=$4
  local prefix="runtime/$msp-anchor"
  "$peer_function" channel fetch config "$RUNTIME_WORKSPACE/$msp-anchor.block" \
    -o orderer1.example.com:7050 --ordererTLSHostnameOverride orderer1.example.com \
    -c "$CHANNEL" --tls --cafile "$ORDERER_CA"
  tool configtxlator proto_decode --input "$prefix.block" --type common.Block \
    | jq '.data.data[0].payload.data.config' > "$RUNTIME/$msp-anchor.json"
  jq --arg host "$host" --argjson port "$port" \
    '.channel_group.groups.Application.groups.'"$msp"'.values.AnchorPeers = {
      mod_policy: "Admins",
      value: {anchor_peers: [{host: $host, port: $port}]},
      version: "0"
    }' "$RUNTIME/$msp-anchor.json" > "$RUNTIME/$msp-anchor-updated.json"
  tool configtxlator proto_encode --input "$prefix.json" --type common.Config --output "$prefix.pb"
  tool configtxlator proto_encode --input "$prefix-updated.json" --type common.Config --output "$prefix-updated.pb"
  tool configtxlator compute_update --channel_id "$CHANNEL" --original "$prefix.pb" \
    --updated "$prefix-updated.pb" --output "$prefix-update.pb"
  tool configtxlator proto_decode --input "$prefix-update.pb" --type common.ConfigUpdate \
    | jq --arg channel "$CHANNEL" '{payload: {header: {channel_header: {channel_id: $channel, type: 2}}, data: {config_update: .}}}' \
    > "$RUNTIME/$msp-anchor-envelope.json"
  tool configtxlator proto_encode --input "$prefix-envelope.json" --type common.Envelope \
    --output "$prefix.tx"
  "$peer_function" channel update -o orderer1.example.com:7050 \
    --ordererTLSHostnameOverride orderer1.example.com -c "$CHANNEL" \
    -f "$RUNTIME_WORKSPACE/$msp-anchor.tx" --tls --cafile "$ORDERER_CA"
}

command=${1:-up}
if [[ $command == down ]]; then
  compose down --volumes
  printf 'Stopped E2 Raft containers. Generated runtime remains at %s.\n' "$RUNTIME"
  exit 0
fi
if [[ $command != up && $command != deploy && $command != anchors ]]; then
  printf 'Usage: %s [up|deploy|anchors|down]\n' "$0" >&2
  exit 2
fi
if [[ $command == up && -e "$RUNTIME" ]]; then
  printf 'Refusing to overwrite generated runtime: %s\nRun `bash %s down`, inspect it, then remove that exact directory before rebuilding.\n' "$RUNTIME" "$0" >&2
  exit 2
fi
if [[ ( $command == deploy || $command == anchors ) && ! -e "$RUNTIME" ]]; then
  printf 'No generated runtime exists at %s; run `%s up` first.\n' "$RUNTIME" "$0" >&2
  exit 2
fi

if [[ $command == up ]]; then
  mkdir -p "$RUNTIME"
  tool cryptogen generate --config=crypto-config.yaml --output=runtime/organizations
  tool configtxgen -configPath . -profile E2Raft -channelID "$CHANNEL" -outputBlock "runtime/$CHANNEL.block"
  compose up --detach

  retry join_orderer orderer1.example.com
  retry join_orderer orderer2.example.com
  retry join_orderer orderer3.example.com
  retry peer_org1 channel join -b "runtime/$CHANNEL.block"
  retry peer_org2 channel join -b "runtime/$CHANNEL.block"
  set_anchor_peer peer_org1 Org1MSP peer0.org1.example.com 7051
  set_anchor_peer peer_org2 Org2MSP peer0.org2.example.com 7051
fi

if [[ $command == anchors ]]; then
  set_anchor_peer peer_org1 Org1MSP peer0.org1.example.com 7051
  set_anchor_peer peer_org2 Org2MSP peer0.org2.example.com 7051
  exit 0
fi

tool peer lifecycle chaincode package "runtime/$CHAINCODE.tar.gz" \
  --path /workspace/infra/fabric/chaincode/e2epcis --lang golang --label "$CHAINCODE"_1
package_id=$(tool peer lifecycle chaincode calculatepackageid "runtime/$CHAINCODE.tar.gz")
if peer_org1 lifecycle chaincode querycommitted --channelID "$CHANNEL" --name "$CHAINCODE" 2>/dev/null | grep -Fq 'Version: 1.0, Sequence: 1'; then
  printf 'Chaincode %s is already committed on %s.\n' "$CHAINCODE" "$CHANNEL"
  exit 0
fi
install_if_missing peer_org1
install_if_missing peer_org2

approval=(lifecycle chaincode approveformyorg -o orderer1.example.com:7050 \
  --ordererTLSHostnameOverride orderer1.example.com --tls --cafile "$ORDERER_CA" \
  --channelID "$CHANNEL" --name "$CHAINCODE" --version 1.0 --package-id "$package_id" --sequence 1 \
  --signature-policy "AND('Org1MSP.peer','Org2MSP.peer')")
peer_org1 "${approval[@]}"
peer_org2 "${approval[@]}"
peer_org1 lifecycle chaincode commit -o orderer1.example.com:7050 \
  --ordererTLSHostnameOverride orderer1.example.com --tls --cafile "$ORDERER_CA" \
  --channelID "$CHANNEL" --name "$CHAINCODE" --version 1.0 --sequence 1 \
  --signature-policy "AND('Org1MSP.peer','Org2MSP.peer')" \
  --peerAddresses peer0.org1.example.com:7051 \
  --tlsRootCertFiles /workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org1.example.com/tlsca/tlsca.org1.example.com-cert.pem \
  --peerAddresses peer0.org2.example.com:7051 \
  --tlsRootCertFiles /workspace/infra/fabric/raft/runtime/organizations/peerOrganizations/org2.example.com/tlsca/tlsca.org2.example.com-cert.pem
peer_org1 lifecycle chaincode querycommitted --channelID "$CHANNEL" --name "$CHAINCODE"
for orderer in orderer1.example.com orderer2.example.com orderer3.example.com; do
  network_tool osnadmin channel list -o "$orderer:7053" --ca-file "$ORDERER_CA" \
    --client-cert "runtime/organizations/ordererOrganizations/example.com/orderers/$orderer/tls/server.crt" \
    --client-key "runtime/organizations/ordererOrganizations/example.com/orderers/$orderer/tls/server.key"
done
printf 'E2 Raft network is ready: 2 organizations, 2 peers, 3 EtcdRaft orderers.\n'
