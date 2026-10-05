use epcis_mapper::{decode_hex, map_with_catalog, sign_canonical, IdCatalog, MappingContext};
use serde_json::Value;
use std::{fs, io::Write, path::Path};

fn main() {
    if let Err(error) = run() {
        eprintln!("epcis-mapper: {error}");
        std::process::exit(1);
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let data = fs::read(path).map_err(|error| format!("read {path}: {error}"))?;
    serde_json::from_slice(&data).map_err(|error| format!("parse {path}: {error}"))
}

fn write_new(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("write new {path}: {error}"))
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 || args.len() % 2 != 0 {
        return Err("usage: epcis-mapper EVENT.json CONTEXT.json [--out NEW.json] [--bytes-out NEW.bin] [--catalog-in CATALOG.json] [--catalog-out NEW.json] [--signing-key-file PRIVATE.hex]".into());
    }
    let mut out = None;
    let mut bytes_out = None;
    let mut catalog_in = None;
    let mut catalog_out = None;
    let mut key_file = None;
    for pair in args[2..].chunks_exact(2) {
        let target = match pair[0].as_str() {
            "--out" => &mut out,
            "--bytes-out" => &mut bytes_out,
            "--catalog-in" => &mut catalog_in,
            "--catalog-out" => &mut catalog_out,
            "--signing-key-file" => &mut key_file,
            _ => return Err(format!("unknown option {}", pair[0])),
        };
        if target.replace(pair[1].as_str()).is_some() {
            return Err(format!("duplicate option {}", pair[0]));
        }
    }
    let destinations: Vec<_> = [out, bytes_out, catalog_out]
        .into_iter()
        .flatten()
        .collect();
    for (index, destination) in destinations.iter().enumerate() {
        if Path::new(destination).exists() || destinations[..index].contains(destination) {
            return Err("output paths must be distinct new files".into());
        }
    }
    let event: Value = read_json(&args[0])?;
    let context: MappingContext = read_json(&args[1])?;
    let mut catalog: IdCatalog = if let Some(path) = catalog_in {
        read_json(path)?
    } else {
        IdCatalog::default()
    };
    let mut mapped = map_with_catalog(&event, &context, &mut catalog)?;
    if let Some(path) = key_file {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(path)
                .map_err(|_| "cannot read private key file")?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(
                    "private key file must deny group and other permissions (chmod 600)".into(),
                );
            }
        }
        // Never include file contents in errors, logs, provenance or catalog.
        let raw = fs::read_to_string(path).map_err(|_| "cannot read private key file")?;
        let mut secret = decode_hex::<32>(raw.trim())
            .map_err(|_| "private key file must contain 32-byte hex")?;
        let result = sign_canonical(&mut mapped, &secret);
        secret.fill(0);
        result?;
    }
    let mut json =
        serde_json::to_vec_pretty(&mapped).map_err(|_| "cannot serialize mapped event")?;
    json.push(b'\n');
    let catalog_json =
        serde_json::to_vec_pretty(&catalog).map_err(|_| "cannot serialize catalog")?;
    if let Some(path) = bytes_out {
        write_new(path, &mapped.canonical_bytes)?;
    }
    if let Some(path) = catalog_out {
        write_new(path, &catalog_json)?;
    }
    if let Some(path) = out {
        write_new(path, &json)?;
    } else {
        std::io::stdout()
            .write_all(&json)
            .map_err(|_| "cannot write mapped event")?;
    }
    Ok(())
}
