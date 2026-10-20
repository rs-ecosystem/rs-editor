//! The Apple shell: the editor view drawn with Skia (Ganesh Metal) into a
//! `CAMetalLayer`, driven from Swift.
//!
//! Swift owns the view (`NSView` on macOS, `UIView` on iOS), the display
//! link and the text input protocol (`NSTextInputClient`, `UITextInput`);
//! it calls the [`NativeEditor`](api::NativeEditor) class that BoltFFI
//! generates from `api.rs` on the main thread. Rust lays out, paints,
//! recognizes gestures and turns text input into editing through the
//! view's IME protocol, so behavior is the same as on the web and Android.
//!
//! The API builds on every platform so bindings can be generated and
//! tested on any host.

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod access;
pub mod api;
#[cfg(target_vendor = "apple")]
mod metal;

#[cfg(test)]
mod tests;
