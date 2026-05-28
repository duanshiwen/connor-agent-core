fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure().compile_protos(
        &["../../schemas/sidecar/v1/sidecar.proto"],
        &["../../schemas/sidecar/v1"],
    )?;
    Ok(())
}
