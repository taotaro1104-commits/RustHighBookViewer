package jp.perfectviewer.rs

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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

/** 文字の大きさ・行間・文字間の範囲（Rust 側の上限・下限と合わせる） */
internal const val SIZE_MIN = 28f
internal const val SIZE_MAX = 160f
private const val SIZE_STEP = 4f
internal const val SPACING_MIN = 100f
internal const val SPACING_MAX = 240f
private const val SPACING_STEP = 5f
internal const val LETTER_MIN = -10f
internal const val LETTER_MAX = 60f
private const val LETTER_STEP = 5f

internal fun typefaceLabel(t: PvTypeface) = when (t) {
    PvTypeface.GOTHIC -> "ゴシック体"
    PvTypeface.MINCHO -> "明朝体"
}

/** 「小さく ──●── 大きく」のような、ボタン付きのスライダー（刻みで動く） */
@Composable
private fun StepSlider(value: Float, min: Float, max: Float, step: Float, less: String, more: String, onChange: (Float) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        TextButton(onClick = { onChange((value - step).coerceAtLeast(min)) }) { Text(less) }
        Slider(
            value = value,
            onValueChange = { onChange(it) },
            valueRange = min..max,
            steps = ((max - min) / step).toInt() - 1,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = { onChange((value + step).coerceAtMost(max)) }) { Text(more) }
    }
}

/**
 * 文章の本の見た目（文字の大きさ・行間・文字間・書体・太字・斜体・色）。変えるとページの区切りが変わるので、
 * 適用すると本を開き直す（今のページは、全体に対する位置で引き継ぐ）。
 */
@Composable
fun TextStyleDialog(state: AppState, onApply: (PvTextStyle) -> Unit, onClose: () -> Unit) {
    val current = remember { state.core.textStyle() }
    val faces = remember { state.core.availableTypefaces() }
    var size by remember { mutableFloatStateOf(current.size.toFloat().coerceIn(SIZE_MIN, SIZE_MAX)) }
    var face by remember { mutableStateOf(current.typeface) }
    var ink by remember { mutableStateOf(current.ink.toInt()) }
    var paper by remember { mutableStateOf(current.paper.toInt()) }
    var spacing by remember { mutableFloatStateOf(current.lineSpacing.toFloat().coerceIn(SPACING_MIN, SPACING_MAX)) }
    var letter by remember { mutableFloatStateOf(current.letterSpacing.toFloat().coerceIn(LETTER_MIN, LETTER_MAX)) }
    var bold by remember { mutableStateOf(current.bold) }
    var italic by remember { mutableStateOf(current.italic) }
    var vertical by remember { mutableStateOf(current.vertical) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("文字の設定") },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("文字の向き")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    FilterChip(selected = vertical, onClick = { vertical = true }, label = { Text("縦書き") })
                    FilterChip(selected = !vertical, onClick = { vertical = false }, label = { Text("横書き") })
                }
                Text(Tr.f("文字の大きさ  {1}", size.roundToInt()))
                StepSlider(size, SIZE_MIN, SIZE_MAX, SIZE_STEP, "小さく", "大きく") { size = it }
                Text(Tr.f("行間  {1}%", spacing.roundToInt()))
                StepSlider(spacing, SPACING_MIN, SPACING_MAX, SPACING_STEP, "狭く", "広く") { spacing = it }
                Text(Tr.f("文字間  {1}%", letter.roundToInt()))
                StepSlider(letter, LETTER_MIN, LETTER_MAX, LETTER_STEP, "詰める", "空ける") { letter = it }
                Text("文字の色")
                ColorSwatches(INK_COLORS, ink) { ink = it }
                Text("背景の色")
                ColorSwatches(PAPER_COLORS, paper) { paper = it }
                ColorPreview(ink, paper, bold, italic)
                Text("書体")
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    faces.forEach { t ->
                        FilterChip(selected = face == t, onClick = { face = t }, label = { Text(typefaceLabel(t)) })
                    }
                    FilterChip(selected = bold, onClick = { bold = !bold }, label = { Text("太字") })
                    FilterChip(selected = italic, onClick = { italic = !italic }, label = { Text("斜体") })
                }
                if (faces.size < 2) Text("この端末には、ほかの書体のフォントがありません。", style = androidx.compose.material3.MaterialTheme.typography.labelSmall)
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onApply(
                    PvTextStyle(
                        size.roundToInt().toUInt(), face, spacing.roundToInt().toUInt(), ink.toUInt(), paper.toUInt(),
                        letter.roundToInt(), bold, italic, vertical,
                    ),
                )
            }) { Text("適用") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("やめる") } },
    )
}
