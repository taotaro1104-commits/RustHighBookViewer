package jp.perfectviewer.rs

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Slider
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import uniffi.pv_ffi.PvTextStyle
import uniffi.pv_ffi.PvTypeface

private const val SIZE_MIN = 28f
private const val SIZE_MAX = 80f
private const val SIZE_STEP = 4f

private fun typefaceLabel(t: PvTypeface) = when (t) {
    PvTypeface.GOTHIC -> "ゴシック体"
    PvTypeface.MINCHO -> "明朝体"
}

/**
 * 文章の本の、文字の大きさと書体。変えるとページの区切りが変わるので、適用すると本を開き直す
 * （今のページは、全体に対する位置で引き継ぐ）。
 */
@Composable
fun TextStyleDialog(state: AppState, onApply: (PvTextStyle) -> Unit, onClose: () -> Unit) {
    val current = remember { state.core.textStyle() }
    val faces = remember { state.core.availableTypefaces() }
    var size by remember { mutableFloatStateOf(current.size.toFloat().coerceIn(SIZE_MIN, SIZE_MAX)) }
    var face by remember { mutableStateOf(current.typeface) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("文字の設定") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(Tr.f("文字の大きさ  {1}", size.roundToInt()))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = { size = (size - SIZE_STEP).coerceAtLeast(SIZE_MIN) }) { Text("小さく") }
                    Slider(
                        value = size,
                        onValueChange = { size = it },
                        valueRange = SIZE_MIN..SIZE_MAX,
                        steps = ((SIZE_MAX - SIZE_MIN) / SIZE_STEP).toInt() - 1,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = { size = (size + SIZE_STEP).coerceAtMost(SIZE_MAX) }) { Text("大きく") }
                }
                Text("書体")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    faces.forEach { t ->
                        FilterChip(selected = face == t, onClick = { face = t }, label = { Text(typefaceLabel(t)) })
                    }
                }
                if (faces.size < 2) Text("この端末には、ほかの書体のフォントがありません。", style = androidx.compose.material3.MaterialTheme.typography.labelSmall)
            }
        },
        confirmButton = {
            TextButton(onClick = { onApply(PvTextStyle(size.roundToInt().toUInt(), face)) }) { Text("適用") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("やめる") } },
    )
}
