# EPCIS ObjectEvent mapper

Standalone host CLI/library; it builds no SP1 program and runs no proof.

```sh
cargo test --manifest-path crates/epcis-mapper/Cargo.toml -j 1
cargo run --manifest-path crates/epcis-mapper/Cargo.toml -j 1 -- \
  crates/epcis-mapper/fixtures/object-event.json \
  crates/epcis-mapper/fixtures/mapping-context.json \
  --out /tmp/mapped-event.json --bytes-out /tmp/mapped-event.bin \
  --catalog-out /tmp/mapped-catalog.json
```

Outputs are new files only. Carry the preceding catalog with `--catalog-in` on
subsequent runs to detect collisions across the dataset; a fresh catalog detects
only identifiers registered in that run. Library callers should retain one
`IdCatalog` throughout ingestion. Catalog changes happen only after successful
validation. The policy owns canonical encoding (86 bytes, big endian, version 1).

Supported input is one compact ObjectEvent with a pinned GS1 EPCIS 2.0.0/2.0.1
context, or the current unversioned GS1 context URL. These URLs are recognized
locally, never fetched. Context arrays may add namespace prefixes only; aliases,
core overrides, imports, expanded JSON-LD, EPCISDocument envelopes and all other
event types are rejected. This is a strict mapping subset, not full EPCIS schema
validation or a general JSON-LD processor. Official definitions:
[GS1 EPCIS artefacts](https://ref.gs1.org/standards/epcis/artefacts).

Exactly one LGTIN URI must appear in either `epcList` or `quantityList`, never
both. LGTINs use a 6–12 digit company prefix, a numeric item reference giving
13 digits together, and a nonempty lot (alphanumeric, `-`, `_`, or canonical
uppercase percent escapes for reserved ASCII). Instance SGTINs are rejected:
there is no unambiguous lot derivation without master data. Quantity must be
positive; it is retained in source provenance through the source-event digest,
not encoded in the policy event.

`readPoint.id` must be exactly `geo:latitude,longitude`, WGS84, representable
at microdegree precision. Altitude/CRS/uncertainty parameters are rejected.
`eventTime` requires strict RFC3339, an explicit timezone, and at most three
fractional digits; its offset must match `eventTimeZoneOffset` (±14:00 bounds).
Pre-1970 time, leap seconds and unknown `-00:00` offsets are rejected.

The separate mapping context requires epoch, valid Ed25519 public key,
certificate ID, role and a KPI sensor type/unit/positive power-of-ten scale.
Exactly one scalar report matching that type is required; it must have the
configured unit. Unit codes are checked for CEFACT 2–3 character syntax; the
trusted context selects the actual unit, without implicit conversion. Decimals
and scientific notation are parsed as integer coefficient/exponent, never float;
inexact scaling, negative reading or u32 overflow are rejected. Other sensor
types are ignored; multiple selected reports or competing statistics are rejected.

ID mapping is the first eight SHA-256 bytes interpreted big-endian, hashing
`EUDR:EPCIS:EVENT-ID:V1\0 || eventID` or `EUDR:EPCIS:LGTIN:V1\0 || LGTIN`.
URIs are hashed exactly after validation, without URI rewriting. Provenance
includes source identifiers, context and a SHA-256 digest of the parsed event
reserialized with serde_json (not a digest of the original file bytes).

`--signing-key-file` is an explicit optional input containing a 32-byte Ed25519
private seed in hex, outside the repository. On Unix its mode must deny group
and other access. The key must match the mapped signer. Only the 86 canonical
bytes are signed; any original JSON signature is ignored and never reused.
`map_object_event`, `map_with_catalog`, and `sign_canonical` expose this flow to
host integration without CLI subprocesses.
