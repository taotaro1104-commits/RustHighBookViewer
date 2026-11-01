package jp.perfectviewer.rs

import android.content.ComponentCallbacks2
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.KeyEvent
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.key
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.lifecycle.lifecycleScope
import jp.perfectviewer.rs.AppState
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private lateinit var state: AppState

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        state = AppState.get(applicationContext)
        handleIntent(intent)

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
                                onPickFolder = { folderPicker.launch(null) },
                                onOpen = { book -> scope.launch { state.open(Uri.parse(book.uri)) } },
                            )
                        }
                    }
                }
            }
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
        if ((up || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) && state.volumeHandler?.invoke(up) == true) return true
        return super.onKeyDown(keyCode, event)
    }

    // 音量キーを押したときの音量 UI を出さないため、ハンドラが処理した場合は keyUp も飲み込む
    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if ((keyCode == KeyEvent.KEYCODE_VOLUME_UP || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) && state.volumeHandler != null) {
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
    }
}
