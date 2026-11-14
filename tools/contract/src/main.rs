//! Connect Chigiri's canonical replay to the common ownership verifier.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let manifest = chigiri_codegen::ResolvedManifest::load(&root.join("chigiri.toml"))?;
    let rendered = chigiri_codegen::check_contract(&manifest)?.render(&manifest)?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => {
            let marker_path = "generated/shiori-contract/chigiri-generated.json";
            let marker: Value = serde_json::from_slice(&std::fs::read(root.join(marker_path))?)?;
            let mut inputs = BTreeMap::new();
            for name in ["chigiri.toml", marker_path, "graphql/schema.graphql"] {
                inputs.insert(name, digest(&std::fs::read(root.join(name))?));
            }
            let files: BTreeMap<_, _> = rendered
                .files()
                .map(|(name, bytes)| (format!("generated/shiori-contract/{name}"), digest(bytes)))
                .collect();
            let record = json!({"schema_version": 1, "owner": "chigiri",
                "generator_revision": marker["generator"]["git_commit"], "inputs": inputs, "files": files});
            std::fs::create_dir_all(root.join(".craftguild"))?;
            std::fs::write(
                root.join(".craftguild/contract-ownership.json"),
                format!("{}\n", serde_json::to_string_pretty(&record)?),
            )?;
            println!(
                "Canonical Chigiri check and ownership conversion: {} files",
                rendered.files().len()
            );
        }
        [output] => {
            let output = Path::new(output);
            if !output.is_dir() || output.is_symlink() || output.read_dir()?.next().is_some() {
                return Err("Expected an empty reproduction directory".into());
            }
            for (name, bytes) in rendered.files() {
                let path = output.join(name);
                std::fs::create_dir_all(path.parent().ok_or("Missing output parent")?)?;
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?
                    .write_all(bytes)?;
            }
        }
        _ => return Err("Usage: shiori-contract-tools [EMPTY_OUTPUT]".into()),
    }
    Ok(())
}
