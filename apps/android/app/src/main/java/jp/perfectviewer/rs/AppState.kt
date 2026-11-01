package jp.perfectviewer.rs

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.util.Log
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.widget.Toast
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.nio.ByteBuffer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvCore
import uniffi.pv_ffi.PvException
import uniffi.pv_ffi.PvReader
import uniffi.pv_ffi.PvSort

private const val DESTROY_DELAY_MS = 600L
private const val FILE_PREFIX = "file://"
private const val KEY_BROWSE_DIR = "browse_dir"
private const val KEY_VOLUME_KEYS = "volume_keys"
private const val KEY_KEEP_SCREEN = "keep_screen"
private const val KEY_SHELF_SORT = "shelf_sort"

/** アプリ全体の状態。画面回転では Activity を作り直さない（Manifest の configChanges）ので単純な保持で足りる。 */
class AppState private constructor(private val context: Context) {
    /**
     * 初回アクセス時に作る。SQLite を開き、前回異常終了で残った展開物を掃除するため重いので、
     * Main スレッドの Activity 生成では作らない（最初に触るのは本棚の読み込み = IO スレッド）。
     */
    val core: PvCore by lazy { PvCore(context.filesDir.resolve("pv").absolutePath) }

    /** 開いている本。null なら本棚を表示する */
    var reader: PvReader? by mutableStateOf(null)
        private set

    /** 本棚の再読み込み用。値が変わるたびに一覧を取り直す */
    var shelfVersion by mutableIntStateOf(0)
        private set

    var busy by mutableStateOf(false)
        private set

    private val prefs = context.getSharedPreferences("app_prefs", Context.MODE_PRIVATE)

    /**
     * 「フォルダを選ぶ」の入口。端末と権限に合わせて、独自のフォルダ選択・許可の案内・標準の選択画面のどれかを開く。
     * 本棚と閲覧中のメニューで共通に使うので、Activity が設定する。
     */
    var pickFolderHandler: (() -> Unit)? = null

    /** フォルダ選択で最後に見ていた場所。次回はここから始める */
    var browseDir: String? = prefs.getString(KEY_BROWSE_DIR, null)
        private set

    fun rememberBrowseDir(path: String) {
        browseDir = path
        prefs.edit().putString(KEY_BROWSE_DIR, path).apply()
    }

    /** アプリ独自のフォルダ選択を表示中か */
    var showFolderBrowser by mutableStateOf(false)

    /** 設定画面を表示中か */
    var showSettings by mutableStateOf(false)


    /** 音量キーでページ送りするか */
    var volumeKeys by mutableStateOf(prefs.getBoolean(KEY_VOLUME_KEYS, true))
        private set

    /** 閲覧中に画面を点灯したままにするか */
    var keepScreenOn by mutableStateOf(prefs.getBoolean(KEY_KEEP_SCREEN, false))
        private set

    /** 本棚（すべて・お気に入り）の並び順 */
    var shelfSort by mutableStateOf(
        runCatching { PvSort.valueOf(prefs.getString(KEY_SHELF_SORT, null) ?: "") }.getOrDefault(PvSort.NAME),
    )
        private set

    val versionName: String = runCatching {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName
    }.getOrNull() ?: ""

    fun updateVolumeKeys(v: Boolean) {
        volumeKeys = v
        prefs.edit().putBoolean(KEY_VOLUME_KEYS, v).apply()
    }

    fun updateKeepScreenOn(v: Boolean) {
        keepScreenOn = v
        prefs.edit().putBoolean(KEY_KEEP_SCREEN, v).apply()
    }

    fun updateShelfSort(v: PvSort) {
        shelfSort = v
        prefs.edit().putString(KEY_SHELF_SORT, v.name).apply()
    }

    /** 「開く」をユーザーが中断した。エラー表示を出さないための印 */
    @Volatile
    private var cancelRequested = false

    /** 7z / RAR の展開など、時間のかかる「開く」を中断する。 */
    fun cancelOpen() {
        if (!busy) return
        cancelRequested = true
        core.cancelOpen()
    }

    /** 閲覧中だけ設定される。音量キーを押したとき処理したら true を返す */
    var volumeHandler: ((Boolean) -> Boolean)? = null

    fun contentResolverOutput(uri: Uri): java.io.OutputStream =
        context.contentResolver.openOutputStream(uri) ?: error("保存先を開けません")

    fun toast(msg: String) = Toast.makeText(context, msg, Toast.LENGTH_SHORT).show()

