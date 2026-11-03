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
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.nio.ByteBuffer
import kotlin.math.roundToInt
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvBook
import uniffi.pv_ffi.PvCore
import uniffi.pv_ffi.PvDirection
import uniffi.pv_ffi.PvGroup
import uniffi.pv_ffi.PvException
import uniffi.pv_ffi.PvReader
import uniffi.pv_ffi.PvTextStyle
import uniffi.pv_ffi.PvSort
import uniffi.pv_ffi.PvStatus

private const val DESTROY_DELAY_MS = 600L
private const val FILE_PREFIX = "file://"
private const val REGISTER_PARALLELISM = 3
private const val KEY_BROWSE_DIR = "browse_dir"
private const val KEY_VOLUME_KEYS = "volume_keys"
private const val KEY_KEEP_SCREEN = "keep_screen"
private const val KEY_SHELF_SORT = "shelf_sort"
private const val KEY_SESSION_BOOK = "session_book"
private const val KEY_SESSION_MENU = "session_menu"
private const val KEY_SESSION_TAB = "session_tab"
private const val KEY_SESSION_FOLDER = "session_folder"
private const val KEY_SESSION_RECURSIVE = "session_recursive"
private const val KEY_SESSION_STATUS = "session_status"
private const val KEY_SESSION_QUERY = "session_query"
private const val KEY_SHELF_SORT_DESC = "shelf_sort_desc"
private const val KEY_SHELF_LIST = "shelf_list"
private const val KEY_LANGUAGE = "language"

/** アプリ全体の状態。画面回転では Activity を作り直さない（Manifest の configChanges）ので単純な保持で足りる。 */
class AppState private constructor(private val context: Context) {
    /**
     * 初回アクセス時に作る。SQLite を開き、前回異常終了で残った展開物を掃除するため重いので、
     * Main スレッドの Activity 生成では作らない（最初に触るのは本棚の読み込み = IO スレッド）。
     */
    val core: PvCore by lazy {
        PvCore(context.filesDir.resolve("pv").absolutePath).also { it.setLanguage(Tr.lang.code) }
    }

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
    /** 本棚で見ていたタブ。本を開いて閉じても、開く前のタブへ戻れるよう、画面の外で持つ */
    var shelfTab by mutableStateOf(ShelfTab.RECENT)

    var shelfQuery by mutableStateOf("")

    /** 開いているグループ。null ならグループの外 */
    var shelfGroup: PvGroup? by mutableStateOf(null)

    /** 「フォルダ」タブで開いているフォルダ（絶対パス）。空ならいちばん上 */
    var shelfFolder by mutableStateOf("")

    /** 「フォルダ」タブで、サブフォルダの本も含めて（配下すべて）表示するか */
    var shelfFolderRecursive by mutableStateOf(false)

    /** 閲覧中のメニュー（上下のバー）が出ているか。画面が作り直されても、バックグラウンドから戻っても保つ */
    var readerMenu by mutableStateOf(false)

    /** 読書状況での絞り込み（null = すべて） */
    var shelfStatus: PvStatus? by mutableStateOf(null)

    init {
        // 本棚のタブ・フォルダ・絞り込みは、前回の終了時のまま始める
        restoreShelf()
    }

    /** 本棚のスクロール位置 */
    val shelfGrid = LazyGridState()

    /** 次に一覧が出たとき、先頭へ戻すか（タブやグループを切り替えたとき） */
    var shelfScrollToTop by mutableStateOf(false)

    /** 画面の言語の設定（"" = 端末の言語に従う / "ja" / "en" / "zh" / "ko" / "es"） */
    var language by mutableStateOf(prefs.getString(KEY_LANGUAGE, LANGUAGE_AUTO) ?: LANGUAGE_AUTO)
        private set

    init {
        Tr.apply(language)
    }

    fun updateLanguage(setting: String) {
        language = setting
        prefs.edit().putString(KEY_LANGUAGE, setting).apply()
        Tr.apply(setting)
        core.setLanguage(Tr.lang.code) // Rust 側のエラーメッセージの言語も合わせる
    }

