package jp.perfectviewer.rs

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.FilterChip
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import uniffi.pv_ffi.PvSort
import uniffi.pv_ffi.PvStatus

private fun sortLabel(s: PvSort) = when (s) {
    PvSort.NAME -> "名前"
    PvSort.LAST_OPENED -> "最近開いた順"
    PvSort.ADDED -> "追加順"
    PvSort.SIZE -> "サイズ"
}

private val SORT_ORDER = listOf(PvSort.NAME, PvSort.LAST_OPENED, PvSort.ADDED, PvSort.SIZE)

/**
 * 本棚の並び替え。ボタンに今の並び順（キーと向き）を出し、押すとキーの一覧が開く。
 * 今のキーをもう一度選ぶと、昇順・降順が入れ替わる。
 */
@Composable
internal fun SortMenu(state: AppState) {
    var open by remember { mutableStateOf(false) }
    val arrow = if (state.shelfSortDesc) "↓" else "↑"
    TextButton(onClick = { open = true }) { Text(Tr.f("並び替え: {1} {2}", Tr.t(sortLabel(state.shelfSort)), arrow)) }
    DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
        SORT_ORDER.forEach { s ->
            val current = s == state.shelfSort
            DropdownMenuItem(
                text = { Text((if (current) "✓ " else "    ") + Tr.t(sortLabel(s)) + if (current) " $arrow" else "") },
                onClick = {
                    open = false
                    if (current) state.updateShelfSortDesc(!state.shelfSortDesc) else state.updateShelfSort(s)
                },
            )
        }
        DropdownMenuItem(
            text = { Text(if (state.shelfSortDesc) "昇順にする（古い・小さい順）" else "降順にする（新しい・大きい順）") },
            onClick = { open = false; state.updateShelfSortDesc(!state.shelfSortDesc) },
        )
    }
}

/** 読書状況での絞り込み: すべて・未読・読書中・既読 */
@Composable
internal fun StatusFilter(state: AppState) {
    val options = listOf<Pair<PvStatus?, String>>(
        null to "すべて", PvStatus.UNREAD to "未読", PvStatus.READING to "読書中", PvStatus.FINISHED to "既読",
    )
    Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        options.forEach { (st, label) ->
            FilterChip(
                selected = state.shelfStatus == st,
                onClick = { state.shelfStatus = st; state.shelfScrollToTop = true },
                label = { Text(label) },
            )
        }
    }
}
