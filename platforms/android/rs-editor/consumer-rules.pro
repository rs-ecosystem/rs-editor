# Rust calls back into these through JNI.
-keep class com.rseditor.android.NativeSurface { *; }
-keepclasseswithmembernames class com.rseditor.android.** { native <methods>; }
