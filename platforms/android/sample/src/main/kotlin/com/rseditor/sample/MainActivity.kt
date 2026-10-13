package com.rseditor.sample

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import com.rseditor.android.RsEditorChain
import com.rseditor.compose.RsEditor
import com.rseditor.compose.RsEditorState
import com.rseditor.compose.rememberRsEditorState

/** The editor full screen with a formatting toolbar above the keyboard. */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val sample = assets.open("sample.json").use { it.readBytes().decodeToString() }
        setContent {
            // shadcn/ui neutral, as the editor itself.
            val colors = if (isSystemInDarkTheme()) {
                darkColorScheme(
                    primary = Color(0xFFFAFAFA), onPrimary = Color(0xFF171717),
                    background = Color(0xFF0A0A0A), surface = Color(0xFF0A0A0A),
                    secondaryContainer = Color(0xFF262626), onSecondaryContainer = Color(0xFFFAFAFA),
                    outlineVariant = Color(0xFF262626),
                )
            } else {
                lightColorScheme(
                    primary = Color(0xFF171717), onPrimary = Color(0xFFFAFAFA),
                    background = Color.White, surface = Color.White,
                    secondaryContainer = Color(0xFFF5F5F5), onSecondaryContainer = Color(0xFF171717),
                    outlineVariant = Color(0xFFE5E5E5),
                )
            }
            MaterialTheme(colorScheme = colors) {
                Surface(Modifier.fillMaxSize()) {
                    val editor = rememberRsEditorState(sample)
                    var toolbarHeight by remember { mutableStateOf(0.dp) }
                    val density = LocalDensity.current
                    // The editor keeps its size; the keyboard and the toolbar riding on
                    // it slide over its bottom.
                    Box(Modifier.fillMaxSize().systemBarsPadding()) {
                        RsEditor(editor, Modifier.fillMaxSize(), overlayBottom = toolbarHeight)
                        Column(
                            Modifier.align(Alignment.BottomCenter)
                                .imePadding()
                                .fillMaxWidth()
                                .background(MaterialTheme.colorScheme.surface)
                                .onSizeChanged { toolbarHeight = with(density) { it.height.toDp() } },
                        ) {
                            HorizontalDivider()
                            Toolbar(editor)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Toolbar(editor: RsEditorState) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 4.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Tool(editor, "B", "bold", weight = FontWeight.Bold) { toggleBold() }
        Tool(editor, "I", "italic", style = FontStyle.Italic) { toggleItalic() }
        Tool(editor, "U", "underline", decoration = TextDecoration.Underline) { toggleUnderline() }
        Tool(editor, "S", "strike", decoration = TextDecoration.LineThrough) { toggleStrike() }
        Tool(editor, "</>", "code") { toggleCode() }
        Tool(editor, "H1", "heading", mapOf("level" to 1)) { toggleHeading(1) }
        Tool(editor, "H2", "heading", mapOf("level" to 2)) { toggleHeading(2) }
        Tool(editor, "•", "bulletList") { toggleBulletList() }
        Tool(editor, "1.", "orderedList") { toggleOrderedList() }
        Tool(editor, "☑", "taskList") { toggleTaskList() }
        Tool(editor, "❝", "blockquote") { toggleBlockquote() }
        Tool(editor, "{ }", "codeBlock") { toggleCodeBlock() }
        Tool(editor, "🔗", "link", onClick = { editor.editLink() }) { this }
        Tool(editor, "↶") { undo() }
        Tool(editor, "↷") { redo() }
    }
}

/**
 * A toolbar button: highlighted while `name` is active at the selection,
 * disabled when the command cannot apply there.
 */
@Composable
private fun Tool(
    editor: RsEditorState,
    label: String,
    name: String? = null,
    attrs: Map<String, Any?> = emptyMap(),
    weight: FontWeight? = null,
    style: FontStyle? = null,
    decoration: TextDecoration? = null,
    /** Replaces running `command` (which then only decides whether the button is enabled). */
    onClick: (() -> Unit)? = null,
    command: RsEditorChain.() -> RsEditorChain,
) {
    val active = name != null && editor.isActive(name, attrs)
    val enabled = editor.revision.let { editor.can().command().run() }
    TextButton(
        onClick = { onClick?.invoke() ?: editor.chain().command().run() },
        enabled = enabled,
        modifier = Modifier.size(width = 44.dp, height = 40.dp),
        contentPadding = PaddingValues(0.dp),
        colors = if (active) {
            ButtonDefaults.textButtonColors(
                containerColor = MaterialTheme.colorScheme.secondaryContainer,
                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        } else {
            ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.onSurface)
        },
        shape = MaterialTheme.shapes.small,
    ) {
        Text(label, fontWeight = weight, fontStyle = style, textDecoration = decoration)
    }
}
