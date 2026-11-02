package jp.perfectviewer.rs

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import uniffi.pv_ffi.PvTocEntry

/**
 * EPUB の目次。階層ごとに字下げして並べ、今いる章を強調する。項目を押すと、その章の最初のページへ移る。
 */
@Composable
fun TocDialog(toc: List<PvTocEntry>, currentPage: Int, onSelect: (Int) -> Unit, onDismiss: () -> Unit) {
    // 今いる章 = 最初のページが今のページ以前の、最後の項目
    val currentIndex = remember(toc, currentPage) {
        toc.indices.lastOrNull { toc[it].page.toInt() <= currentPage } ?: -1
    }
    val listState = rememberLazyListState()
    LaunchedEffect(currentIndex) { if (currentIndex > 2) listState.scrollToItem(currentIndex - 2) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("目次") },
        text = {
            LazyColumn(Modifier.heightIn(max = 460.dp), state = listState) {
                itemsIndexed(toc) { i, e ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable { onSelect(e.page.toInt()) }
                            .padding(start = (e.level.toInt().coerceAtMost(5) * 16).dp, top = 10.dp, bottom = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            e.title,
                            modifier = Modifier.weight(1f),
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            color = if (i == currentIndex) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                            style = if (i == currentIndex) MaterialTheme.typography.titleSmall else MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "${e.page.toInt() + 1}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    HorizontalDivider()
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("閉じる") } },
    )
}
