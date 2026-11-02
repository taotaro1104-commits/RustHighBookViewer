package jp.perfectviewer.rs

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import uniffi.pv_ffi.PvFolderEntry

/** 1 つ上のフォルダ（絶対パス）。いちばん上に着いたら空 */
internal fun parentFolder(path: String): String {
    val p = path.trimEnd('/')
    val i = p.lastIndexOf('/')
    return if (i <= 0) "" else p.substring(0, i)
}

/** サブフォルダの 1 行。タップでそのフォルダへ入る。 */
@Composable
internal fun FolderRow(f: PvFolderEntry, onOpen: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onOpen)
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text("📁")
        Text(
            f.name,
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            Tr.f("{1} 冊", f.count),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.primary,
        )
    }
}

/** 今開いているフォルダの場所と、「上へ」「配下すべて表示」の切り替え。 */
@Composable
internal fun FolderHeader(state: AppState) {
    val here = state.shelfFolder
    Column {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            TextButton(
                enabled = here.isNotEmpty(),
                onClick = { state.shelfFolder = parentFolder(here); state.shelfScrollToTop = true },
            ) { Text("↑ 上へ") }
            FilterChip(
                selected = state.shelfFolderRecursive,
                onClick = { state.shelfFolderRecursive = !state.shelfFolderRecursive; state.shelfScrollToTop = true },
                label = { Text("配下すべて表示") },
            )
        }
        Text(
            if (here.isEmpty()) "📁 /" else "📁 $here",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
