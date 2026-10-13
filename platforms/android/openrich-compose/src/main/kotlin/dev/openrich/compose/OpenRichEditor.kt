package dev.openrich.compose

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import dev.openrich.android.OpenRichChain
import dev.openrich.android.OpenRichEditorView

/**
 * State of an [OpenRichEditor]: the content to start with, and the editor's
 * commands once it is shown. Reads such as [isActive] are observable, so a
 * toolbar recomposes as the selection moves.
 */
@Stable
class OpenRichEditorState internal constructor(internal val content: String) {
    /** The view while the editor is in the composition. */
    var view: OpenRichEditorView? by mutableStateOf(null)
        internal set

    /** Changes whenever the document or the selection may have changed. */
    var revision: Long by mutableLongStateOf(0L)
        private set

    internal val onChange = OpenRichEditorView.OnChangeListener { revision = it.revision }

    /** `chain().toggleBold().run()`; false while the editor is not shown. */
    fun chain(): OpenRichChain = view?.chain() ?: OpenRichChain.detached()

    /** Whether a chain would apply: `can().toggleBold().run()`. */
    fun can(): OpenRichChain = view?.can() ?: OpenRichChain.detached()

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
}

/** Remembers editor state starting from `content`: a Tiptap JSON document or HTML. */
@Composable
fun rememberOpenRichEditorState(content: String = ""): OpenRichEditorState =
    remember { OpenRichEditorState(content) }

/**
 * The OpenRich editor. [dark] null follows the system theme. The Rust editor
 * lives as long as this composable stays in the composition.
 */
@Composable
fun OpenRichEditor(
    state: OpenRichEditorState,
    modifier: Modifier = Modifier,
    editable: Boolean = true,
    dark: Boolean? = null,
) {
    AndroidView(
        factory = { context ->
            OpenRichEditorView(context, state.content).also { view ->
                view.addOnChangeListener(state.onChange)
                state.view = view
            }
        },
        modifier = modifier,
        update = { view ->
            if (view.isEditable != editable) view.isEditable = editable
            if (view.dark != dark) view.dark = dark
        },
        onRelease = { view ->
            view.removeOnChangeListener(state.onChange)
            if (state.view === view) state.view = null
            view.release()
        },
    )
}
