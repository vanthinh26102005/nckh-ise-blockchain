use eudr_disclosure::{cli, open_bundle, parse_fields, preset, AuthenticationPath};
use eudr_policy::PolicyInput;

fn run() -> Result<(), String> {
    let args = cli::args(&[
        "--lot",
        "--fields",
        "--preset",
        "--witness",
        "--aggregation-path",
        "--out",
    ])?;
    if args.contains_key("--help") {
        println!("view-open --lot ID (--fields polygon,events[0..3],credential | --preset regulator|importer|certifier) --witness private-v2.bin --aggregation-path path.json --out new-view.json");
        return Ok(());
    }
    let lot = cli::required(&args, "--lot")?
        .parse()
        .map_err(|_| "--lot must be an unsigned integer")?;
    let input = PolicyInput::decode(&cli::read(cli::required(&args, "--witness")?)?)
        .map_err(|_| "private witness is not a canonical policy-v2 input")?;
    let fields = match (args.get("--fields"), args.get("--preset")) {
        (Some(fields), None) => fields.as_str(),
        (None, Some(name)) => preset(name)?,
        _ => return Err("choose exactly one of --fields or --preset".into()),
    };
    let selection = parse_fields(fields, input.events.len())?;
    let authentication: AuthenticationPath =
        cli::json(&cli::read(cli::required(&args, "--aggregation-path")?)?)?;
    let bundle = open_bundle(&input, lot, &selection, authentication)?;
    let bytes = serde_json::to_vec_pretty(&bundle).map_err(|_| "bundle serialization failed")?;
    cli::write_new(cli::required(&args, "--out")?, &bytes)?;
    println!(
        "{}",
        serde_json::json!({"protocolVersion":2,"measurementKind":"selective-disclosure-prototype","formal":false,
        "bundleBytes":bytes.len(),"selectedEvents":selection.events.len(),"official30Repetitions":"pending"})
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("view-open: {error}");
        std::process::exit(1);
    }
}
