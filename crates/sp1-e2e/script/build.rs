fn git(args: &[&str]) -> Option<String> {
    std::process::Command::new("git").args(args).env("GIT_OPTIONAL_LOCKS", "0").output().ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok()).map(|text| text.trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_REVISION");
    // Cargo otherwise caches the embedded revision across commits and host/dependency edits.
    let mut metadata = vec!["HEAD".to_string(), "index".to_string(), "packed-refs".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) { metadata.push(branch); }
    for name in metadata {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            if std::path::Path::new(&path).exists() { println!("cargo:rerun-if-changed={path}"); }
        }
    }
    let sources = ["src", "Cargo.toml", "build.rs", "../Cargo.toml", "../Cargo.lock",
        "../policy/src", "../policy/Cargo.toml", "../program/src", "../program/Cargo.toml",
        "../epoch-program/src", "../epoch-program/Cargo.toml", "../../e2-bench/src", "../../e2-bench/Cargo.toml",
        "../../e1-bench/src", "../../e1-bench/Cargo.toml", "../../epcis-mapper/src", "../../epcis-mapper/Cargo.toml",
        "../../epcis-mapper/fixtures", "../../../Cargo.toml", "../../../Cargo.lock"];
    for path in sources { println!("cargo:rerun-if-changed={path}"); }
    let mut status_args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(sources);
    let dirty = git(&status_args).is_some_and(|status| !status.is_empty());
    let revision = if dirty { "unknown-dirty-source".to_string() } else {
        git(&["rev-parse", "HEAD"]).or_else(|| std::env::var("SOURCE_REVISION").ok())
            .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).unwrap_or_else(|| "unknown".into())
    };
    println!("cargo:rustc-env=EUDR_SOURCE_REVISION={revision}");

    sp1_build::build_program("../program");
    sp1_build::build_program("../epoch-program");
}
