fn main() {
    // EGL comes from the NDK's sysroot (khronos-egl's pkg-config lookup
    // does not work when cross-compiling).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=EGL");
    }
}
