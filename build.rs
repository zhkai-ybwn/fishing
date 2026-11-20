fn main() -> std::io::Result<()> {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/fishing.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/fishing.ico")
            .set("ProductName", "fishing")
            .set("FileDescription", "fishing desktop camera monitor")
            .set("InternalName", "fishing")
            .set("OriginalFilename", "fishing.exe")
            .compile()?;
        stage_runtime()?;
    }
    Ok(())
}

#[cfg(windows)]
fn stage_runtime() -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::path::{Path, PathBuf};

    if std::env::var("TARGET").as_deref() != Ok("x86_64-pc-windows-msvc") {
        return Ok(());
    }
    let cache = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/dependencies/onnxruntime-1.22.1-win-x64");
    let libraries = ["onnxruntime.dll", "onnxruntime_providers_shared.dll"];
    for library in libraries {
        println!("cargo:rerun-if-changed={}", cache.join(library).display());
    }
    if libraries
        .iter()
        .any(|library| !cache.join(library).is_file())
    {
        println!(
            "cargo:warning=ONNX Runtime is missing. Run .\\packaging\\Get-OnnxRuntime.ps1 from the repository root, then rebuild to copy both DLLs beside fishing.exe."
        );
        return Ok(());
    }

    // OUT_DIR is <profile>/build/<package>/out even with --target or CARGO_TARGET_DIR.
    let output = PathBuf::from(
        std::env::var_os("OUT_DIR")
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Cargo did not provide OUT_DIR"))?,
    );
    let profile = output.ancestors().nth(3).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidData,
            "Cargo profile could not be determined from OUT_DIR",
        )
    })?;
    for library in libraries {
        let source = std::fs::read(cache.join(library))?;
        let destination = profile.join(library);
        if std::fs::read(&destination).is_ok_and(|installed| installed == source) {
            continue;
        }
        let temporary = profile.join(format!("{library}.{}.tmp", std::process::id()));
        if let Err(error) = std::fs::write(&temporary, source)
            .and_then(|()| std::fs::rename(&temporary, &destination))
        {
            let _ = std::fs::remove_file(&temporary);
            return Err(Error::new(
                error.kind(),
                format!(
                    "Could not stage {}: {error}. Close any running copy of fishing, then rebuild.",
                    destination.display()
                ),
            ));
        }
    }
    Ok(())
}
