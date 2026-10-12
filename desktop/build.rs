fn main() {
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        // cef-dll-sys places the Chromium runtime beside the executable.
        Ok("linux") => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        // App icon plus the manifest CEF needs (Windows 10 compatibility, common controls).
        Ok("windows") => {
            embed_resource::compile("../packaging/windows/cardboard.rc", embed_resource::NONE)
                .manifest_required()
                .unwrap();
        }
        _ => {}
    }
}
