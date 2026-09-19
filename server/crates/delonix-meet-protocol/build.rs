use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: build script de um só thread, antes de qualquer outro código.
    unsafe {
        std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let protos = [
        "delonix/meet/telephony/v1/ivr.proto",
        "delonix/meet/transcription/v1/transcription.proto",
    ];
    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    tonic_prost_build::configure()
        .file_descriptor_set_path(out.join("delonix_meet_descriptor.bin"))
        .compile_protos(
            &protos.iter().map(|p| root.join(p)).collect::<Vec<_>>(),
            std::slice::from_ref(&root),
        )?;
    for p in protos {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    Ok(())
}
