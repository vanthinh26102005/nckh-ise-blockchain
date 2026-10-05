//! Tiny synthetic mapper -> signature -> preissued registry -> private witness flow.
use anyhow::{anyhow,bail,Result};
use ed25519_dalek::SigningKey;
use epcis_mapper::{map_with_catalog,sign_canonical,IdCatalog,MappingContext};
use sp1_e2e::{benchmark_registry,benchmark_owner,benchmark_nullifier_key,benchmark_batch_nonce,policy_input_from_events};
fn main()->Result<()> {
    let out=std::env::args().nth(1).ok_or_else(||anyhow!("private output path required outside Git"))?;
    let out=std::path::Path::new(&out);
    let out=out.parent().ok_or_else(||anyhow!("output directory required"))?.canonicalize()?.join(out.file_name().ok_or_else(||anyhow!("output file required"))?);
    let repo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).ok_or_else(||anyhow!("repository path"))?.canonicalize()?;
    if out.starts_with(repo) { bail!("private witness output must be outside Git"); }
    if out.exists() { bail!("output exists"); }
    let key=[7;32];
    let mut context:MappingContext=serde_json::from_str(include_str!("../../../../epcis-mapper/fixtures/mapping-context.json"))?;
    context.actor_public_key=hex::encode(SigningKey::from_bytes(&key).verifying_key().to_bytes());
    let original:serde_json::Value=serde_json::from_str(include_str!("../../../../epcis-mapper/fixtures/object-event.json"))?;
    let mut catalog=IdCatalog::default();let mut events=Vec::new();
    for index in 0..8 {
        let mut value=original.clone();
        value["eventID"]=format!("urn:uuid:00000000-0000-4000-8000-{index:012}").into();
        value["eventTime"]=format!("2026-10-05T14:30:{index:02}.125+07:00").into();
        let mut mapped=map_with_catalog(&value,&context,&mut catalog).map_err(|e|anyhow!(e))?;
        sign_canonical(&mut mapped,&key).map_err(|e|anyhow!(e))?;
        events.push(eudr_policy::EventWitness {event_bytes:mapped.canonical_bytes.try_into().map_err(|_|anyhow!("canonical width"))?,
            signature:hex::decode(mapped.signature_hex.unwrap())?.try_into().map_err(|_|anyhow!("signature width"))?,
            credential_index:0,certificate_path:[[0;32];eudr_policy::CERTIFICATE_TREE_DEPTH]});
    }
    let lot=eudr_policy::decode_event(&events[0].event_bytes).map_err(|e|anyhow!("event:{e:?}"))?.lot_id;
    let registry=benchmark_registry(7,&events).map_err(|e|anyhow!(e))?;
    let input=policy_input_from_events(context.epoch_id,lot,events,eudr_policy::nullifier_default_root(),benchmark_nullifier_key(7,lot),
        eudr_policy::nullifier_default_path(),&registry,benchmark_owner(7),benchmark_batch_nonce(7,lot)).map_err(|e|anyhow!(e))?;
    let public=eudr_policy::evaluate(&input).map_err(|e|anyhow!("host:{e:?}"))?;
    use std::io::Write;
    let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt;options.mode(0o600); }
    options.open(&out)?.write_all(&input.encode().map_err(|e|anyhow!("input:{e:?}"))?)?;
    println!("{}",serde_json::json!({"protocolVersion":2,"validationKind":"synthetic-mapper-host-evaluation","events":8,
        "lotId":lot,"publicValues":format!("0x{}",hex::encode(public.abi_encode())),"proofGenerated":false}));
    Ok(())
}
