package jp.perfectviewer.rs

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
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
import uniffi.pv_ffi.PvDirection
import uniffi.pv_ffi.PvFit
import uniffi.pv_ffi.PvSettings
import uniffi.pv_ffi.PvSort
import uniffi.pv_ffi.PvViewMode

/** アプリ全体の設定画面。変更はその場で保存される。 */
@OptIn(ExperimentalLayoutApi::class, ExperimentalFoundationApi::class)
@Composable
fun SettingsScreen(state: AppState) {
    val scope = rememberCoroutineScope()
    var s by remember { mutableStateOf(state.core.settings()) }
    var confirmReset by remember { mutableStateOf(false) }

    fun update(next: PvSettings) {
        s = next
        scope.launch(Dispatchers.IO) {
            runCatching { state.core.setSettings(next) }
                .onFailure { e -> withContext(Dispatchers.Main) { state.toast(Tr.f("設定を保存できません: {1}", e.message)) } }
        }
    }

    BackHandler { state.showSettings = false }

    // Surface が文字色（contentColor）を背景に合わせる。無いと既定の黒で、暗い背景に溶けて読めない
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
    Column(
        Modifier
            .fillMaxSize()
            .statusBarsPadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = { state.showSettings = false }) { Text("← 戻る") }
            Text("設定", style = MaterialTheme.typography.headlineSmall)
        }

        Section("言語 / Language") {
            Choice(
                "言語 / Language", state.language,
                listOf(LANGUAGE_AUTO to "端末の言語") + Lang.entries.map { it.code to it.nativeName },
            ) { state.updateLanguage(it) }
        }

        Section("表示（これから開く本の既定）") {
            Note("すでに開いたことのある本は、その本ごとの設定が優先されます。")
            Choice(
                "表示モード", s.mode,
                listOf(
                    PvViewMode.SINGLE to "単ページ", PvViewMode.DOUBLE to "見開き",
                    PvViewMode.AUTO to "自動見開き", PvViewMode.WEBTOON to "縦スクロール",
                ),
            ) { update(s.copy(mode = it)) }
            Choice(
                "綴じ方向", s.direction,
                listOf(PvDirection.RIGHT_TO_LEFT to "右綴じ", PvDirection.LEFT_TO_RIGHT to "左綴じ"),
            ) { update(s.copy(direction = it)) }
            Choice(
                "フィット", s.fit,
                listOf(PvFit.PAGE to "全体", PvFit.WIDTH to "幅", PvFit.HEIGHT to "高さ"),
            ) { update(s.copy(fit = it)) }
            Toggle("表紙を単独で表示", "先頭ページだけ 1 枚で表示します", s.firstPageSingle) { update(s.copy(firstPageSingle = it)) }
            Toggle("横長のページは単独で表示", "見開きの絵などを 2 ページ分に割り込ませません", s.widePageAlone) { update(s.copy(widePageAlone = it)) }
        }

        Section("操作") {
            Toggle("音量キーでページ送り", "音量＋で前へ、音量－で次へ（綴じ方向に従います）", state.volumeKeys) { state.updateVolumeKeys(it) }
            Toggle("閲覧中は画面を点灯したままにする", "オートめくり中は、この設定にかかわらず点灯します", state.keepScreenOn) { state.updateKeepScreenOn(it) }
        }

        Section("本棚") {
            Choice(
                "「すべて」「お気に入り」の並び順", state.shelfSort,
                listOf(
                    PvSort.NAME to "名前", PvSort.LAST_OPENED to "最近開いた順",
                    PvSort.ADDED to "追加順", PvSort.SIZE to "サイズ",
                ),
            ) { state.updateShelfSort(it) }
        }

        Section("文章の本（EPUB）") {
            Note("画像の無い文章の EPUB を、縦書きで表示するときの見た目です。変更は、次に本を開いたときから反映されます（開いている本は、閲覧メニューの「文字」で開き直せます）。")
            var text by remember { mutableStateOf(state.core.textStyle()) }
            val faces = remember { state.core.availableTypefaces() }
            fun updateText(next: uniffi.pv_ffi.PvTextStyle) {
                text = next
                scope.launch(Dispatchers.IO) {
                    runCatching { state.core.setTextStyle(next) }
                        .onFailure { e -> withContext(Dispatchers.Main) { state.toast(Tr.f("設定を保存できません: {1}", e.message)) } }
                }
            }
            Choice("書体", text.typeface, faces.map { it to typefaceLabel(it) }) { updateText(text.copy(typeface = it)) }
            Choice(
                "文字の大きさ", text.size.toInt(),
                listOf(32, 36, 40, 44, 48, 56, 64).map { it to it.toString() },
            ) { updateText(text.copy(size = it.toUInt())) }
            Choice(
                "行間", text.lineSpacing.toInt(),
                listOf(130, 150, 160, 180, 200, 220).map { it to "$it%" },
            ) { updateText(text.copy(lineSpacing = it.toUInt())) }
        }

        Section("キャッシュ（メモリ）") {
            Note("変更は、次に本を開いたときから反映されます。")
            Choice(
                "画像キャッシュの上限", s.cacheMb.toInt(),
                listOf(0 to "自動", 128 to "128MB", 256 to "256MB", 512 to "512MB", 1024 to "1GB"),
            ) { update(s.copy(cacheMb = it.toUInt())) }
            Choice(
                "先読み（次のページ側）", s.prefetchForward.toInt(),
                listOf(0, 1, 2, 4, 6, 8).map { it to Tr.f("{1}枚", it) },
            ) { update(s.copy(prefetchForward = it.toUInt())) }
            Choice(
                "先読み（前のページ側）", s.prefetchBack.toInt(),
                listOf(0, 1, 2, 4).map { it to Tr.f("{1}枚", it) },
            ) { update(s.copy(prefetchBack = it.toUInt())) }
        }

        Section("データ") {
            Button(onClick = { confirmReset = true }) { Text("本ごとの設定をすべて初期化") }
            Note("各本に保存された表示モード・綴じ方向・フィット・色調整を消し、上の既定が使われるようにします。しおりと読書位置は消えません。")
        }

        Section("このアプリについて") {
            Text("RustHighBookViewer ${state.versionName}", style = MaterialTheme.typography.bodyLarge)
            Note("対応形式: ZIP / CBZ、7z、RAR、EPUB（画像・文章）、画像フォルダ。MIT License。RAR の展開には UnRAR を使用しています。")
        }
    }
    }

    if (confirmReset) {
        AlertDialog(
            onDismissRequest = { confirmReset = false },
            title = { Text("本ごとの設定を初期化しますか？") },
            text = { Text("すべての本の表示設定が、既定に戻ります。") },
            confirmButton = {
                TextButton(onClick = {
                    confirmReset = false
                    scope.launch(Dispatchers.IO) {
                        val msg = runCatching { state.core.resetBookSettings() }
                            .fold({ "初期化しました" }, { Tr.f("初期化できません: {1}", it.message) })
                        withContext(Dispatchers.Main) { state.toast(msg) }
                    }
                }) { Text("初期化") }
            },
            dismissButton = { TextButton(onClick = { confirmReset = false }) { Text("やめる") } },
        )
    }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        HorizontalDivider()
        Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary)
        content()
    }
}

@Composable
private fun Note(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T> Choice(label: String, selected: T, options: List<Pair<T, String>>, onSelect: (T) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, style = MaterialTheme.typography.bodyMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            options.forEach { (value, text) ->
                FilterChip(selected = value == selected, onClick = { onSelect(value) }, label = { Text(text) })
            }
        }
    }
}

@Composable
private fun Toggle(title: String, description: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium)
            Note(description)
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}
