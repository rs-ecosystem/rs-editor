//! The Android shell: the editor view drawn with Skia (Ganesh GL) into a
//! `SurfaceView`, driven from Kotlin/Java through JNI.
//!
//! Java owns the `SurfaceView`, the `Choreographer` frame loop and the
//! `InputConnection`; it calls the `OpenRichNative` methods implemented in
//! this crate on the UI thread. Rust recognizes touch gestures and turns
//! IME calls into editing through the view's IME protocol, so behavior is
//! the same as on the web and desktop.

#[cfg(target_os = "android")]
mod shell;
