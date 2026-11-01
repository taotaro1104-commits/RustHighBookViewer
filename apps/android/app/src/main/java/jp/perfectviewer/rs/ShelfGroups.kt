package jp.perfectviewer.rs

import android.graphics.Bitmap
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvGroup

/** グループの一覧（本棚の「グループ」タブ）。表紙は、中の最初の本のもの。 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun GroupGrid(state: AppState, groups: List<PvGroup>, onOpen: (PvGroup) -> Unit) {
    if (groups.isEmpty()) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Text("グループがありません。本を長押しして「グループに追加」するか、複数の本が入ったフォルダを読み込むと作られます。")
        }
        return
    }
    var renaming by remember { mutableStateOf<PvGroup?>(null) }
    var deleting by remember { mutableStateOf<PvGroup?>(null) }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(130.dp),
        state = state.shelfGrid,
        modifier = Modifier.fillMaxSize().padding(top = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        items(groups, key = { it.id }) { g ->
            var menu by remember { mutableStateOf(false) }
            Column(
                Modifier
                    .combinedClickable(onClick = { onOpen(g) }, onLongClick = { menu = true })
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .padding(6.dp),
            ) {
                GroupCover(state, g)
                Text(
                    "📚 ${g.name}",
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    "${g.count} 冊",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                )
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text("名前を変更") }, onClick = { menu = false; renaming = g })
                    DropdownMenuItem(text = { Text("グループを解除（本は残る）") }, onClick = { menu = false; deleting = g })
                }
            }
        }
    }
    renaming?.let { g -> RenameGroupDialog(state, g, onClose = { renaming = null }) }
    deleting?.let { g -> DeleteGroupDialog(state, g, onClose = { deleting = null }) }
}

@Composable
private fun GroupCover(state: AppState, g: PvGroup) {
    val bmp: Bitmap? by produceState<Bitmap?>(null, g.id, state.shelfVersion) {
        value = withContext(Dispatchers.IO) {
            runCatching { state.core.groupBooks(g.id).firstOrNull() }.getOrNull()
                ?.let { b -> state.core.thumbnail(b.id)?.let { rgbaToBitmap(it.width, it.height, it.rgba) } }
        }
    }
    Box(Modifier.fillMaxWidth().aspectRatio(0.72f), contentAlignment = Alignment.Center) {
        bmp?.let {
            Image(it.asImageBitmap(), contentDescription = g.name, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
        }
    }
}

/** 本を入れるグループを選ぶ（新しく作ることもできる）。 */
@Composable
fun AddToGroupDialog(state: AppState, bookIds: List<Long>, onClose: (added: Boolean) -> Unit) {
    val groups by produceState(emptyList<PvGroup>()) {
        value = withContext(Dispatchers.IO) { runCatching { state.core.groups() }.getOrDefault(emptyList()) }
    }
    var name by remember { mutableStateOf("") }
    fun addTo(id: Long, label: String) {
        val ok = runCatching { state.core.addToGroup(id, bookIds) }.isSuccess
        state.toast(if (ok) "${bookIds.size} 冊を「$label」に追加しました" else "グループに追加できませんでした")
        state.bumpShelf()
        onClose(ok)
    }
    AlertDialog(
        onDismissRequest = { onClose(false) },
        title = { Text("グループに追加（${bookIds.size} 冊）") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                groups.forEach { g ->
                    TextButton(onClick = { addTo(g.id, g.name) }, modifier = Modifier.fillMaxWidth()) {
                        Text("📚 ${g.name}（${g.count} 冊）", maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    singleLine = true,
                    label = { Text("新しいグループ名") },
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(
                    enabled = name.isNotBlank(),
                    onClick = {
                        val g = runCatching { state.core.createGroup(name) }.getOrNull()
                        if (g != null) addTo(g.id, g.name) else state.toast("グループを作れませんでした")
                    },
                ) { Text("作って追加") }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = { onClose(false) }) { Text("閉じる") } },
    )
}

@Composable
fun RenameGroupDialog(state: AppState, g: PvGroup, onClose: () -> Unit) {
    var name by remember { mutableStateOf(g.name) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("グループ名を変更") },
        text = { OutlinedTextField(value = name, onValueChange = { name = it }, singleLine = true, modifier = Modifier.fillMaxWidth()) },
        confirmButton = {
            TextButton(enabled = name.isNotBlank(), onClick = {
                val ok = runCatching { state.core.renameGroup(g.id, name) }.isSuccess
                if (!ok) state.toast("変更できませんでした（同じ名前のグループがあるかもしれません）")
                else if (state.shelfGroup?.id == g.id) state.shelfGroup = g.copy(name = name.trim())
                state.bumpShelf()
                onClose()
            }) { Text("変更") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("やめる") } },
    )
}

@Composable
fun DeleteGroupDialog(state: AppState, g: PvGroup, onClose: () -> Unit) {
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("グループを解除") },
        text = { Text("「${g.name}」をやめます。中の本はライブラリに残ります。") },
        confirmButton = {
            TextButton(onClick = {
                runCatching { state.core.deleteGroup(g.id) }
                if (state.shelfGroup?.id == g.id) state.shelfGroup = null
                state.bumpShelf()
                onClose()
            }) { Text("解除する") }
        },
        dismissButton = { TextButton(onClick = onClose) { Text("やめる") } },
    )
}

/** グループを開いているときの見出し。 */
@Composable
fun GroupHeader(state: AppState, g: PvGroup) {
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        TextButton(onClick = { state.shelfGroup = null; state.shelfScrollToTop = true }) { Text("← グループ") }
        Text("📚 ${g.name}", maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
        TextButton(onClick = { renaming = true }) { Text("名前") }
        TextButton(onClick = { deleting = true }) { Text("解除") }
    }
    if (renaming) RenameGroupDialog(state, g, onClose = { renaming = false })
    if (deleting) DeleteGroupDialog(state, g, onClose = { deleting = false })
}
