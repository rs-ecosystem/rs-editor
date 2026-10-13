//! The web shell: Rs Editor compiled to WebAssembly with Emscripten,
//! painting with Skia into a WebGL2 canvas.
//!
//! JavaScript (`web/rs-editor.js`) owns the canvas, the input element
//! (EditContext, or a hidden textarea where it is missing) and the event
//! loop; it calls the `rse_*` functions below. Strings cross the boundary
//! as UTF-8 bytes in wasm memory: JavaScript writes them into a buffer from
//! [`rse_alloc`], and results are read from [`rse_result`].

#[cfg(target_os = "emscripten")]
mod shell;

#[cfg(target_os = "emscripten")]
fn main() {}

#[cfg(not(target_os = "emscripten"))]
fn main() {
    eprintln!(
        "rs-editor-web only runs on wasm32-unknown-emscripten; see crates/rs-editor-web/build.sh"
    );
}
