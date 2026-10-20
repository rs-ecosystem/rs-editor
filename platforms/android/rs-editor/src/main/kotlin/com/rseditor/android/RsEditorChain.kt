package com.rseditor.android

import org.json.JSONArray
import org.json.JSONObject

/**
 * Commands to run together as one transaction (one undo step), like Tiptap's
 * `editor.chain()...run()`:
 *
 * ```
 * editor.chain().focus().toggleBold().toggleHeading(2).run()
 * editor.can().toggleBulletList().run()   // would it apply? changes nothing
 * ```
 *
 * [run] is true only if every command applied. Command names and arguments
 * are those of the Rust extensions; [command] reaches the ones without a
 * method here.
 */
class RsEditorChain internal constructor(
    private val editor: RsEditorView?,
    private val dryRun: Boolean,
) {
    private val steps = JSONArray()
    private var focus = false

    /** Any command by name, with JSON-compatible arguments. */
    fun command(name: String, args: Any? = null): RsEditorChain = apply {
        steps.put(JSONObject().put("name", name).put("args", json(args)))
    }

    /** Focuses the editor (and shows the keyboard) after running. */
    fun focus(): RsEditorChain = apply { focus = true }

    fun toggleBold() = command("toggleBold")
    fun toggleItalic() = command("toggleItalic")
    fun toggleUnderline() = command("toggleUnderline")
    fun toggleStrike() = command("toggleStrike")
    fun toggleCode() = command("toggleCode")
    fun toggleHighlight(color: String? = null) =
        command("toggleHighlight", color?.let { mapOf("color" to it) })
    fun toggleSubscript() = command("toggleSubscript")
    fun toggleSuperscript() = command("toggleSuperscript")
    fun unsetAllMarks() = command("unsetAllMarks")

    fun setParagraph() = command("setParagraph")
    fun toggleHeading(level: Int) = command("toggleHeading", mapOf("level" to level))
    fun toggleBulletList() = command("toggleBulletList")
    fun toggleOrderedList() = command("toggleOrderedList")
    fun toggleTaskList() = command("toggleTaskList")
    fun toggleBlockquote() = command("toggleBlockquote")
    fun toggleCodeBlock() = command("toggleCodeBlock")
    fun setHorizontalRule() = command("setHorizontalRule")
    fun setHardBreak() = command("setHardBreak")
    fun sinkListItem() = command("sinkListItem")
    fun liftListItem() = command("liftListItem")

    fun setLink(href: String) = command("setLink", mapOf("href" to href))
    fun unsetLink() = command("unsetLink")
    /** `left`, `center`, `right` or `justify`. */
    fun setTextAlign(alignment: String) = command("setTextAlign", alignment)
    fun unsetTextAlign() = command("unsetTextAlign")

    fun selectAll() = command("selectAll")
    fun undo() = command("undo")
    fun redo() = command("redo")

    /** Runs the chain (or, from [RsEditorView.can], checks it). */
    fun run(): Boolean {
        val editor = editor ?: return false
        return editor.runSteps(steps.toString(), dryRun, focus)
    }

    companion object {
        /** A chain with no editor behind it, whose [run] is false (for wrappers). */
        fun detached(): RsEditorChain = RsEditorChain(null, dryRun = false)

        /** JSON for an argument: maps, lists, strings, numbers, booleans or null. */
        internal fun json(value: Any?): Any = when (value) {
            null -> JSONObject.NULL
            is Map<*, *> -> JSONObject().also { obj ->
                value.forEach { (key, item) -> obj.put(key.toString(), json(item)) }
            }
            is Iterable<*> -> JSONArray().also { array -> value.forEach { array.put(json(it)) } }
            is Array<*> -> json(value.asList())
            else -> value
        }
    }
}