    /** 本の一覧を、サムネイルではなく「ファイル・フォルダ名の一覧」で見せるか */
    var shelfAsList by mutableStateOf(prefs.getBoolean(KEY_SHELF_LIST, false))
        private set

    fun updateShelfAsList(v: Boolean) {
        shelfAsList = v
        prefs.edit().putBoolean(KEY_SHELF_LIST, v).apply()
        shelfScrollToTop = true
    }

    var shelfSort by mutableStateOf(
        runCatching { PvSort.valueOf(prefs.getString(KEY_SHELF_SORT, null) ?: "") }.getOrDefault(PvSort.NAME),
    )
        private set

    /** 並び順が降順か。名前は昇順（あ→ん）、日付・サイズは新しい・大きい順が既定 */
    var shelfSortDesc by mutableStateOf(
        if (prefs.contains(KEY_SHELF_SORT_DESC)) prefs.getBoolean(KEY_SHELF_SORT_DESC, false) else shelfSort != PvSort.NAME,
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

    /** 並び順のキーを変える。向きは、そのキーの既定（名前は昇順、それ以外は降順）に戻す */
    fun updateShelfSort(v: PvSort) {
        shelfSort = v
        shelfSortDesc = v != PvSort.NAME
        prefs.edit().putString(KEY_SHELF_SORT, v.name).putBoolean(KEY_SHELF_SORT_DESC, shelfSortDesc).apply()
        shelfScrollToTop = true
    }

    fun updateShelfSortDesc(desc: Boolean) {
        shelfSortDesc = desc
        prefs.edit().putBoolean(KEY_SHELF_SORT_DESC, desc).apply()
        shelfScrollToTop = true
    }

    /** 「開く」をユーザーが中断した。エラー表示を出さないための印 */
    @Volatile private var cancelRequested = false

    /** 7z / RAR の展開など、時間のかかる「開く」を中断する。 */
    fun cancelOpen() {
        if (!busy) return
        cancelRequested = true
        core.cancelOpen()
    }

    /** 閲覧中だけ設定される。音量キーを押したとき処理したら true を返す */
    var volumeHandler: ((Boolean) -> Boolean)? = null

    fun contentResolverOutput(uri: Uri): java.io.OutputStream =
        context.contentResolver.openOutputStream(uri) ?: error(Tr.t("保存先を開けません"))

    fun toast(msg: String) = Toast.makeText(context, Tr.t(msg), Toast.LENGTH_SHORT).show()

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
                ?: error(Tr.t("ファイルを開けません"))
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
                else -> registerAll(items.map { it.path }, java.io.File(path).name)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            toast(Tr.f("開けませんでした: {1}", userMessage(e)))
        } finally {
            busy = false
            importProgress = null
        }
    }

    private suspend fun registerAll(paths: List<String>, groupName: String) =
        registerAll(paths.size, groupName, { paths[it] }) { core.registerLocal(paths[it]) }

    /**
     * 選んだ複数のファイルを、開かずに本棚へ登録する。1 つだけなら、そのまま開く。
     * 並べ替えや読書状況は、あとから本棚で扱える。
     */
    suspend fun openDocuments(uris: List<Uri>) {
        if (uris.isEmpty()) return
        if (uris.size == 1) return open(uris[0])
        if (busy) return
        busy = true
        cancelRequested = false
        try {
            core.clearCancel()
            registerAll(uris.size, "", { uris[it].toString() }) { i ->
                val pfd = context.contentResolver.openFileDescriptor(uris[i], "r") ?: error(Tr.t("ファイルを開けません"))
                core.registerFd(pfd.detachFd(), uris[i].toString(), displayName(uris[i]))
            }
            // 履歴タブには開いた本しか出ないので、追加した本が見える「すべて」へ移す
            shelfTab = ShelfTab.ALL
            shelfGroup = null
            shelfScrollToTop = true
        } finally {
            busy = false
            importProgress = null
        }
    }

    /** `count` 件を数冊ずつ並べて登録する。`label` はログ用、`register` は 1 件の登録（失敗は例外）。 */
    private suspend fun registerAll(
        count: Int,
        groupName: String,
        label: (Int) -> String,
        register: (Int) -> PvBook,
    ) {
        val registered = arrayOfNulls<PvBook>(count) // 読んだ順ではなく、並べた順に持つため添字で持つ
        val failed = AtomicInteger()
        val finished = AtomicInteger()
        importProgress = 0 to count
        // 画像のデコードや 7z/RAR の展開は重いので、数冊ずつ並べて登録する（多すぎると作業領域と電池を食う）
        val gate = Semaphore(REGISTER_PARALLELISM)
        coroutineScope {
            (0 until count).map { i ->
                async(Dispatchers.IO) {
                    gate.withPermit {
                        if (cancelRequested) return@withPermit
                        val t0 = System.nanoTime()
                        val book = runCatching { register(i) }
                            .onFailure { Log.w("PV", "register failed: ${label(i)}", it) }
                            .getOrNull()
                        Log.i("PV", "registered in ${(System.nanoTime() - t0) / 1_000_000} ms: ${label(i)}")
                        if (book != null) registered[i] = book else failed.incrementAndGet()
                        val n = finished.incrementAndGet()
                        importProgress = n to count
                        if (n % 5 == 0) shelfVersion++ // 途中経過も本棚に出す
                    }
                }
            }.awaitAll()
        }
        val books = registered.filterNotNull()
        // 複数の本が見つかったときは、フォルダ名のグループにまとめる（見つけた順）
        var grouped = ""
        if (books.size > 1 && groupName.isNotBlank()) {
            runCatching {
                val g = core.createGroup(groupName)
                core.addToGroup(g.id, books.map { it.id })
                grouped = Tr.f("（グループ「{1}」）", g.name)
            }
        }
        close() // 閲覧中なら閉じて本棚へ
        shelfVersion++
        val tail = if (failed.get() > 0) Tr.f("（読めなかった {1} 件は除く）", failed.get()) else ""
        toast(
            if (cancelRequested) Tr.f("{1} 冊を追加して中止しました", books.size)
            else Tr.f("{1} 冊を本棚に追加しました", books.size) + grouped + tail,
        )
    }

    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** 本棚に保存された識別子から開き直す。`file://` はパスで、それ以外は content:// として開く。 */
    suspend fun openBook(uri: String) {
        if (uri.startsWith(FILE_PREFIX)) openLocalPath(uri.removePrefix(FILE_PREFIX)) else open(Uri.parse(uri))
    }

    /** 次に開き終えた本に対する後処理（位置の引き継ぎなど）。一度使うと消える */
    private var afterOpen: ((PvReader) -> Unit)? = null

    /**
     * 文章の本の文字の大きさ・書体を変えて、開き直す。ページの区切りが変わるので、今のページは
     * 本全体に対する位置の割合で引き継ぐ。
     */
    suspend fun applyTextStyle(current: PvReader, style: PvTextStyle) {
        if (busy) return
        val uri = current.uri()
        val last = (current.pageCount().toInt() - 1).coerceAtLeast(1)
        val fraction = current.currentPage().toFloat() / last
        val wasVertical = withContext(Dispatchers.IO) { core.textStyle().vertical }
        withContext(Dispatchers.IO) { core.setTextStyle(style) }
        toast("文字の設定を適用中…")
        afterOpen = { r ->
            if (style.vertical != wasVertical) {
                // 縦書きは右綴じ（左へめくる）、横書きは左綴じ（右へめくる）に合わせる
                val layout = r.layout()
                val dir = if (style.vertical) PvDirection.RIGHT_TO_LEFT else PvDirection.LEFT_TO_RIGHT
                if (layout.direction != dir) r.setLayout(layout.mode, dir)
            }
            r.goToPage(((r.pageCount().toInt() - 1).coerceAtLeast(0) * fraction).roundToInt().toUInt())
        }
        openBook(uri)
        afterOpen = null
    }

    private suspend fun openWith(block: () -> PvReader) {
        if (busy) return // 連打・外部インテントとの並行オープンを防ぐ
        busy = true
        cancelRequested = false
        try {
            val startedAt = System.nanoTime()
            val opened = withContext(Dispatchers.IO) { block() }
            Log.i("PV", "opened in ${(System.nanoTime() - startedAt) / 1_000_000} ms: ${opened.title()} (${opened.pageCount()} pages)")
            afterOpen?.invoke(opened)
            afterOpen = null
            val old = reader
            reader = opened
            readerMenu = pendingMenu ?: false
            old?.let { release(it) } // 閲覧中に別の本を開いた場合、旧 reader を残さない
        } catch (e: CancellationException) {
            throw e // 呼び出し元の取り消しはエラーではない（表示しない）
        } catch (e: SecurityException) {
            toast("この本へのアクセス権がありません。もう一度選び直してください")
        } catch (e: Exception) {
            toast(if (cancelRequested) "キャンセルしました" else Tr.f("開けませんでした: {1}", userMessage(e)))
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
        readerMenu = false
        prefs.edit().remove(KEY_SESSION_BOOK).putBoolean(KEY_SESSION_MENU, false).apply() // 自分で閉じた本は、次回は開き直さない
        release(r)
        shelfVersion++
    }

    /**
     * 今の状態（開いている本・メニュー・本棚のタブや絞り込み）を保存する。バックグラウンドへ移るときに呼ぶ。
     * 他のアプリを使っている間にシステムがこのアプリを終了しても、戻ったときに続きから再開できる。
     */
    fun persistSession() {
        val r = reader
        prefs.edit()
            .putString(KEY_SESSION_BOOK, r?.uri() ?: "")
            .putBoolean(KEY_SESSION_MENU, r != null && readerMenu)
            .putString(KEY_SESSION_TAB, shelfTab.name)
            .putString(KEY_SESSION_FOLDER, shelfFolder)
            .putBoolean(KEY_SESSION_RECURSIVE, shelfFolderRecursive)
            .putString(KEY_SESSION_STATUS, shelfStatus?.name ?: "")
            .putString(KEY_SESSION_QUERY, shelfQuery)
            .apply()
    }

    /** 保存した本棚の状態を戻す（起動時に 1 度）。 */
    private fun restoreShelf() {
        runCatching { ShelfTab.valueOf(prefs.getString(KEY_SESSION_TAB, null) ?: "") }.getOrNull()?.let { shelfTab = it }
        shelfFolder = prefs.getString(KEY_SESSION_FOLDER, "") ?: ""
        shelfFolderRecursive = prefs.getBoolean(KEY_SESSION_RECURSIVE, false)
        shelfStatus = runCatching { PvStatus.valueOf(prefs.getString(KEY_SESSION_STATUS, null) ?: "") }.getOrNull()
        shelfQuery = prefs.getString(KEY_SESSION_QUERY, "") ?: ""
    }

    private var sessionRestored = false

    /**
     * 前回、本を開いたままバックグラウンドへ移ってアプリが終了していたら、その本を同じページ・同じメニューの状態で開き直す。
     * すでに本を開いている（プロセスが生きている）ときや、起動のたびに 2 回目以降は何もしない。
     */
    suspend fun restoreSession() {
        if (sessionRestored) return
        sessionRestored = true
        if (reader != null) return
        val uri = prefs.getString(KEY_SESSION_BOOK, null)?.takeIf { it.isNotBlank() } ?: return
        pendingMenu = prefs.getBoolean(KEY_SESSION_MENU, false)
        openBook(uri)
        pendingMenu = null
        if (reader == null) prefs.edit().remove(KEY_SESSION_BOOK).apply() // 開けなかった本を、毎回試さない
    }

    /** 開き直した本に、保存しておいたメニューの状態を使うための一時的な値 */
    private var pendingMenu: Boolean? = null

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
