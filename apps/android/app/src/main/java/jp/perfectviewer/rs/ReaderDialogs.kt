package jp.perfectviewer.rs

import android.graphics.Bitmap
import android.util.LruCache
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvColor
import uniffi.pv_ffi.PvReader

private const val THUMB_PARALLELISM = 2

/** 色調整。スライダーを離したとき（と切替ボタン）で [onChange] を呼び、閉じるまでは画面を再描画し続ける。 */
@Composable
fun ColorDialog(initial: PvColor, onChange: (PvColor) -> Unit, onDismiss: () -> Unit) {
    var c by remember { mutableStateOf(initial) }
    var draft by remember { mutableStateOf(initial) }

    fun commit(next: PvColor) {
        c = next
        draft = next
        onChange(next)
    }

    @Composable
    fun Row3(label: String, value: Int, set: (PvColor, Int) -> PvColor) {
        Column {
            Text("$label  ${value.signed()}", style = MaterialTheme.typography.labelLarge)
            Slider(
                value = value.toFloat(),
                onValueChange = { draft = set(draft, it.roundToInt()) },
                onValueChangeFinished = { commit(draft) },
                valueRange = -100f..100f,
            )
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("色調整") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row3("明るさ", draft.brightness) { x, v -> x.copy(brightness = v) }
                Row3("コントラスト", draft.contrast) { x, v -> x.copy(contrast = v) }
                Row3("ガンマ", draft.gamma) { x, v -> x.copy(gamma = v) }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(c.grayscale, { commit(c.copy(grayscale = !c.grayscale, sepia = false)) }, { Text("白黒") })
                    FilterChip(c.sepia, { commit(c.copy(sepia = !c.sepia, grayscale = false)) }, { Text("セピア") })
                    FilterChip(c.invert, { commit(c.copy(invert = !c.invert)) }, { Text("反転") })
                }
            }
        },
        dismissButton = { TextButton(onClick = { commit(PvColor(0, 0, 0, false, false, false)) }) { Text("リセット") } },
        confirmButton = { TextButton(onClick = onDismiss) { Text("閉じる") } },
    )
}

private fun Int.signed() = if (this > 0) "+$this" else "$this"

/** 全ページのサムネイル一覧。タップでそのページへ移動する。 */
@Composable
fun PageGridDialog(reader: PvReader, currentPage: Int, onPick: (Int) -> Unit, onDismiss: () -> Unit) {
    val cache = remember { object : LruCache<Int, Bitmap>(24 * 1024 * 1024) {
        override fun sizeOf(key: Int, value: Bitmap) = value.byteCount
    } }
    val total = reader.pageCount().toInt()
    // 大きな画像を同時に何枚もデコードしてメモリが跳ねないよう、サムネイル生成は 2 並列まで。
    // 待機中にスクロールで画面外へ出たものは、取り消されて生成されない。
    val gate = remember { Semaphore(THUMB_PARALLELISM) }
    val state = rememberLazyGridState(initialFirstVisibleItemIndex = (currentPage - 3).coerceAtLeast(0))
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize(), color = Color(0xF2000000)) {
            Column {
                Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("ページ一覧  ${currentPage + 1}/$total", Modifier.weight(1f).padding(start = 8.dp), color = Color.White)
                    TextButton(onClick = onDismiss) { Text("閉じる") }
                }
                LazyVerticalGrid(
                    state = state,
                    columns = GridCells.Adaptive(96.dp),
                    modifier = Modifier.fillMaxSize().padding(horizontal = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(count = total, key = { it }) { page ->
                        val bmp: Bitmap? by produceState(cache.get(page), page) {
                            if (value == null) {
                                value = gate.withPermit {
                                    withContext(Dispatchers.IO) {
                                        // 本を閉じた直後に破棄済みになることがある。その場合は空のまま
                                        runCatching {
                                            reader.pageThumbnail(page.toUInt(), 200u)
                                                ?.let { rgbaToBitmap(it.width, it.height, it.rgba) }
                                                ?.also { cache.put(page, it) }
                                        }.getOrNull()
                                    }
                                }
                            }
                        }
                        Column(
                            Modifier.clickable { onPick(page) },
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Box(
                                Modifier.fillMaxWidth().aspectRatio(0.7f)
                                    .background(Color(0xFF1E1E1E))
                                    .border(if (page == currentPage) BorderStroke(2.dp, Color(0xFF80CBC4)) else BorderStroke(0.dp, Color.Transparent)),
                                contentAlignment = Alignment.Center,
                            ) {
                                bmp?.let { Image(it.asImageBitmap(), "p${page + 1}", Modifier.fillMaxSize(), contentScale = ContentScale.Fit) }
                            }
                            Text("${page + 1}", color = Color.White, style = MaterialTheme.typography.labelSmall)
                        }
                    }
                }
            }
        }
    }
}
