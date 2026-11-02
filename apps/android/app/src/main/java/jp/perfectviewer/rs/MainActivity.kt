package jp.perfectviewer.rs

import android.content.ActivityNotFoundException
import android.content.ComponentCallbacks2
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.KeyEvent
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.key
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.lifecycleScope
import jp.perfectviewer.rs.AppState
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private lateinit var state: AppState

    /** 「すべてのファイルへのアクセス」の許可画面へ送り出し、戻りを待っているか */
    private var awaitingAccess = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        state = AppState.get(applicationContext)
        handleIntent(intent)
        // 他のアプリを使っている間にアプリが終了していたら、開いていた本を続きから開き直す
        if (intent?.action != Intent.ACTION_VIEW) lifecycleScope.launch { state.restoreSession() }

        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                val scope = rememberCoroutineScope()
                val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
                    if (uri != null) scope.launch { openPersisted(uri) }
                }
                val folderPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
                    if (uri != null) scope.launch { openPersisted(uri) }
                }
                val reader = state.reader
                var showAccessDialog by remember { mutableStateOf(false) }
                // 本棚・閲覧中のメニューのどちらからでも同じ流れでフォルダを選べるようにする
                SideEffect {
                    state.pickFolderHandler = {
                        when {
                            !state.needsAllFilesAccess -> folderPicker.launch(null)
                            state.hasAllFilesAccess() -> state.showFolderBrowser = true
                            else -> showAccessDialog = true
                        }
                    }
                }
                Box(Modifier.fillMaxSize()) {
                    if (reader != null) {
                        // 本ごとに状態（ページ・要求中フレーム等）を作り直す
                        key(reader) { ReaderScreen(state, reader) }
                    } else {
                        Box(Modifier.statusBarsPadding()) {
                            ShelfScreen(
                                state,
                                onPickFile = {
                                    // 7z / RAR は MIME が端末ごとにまちまちなので、種類では絞らない
                                    picker.launch(arrayOf("*/*"))
                                },
                                onPickFolder = { state.pickFolderHandler?.invoke() },
                                onOpen = { book -> scope.launch { state.openBook(book.uri) } },
                            )
                        }
                    }
                    if (state.showFolderBrowser) FolderBrowserScreen(state)
                    if (state.showSettings) SettingsScreen(state)
                    if (showAccessDialog) {
                        AlertDialog(
                            onDismissRequest = { showAccessDialog = false },
                            title = { Text("フォルダを選ぶには") },
                            text = {
                                Text(
                                    Tr.t("Android の標準のフォルダ選択では、Download や内部ストレージの最上位は「プライバシー保護のため使用できません」と表示され、選べません。") +
                                        "\n\n" +
                                        Tr.t("「すべてのファイルへのアクセス」を許可すると、アプリ内の画面からどのフォルダでも選べます（読み込みも速くなります）。画像の閲覧以外には使いません。"),
                                )
                            },
                            confirmButton = {
                                TextButton(onClick = {
                                    showAccessDialog = false
                                    awaitingAccess = true
                                    requestAllFilesAccess()
                                }) { Text("許可する") }
                            },
                            dismissButton = {
                                Row {
                                    TextButton(onClick = {
                                        showAccessDialog = false
                                        folderPicker.launch(null)
                                    }) { Text("標準の選択画面") }
                                    TextButton(onClick = { showAccessDialog = false }) { Text("やめる") }
                                }
                            },
                        )
                    }
                }
            }
        }
    }

    /** 設定画面（すべてのファイルへのアクセス）を開く。戻ってきたら onResume で続きの操作を行う。 */
    private fun requestAllFilesAccess() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
        val perApp = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
        try {
            startActivity(perApp)
        } catch (e: ActivityNotFoundException) {
            startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
        }
    }

    override fun onResume() {
        super.onResume()
        // 許可の画面から戻ってきて、許可されていたら、そのまま独自のフォルダ選択を開く
        if (awaitingAccess) {
            awaitingAccess = false
            if (state.hasAllFilesAccess()) state.showFolderBrowser = true
        }
    }

    /** 次回以降も本棚から開けるよう、読み取り権限を永続化してから開く */
    private suspend fun openPersisted(uri: Uri) {
        runCatching { contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        state.open(uri)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        val uri = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.data ?: return
        lifecycleScope.launch { openPersisted(uri) }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val up = keyCode == KeyEvent.KEYCODE_VOLUME_UP
        if ((up || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) && !state.showSettings && state.volumeHandler?.invoke(up) == true) return true
        return super.onKeyDown(keyCode, event)
    }

    // 音量キーを押したときの音量 UI を出さないため、ハンドラが処理した場合は keyUp も飲み込む
    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if ((keyCode == KeyEvent.KEYCODE_VOLUME_UP || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) && !state.showSettings && state.volumeHandler != null && state.volumeKeys) {
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    @Deprecated("Deprecated in Java")
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW) state.onTrimMemory()
    }

    override fun onStop() {
        super.onStop()
        state.reader?.let { runCatching { it.saveProgress(true) } }
        state.persistSession()
    }
}
