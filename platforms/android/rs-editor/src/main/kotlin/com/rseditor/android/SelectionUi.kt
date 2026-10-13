package com.rseditor.android

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.ResolveInfo
import android.graphics.Rect
import android.net.Uri
import android.os.Build
import android.view.ActionMode
import android.view.HapticFeedbackConstants
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.widget.Magnifier
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import kotlin.math.ceil

/**
 * The platform side of touch selection: Android's own floating text menu
 * (with "process text" actions such as Translate), its magnifier and its
 * haptics. The Rust view decides when; this class only shows them.
 */
internal class SelectionUi(private val view: RsEditorView) : ActionMode.Callback2() {
    private val contentRect = Rect()
    private var items: List<String> = emptyList()
    private val processText: List<ResolveInfo> by lazy {
        val intent = Intent(Intent.ACTION_PROCESS_TEXT).setType("text/plain")
        view.context.packageManager.queryIntentActivities(intent, 0)
    }
    private var mode: ActionMode? = null
    private var magnifier: Magnifier? = null

    /** Applies effects from [NativeEditor.takeEffects]. */
    fun apply(effects: JSONArray) {
        for (i in 0 until effects.length()) {
            val effect = effects.optJSONObject(i) ?: continue
            when (effect.optString("type")) {
                "haptic" -> haptic(effect.optString("kind"))
                "menu" -> effect.optJSONObject("menu")?.let(::showMenu) ?: hideMenu()
                "magnifier" -> effect.optJSONArray("at")
                    ?.let { showMagnifier(it.optDouble(0).toFloat(), it.optDouble(1).toFloat()) }
                    ?: dismissMagnifier()
                "showKeyboard" -> view.showKeyboard()
                "clipboard" -> setClipboard(effect.optString("text"), effect.optString("html"))
                "openUrl" -> openUrl(effect.optString("url"))
                "missingImages" -> effect.optJSONArray("sources")?.let { sources ->
                    for (j in 0 until sources.length()) view.loadImage(sources.optString(j))
                }
                "pickImage" -> view.pickImage()
            }
        }
    }

    fun release() {
        hideMenu()
        dismissMagnifier()
    }

    // --- haptics ------------------------------------------------------------------

    private fun haptic(kind: String) {
        val constant = when {
            kind == "longPress" -> HapticFeedbackConstants.LONG_PRESS
            Build.VERSION.SDK_INT >= 27 -> HapticFeedbackConstants.TEXT_HANDLE_MOVE
            else -> HapticFeedbackConstants.CLOCK_TICK
        }
        // Follows the user's touch feedback setting.
        view.performHapticFeedback(constant)
    }

    // --- magnifier ----------------------------------------------------------------

    private fun showMagnifier(x: Float, y: Float) {
        if (Build.VERSION.SDK_INT < 28) return
        val magnifier = magnifier
            ?: (if (Build.VERSION.SDK_INT >= 29) Magnifier.Builder(view).build() else Magnifier(view))
                .also { magnifier = it }
        magnifier.show(x, y)
    }

    private fun dismissMagnifier() {
        if (Build.VERSION.SDK_INT >= 28) magnifier?.dismiss()
    }

    // --- floating menu ------------------------------------------------------------

    private fun showMenu(menu: JSONObject) {
        menu.optJSONArray("rect")?.let { rect ->
            contentRect.set(
                rect.optDouble(0).toInt(), rect.optDouble(1).toInt(),
                ceil(rect.optDouble(2)).toInt(), ceil(rect.optDouble(3)).toInt(),
            )
        }
        val list = menu.optJSONArray("items")
        items = (0 until (list?.length() ?: 0)).map { list!!.optString(it) }
        val current = mode
        if (current == null) {
            mode = view.startActionMode(this, ActionMode.TYPE_FLOATING)
        } else {
            current.invalidate()
            current.invalidateContentRect()
        }
    }

    private fun hideMenu() {
        val closing = mode ?: return
        mode = null
        closing.finish()
    }

