//! The web shell: OpenRich compiled to WebAssembly with Emscripten,
//! painting with Skia into a WebGL2 canvas.
//!
//! JavaScript (`web/openrich.js`) owns the canvas, the input element
//! (EditContext, or a hidden textarea where it is missing) and the event
//! loop; it calls the `orx_*` functions below. Strings cross the boundary
//! as UTF-8 bytes in wasm memory: JavaScript writes them into a buffer from
//! [`orx_alloc`], and results are read from [`orx_result`].

#[cfg(target_os = "emscripten")]
mod shell;

#[cfg(target_os = "emscripten")]
fn main() {}

#[cfg(not(target_os = "emscripten"))]
fn main() {
    eprintln!(
        "openrich-web only runs on wasm32-unknown-emscripten; see crates/openrich-web/build.sh"
    );
}
