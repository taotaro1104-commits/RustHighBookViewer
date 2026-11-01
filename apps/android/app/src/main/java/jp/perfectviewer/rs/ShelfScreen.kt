package jp.perfectviewer.rs

import android.graphics.Bitmap
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
import uniffi.pv_ffi.PvBook
import uniffi.pv_ffi.PvSort
import uniffi.pv_ffi.PvStatus

private enum class Tab(val label: String) { RECENT("履歴"), ALL("すべて"), FAVORITES("お気に入り") }

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ShelfScreen(state: AppState, onPickFile: () -> Unit, onPickFolder: () -> Unit, onOpen: (PvBook) -> Unit) {
    var tab by remember { mutableStateOf(Tab.RECENT) }
    var query by remember { mutableStateOf("") }
    val books by produceState(emptyList<PvBook>(), tab, query, state.shelfVersion, state.shelfSort) {
        value = withContext(Dispatchers.IO) {
            runCatching {
                when (tab) {
                    Tab.RECENT -> state.core.recentBooks(200u)
                        .filter { query.isBlank() || it.title.contains(query, ignoreCase = true) }
                    Tab.ALL -> state.core.listBooks(query, false, state.shelfSort)
                    Tab.FAVORITES -> state.core.listBooks(query, true, state.shelfSort)
                }
            }.getOrDefault(emptyList())
        }
    }

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
                    Text(if (prog != null) "登録中 ${prog.first + 1}/${prog.second}… キャンセル" else "読み込み中… キャンセル")
                }
            } else {
                Button(onClick = onPickFile) { Text("ファイル") }
            }
            Button(onClick = onPickFolder, enabled = !state.busy) { Text("フォルダ") }
            TextButton(onClick = { state.showSettings = true }) { Text("設定") }
            Tab.entries.forEach { t ->
                FilterChip(selected = tab == t, onClick = { tab = t }, label = { Text(t.label) })
            }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            singleLine = true,
            label = { Text("タイトル検索") },
            modifier = Modifier.fillMaxWidth(),
        )
        if (books.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("本がありません。「ファイル」から ZIP / CBZ / 7z / RAR を、「フォルダ」から画像フォルダを選んでください。")
            }
            return@Column
        }
        LazyVerticalGrid(
            columns = GridCells.Adaptive(130.dp),
            modifier = Modifier.fillMaxSize().padding(top = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            items(books, key = { it.id }) { book ->
                var menu by remember { mutableStateOf(false) }
                Column(
                    Modifier
                        .combinedClickable(onClick = { onOpen(book) }, onLongClick = { menu = true })
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(6.dp),
                ) {
                    Cover(state, book)
                    Text(
                        (if (book.favorite) "★ " else "") + book.title,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    val status = when (book.status) {
                        PvStatus.UNREAD -> ""
                        PvStatus.READING -> " 読書中"
                        PvStatus.FINISHED -> " 読了"
                    }
                    Text(
                        "${book.pageCount}p$status",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.labelSmall,
                    )
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(
                            text = { Text(if (book.favorite) "お気に入りを外す" else "お気に入りにする") },
                            onClick = {
                                menu = false
                                runCatching { state.core.setFavorite(book.id, !book.favorite) }
                                state.bumpShelf()
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("ライブラリから削除") },
                            onClick = {
                                menu = false
                                runCatching { state.core.deleteBook(book.id) }
                                state.bumpShelf()
                            },
                        )
                    }
                }
            }
        }
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
