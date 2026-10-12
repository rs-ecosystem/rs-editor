package dev.openrich.android;

import android.text.TextUtils;
import android.util.Log;
import android.view.KeyEvent;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;

/**
 * Turns IME calls into editing in Rust. The IME sees the textblock around
 * the caret (offsets in UTF-16 units); when the caret leaves the block the
 * view restarts input.
 */
final class OpenRichInputConnection extends BaseInputConnection {
    private static final String TAG = "OpenRichIme";
    private final OpenRichView view;
    private final long handle;

    private static void log(String message) {
        if (Log.isLoggable(TAG, Log.DEBUG)) Log.d(TAG, message);
    }

    OpenRichInputConnection(OpenRichView view) {
        super(view, true);
        this.view = view;
        this.handle = view.handle;
    }

    @Override
    public boolean beginBatchEdit() {
        log("beginBatchEdit");
        view.batchDepth++;
        return true;
    }

    @Override
    public boolean endBatchEdit() {
        log("endBatchEdit");
        if (view.batchDepth > 0 && --view.batchDepth == 0) view.afterInput();
        return view.batchDepth > 0;
    }

    @Override
    public boolean commitText(CharSequence text, int newCursorPosition) {
        log("commitText \"" + text + "\" " + newCursorPosition);
        OpenRichNative.commitText(handle, text.toString());
        view.afterInput();
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int newCursorPosition) {
        log("setComposingText \"" + text + "\" " + newCursorPosition);
        OpenRichNative.setComposingText(handle, text.toString());
        view.afterInput();
        return true;
    }

    @Override
    public boolean setComposingRegion(int start, int end) {
        log("setComposingRegion " + start + " " + end);
        OpenRichNative.setComposingRegion(handle, start, end);
        view.afterInput();
        return true;
    }

    @Override
    public boolean finishComposingText() {
        log("finishComposingText");
        OpenRichNative.finishComposing(handle);
        view.afterInput();
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int beforeLength, int afterLength) {
        log("deleteSurroundingText " + beforeLength + " " + afterLength);
        OpenRichNative.deleteSurrounding(handle, beforeLength, afterLength);
        view.afterInput();
        return true;
    }

    @Override
    public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
        OpenRichView.ImeState state = view.imeState();
        String text = state.text;
        int before = 0, at = state.selectionStart;
        for (int i = 0; i < beforeLength && at - before > 0; i++) {
            before += Character.charCount(text.codePointBefore(at - before));
        }
        int after = 0, end = state.selectionEnd;
        for (int i = 0; i < afterLength && end + after < text.length(); i++) {
            after += Character.charCount(text.codePointAt(end + after));
        }
        return deleteSurroundingText(before, after);
    }

    @Override
    public boolean replaceText(int start, int end, CharSequence text, int newCursorPosition,
            android.view.inputmethod.TextAttribute textAttribute) {
        log("replaceText " + start + " " + end + " \"" + text + "\" " + newCursorPosition);
        OpenRichNative.replaceText(handle, Math.min(start, end), Math.max(start, end), text.toString());
        view.afterInput();
        return true;
    }

    @Override
    public boolean requestCursorUpdates(int cursorUpdateMode) {
        log("requestCursorUpdates " + cursorUpdateMode);
        view.monitorCursor = (cursorUpdateMode & CURSOR_UPDATE_MONITOR) != 0;
        if ((cursorUpdateMode & CURSOR_UPDATE_IMMEDIATE) != 0) {
            view.sendCursorAnchorInfo(view.imeState());
        }
        return true;
    }

    @Override
    public boolean requestCursorUpdates(int cursorUpdateMode, int cursorUpdateFilter) {
        return requestCursorUpdates(cursorUpdateMode);
    }

    @Override
    public boolean commitCorrection(android.view.inputmethod.CorrectionInfo correctionInfo) {
        log("commitCorrection " + correctionInfo);
        return true;
    }

    @Override
    public boolean setSelection(int start, int end) {
        log("setSelection " + start + " " + end);
        OpenRichNative.setSelection(handle, start, end);
        view.afterInput();
        return true;
    }

    @Override
    public CharSequence getTextBeforeCursor(int length, int flags) {
        OpenRichView.ImeState state = view.imeState();
        int end = state.selectionStart;
        return state.text.substring(Math.max(0, end - length), end);
    }

    @Override
    public CharSequence getTextAfterCursor(int length, int flags) {
        OpenRichView.ImeState state = view.imeState();
        int start = state.selectionEnd;
        return state.text.substring(start, Math.min(state.text.length(), start + length));
    }

    @Override
    public CharSequence getSelectedText(int flags) {
        OpenRichView.ImeState state = view.imeState();
        if (state.selectionStart == state.selectionEnd) return null;
        return state.text.substring(state.selectionStart, state.selectionEnd);
    }

    @Override
    public ExtractedText getExtractedText(ExtractedTextRequest request, int flags) {
        OpenRichView.ImeState state = view.imeState();
        ExtractedText extracted = new ExtractedText();
        extracted.text = state.text;
        extracted.startOffset = 0;
        extracted.selectionStart = state.selectionStart;
        extracted.selectionEnd = state.selectionEnd;
        extracted.flags = 0;
        return extracted;
    }

    @Override
    public int getCursorCapsMode(int reqModes) {
        OpenRichView.ImeState state = view.imeState();
        return TextUtils.getCapsMode(state.text, state.selectionStart, reqModes);
    }

    @Override
    public boolean sendKeyEvent(KeyEvent event) {
        log("sendKeyEvent " + event);
        if (event.getAction() == KeyEvent.ACTION_DOWN && view.handleKey(event)) {
            view.afterInput();
        }
        return true;
    }

    @Override
    public boolean performEditorAction(int actionCode) {
        log("performEditorAction " + actionCode);
        OpenRichNative.keyDown(handle, "Enter", false, false, false);
        view.afterInput();
        return true;
    }

    @Override
    public boolean performContextMenuAction(int id) {
        if (id == android.R.id.selectAll) {
            OpenRichNative.keyDown(handle, "a", false, true, false);
        } else if (id == android.R.id.copy || id == android.R.id.cut) {
            view.selectionUi().setClipboardJson(OpenRichNative.copy(handle, id == android.R.id.cut));
        } else if (id == android.R.id.paste) {
            view.selectionUi().paste();
        } else {
            return false;
        }
        view.afterInput();
        return true;
    }
}
