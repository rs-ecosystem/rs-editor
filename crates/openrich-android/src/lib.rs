//! The Android shell: the editor view drawn with Skia (Ganesh GL) into a
//! `SurfaceView`, driven from Kotlin.
//!
//! Kotlin owns the `SurfaceView`, the `Choreographer` frame loop and the
//! `InputConnection`; it calls the [`NativeEditor`](api::NativeEditor)
//! class that BoltFFI generates from `api.rs`, plus a few JNI functions for
//! the surface and accessibility (`shell.rs`), on the UI thread. Rust
//! recognizes touch gestures and turns IME calls into editing through the
//! view's IME protocol, so behavior is the same as on the web and desktop.
//!
//! The API builds on every platform so bindings can be generated and
//! tested on the host.

pub mod api;
#[cfg(target_os = "android")]
mod shell;

#[cfg(test)]
mod tests;
