package com.rseditor.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.rseditor.android.RsEditorChain
import com.rseditor.android.RsEditorView

/**
 * State of an [RsEditor]: the content to start with, and the editor's
 * commands once it is shown. Reads such as [isActive] are observable, so a
 * toolbar recomposes as the selection moves.
 */
@Stable
class RsEditorState internal constructor(
    internal val content: String,
    internal val contentIsMarkdown: Boolean,
) {
    /** The view while the editor is in the composition. */
    var view: RsEditorView? by mutableStateOf(null)
        internal set

    /** Changes whenever the document or the selection may have changed. */
    var revision: Long by mutableLongStateOf(0L)
        private set

    internal val onChange = RsEditorView.OnChangeListener { revision = it.revision }

    /** `chain().toggleBold().run()`; false while the editor is not shown. */
    fun chain(): RsEditorChain = view?.chain() ?: RsEditorChain.detached()

    /** Whether a chain would apply: `can().toggleBold().run()`. */
    fun can(): RsEditorChain = view?.can() ?: RsEditorChain.detached()

    fun isActive(name: String, attrs: Map<String, Any?> = emptyMap()): Boolean {
        revision // Recompose readers when the selection changes.
        return view?.isActive(name, attrs) ?: false
    }

    fun getAttributes(name: String): Map<String, Any?> {
        revision
        return view?.getAttributes(name).orEmpty()
    }

    /** The document as Tiptap JSON (null while the editor is not shown). */
    val json: String?
        get() {
            revision
            return view?.json
        }

    val html: String?
        get() {
            revision
            return view?.html
        }

    val markdown: String?
        get() {
            revision
            return view?.markdown
        }
}

/**
 * Remembers editor state starting from `content`: a Tiptap JSON document or HTML, or Markdown
 * when [markdown].
 */
@Composable
fun rememberRsEditorState(content: String = "", markdown: Boolean = false): RsEditorState =
    remember { RsEditorState(content, markdown) }

/**
 * The Rs Editor. [dark] null follows the system theme. The Rust editor
 * lives as long as this composable stays in the composition.
 *
 * Do not shrink the editor for the keyboard (no `imePadding`): it tracks the
 * keyboard sliding over it and scrolls the caret clear, which is smoother
 * than resizing its surface every frame. [overlayBottom] is the height of the
 * app's own UI drawn over its bottom, such as a toolbar above the keyboard.
 */
@Composable
fun RsEditor(
    state: RsEditorState,
    modifier: Modifier = Modifier,
    editable: Boolean = true,
    dark: Boolean? = null,
    overlayBottom: Dp = 0.dp,
) {
    val overlayPixels = with(LocalDensity.current) { overlayBottom.roundToPx() }
    AndroidView(
        factory = { context ->
            RsEditorView(context, state.content, state.contentIsMarkdown).also { view ->
                view.addOnChangeListener(state.onChange)
                state.view = view
            }
        },
        modifier = modifier,
        update = { view ->
            if (view.isEditable != editable) view.isEditable = editable
            if (view.dark != dark) view.dark = dark
            view.overlayBottom = overlayPixels
        },
        onRelease = { view ->
            view.removeOnChangeListener(state.onChange)
            if (state.view === view) state.view = null
            view.release()
        },
    )
}
