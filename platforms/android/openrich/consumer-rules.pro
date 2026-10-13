# Rust calls back into these through JNI.
-keep class dev.openrich.android.NativeSurface { *; }
-keepclasseswithmembernames class dev.openrich.android.** { native <methods>; }
