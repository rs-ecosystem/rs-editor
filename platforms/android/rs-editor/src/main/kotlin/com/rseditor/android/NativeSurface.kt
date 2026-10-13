package com.rseditor.android

import android.view.Surface
import android.view.View

/**
 * The JNI functions that need Java objects (crates/rs-editor-android/src/shell.rs);
 * everything else goes through the BoltFFI class [NativeEditor]. `handle` is
 * [NativeEditor.boltffiHandle]; calls happen on the UI thread.
 */
internal object NativeSurface {
    init {
        System.loadLibrary("rs_editor_android")
    }

    /** Starts drawing into `surface`; false when EGL could not be set up. */
    @JvmStatic external fun attachSurface(handle: Long, surface: Surface): Boolean

    /** The surface changed size, in pixels. */
    @JvmStatic external fun resize(handle: Long, width: Int, height: Int, density: Float)

    /** The surface is going away; the GL context survives. */
    @JvmStatic external fun detachSurface(handle: Long)

    /** Draws a frame; ms until the next one is needed (0 while animating), or -1 when idle. */
    @JvmStatic external fun render(handle: Long): Long

    /**
     * Exposes the editor to TalkBack: AccessKit becomes the view's accessibility
     * delegate. `changed` runs after an accessibility action edits the document.
     */
    @JvmStatic external fun attachAccessibility(handle: Long, view: View, changed: Runnable)
}
