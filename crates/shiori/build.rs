fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let package = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    craftguild_cli_build::emit(&package.join("../.."), "shiori")?;
    Ok(())
}
