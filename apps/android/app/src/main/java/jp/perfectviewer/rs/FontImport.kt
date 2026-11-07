package jp.perfectviewer.rs

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvFontInfo
import uniffi.pv_ffi.PvTypeface

/** 取り込めるフォントファイルの上限（Rust 側の `MAX_FONT_BYTES` と合わせる） */
internal const val MAX_FONT_BYTES = 64L shl 20

internal fun typefaceLabel(t: PvTypeface, fonts: List<PvFontInfo>): String = when (t) {
    PvTypeface.Gothic -> "ゴシック体"
    PvTypeface.Mincho -> "明朝体"
    is PvTypeface.Custom -> fonts.firstOrNull { it.id == t.id }?.name ?: t.id.substringAfter('-')
}

/**
 * フォントを取り込むボタンと、取り込み済みフォントの一覧（削除つき）。
 * 取り込み・削除のあとに [onChanged] を呼ぶので、呼び出し側は書体の選択肢と設定を読み直す。
 */
@Composable
fun FontImportSection(state: AppState, fonts: List<PvFontInfo>, onChanged: () -> Unit) {
    val scope = rememberCoroutineScope()
    var pendingDelete by remember { mutableStateOf<PvFontInfo?>(null) }
    // 種類はファイルの名前と中身を Rust 側で確かめるので、ここでは絞らない
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val (name, data) = state.readFont(uri)
                    state.core.importFont(name, data)
                }
            }
            result
                .onSuccess { onChanged() }
                .onFailure { e -> state.toast(Tr.f("フォントを取り込めません: {1}", e.message)) }
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Button(onClick = { picker.launch(arrayOf("*/*")) }) { Text("フォントを取り込む") }
        Text(
            "ttf / otf / ttc（64 MB まで）。日本語の字形が無い文字は、ゴシック体で表示されます。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (fonts.isNotEmpty()) Text("取り込んだフォント", style = MaterialTheme.typography.bodyMedium)
        fonts.forEach { f ->
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(f.name, modifier = Modifier.weight(1f))
                TextButton(onClick = { pendingDelete = f }) { Text("削除") }
            }
        }
    }

    pendingDelete?.let { f ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            title = { Text("このフォントを削除しますか？") },
            text = { Text(f.name) },
            confirmButton = {
                TextButton(onClick = {
                    pendingDelete = null
                    scope.launch {
                        val result = withContext(state.settingsIo) { runCatching { state.core.removeFont(f.id) } }
                        result
                            .onSuccess { onChanged() }
                            .onFailure { e -> state.toast(Tr.f("フォントを削除できません: {1}", e.message)) }
                    }
                }) { Text("削除") }
            },
            dismissButton = { TextButton(onClick = { pendingDelete = null }) { Text("やめる") } },
        )
    }
}
