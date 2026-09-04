package main

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strconv"

	"github.com/hyperledger/fabric-contract-api-go/contractapi"
)

const epcisEventV1Bytes = 86

type EpcisContract struct {
	contractapi.Contract
}

type EventRecord struct {
	EventID        string `json:"eventId"`
	CanonicalEvent string `json:"canonicalEvent"`
	Digest         string `json:"digest"`
}

func (c *EpcisContract) CreateEvent(
	ctx contractapi.TransactionContextInterface,
	canonicalEventBase64 string,
	digestHex string,
) (*EventRecord, error) {
	canonicalEvent, digest, err := decodeSubmission(canonicalEventBase64, digestHex)
	if err != nil {
		return nil, err
	}
	eventID, err := validateEvent(canonicalEvent, digest)
	if err != nil {
		return nil, err
	}
	key := eventKey(eventID)
	existing, err := ctx.GetStub().GetState(key)
	if err != nil {
		return nil, fmt.Errorf("read event %s: %w", key, err)
	}
	if existing != nil {
		return nil, fmt.Errorf("event %d already exists", eventID)
	}
	record := &EventRecord{
		EventID:        fmt.Sprintf("%d", eventID),
		CanonicalEvent: base64.StdEncoding.EncodeToString(canonicalEvent),
		Digest:         hex.EncodeToString(digest),
	}
	encoded, err := json.Marshal(record)
	if err != nil {
		return nil, fmt.Errorf("encode event %d: %w", eventID, err)
	}
	if err := ctx.GetStub().PutState(key, encoded); err != nil {
		return nil, fmt.Errorf("store event %d: %w", eventID, err)
	}
	if err := ctx.GetStub().SetEvent("EventIngested", encoded); err != nil {
		return nil, fmt.Errorf("emit EventIngested for %d: %w", eventID, err)
	}
	return record, nil
}

func (c *EpcisContract) ReadEvent(
	ctx contractapi.TransactionContextInterface,
	eventID string,
) (*EventRecord, error) {
	id, err := strconv.ParseUint(eventID, 10, 64)
	if err != nil {
		return nil, fmt.Errorf("invalid event ID %q: %w", eventID, err)
	}
	key := eventKey(id)
	stored, err := ctx.GetStub().GetState(key)
	if err != nil {
		return nil, fmt.Errorf("read event %s: %w", key, err)
	}
	if stored == nil {
		return nil, fmt.Errorf("event %s does not exist", eventID)
	}
	var record EventRecord
	if err := json.Unmarshal(stored, &record); err != nil {
		return nil, fmt.Errorf("decode event %s: %w", eventID, err)
	}
	return &record, nil
}

func validateEvent(canonicalEvent, digest []byte) (uint64, error) {
	if len(canonicalEvent) != epcisEventV1Bytes {
		return 0, fmt.Errorf("EPCIS event must be exactly %d bytes", epcisEventV1Bytes)
	}
	if canonicalEvent[0] != 1 {
		return 0, fmt.Errorf("unsupported EPCIS event version %d", canonicalEvent[0])
	}
	if len(digest) != sha256.Size {
		return 0, fmt.Errorf("event digest must be %d bytes", sha256.Size)
	}
	calculated := sha256.Sum256(canonicalEvent)
	if string(calculated[:]) != string(digest) {
		return 0, fmt.Errorf("event digest does not match canonical bytes")
	}
	return binary.BigEndian.Uint64(canonicalEvent[1:9]), nil
}

func decodeSubmission(canonicalEventBase64, digestHex string) ([]byte, []byte, error) {
	canonicalEvent, err := base64.StdEncoding.DecodeString(canonicalEventBase64)
	if err != nil {
		return nil, nil, fmt.Errorf("decode canonical event: %w", err)
	}
	digest, err := hex.DecodeString(digestHex)
	if err != nil {
		return nil, nil, fmt.Errorf("decode event digest: %w", err)
	}
	return canonicalEvent, digest, nil
}

func eventKey(eventID uint64) string {
	return fmt.Sprintf("event:%020d", eventID)
}

func main() {
	chaincode, err := contractapi.NewChaincode(&EpcisContract{})
	if err != nil {
		panic(err)
	}
	if err := chaincode.Start(); err != nil {
		panic(err)
	}
}