    /** 「すべてのファイルへのアクセス」が許可されているか（Android 11 以上。未満は標準の選択画面で足りる）。 */
    fun hasAllFilesAccess(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && Environment.isExternalStorageManager()

    /** この端末で、アプリ独自のフォルダ選択が必要か（標準の選択画面が Download 等を拒否する Android 11 以上）。 */
    val needsAllFilesAccess: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R

    /** content:// の書庫・フォルダを開く。fd は Rust 側へ所有権ごと渡し、以降の読み出しは pread で行う。 */
    suspend fun open(uri: Uri) = openWith {
        if (DocumentsContract.isTreeUri(uri) && !isDocumentUri(uri)) {
            // フォルダ（SAF ツリー）。列挙と読み出しは SafTree 経由
            core.openTree(SafTree(context, uri), uri.toString(), SafTree.displayName(context, uri))
        } else {
            val pfd = context.contentResolver.openFileDescriptor(uri, "r")
                ?: error("ファイルを開けません")
            core.openFd(pfd.detachFd(), uri.toString(), displayName(uri))
        }
    }

    /** フォルダをパスで直接開く（「すべてのファイルへのアクセス」が必要）。サブフォルダの画像も 1 冊にまとめる。 */
    suspend fun openLocalPath(path: String) = openWith { core.openLocal(path) }

    /**
     * 画面（Composable）の外で本を開く。フォルダ選択のように「開く指示を出した直後に画面が閉じる」場面で、
     * 画面の CoroutineScope で起動すると、画面と一緒に取り消されてしまうため、アプリの寿命で動く場所から起動する。
     */
    fun openFolderPathInBackground(path: String) {
        appScope.launch { importFolder(path) }
    }

    /** 一括読み込みの進み具合（登録済み to 全体）。null なら実行中でない */
    var importProgress: Pair<Int, Int>? by mutableStateOf(null)
        private set

    /**
     * フォルダ以下を「1 冊ずつ」に分けて読み込む。各フォルダの直下の画像をまとめて 1 冊、EPUB などの書庫は 1 ファイルで 1 冊。
     * 見つかった本が 1 冊ならそのまま開き、複数なら本棚に登録して本棚を見せる。
     */
    private suspend fun importFolder(path: String) {
        if (busy) return
        busy = true
        cancelRequested = false
        try {
            val items = withContext(Dispatchers.IO) { core.scanFolder(path) }
            when (items.size) {
                0 -> toast("画像も書庫（EPUB・ZIP・7z・RAR）も見つかりませんでした")
                1 -> { busy = false; openLocalPath(items[0].path) }
                else -> registerAll(items.map { it.path })
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            toast("開けませんでした: ${userMessage(e)}")
        } finally {
            busy = false
            importProgress = null
        }
    }

    private suspend fun registerAll(paths: List<String>) {
        var added = 0
        var failed = 0
        for ((i, p) in paths.withIndex()) {
            if (cancelRequested) break
            importProgress = i to paths.size
            val ok = withContext(Dispatchers.IO) { runCatching { core.registerLocal(p) }.isSuccess }
            if (ok) added++ else failed++
            if (i % 5 == 4) shelfVersion++ // 途中経過も本棚に出す
        }
        close() // 閲覧中なら閉じて本棚へ
        shelfVersion++
        val tail = if (failed > 0) "（読めなかった $failed 件は除く）" else ""
        toast(if (cancelRequested) "$added 冊を追加して中止しました" else "$added 冊を本棚に追加しました$tail")
    }

    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** 本棚に保存された識別子から開き直す。`file://` はパスで、それ以外は content:// として開く。 */
    suspend fun openBook(uri: String) {
        if (uri.startsWith(FILE_PREFIX)) openLocalPath(uri.removePrefix(FILE_PREFIX)) else open(Uri.parse(uri))
    }

    private suspend fun openWith(block: () -> PvReader) {
        if (busy) return // 連打・外部インテントとの並行オープンを防ぐ
        busy = true
        cancelRequested = false
        try {
            val startedAt = System.nanoTime()
            val opened = withContext(Dispatchers.IO) { block() }
            Log.i("PV", "opened in ${(System.nanoTime() - startedAt) / 1_000_000} ms: ${opened.title()} (${opened.pageCount()} pages)")
            val old = reader
            reader = opened
            old?.let { release(it) } // 閲覧中に別の本を開いた場合、旧 reader を残さない
        } catch (e: CancellationException) {
            throw e // 呼び出し元の取り消しはエラーではない（表示しない）
        } catch (e: SecurityException) {
            toast("この本へのアクセス権がありません。もう一度選び直してください")
        } catch (e: Exception) {
            toast(if (cancelRequested) "キャンセルしました" else "開けませんでした: ${userMessage(e)}")
        } finally {
            busy = false
        }
    }

    /** UniFFI の例外は message が `msg=…` 形式になるので、本文だけを取り出す。 */
    private fun userMessage(e: Exception): String =
        if (e is PvException.Failed) e.msg else e.message ?: e.javaClass.simpleName

    /** `.../tree/X/document/Y` のようにツリー配下の 1 ファイルを指す URI か */
    private fun isDocumentUri(uri: Uri) = uri.pathSegments.contains("document")

    fun close() {
        val r = reader ?: return
        reader = null
        release(r)
        shelfVersion++
    }

    private fun release(r: PvReader) {
        runCatching { r.saveProgress(true) }
        // 閲覧画面のイベントループが nextEvent で待っている可能性があるため、止まるまで待ってから破棄する
        CoroutineScope(Dispatchers.IO).launch {
            delay(DESTROY_DELAY_MS)
            runCatching { r.close() }
        }
    }

    fun onTrimMemory() {
        reader?.trimMemory()
    }

    private fun displayName(uri: Uri): String {
        val raw = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
            ?: uri.lastPathSegment
            ?: "book"
        return raw.substringBeforeLast('.', raw)
    }

    fun bumpShelf() {
        shelfVersion++
    }

    companion object {
        @Volatile
        private var instance: AppState? = null

        /**
         * プロセス内で 1 つだけ作る。Activity が再生成（ダークモード切替など）されても、
         * 開いている本を保ち、PvCore の初期化（展開領域の掃除）を繰り返さない。
         */
        fun get(context: Context): AppState =
            instance ?: synchronized(this) {
                instance ?: AppState(context.applicationContext).also { instance = it }
            }
    }
}

/** Rust が返す RGBA8 を Bitmap にする（不透明なのでプリマルチプライ不要）。 */
fun rgbaToBitmap(width: UInt, height: UInt, rgba: ByteArray): Bitmap {
    val bmp = Bitmap.createBitmap(width.toInt(), height.toInt(), Bitmap.Config.ARGB_8888)
    bmp.copyPixelsFromBuffer(ByteBuffer.wrap(rgba))
    return bmp
}
