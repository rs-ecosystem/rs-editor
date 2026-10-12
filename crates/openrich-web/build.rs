fn main() {
    // rustc links with `emcc`, which leaves out the C++ runtime Skia and ICU
    // need; ask for it (Emscripten picks the variant matching the
    // exception model).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        println!("cargo:rustc-link-arg=-lc++");
        println!("cargo:rustc-link-arg=-lc++abi");
    }
}
