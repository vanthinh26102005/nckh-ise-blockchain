fn main() {
    sp1_build::build_program("../program");
    sp1_build::build_program("../epoch-leaf-program");

    // The aggregate guest must trust one concrete leaf program. Derive its
    // digest from the built ELF, then bake it into the aggregate ELF.
    if std::env::var("SP1_SKIP_PROGRAM_BUILD").as_deref() != Ok("true") {
        use sp1_sdk::{
            blocking::{Prover, ProverClient},
            HashableKey, ProvingKey,
        };
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path("../epoch-leaf-program/Cargo.toml")
            .exec()
            .expect("read leaf program metadata");
        let paths = sp1_build::generate_elf_paths(&metadata, None).expect("locate leaf ELF");
        let leaf_path = paths
            .iter()
            .find(|(name, _)| name == "eudr-epoch-leaf-program")
            .expect("leaf ELF path")
            .1
            .as_std_path();
        let elf = sp1_build::Elf::from(std::fs::read(leaf_path).expect("read leaf ELF"));
        // The light prover computes the same program vkey without initializing
        // the full recursion/Groth16 proving pipeline during every Cargo build.
        let prover = ProverClient::builder().light().build();
        let key = prover.setup(elf).expect("setup leaf vkey");
        let words = key
            .verifying_key()
            .hash_u32()
            .map(|word| word.to_string())
            .join(",");
        std::env::set_var("E3_LEAF_VKEY_WORDS", &words);
        println!("cargo:rustc-env=E3_LEAF_VKEY_WORDS={words}");
    }
    sp1_build::build_program("../epoch-aggregate-program");
}
