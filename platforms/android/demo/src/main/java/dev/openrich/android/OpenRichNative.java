package dev.openrich.android;

import android.view.Surface;

/**
 * JNI bindings to the Rust editor (crates/openrich-android). Every method
 * takes the handle returned by {@link #create} and must run on the UI thread.
 */
public final class OpenRichNative {
    static {
        System.loadLibrary("openrich_android");
    }

    private OpenRichNative() {}

    /** Creates an editor; sizes are in pixels. Returns 0 on failure. */
    public static native long create(float width, float height, float density, boolean dark, String contentJson);

    public static native void destroy(long handle);

    public static native boolean attachSurface(long handle, Surface surface);

    public static native void resize(long handle, int width, int height, float density);

    public static native void detachSurface(long handle);

    /** Draws a frame; returns ms until the next one is needed (0 animating), or -1 idle. */
    public static native long render(long handle);

    /** action: 0 down, 1 move, 2 up, 3 cancel; results arrive through {@link #takeEffects}. */
    public static native void touch(long handle, int action, float x, float y);

    /**
     * Effects since the last call, as a JSON array of
     * {type: haptic|menu|magnifier|showKeyboard|clipboard|openUrl, ...} (pixels).
     */
    public static native String takeEffects(long handle);

    /** A selection menu command: cut, copy, paste or selectAll. Returns clipboard text or null. */
    public static native String menuAction(long handle, String item, String clipboardText);

    /** The system closed the selection menu on its own. */
    public static native void menuDismissed(long handle);

    /** A key as a W3C key name ("Backspace", "Enter", "ArrowLeft", "z"...). */
    public static native boolean keyDown(long handle, String key, boolean shift, boolean ctrl, boolean alt);

    public static native void commitText(long handle, String text);

    public static native void setComposingText(long handle, String text);

    public static native void setComposingRegion(long handle, int start, int end);

    public static native void finishComposing(long handle);

    public static native void deleteSurrounding(long handle, int before, int after);

    public static native void setSelection(long handle, int start, int end);

    /** Replaces start..end (UTF-16, within the IME block) with text, typed (input rules run). */
    public static native void replaceText(long handle, int start, int end, String text);

    public static native void setFocused(long handle, boolean focused);

    /** JSON: {start, text, selection: [a, b], composition: [a, b] | null, caret: [x, y, w, h] | null}. */
    public static native String imeState(long handle);

    public static native String copy(long handle, boolean cut);

    public static native void paste(long handle, String text);

    public static native void setDark(long handle, boolean dark);
}
