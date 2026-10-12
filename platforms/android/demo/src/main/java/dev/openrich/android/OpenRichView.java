package dev.openrich.android;

import android.content.Context;
import android.text.InputType;
import android.util.Log;
import android.view.Choreographer;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * The editor as an Android view: a SurfaceView that Rust draws into with
 * Skia, a Choreographer frame loop, and a custom InputConnection.
 */
public final class OpenRichView extends SurfaceView implements SurfaceHolder.Callback, Choreographer.FrameCallback {
    private static final String TAG = "OpenRichView";
    final long handle;
    private final float density;
    private final InputMethodManager imm;
    private final SelectionUi selection;
    private boolean surfaceReady;
    private boolean frameScheduled;
    private int imeStart = Integer.MIN_VALUE;
    int batchDepth;

    public OpenRichView(Context context, String contentJson, boolean dark) {
        super(context);
        density = getResources().getDisplayMetrics().density;
        imm = context.getSystemService(InputMethodManager.class);
        handle = OpenRichNative.create(1, 1, density, dark, contentJson);
        if (handle == 0) throw new IllegalStateException("OpenRich gagal dibuat");
        selection = new SelectionUi(this);
        setFocusable(true);
        setFocusableInTouchMode(true);
        getHolder().addCallback(this);
    }

    private static void log(String message) {
        if (Log.isLoggable(TAG, Log.DEBUG)) Log.d(TAG, message);
    }

    public void release() {
        selection.release();
        OpenRichNative.destroy(handle);
    }

    public void setDark(boolean dark) {
        OpenRichNative.setDark(handle, dark);
        scheduleFrame();
    }

