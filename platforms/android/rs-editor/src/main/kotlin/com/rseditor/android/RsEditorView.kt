package com.rseditor.android

import android.content.Context
import android.content.res.Configuration
import android.graphics.Matrix
import android.graphics.Rect
import android.graphics.RectF
import android.os.Build
import android.text.InputType
import android.util.AttributeSet
import android.util.Log
import android.view.Choreographer
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowInsets
import android.view.WindowInsetsAnimation
import android.view.inputmethod.CursorAnchorInfo
import android.view.inputmethod.EditorBoundsInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

/**
 * Rs Editor as an Android view. Rust draws it with Skia into this
 * SurfaceView, recognizes touch gestures and edits through the IME protocol;
 * this class connects the frame loop, the input method, the selection menu,
 * the clipboard and accessibility.
 *
 * Content and commands follow Tiptap: [json], [html], [chain], [can],
 * [isActive], [getAttributes]. Call [release] when the view is discarded.
 */
class RsEditorView private constructor(
    context: Context,
    attrs: AttributeSet?,
    content: String,
    markdown: Boolean,
) : SurfaceView(context, attrs), SurfaceHolder.Callback2, Choreographer.FrameCallback {

    @JvmOverloads
    constructor(context: Context, attrs: AttributeSet? = null) : this(context, attrs, "", false)

    /**
     * An editor starting with `content`: a Tiptap JSON document or HTML, or Markdown when
     * [markdown]. Unlike setting [json] later, this is not an undoable change.
     */
    @JvmOverloads
    constructor(context: Context, content: String, markdown: Boolean = false) :
        this(context, null, content, markdown)

    /** Told whenever the document or the selection may have changed. */
    fun interface OnChangeListener {
        fun onChange(editor: RsEditorView)
    }

    private val density = resources.displayMetrics.density
    private val imm = context.getSystemService(InputMethodManager::class.java)
    internal val native = NativeEditor(1f, 1f, density, systemDark(), content, markdown)
    private val selection = SelectionUi(this)
    private val listeners = mutableListOf<OnChangeListener>()
    private var surfaceReady = false
    private var frameScheduled = false
    private var imeStart = Int.MIN_VALUE
    private var seenRevision = native.revision()
    private var released = false
    internal var batchDepth = 0
    /** The IME asked for CursorAnchorInfo after every change. */
    internal var monitorCursor = false

    /** null follows the system's dark theme; true or false forces one. */
    var dark: Boolean? = null
        set(value) {
            field = value
            native.setDark(value ?: systemDark())
            scheduleFrame()
        }

    /** Pixels at the bottom of the view the keyboard covers right now. */
    private var keyboardOverlap = 0
    /** The keyboard is sliding; window insets already hold where it will end. */
    private var keyboardAnimating = false

    /**
     * Pixels at the bottom covered by the app's own UI drawn over the editor, such as a
     * formatting toolbar above the keyboard. The keyboard itself is tracked: the editor keeps
     * its size while the keyboard slides over it, and the caret scrolls out from under both.
     */
    var overlayBottom: Int = 0
        set(value) {
            if (field == value) return
            field = value
            updateObscured()
        }

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        holder.addCallback(this)
        // The editor's own menus speak the user's language.
        native.setLocale(java.util.Locale.getDefault().toLanguageTag())
        if (Build.VERSION.SDK_INT >= 30) {
            // Follow the keyboard frame by frame while it slides.
            setWindowInsetsAnimationCallback(object :
                WindowInsetsAnimation.Callback(DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
                override fun onPrepare(animation: WindowInsetsAnimation) {
                    if (animation.isKeyboard()) keyboardAnimating = true
                }

                override fun onProgress(
                    insets: WindowInsets,
                    running: MutableList<WindowInsetsAnimation>,
                ): WindowInsets {
                    if (running.any { it.isKeyboard() }) trackKeyboard(insets, animated = true)
                    return insets
                }

                override fun onEnd(animation: WindowInsetsAnimation) {
                    if (!animation.isKeyboard()) return
                    keyboardAnimating = false
                    rootWindowInsets?.let(::trackKeyboard)
                }

                private fun WindowInsetsAnimation.isKeyboard() =
                    typeMask and WindowInsets.Type.ime() != 0
            })
        }
        NativeSurface.attachAccessibility(handle(), this) { afterInput() }
    }

    internal fun handle(): Long = native.boltffiHandle()

    /** Frees the Rust editor; the view is unusable afterwards. */
    fun release() {
        if (released) return
        released = true
        selection.release()
        holder.removeCallback(this)
        surfaceReady = false
        native.close()
    }

    // --- content and commands (Tiptap's API) ----------------------------------

    /** The document as Tiptap JSON; setting it replaces the document. */
    var json: String
        get() = native.getJson()
        set(value) {
            if (native.setJson(value)) afterInput()
        }

    /** The document as Tiptap-style HTML; setting it parses HTML like a paste. */
    var html: String
        get() = native.getHtml()
        set(value) {
            if (native.setHtml(value)) afterInput()
        }

    /** Plain text, blocks separated by newlines. */
    val text: String get() = native.getText()

    /**
     * The document as GitHub-flavored Markdown (marks Markdown lacks are written as HTML
     * tags); setting it replaces the document.
     */
    var markdown: String
        get() = native.getMarkdown()
        set(value) {
            if (native.setMarkdown(value)) afterInput()
        }

    /** Read pasted plain text as Markdown (off by default). */
    var pasteMarkdown: Boolean = false
        set(value) {
            field = value
            native.setMarkdownPaste(value)
        }

    /** Shows a menu of blocks (headings, lists, quote, code…) when `/` is typed (on by default). */
    var slashMenu: Boolean = true
        set(value) {
            field = value
            native.setSlashMenu(value)
            scheduleFrame()
        }

    /**
     * Opens the link address field over the link at the caret or the selected text, for an
     * app's link button. Returns false when there is nothing to link.
     */
    fun editLink(): Boolean {
        if (!hasFocus()) requestFocus()
        val opened = native.editLink()
        afterInput()
        return opened
    }

    val isEmpty: Boolean get() = native.isEmpty()

    var isEditable: Boolean
        get() = native.isEditable()
        set(value) {
            native.setEditable(value)
            if (!value) imm.hideSoftInputFromWindow(windowToken, 0)
            afterInput()
        }

    /** Changes whenever the document or the selection may have changed. */
    val revision: Long get() = native.revision().toLong()

    /** Commands to run as one transaction: `chain().toggleBold().run()`. */
    fun chain(): RsEditorChain = RsEditorChain(this, dryRun = false)

    /** Whether a chain would apply, without changing anything: `can().toggleBold().run()`. */
    fun can(): RsEditorChain = RsEditorChain(this, dryRun = true)

    /** Runs one command by name; false when it did not apply. */
    fun run(name: String, args: Any? = null): Boolean = chain().command(name, args).run()

    /** Whether the mark or node `name` (with `attrs`, if given) is active at the selection. */
    fun isActive(name: String, attrs: Map<String, Any?> = emptyMap()): Boolean =
        native.isActive(name, RsEditorChain.json(attrs).toString())

    /** Attributes of the mark or node `name` at the selection. */
    fun getAttributes(name: String): Map<String, Any?> =
        try {
            JSONObject(native.getAttributes(name)).toMap()
        } catch (_: JSONException) {
            emptyMap()
        }

    fun addOnChangeListener(listener: OnChangeListener) {
        listeners += listener
    }

    fun removeOnChangeListener(listener: OnChangeListener) {
        listeners -= listener
    }

    internal fun runSteps(steps: String, dryRun: Boolean, focus: Boolean): Boolean {
        if (released) return false
        if (dryRun) return native.canChain(steps)
        val applied = native.runChain(steps)
        if (focus) showKeyboard()
        afterInput()
        return applied
    }

    // --- surface and frames ----------------------------------------------------

    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = NativeSurface.attachSurface(handle(), holder.surface)
        log("surfaceCreated ${holder.surfaceFrame} ready=$surfaceReady")
        scheduleFrame()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        log("surfaceChanged ${width}x$height")
        NativeSurface.resize(handle(), width, height, density)
        drawNow()
    }

    /**
     * The surface must show a frame of its new size before the window does:
     * otherwise a resize (a keyboard sliding in or out, animated frame by
     * frame) shows the empty buffer.
     */
    override fun surfaceRedrawNeeded(holder: SurfaceHolder) {
        drawNow()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        log("surfaceDestroyed")
        surfaceReady = false
        if (!released) NativeSurface.detachSurface(handle())
    }

    internal fun scheduleFrame() {
        if (frameScheduled || released) return
        frameScheduled = true
        Choreographer.getInstance().postFrameCallback(this)
    }

    override fun doFrame(frameTimeNanos: Long) {
        frameScheduled = false
        drawNow()
    }

    private fun drawNow() {
        if (!surfaceReady || released) return
        var next = NativeSurface.render(handle())
        // Right after a resize EGL can still hand out a buffer of the old
        // size; the next frame gets the new one.
        if (next == NativeSurface.RESIZING) next = NativeSurface.render(handle())
        // A frame can start a long press or bring the menu back after a fling.
        applyEffects()
        notifyIfChanged()
        when {
            next == NativeSurface.RESIZING || next in 0..16 -> scheduleFrame()
            next > 16 -> postDelayed(::scheduleFrame, next - 8)
        }
    }

    override fun onConfigurationChanged(config: Configuration) {
        super.onConfigurationChanged(config)
        if (dark == null && !released) {
            native.setDark(systemDark())
            scheduleFrame()
        }
    }

    private fun systemDark(): Boolean =
        resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK ==
            Configuration.UI_MODE_NIGHT_YES

    // --- touch -----------------------------------------------------------------

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val action = when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> 0
            MotionEvent.ACTION_MOVE -> 1
            MotionEvent.ACTION_UP -> 2
            MotionEvent.ACTION_CANCEL -> 3
            else -> return true
        }
        if (released) return false
        native.touch(action, event.x, event.y)
        afterInput()
        return true
    }

    override fun onApplyWindowInsets(insets: WindowInsets): WindowInsets {
        trackKeyboard(insets)
        return super.onApplyWindowInsets(insets)
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        super.onLayout(changed, left, top, right, bottom)
        rootWindowInsets?.let(::trackKeyboard)
    }

    /** How much of this view the keyboard covers, from the window's insets. */
    private fun trackKeyboard(insets: WindowInsets, animated: Boolean = false) {
        if (Build.VERSION.SDK_INT < 30) return // The window resizes instead.
        if (keyboardAnimating && !animated) return
        val keyboard = insets.getInsets(WindowInsets.Type.ime()).bottom
        val location = IntArray(2)
        getLocationInWindow(location)
        val keyboardTop = rootView.height - keyboard
        val overlap = (location[1] + height - keyboardTop).coerceIn(0, height)
        if (overlap == keyboardOverlap) return
        keyboardOverlap = overlap
        updateObscured()
    }

    private fun updateObscured() {
        if (released) return
        native.setObscuredBottom((keyboardOverlap + overlayBottom).toFloat())
        // Draw now so the text moves in step with the keyboard.
        drawNow()
        // The caret moved on screen.
        if (monitorCursor && batchDepth == 0) sendCursorAnchorInfo(imeState())
    }

    override fun onFocusChanged(focused: Boolean, direction: Int, previous: Rect?) {
        super.onFocusChanged(focused, direction, previous)
        if (released) return
        native.setFocused(focused)
        scheduleFrame()
    }

    // --- keys ------------------------------------------------------------------

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (!released && handleKey(event)) {
            afterInput()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    /** Hardware keys and keys sent by IMEs through sendKeyEvent. */
    internal fun handleKey(event: KeyEvent): Boolean {
        val name = keyName(event) ?: return false
        val ctrl = event.isCtrlPressed
        if (native.keyDown(name, event.isShiftPressed, ctrl, event.isAltPressed)) return true
        if (!ctrl && name.isNotEmpty() && !Character.isISOControl(name.codePointAt(0)) &&
            name.codePointCount(0, name.length) == 1
        ) {
            native.commitText(name)
            return true
        }
        return false
    }

    // --- IME -------------------------------------------------------------------

    override fun onCheckIsTextEditor(): Boolean = !released && native.isEditable()

    override fun onCreateInputConnection(out: EditorInfo): InputConnection? {
        if (released || !native.isEditable()) return null
        val state = imeState()
        if (state.start == LINK_FIELD) {
            // The link address field: a keyboard for URLs with a Go key.
            out.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            out.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or
                EditorInfo.IME_FLAG_NO_FULLSCREEN or
                EditorInfo.IME_ACTION_GO
        } else {
            out.inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_FLAG_MULTI_LINE or
                InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or
                InputType.TYPE_TEXT_FLAG_AUTO_CORRECT
            out.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or
                EditorInfo.IME_FLAG_NO_FULLSCREEN or
                EditorInfo.IME_ACTION_NONE
        }
        out.initialSelStart = state.selectionStart
        out.initialSelEnd = state.selectionEnd
        if (Build.VERSION.SDK_INT >= 30) out.setInitialSurroundingText(state.text)
        imeStart = state.start
        return RsEditorInputConnection(this)
    }

    internal fun selectionUi(): SelectionUi = selection

    internal fun showKeyboard() {
        requestFocus()
        imm.showSoftInput(this, 0)
    }

    private fun applyEffects() {
        try {
            val effects = native.takeEffects()
            if (effects.length > 2) log("effects $effects")
            selection.apply(JSONArray(effects))
        } catch (_: JSONException) {
            // Effects are best effort.
        }
    }

    private fun notifyIfChanged() {
        val revision = native.revision()
        if (revision == seenRevision) return
        seenRevision = revision
        for (listener in listeners.toList()) listener.onChange(this)
    }

    /** Redraws, tells the IME where the selection is now and notifies listeners. */
    internal fun afterInput() {
        if (released) return
        applyEffects()
        scheduleFrame()
        notifyIfChanged()
        if (batchDepth > 0) return
        val state = imeState()
        if (state.start != imeStart) {
            // The caret moved to another block: start a fresh input session.
            imeStart = state.start
            imm.restartInput(this)
        } else {
            imm.updateSelection(
                this, state.selectionStart, state.selectionEnd,
                state.compositionStart, state.compositionEnd,
            )
            if (monitorCursor) sendCursorAnchorInfo(state)
        }
    }

    /**
     * Tells the IME where the caret and the composing characters are, so
     * candidate popups and handwriting line up with the text.
     */
    internal fun sendCursorAnchorInfo(state: ImeState) {
        val composing = state.compositionStart >= 0
        val from = if (composing) state.compositionStart else state.selectionStart
        val to = if (composing) state.compositionEnd else state.selectionEnd
        val builder = CursorAnchorInfo.Builder()
        builder.setSelectionRange(state.selectionStart, state.selectionEnd)
        if (composing) {
            builder.setComposingText(from, state.text.substring(from, minOf(to, state.text.length)))
        }
        try {
            val info = JSONObject(native.imeCursorInfo(from, to))
            info.optJSONArray("caret")?.let { caret ->
                val x = caret.getDouble(0).toFloat()
                val top = caret.getDouble(1).toFloat()
                val baseline = caret.getDouble(2).toFloat()
                val bottom = caret.getDouble(3).toFloat()
                builder.setInsertionMarkerLocation(x, top, baseline, bottom, visibility(x, top, bottom))
            }
            val bounds = info.optJSONArray("bounds")
            for (i in 0 until (bounds?.length() ?: 0)) {
                val r = bounds!!.optJSONArray(i) ?: continue
                val left = r.getDouble(0).toFloat()
                val top = r.getDouble(1).toFloat()
                val right = r.getDouble(2).toFloat()
                val bottom = r.getDouble(3).toFloat()
                builder.addCharacterBounds(from + i, left, top, right, bottom, visibility(left, top, bottom))
            }
        } catch (_: JSONException) {
            // Send what we have.
        }
        val location = IntArray(2)
        getLocationOnScreen(location)
        builder.setMatrix(Matrix().apply { setTranslate(location[0].toFloat(), location[1].toFloat()) })
        if (Build.VERSION.SDK_INT >= 33) {
            val bounds = RectF(0f, 0f, width.toFloat(), height.toFloat())
            builder.setEditorBoundsInfo(
                EditorBoundsInfo.Builder().setEditorBounds(bounds).setHandwritingBounds(bounds).build()
            )
        }
        imm.updateCursorAnchorInfo(this, builder.build())
    }

    private fun visibility(x: Float, top: Float, bottom: Float): Int {
        val visible = x >= 0 && x <= width && bottom >= 0 && top <= height
        return if (visible) CursorAnchorInfo.FLAG_HAS_VISIBLE_REGION else CursorAnchorInfo.FLAG_HAS_INVISIBLE_REGION
    }

    internal fun imeState(): ImeState = ImeState.parse(native.imeState())

    /** The IME block as the editor sees it (UTF-16 offsets). */
    internal class ImeState {
        var start = -1
        var text = ""
        var selectionStart = 0
        var selectionEnd = 0
        var compositionStart = -1
        var compositionEnd = -1

        companion object {
            fun parse(json: String): ImeState {
                val state = ImeState()
                try {
                    val obj = JSONObject(json)
                    state.start = obj.getInt("start")
                    state.text = obj.getString("text")
                    val selection = obj.getJSONArray("selection")
                    val a = selection.getInt(0)
                    val b = selection.getInt(1)
                    state.selectionStart = minOf(a, b)
                    state.selectionEnd = maxOf(a, b)
                    obj.optJSONArray("composition")?.let {
                        state.compositionStart = it.getInt(0)
                        state.compositionEnd = it.getInt(1)
                    }
                } catch (_: JSONException) {
                    // An unreadable state reads as an empty block.
                }
                return state
            }
        }
    }

    private companion object {
        const val TAG = "RsEditorView"
        /** `ImeState.start` while the link address field takes input. */
        const val LINK_FIELD = -2

        fun log(message: String) {
            if (Log.isLoggable(TAG, Log.DEBUG)) Log.d(TAG, message)
        }

        fun keyName(event: KeyEvent): String? = when (event.keyCode) {
            KeyEvent.KEYCODE_DEL -> "Backspace"
            KeyEvent.KEYCODE_FORWARD_DEL -> "Delete"
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> "Enter"
            KeyEvent.KEYCODE_TAB -> "Tab"
            KeyEvent.KEYCODE_ESCAPE -> "Escape"
            KeyEvent.KEYCODE_DPAD_LEFT -> "ArrowLeft"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "ArrowRight"
            KeyEvent.KEYCODE_DPAD_UP -> "ArrowUp"
            KeyEvent.KEYCODE_DPAD_DOWN -> "ArrowDown"
            KeyEvent.KEYCODE_MOVE_HOME -> "Home"
            KeyEvent.KEYCODE_MOVE_END -> "End"
            KeyEvent.KEYCODE_PAGE_UP -> "PageUp"
            KeyEvent.KEYCODE_PAGE_DOWN -> "PageDown"
            else -> {
                val meta = event.metaState and (KeyEvent.META_CTRL_MASK or KeyEvent.META_ALT_MASK).inv()
                val unicode = event.getUnicodeChar(meta)
                if (unicode > 0) String(Character.toChars(unicode)) else null
            }
        }
    }
}

/** JSON values as Kotlin maps and lists. */
internal fun JSONObject.toMap(): Map<String, Any?> =
    keys().asSequence().associateWith { key -> unwrap(opt(key)) }

private fun unwrap(value: Any?): Any? = when (value) {
    JSONObject.NULL, null -> null
    is JSONObject -> value.toMap()
    is JSONArray -> (0 until value.length()).map { unwrap(value.opt(it)) }
    else -> value
}
