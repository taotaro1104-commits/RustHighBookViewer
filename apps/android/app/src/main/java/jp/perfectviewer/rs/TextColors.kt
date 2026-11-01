package jp.perfectviewer.rs

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** 文字色の候補（0xRRGGBB）。暗い背景向けの明るい色も含む */
internal val INK_COLORS = listOf(0x1C1C1C, 0x4A3728, 0x1F3A5F, 0x2E5E3E, 0xBFBFBF, 0xFFFFFF)

/** 背景色の候補（0xRRGGBB）。白・生成り・セピア・うすい緑・濃いグレー・黒 */
internal val PAPER_COLORS = listOf(0xF5F5F5, 0xF8F1E0, 0xE9DCC0, 0xDDEBDD, 0x2B2B2B, 0x000000)

internal fun rgb(v: Int) = Color(0xFF000000.toInt() or (v and 0xFFFFFF))

private fun luminance(v: Int): Double {
    fun ch(shift: Int): Double {
        val c = ((v shr shift) and 0xFF) / 255.0
        return if (c <= 0.03928) c / 12.92 else Math.pow((c + 0.055) / 1.055, 2.4)
    }
    return 0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/** 文字色と背景色のコントラスト比（1〜21）。小さいと読みにくい */
internal fun contrastRatio(a: Int, b: Int): Double {
    val (hi, lo) = luminance(a).let { x -> luminance(b).let { y -> maxOf(x, y) to minOf(x, y) } }
    return (hi + 0.05) / (lo + 0.05)
}

internal const val MIN_READABLE_CONTRAST = 3.0

/** 色の見本を並べて、1 つ選ぶ。選んでいる色は縁を太く、色の名前の代わりに 16 進の値を読み上げる */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ColorSwatches(options: List<Int>, selected: Int, onSelect: (Int) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        options.forEach { c ->
            val on = c == (selected and 0xFFFFFF)
            Box(
                Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .background(rgb(c))
                    .border(
                        BorderStroke(if (on) 4.dp else 1.dp, if (on) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline),
                        CircleShape,
                    )
                    .semantics { contentDescription = "#%06X".format(c) }
                    .clickable { onSelect(c) },
            )
        }
    }
}

/** 選んだ色での見え方。コントラストが低いときは注意を出す */
@Composable
internal fun ColorPreview(ink: Int, paper: Int) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(
            Modifier.fillMaxWidth().clip(MaterialTheme.shapes.small).background(rgb(paper)).padding(12.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("吾輩は猫である。名前はまだ無い。", color = rgb(ink))
        }
        if (contrastRatio(ink, paper) < MIN_READABLE_CONTRAST) {
            Text(
                "文字の色と背景の色が近く、読みにくくなります。",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}
