fn main() {
    // The bundler merges `Info.plist` into the packaged `.app`, but `tauri dev` runs a bare binary with no
    // bundle around it -- and macOS reads the microphone usage string out of the binary itself in that case.
    // Without it, the first call to open an input device kills the process with no error anywhere. Embedding
    // the same file in the `__TEXT,__info_plist` section makes dictation work in development too.
    #[cfg(target_os = "macos")]
    {
        let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
        println!("cargo:rerun-if-changed={}", plist.display());
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
    tauri_build::build()
}
