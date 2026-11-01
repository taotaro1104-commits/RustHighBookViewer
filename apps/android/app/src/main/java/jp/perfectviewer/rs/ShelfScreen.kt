package jp.perfectviewer.rs

import android.graphics.Bitmap
import android.net.Uri
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items as listItems
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateListOf
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
import uniffi.pv_ffi.PvBook
import uniffi.pv_ffi.PvGroup
import uniffi.pv_ffi.PvSort
import uniffi.pv_ffi.PvStatus

/** 本棚のタブ。選んだタブは AppState に覚えておき、本を閉じたときに開く前の状態へ戻す。 */
enum class ShelfTab(val label: String) { RECENT("履歴"), ALL("すべて"), FAVORITES("お気に入り"), GROUPS("グループ") }

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ShelfScreen(state: AppState, onPickFile: () -> Unit, onPickFolder: () -> Unit, onOpen: (PvBook) -> Unit) {
    val tab = state.shelfTab
    val query = state.shelfQuery
    val group = state.shelfGroup
    // 複数選択（まとめてグループへ入れる）
    var selecting by remember { mutableStateOf(false) }
    val selected = remember { mutableStateListOf<Long>() }
    var addTarget by remember { mutableStateOf<List<Long>?>(null) }
    fun stopSelecting() { selecting = false; selected.clear() }

    val books by produceState(emptyList<PvBook>(), tab, query, group, state.shelfVersion, state.shelfSort) {
        value = withContext(Dispatchers.IO) {
            runCatching {
                when {
                    group != null -> state.core.groupBooks(group.id)
                        .filter { query.isBlank() || it.title.contains(query, ignoreCase = true) }
                    tab == ShelfTab.RECENT -> state.core.recentBooks(200u)
                        .filter { query.isBlank() || it.title.contains(query, ignoreCase = true) }
                    tab == ShelfTab.ALL -> state.core.listBooks(query, false, state.shelfSort)
                    tab == ShelfTab.FAVORITES -> state.core.listBooks(query, true, state.shelfSort)
                    else -> emptyList()
                }
            }.getOrDefault(emptyList())
        }
    }
    val groups by produceState(emptyList<PvGroup>(), tab, query, group, state.shelfVersion) {
        value = if (tab != ShelfTab.GROUPS || group != null) emptyList() else withContext(Dispatchers.IO) {
            runCatching { state.core.groups().filter { query.isBlank() || it.name.contains(query, ignoreCase = true) } }
                .getOrDefault(emptyList())
        }
    }
    // タブやグループを切り替えたときだけ先頭へ戻す（本を閉じて戻ったときは、前のスクロール位置のまま）
    LaunchedEffect(state.shelfScrollToTop, books, groups) {
        if (state.shelfScrollToTop) {
            state.shelfScrollToTop = false
            state.shelfGrid.scrollToItem(0)
        }
    }
    BackHandler(enabled = selecting) { stopSelecting() }
    BackHandler(enabled = !selecting && group != null) { state.shelfGroup = null; state.shelfScrollToTop = true }

    Column(Modifier.fillMaxSize().padding(horizontal = 12.dp)) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (state.busy) {
                // 大きな 7z / RAR は展開に時間がかかるため、途中でやめられるようにする
                val prog = state.importProgress
                Button(onClick = { state.cancelOpen() }) {
                    Text(if (prog != null) "登録中 ${prog.first}/${prog.second}… キャンセル" else "読み込み中… キャンセル")
                }
            } else {
                Button(onClick = onPickFile) { Text("ファイル") }
            }
            Button(onClick = onPickFolder, enabled = !state.busy) { Text("フォルダ") }
            TextButton(onClick = { state.showSettings = true }) { Text("設定") }
            TextButton(onClick = { state.updateShelfAsList(!state.shelfAsList) }) {
                Text(if (state.shelfAsList) "▦ サムネイル表示" else "☰ 名前の一覧")
            }
            ShelfTab.entries.forEach { t ->
                FilterChip(selected = tab == t, onClick = {
                    state.shelfTab = t
                    state.shelfGroup = null
                    state.shelfScrollToTop = true
                    stopSelecting()
                }, label = { Text(t.label) })
            }
        }
        if (selecting) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("${selected.size} 冊を選択中", modifier = Modifier.weight(1f))
                Button(enabled = selected.isNotEmpty(), onClick = { addTarget = selected.toList() }) { Text("グループに追加") }
                TextButton(onClick = { stopSelecting() }) { Text("やめる") }
            }
        } else if (group != null) {
            GroupHeader(state, group)
        }
        OutlinedTextField(
            value = query,
            onValueChange = { state.shelfQuery = it },
            singleLine = true,
            label = { Text("タイトル検索") },
            modifier = Modifier.fillMaxWidth(),
        )
        if (tab == ShelfTab.GROUPS && group == null) {
            GroupGrid(state, groups, onOpen = { state.shelfGroup = it; state.shelfScrollToTop = true })
        } else if (books.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("本がありません。「ファイル」から ZIP / CBZ / 7z / RAR / EPUB を、「フォルダ」から画像フォルダを選んでください。")
            }
        } else {
            val click: (PvBook) -> Unit = { book ->
                if (!selecting) onOpen(book)
                else if (selected.contains(book.id)) selected.remove(book.id) else selected.add(book.id)
            }
            if (state.shelfAsList) {
                LazyColumn(Modifier.fillMaxSize().padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    listItems(books, key = { it.id }) { book ->
                        BookRow(
                            state, book, group, selecting, selected.contains(book.id),
                            onClick = { click(book) },
                            onSelect = { selecting = true; if (!selected.contains(book.id)) selected.add(book.id) },
                            onAddToGroup = { addTarget = listOf(book.id) },
                        )
                    }
                }
            } else {
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(130.dp),
                    state = state.shelfGrid,
                    modifier = Modifier.fillMaxSize().padding(top = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    items(books, key = { it.id }) { book ->
                        BookCard(
                            state, book, group, selecting, selected.contains(book.id),
                            onClick = { click(book) },
                            onSelect = { selecting = true; if (!selected.contains(book.id)) selected.add(book.id) },
                            onAddToGroup = { addTarget = listOf(book.id) },
                        )
                    }
                }
            }
        }
    }
    addTarget?.let { ids ->
        AddToGroupDialog(state, ids, onClose = { added -> addTarget = null; if (added) stopSelecting() })
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BookCard(
    state: AppState,
    book: PvBook,
    group: PvGroup?,
    selecting: Boolean,
    isSelected: Boolean,
    onClick: () -> Unit,
    onSelect: () -> Unit,
    onAddToGroup: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val bg = if (isSelected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant
    Column(
        Modifier
            .combinedClickable(onClick = onClick, onLongClick = { if (!selecting) menu = true })
            .background(bg)
            .padding(6.dp),
    ) {
        Cover(state, book)
        Text(
            (if (isSelected) "✓ " else "") + (if (book.favorite) "★ " else "") + book.title,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
        Text(
            "${book.pageCount}p${statusLabel(book.status)}",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.labelSmall,
        )
        BookMenu(state, book, group, menu, onDismiss = { menu = false }, onSelect = onSelect, onAddToGroup = onAddToGroup)
    }
}

/** 名前の一覧での 1 行。ファイル・フォルダ名を大きく、ページ数などを小さく出す。 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BookRow(
    state: AppState,
    book: PvBook,
    group: PvGroup?,
    selecting: Boolean,
    isSelected: Boolean,
    onClick: () -> Unit,
    onSelect: () -> Unit,
    onAddToGroup: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val bg = if (isSelected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant
    Column(
        Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = onClick, onLongClick = { if (!selecting) menu = true })
            .background(bg)
            .padding(horizontal = 12.dp, vertical = 10.dp),
    ) {
        Text(
            (if (isSelected) "✓ " else "") + (if (book.favorite) "★ " else "") + bookFileName(book),
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyLarge,
        )
        Text(
            "${book.pageCount}p${statusLabel(book.status)}",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.labelSmall,
        )
        BookMenu(state, book, group, menu, onDismiss = { menu = false }, onSelect = onSelect, onAddToGroup = onAddToGroup)
    }
}

private fun statusLabel(s: PvStatus) = when (s) {
    PvStatus.UNREAD -> ""
    PvStatus.READING -> " 読書中"
    PvStatus.FINISHED -> " 読了"
}

/** ファイル・フォルダ名。ファイルは拡張子付きで出す。取れないとき（SAF など）は題名。 */
private fun bookFileName(book: PvBook): String {
    val last = Uri.decode(book.uri.substringBefore('?').trimEnd('/')).substringAfterLast('/').substringAfterLast(':')
    return last.ifBlank { book.title }
}

@Composable
private fun BookMenu(
    state: AppState,
    book: PvBook,
    group: PvGroup?,
    expanded: Boolean,
    onDismiss: () -> Unit,
    onSelect: () -> Unit,
    onAddToGroup: () -> Unit,
) {
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        DropdownMenuItem(text = { Text("グループに追加") }, onClick = { onDismiss(); onAddToGroup() })
        DropdownMenuItem(text = { Text("選んで、まとめてグループへ") }, onClick = { onDismiss(); onSelect() })
        if (group != null) {
            DropdownMenuItem(
                text = { Text("このグループから外す") },
                onClick = {
                    onDismiss()
                    runCatching { state.core.removeFromGroup(group.id, listOf(book.id)) }
                    state.bumpShelf()
                },
            )
        }
        DropdownMenuItem(
            text = { Text(if (book.favorite) "お気に入りを外す" else "お気に入りにする") },
            onClick = {
                onDismiss()
                runCatching { state.core.setFavorite(book.id, !book.favorite) }
                state.bumpShelf()
            },
        )
        DropdownMenuItem(
            text = { Text("ライブラリから削除") },
            onClick = {
                onDismiss()
                runCatching { state.core.deleteBook(book.id) }
                state.bumpShelf()
            },
        )
    }
}

@Composable
private fun Cover(state: AppState, book: PvBook) {
    val bmp: Bitmap? by produceState<Bitmap?>(null, book.id) {
        value = withContext(Dispatchers.IO) {
            state.core.thumbnail(book.id)?.let { rgbaToBitmap(it.width, it.height, it.rgba) }
        }
    }
    Box(Modifier.fillMaxWidth().aspectRatio(0.72f), contentAlignment = Alignment.Center) {
        bmp?.let {
            Image(it.asImageBitmap(), contentDescription = book.title, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize())
        }
    }
}
