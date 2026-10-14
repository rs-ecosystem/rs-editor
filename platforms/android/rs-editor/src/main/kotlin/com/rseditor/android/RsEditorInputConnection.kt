package com.rseditor.android

import android.os.Bundle
import android.text.TextUtils
import android.util.Log
import android.view.KeyEvent
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.CorrectionInfo
import android.view.inputmethod.ExtractedText
import android.view.inputmethod.ExtractedTextRequest
import android.view.inputmethod.InputContentInfo
import android.view.inputmethod.TextAttribute

/**
 * Turns IME calls into editing in Rust. The IME sees the textblock around
 * the caret (offsets in UTF-16 units); when the caret leaves the block the
 * view restarts input.
 */
internal class RsEditorInputConnection(private val view: RsEditorView) :
    BaseInputConnection(view, true) {
    private val native get() = view.native

    override fun beginBatchEdit(): Boolean {
        log("beginBatchEdit")
        view.batchDepth++
        return true
    }

    override fun endBatchEdit(): Boolean {
        log("endBatchEdit")
        if (view.batchDepth > 0 && --view.batchDepth == 0) view.afterInput()
        return view.batchDepth > 0
    }

    override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
        log("commitText \"$text\" $newCursorPosition")
        native.commitText(text.toString())
        view.afterInput()
        return true
    }

    override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
        log("setComposingText \"$text\" $newCursorPosition")
        native.setComposingText(text.toString())
        view.afterInput()
        return true
    }

    override fun setComposingRegion(start: Int, end: Int): Boolean {
        log("setComposingRegion $start $end")
        native.setComposingRegion(start, end)
        view.afterInput()
        return true
    }

    override fun finishComposingText(): Boolean {
        log("finishComposingText")
        native.finishComposing()
        view.afterInput()
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        log("deleteSurroundingText $beforeLength $afterLength")
        native.deleteSurrounding(beforeLength, afterLength)
        view.afterInput()
        return true
    }

    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean {
        val state = view.imeState()
        val text = state.text
        var before = 0
        val at = state.selectionStart
        repeat(beforeLength) {
            if (at - before > 0) before += Character.charCount(text.codePointBefore(at - before))
        }
        var after = 0
        val end = state.selectionEnd
        repeat(afterLength) {
            if (end + after < text.length) after += Character.charCount(text.codePointAt(end + after))
        }
        return deleteSurroundingText(before, after)
    }

    override fun replaceText(
        start: Int,
        end: Int,
        text: CharSequence,
        newCursorPosition: Int,
        textAttribute: TextAttribute?,
    ): Boolean {
        log("replaceText $start $end \"$text\" $newCursorPosition")
        native.replaceText(minOf(start, end), maxOf(start, end), text.toString())
        view.afterInput()
        return true
    }

    override fun requestCursorUpdates(cursorUpdateMode: Int): Boolean {
        log("requestCursorUpdates $cursorUpdateMode")
        view.monitorCursor = cursorUpdateMode and CURSOR_UPDATE_MONITOR != 0
        if (cursorUpdateMode and CURSOR_UPDATE_IMMEDIATE != 0) {
            view.sendCursorAnchorInfo(view.imeState())
        }
        return true
    }

    override fun requestCursorUpdates(cursorUpdateMode: Int, cursorUpdateFilter: Int): Boolean =
        requestCursorUpdates(cursorUpdateMode)

    override fun commitCorrection(correctionInfo: CorrectionInfo?): Boolean {
        log("commitCorrection $correctionInfo")
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        log("setSelection $start $end")
        native.setSelection(start, end)
        view.afterInput()
        return true
    }

    override fun getTextBeforeCursor(length: Int, flags: Int): CharSequence {
        val state = view.imeState()
        val end = state.selectionStart
        return state.text.substring(maxOf(0, end - length), end)
    }

    override fun getTextAfterCursor(length: Int, flags: Int): CharSequence {
        val state = view.imeState()
        val start = state.selectionEnd
        return state.text.substring(start, minOf(state.text.length, start + length))
    }

    override fun getSelectedText(flags: Int): CharSequence? {
        val state = view.imeState()
        if (state.selectionStart == state.selectionEnd) return null
        return state.text.substring(state.selectionStart, state.selectionEnd)
    }

    override fun getExtractedText(request: ExtractedTextRequest?, flags: Int): ExtractedText {
        val state = view.imeState()
        return ExtractedText().apply {
            text = state.text
            startOffset = 0
            selectionStart = state.selectionStart
            selectionEnd = state.selectionEnd
            this.flags = 0
        }
    }

    override fun getCursorCapsMode(reqModes: Int): Int {
        val state = view.imeState()
        return TextUtils.getCapsMode(state.text, state.selectionStart, reqModes)
    }

    override fun sendKeyEvent(event: KeyEvent): Boolean {
        log("sendKeyEvent $event")
        if (event.action == KeyEvent.ACTION_DOWN && view.handleKey(event)) view.afterInput()
        return true
    }

    override fun performEditorAction(actionCode: Int): Boolean {
        log("performEditorAction $actionCode")
        native.keyDown("Enter", false, false, false)
        view.afterInput()
        return true
    }

    /** A GIF or sticker from the keyboard (Gboard): inserted as an image. */
    override fun commitContent(info: InputContentInfo, flags: Int, opts: Bundle?): Boolean {
        log("commitContent ${info.description}")
        return view.insertKeyboardImage(info, flags)
    }

    override fun performContextMenuAction(id: Int): Boolean {
        when (id) {
            android.R.id.selectAll -> native.keyDown("a", false, true, false)
            android.R.id.copy, android.R.id.cut ->
                view.selectionUi().setClipboardJson(native.copy(id == android.R.id.cut))
            android.R.id.paste -> view.selectionUi().paste()
            else -> return false
        }
        view.afterInput()
        return true
    }

    private companion object {
        const val TAG = "RsEditorIme"

        fun log(message: String) {
            if (Log.isLoggable(TAG, Log.DEBUG)) Log.d(TAG, message)
        }
    }
}
