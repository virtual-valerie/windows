fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();

        // When cross-compiling with cargo-xwin, use llvm-rc instead of rc.exe
        if std::env::var("HOST").unwrap_or_default() != std::env::var("TARGET").unwrap_or_default()
        {
            // Try to find llvm-rc for cross-compilation
            if let Ok(path) = which_llvm_rc() {
                res.set_windres_path(&path);
            } else {
                // Skip resource compilation if no resource compiler available
                eprintln!("cargo:warning=Skipping Windows resource compilation (no rc.exe or llvm-rc found). The EXE will work but won't have an embedded manifest or version info.");
                return;
            }
        }

        res.set_manifest_file("assets/app.manifest");
        // res.set_icon("assets/minerva.ico"); // Uncomment when icon is available
        res.set("ProductName", "Minerva DPN Worker");
        res.set("FileDescription", "Minerva DPN Worker");
        res.set("LegalCopyright", "Minerva Archive Project");

        match res.compile() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("cargo:warning=Windows resource compilation failed: {e}. Building without embedded manifest.");
            }
        }
    }
}

fn which_llvm_rc() -> Result<String, ()> {
    // Check common locations for llvm-rc
    for name in &["llvm-rc", "x86_64-w64-mingw32-windres", "windres"] {
        if let Ok(output) = std::process::Command::new("which").arg(name).output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(path);
                }
            }
        }
    }
    Err(())
}