    // --- surface and frames -------------------------------------------------

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        surfaceReady = OpenRichNative.attachSurface(handle, holder.getSurface());
        log("surfaceCreated " + holder.getSurfaceFrame() + " ready=" + surfaceReady);
        scheduleFrame();
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        log("surfaceChanged " + width + "x" + height);
        OpenRichNative.resize(handle, width, height, density);
        scheduleFrame();
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        log("surfaceDestroyed");
        surfaceReady = false;
        OpenRichNative.detachSurface(handle);
    }

    void scheduleFrame() {
        if (frameScheduled) return;
        frameScheduled = true;
        Choreographer.getInstance().postFrameCallback(this);
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        frameScheduled = false;
        if (!surfaceReady) return;
        long next = OpenRichNative.render(handle);
        // A frame can start a long press or bring the menu back after a fling.
        applyEffects();
        if (next >= 0 && next <= 16) {
            scheduleFrame();
        } else if (next > 16) {
            postDelayed(this::scheduleFrame, next - 8);
        }
    }

    // --- touch --------------------------------------------------------------

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        int action;
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN: action = 0; break;
            case MotionEvent.ACTION_MOVE: action = 1; break;
            case MotionEvent.ACTION_UP: action = 2; break;
            case MotionEvent.ACTION_CANCEL: action = 3; break;
            default: return true;
        }
        OpenRichNative.touch(handle, action, event.getX(), event.getY());
        afterInput();
        return true;
    }

    @Override
    protected void onFocusChanged(boolean focused, int direction, android.graphics.Rect previous) {
        super.onFocusChanged(focused, direction, previous);
        OpenRichNative.setFocused(handle, focused);
        scheduleFrame();
    }

    // --- keys ---------------------------------------------------------------

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (handleKey(event)) {
            afterInput();
            return true;
        }
        return super.onKeyDown(keyCode, event);
    }

    /** Hardware keys and keys sent by IMEs through sendKeyEvent. */
    boolean handleKey(KeyEvent event) {
        String name = keyName(event);
        if (name == null) return false;
        boolean ctrl = event.isCtrlPressed();
        if (OpenRichNative.keyDown(handle, name, event.isShiftPressed(), ctrl, event.isAltPressed())) {
            return true;
        }
        if (!ctrl && name.length() > 0 && !Character.isISOControl(name.codePointAt(0)) && name.codePointCount(0, name.length()) == 1) {
            OpenRichNative.commitText(handle, name);
            return true;
        }
        return false;
    }

    private static String keyName(KeyEvent event) {
        switch (event.getKeyCode()) {
            case KeyEvent.KEYCODE_DEL: return "Backspace";
            case KeyEvent.KEYCODE_FORWARD_DEL: return "Delete";
            case KeyEvent.KEYCODE_ENTER:
            case KeyEvent.KEYCODE_NUMPAD_ENTER: return "Enter";
            case KeyEvent.KEYCODE_TAB: return "Tab";
            case KeyEvent.KEYCODE_ESCAPE: return "Escape";
            case KeyEvent.KEYCODE_DPAD_LEFT: return "ArrowLeft";
            case KeyEvent.KEYCODE_DPAD_RIGHT: return "ArrowRight";
            case KeyEvent.KEYCODE_DPAD_UP: return "ArrowUp";
            case KeyEvent.KEYCODE_DPAD_DOWN: return "ArrowDown";
            case KeyEvent.KEYCODE_MOVE_HOME: return "Home";
            case KeyEvent.KEYCODE_MOVE_END: return "End";
            case KeyEvent.KEYCODE_PAGE_UP: return "PageUp";
            case KeyEvent.KEYCODE_PAGE_DOWN: return "PageDown";
            default:
                int meta = event.getMetaState() & ~(KeyEvent.META_CTRL_MASK | KeyEvent.META_ALT_MASK);
                int unicode = event.getUnicodeChar(meta);
                return unicode > 0 ? new String(Character.toChars(unicode)) : null;
        }
    }

    // --- IME ----------------------------------------------------------------

    @Override
    public boolean onCheckIsTextEditor() {
        return true;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo out) {
        out.inputType = InputType.TYPE_CLASS_TEXT
                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
        out.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI
                | EditorInfo.IME_FLAG_NO_FULLSCREEN
                | EditorInfo.IME_ACTION_NONE;
        ImeState state = imeState();
        out.initialSelStart = state.selectionStart;
        out.initialSelEnd = state.selectionEnd;
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            out.setInitialSurroundingText(state.text);
        }
        imeStart = state.start;
        return new OpenRichInputConnection(this);
    }

    /** Redraws and tells the IME where the selection is now. */
    void showKeyboard() {
        requestFocus();
        imm.showSoftInput(this, 0);
    }

    private void applyEffects() {
        try {
            String effects = OpenRichNative.takeEffects(handle);
            if (effects.length() > 2) log("effects " + effects);
            selection.apply(new JSONArray(effects));
        } catch (JSONException ignored) {
            // Effects are best effort.
        }
    }

    void afterInput() {
        applyEffects();
        scheduleFrame();
        if (batchDepth > 0) return;
        ImeState state = imeState();
        if (state.start != imeStart) {
            // The caret moved to another block: start a fresh input session.
            imeStart = state.start;
            imm.restartInput(this);
        } else {
            imm.updateSelection(this, state.selectionStart, state.selectionEnd,
                    state.compositionStart, state.compositionEnd);
        }
    }

    ImeState imeState() {
        return ImeState.parse(OpenRichNative.imeState(handle));
    }

    /** The IME block as the editor sees it. */
    static final class ImeState {
        int start = -1;
        String text = "";
        int selectionStart, selectionEnd;
        int compositionStart = -1, compositionEnd = -1;

        static ImeState parse(String json) {
            ImeState state = new ImeState();
            try {
                JSONObject object = new JSONObject(json);
                state.start = object.getInt("start");
                state.text = object.getString("text");
                JSONArray selection = object.getJSONArray("selection");
                int a = selection.getInt(0), b = selection.getInt(1);
                state.selectionStart = Math.min(a, b);
                state.selectionEnd = Math.max(a, b);
                JSONArray composition = object.optJSONArray("composition");
                if (composition != null) {
                    state.compositionStart = composition.getInt(0);
                    state.compositionEnd = composition.getInt(1);
                }
            } catch (JSONException ignored) {
                // An unreadable state reads as an empty block.
            }
            return state;
        }
    }
}
