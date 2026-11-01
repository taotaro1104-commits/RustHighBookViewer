package jp.perfectviewer.rs

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.io.File
import java.math.BigInteger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.isImageFileName

/** 一覧に出す 1 つのフォルダ */
private data class Sub(val file: File)

/**
 * アプリ独自のフォルダ選択。標準のフォルダ選択（SAF）は Download や内部ストレージの最上位を
 * 「使用できません」として選ばせないが、「すべてのファイルへのアクセス」を許可していれば、
 * ここから自由に選べる。直接パスで読むので、SAF より速い。
 */
@Composable
fun FolderBrowserScreen(state: AppState) {
    val context = LocalContext.current
    val roots = remember { storageRoots(context) }
    var dir by remember {
        mutableStateOf(
            state.browseDir?.let(::File)?.takeIf { it.isDirectory && it.canRead() } ?: roots.first().second,
        )
    }
    val root = roots.map { it.second }.firstOrNull { dir.path == it.path || dir.path.startsWith(it.path + "/") } ?: roots.first().second
    val canGoUp = dir.path != root.path

    BackHandler {
        if (canGoUp) dir = dir.parentFile ?: root else state.showFolderBrowser = false
    }

    // null = 読み込み中 / 読めないフォルダは空リスト + 警告
    val listing by produceState<List<File>?>(null, dir) {
        value = null
        value = withContext(Dispatchers.IO) { dir.listFiles()?.toList() }
    }
    val readable = listing != null || true

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(Modifier.fillMaxSize().statusBarsPadding().padding(horizontal = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = { state.showFolderBrowser = false }) { Text("← 戻る") }
                Text("フォルダを選ぶ", style = MaterialTheme.typography.titleLarge)
            }

            // 場所の候補（内部ストレージ・SD カード・よく使うフォルダ）
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                roots.forEach { (label, f) ->
                    FilterChip(selected = dir.path == f.path, onClick = { dir = f }, label = { Text(label) })
                }
                listOf("Download", "DCIM", "Pictures", "Documents", "Books", "Comics").forEach { name ->
                    val f = File(roots.first().second, name)
                    if (f.isDirectory) FilterChip(selected = dir.path == f.path, onClick = { dir = f }, label = { Text(name) })
                }
            }

            Text(
                dir.path,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )

            val files = listing
            val images = files?.count { it.isFile && isImageFileName(it.name) } ?: 0
            val subs = files
                ?.filter { it.isDirectory && !it.name.startsWith(".") }
                ?.sortedWith { a, b -> naturalCompare(a.name, b.name) }
                ?.map { Sub(it) }
                .orEmpty()

            Row(Modifier.padding(vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Button(
                    enabled = files != null && !state.busy,
                    onClick = {
                        state.rememberBrowseDir(dir.path)
                        state.showFolderBrowser = false
                        state.openFolderPathInBackground(dir.path)
                    },
                ) { Text("このフォルダを読み込む") }
                if (canGoUp) TextButton(onClick = { dir = dir.parentFile ?: root }) { Text("↑ 上へ") }
            }
            Text(
                when {
                    files == null -> "読み込み中…"
                    !dir.canRead() -> "このフォルダは読めません"
                    else -> "このフォルダの画像: ${images} 枚。画像は 1 冊に、EPUB などの書庫は 1 ファイルごとに 1 冊として、サブフォルダも含めて本棚に追加します"
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HorizontalDivider(Modifier.padding(vertical = 6.dp))

            if (readable && files != null && subs.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.TopCenter) {
                    Text("サブフォルダはありません", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            LazyColumn(Modifier.fillMaxSize()) {
                items(subs, key = { it.file.path }) { sub ->
                    // 画像の枚数は、見えている行だけ非同期で数える
                    val count by produceState<Int?>(null, sub.file) {
                        value = withContext(Dispatchers.IO) { sub.file.list()?.count { isImageFileName(it) } }
                    }
                    Row(
                        Modifier.fillMaxWidth().clickable { dir = sub.file }.padding(vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Text("📁")
                        Column(Modifier.weight(1f)) {
                            Text(sub.file.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            count?.takeIf { it > 0 }?.let {
                                Text("画像 $it 枚", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                            }
                        }
                        TextButton(
                            enabled = !state.busy,
                            onClick = {
                                state.rememberBrowseDir(dir.path)
                                state.showFolderBrowser = false
                                state.openFolderPathInBackground(sub.file.path)
                            },
                        ) { Text("読み込む") }
                    }
                    HorizontalDivider()
                }
            }
        }
    }
}

/** 内部ストレージと、取り外し可能なストレージ（SD カード等）の根。 */
private fun storageRoots(context: Context): List<Pair<String, File>> {
    val out = mutableListOf("内部ストレージ" to Environment.getExternalStorageDirectory())
    runCatching {
        if (Build.VERSION.SDK_INT >= 30) {
            val sm = context.getSystemService(StorageManager::class.java)
            sm.storageVolumes
                .filter { !it.isPrimary && it.state == Environment.MEDIA_MOUNTED }
                .forEach { v -> v.directory?.let { out += (v.getDescription(context) ?: "SD カード") to it } }
        }
    }
    return out
}

/** 数字の並びを数として比べる（「2巻」が「10巻」より前になる）。 */
internal fun naturalCompare(a: String, b: String): Int {
    val split = Regex("(?<=\\D)(?=\\d)|(?<=\\d)(?=\\D)")
    val pa = a.split(split)
    val pb = b.split(split)
    for (i in 0 until minOf(pa.size, pb.size)) {
        val x = pa[i]
        val y = pb[i]
        val c = if (x.firstOrNull()?.isDigit() == true && y.firstOrNull()?.isDigit() == true) {
            BigInteger(x).compareTo(BigInteger(y))
        } else {
            x.compareTo(y, ignoreCase = true)
        }
        if (c != 0) return c
    }
    return pa.size - pb.size
}
