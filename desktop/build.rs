fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // cef-dll-sys places the Chromium runtime beside the executable.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }
}