    override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
        populate(menu)
        return true
    }

    override fun onPrepareActionMode(mode: ActionMode, menu: Menu): Boolean {
        menu.clear()
        populate(menu)
        return true
    }

    private fun populate(menu: Menu) {
        items.forEachIndexed { order, item ->
            when (item) {
                "cut" -> add(menu, ID_CUT, order, android.R.string.cut)
                "copy" -> add(menu, ID_COPY, order, android.R.string.copy)
                "paste" -> add(menu, ID_PASTE, order, android.R.string.paste)
                "selectAll" -> add(menu, ID_SELECT_ALL, order, android.R.string.selectAll)
            }
        }
        // Actions other apps offer on selected text (Translate, search...).
        if ("copy" in items) {
            val pm = view.context.packageManager
            processText.forEachIndexed { i, action ->
                menu.add(Menu.NONE, ID_PROCESS_TEXT + i, 100 + i, action.loadLabel(pm))
                    .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER)
            }
        }
    }

    private fun add(menu: Menu, id: Int, order: Int, title: Int) {
        menu.add(Menu.NONE, id, order, title).setShowAsAction(MenuItem.SHOW_AS_ACTION_IF_ROOM)
    }

    override fun onActionItemClicked(mode: ActionMode, menuItem: MenuItem): Boolean {
        val id = menuItem.itemId
        if (id >= ID_PROCESS_TEXT) {
            startProcessText(processText[id - ID_PROCESS_TEXT])
            hideMenu()
            return true
        }
        val item = when (id) {
            ID_CUT -> "cut"
            ID_COPY -> "copy"
            ID_PASTE -> "paste"
            ID_SELECT_ALL -> "selectAll"
            else -> return false
        }
        var text: String? = null
        var html: String? = null
        if (item == "paste") {
            clipItem()?.let { clip ->
                html = clip.htmlText
                text = clip.coerceToText(view.context)?.toString()
            }
        }
        setClipboardJson(view.native.menuAction(item, text, html))
        view.afterInput()
        return true
    }

    override fun onDestroyActionMode(destroyed: ActionMode) {
        // Still current means the system closed it, not the editor.
        if (mode === destroyed) {
            mode = null
            view.native.menuDismissed()
        }
    }

    override fun onGetContentRect(mode: ActionMode, view: View, outRect: Rect) {
        outRect.set(contentRect)
    }

    private fun startProcessText(action: ResolveInfo) {
        val state = view.imeState()
        val text = state.text.substring(state.selectionStart, state.selectionEnd)
        val intent = Intent(Intent.ACTION_PROCESS_TEXT)
            .setType("text/plain")
            .setClassName(action.activityInfo.packageName, action.activityInfo.name)
            .putExtra(Intent.EXTRA_PROCESS_TEXT, text)
            .putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY, true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            view.context.startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            // The app went away; nothing to do.
        } catch (_: SecurityException) {
            // The app refuses; nothing to do.
        }
    }

    // --- clipboard and links ------------------------------------------------------

    private fun clipboard(): ClipboardManager =
        view.context.getSystemService(ClipboardManager::class.java)

    /** Puts copied content on the clipboard, as HTML with a plain text fallback. */
    private fun setClipboard(text: String?, html: String?) {
        if (text.isNullOrEmpty() && html.isNullOrEmpty()) return
        val clip = if (html.isNullOrEmpty()) {
            ClipData.newPlainText("Rs Editor", text)
        } else {
            ClipData.newHtmlText("Rs Editor", text.orEmpty(), html)
        }
        clipboard().setPrimaryClip(clip)
    }

    /** Applies `{text, html}` JSON from the editor, if any. */
    fun setClipboardJson(json: String) {
        if (json.isEmpty()) return
        try {
            val copied = JSONObject(json)
            setClipboard(copied.optString("text"), copied.optString("html"))
        } catch (_: JSONException) {
            // Nothing to copy.
        }
    }

    private fun clipItem(): ClipData.Item? {
        val clip = clipboard().primaryClip ?: return null
        return if (clip.itemCount == 0) null else clip.getItemAt(0)
    }

    /** Pastes the clipboard: HTML when present, otherwise text. */
    fun paste() {
        val clip = clipItem() ?: return
        val html = clip.htmlText
        if (html != null && view.native.pasteHtml(html)) return
        clip.coerceToText(view.context)?.let { view.native.paste(it.toString()) }
    }

    private fun openUrl(url: String) {
        try {
            view.context.startActivity(
                Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
        } catch (_: ActivityNotFoundException) {
            // No browser.
        }
    }

    private companion object {
        const val ID_CUT = 1
        const val ID_COPY = 2
        const val ID_PASTE = 3
        const val ID_SELECT_ALL = 4
        const val ID_PROCESS_TEXT = 100
    }
}
