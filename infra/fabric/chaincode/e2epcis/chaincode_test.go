package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"testing"
)

func TestValidateEvent(t *testing.T) {
	event := make([]byte, epcisEventV1Bytes)
	event[0] = 1
	event[8] = 9
	digest := sha256.Sum256(event)
	eventID, err := validateEvent(event, digest[:])
	if err != nil {
		t.Fatal(err)
	}
	if eventID != 9 {
		t.Fatalf("event ID = %d, want 9", eventID)
	}

	event[0] = 2
	if _, err := validateEvent(event, digest[:]); err == nil {
		t.Fatal("invalid version was accepted")
	}
}

func TestDecodeSubmissionPreservesBinaryCanonicalEvent(t *testing.T) {
	event := make([]byte, epcisEventV1Bytes)
	event[0] = 1
	event[8] = 9
	event[54] = 0xff
	digest := sha256.Sum256(event)

	decodedEvent, decodedDigest, err := decodeSubmission(
		base64.StdEncoding.EncodeToString(event),
		hex.EncodeToString(digest[:]),
	)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(decodedEvent, event) || !bytes.Equal(decodedDigest, digest[:]) {
		t.Fatal("binary canonical event was not preserved")
	}
}

func TestEventRecordUsesBase64ForCanonicalEvent(t *testing.T) {
	record := EventRecord{
		EventID:        "9",
		CanonicalEvent: "AQID",
		Digest:         "digest",
	}
	encoded, err := json.Marshal(record)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(encoded, []byte(`"canonicalEvent":"AQID"`)) {
		t.Fatalf("record did not preserve base64 canonical event: %s", encoded)
	}
}
