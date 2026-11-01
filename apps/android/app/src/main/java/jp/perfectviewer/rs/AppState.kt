package jp.perfectviewer.rs

import android.content.Context
import android.graphics.Bitmap
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.widget.Toast
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.nio.ByteBuffer
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvCore
import uniffi.pv_ffi.PvReader

private const val DESTROY_DELAY_MS = 600L

/** アプリ全体の状態。画面回転では Activity を作り直さない（Manifest の configChanges）ので単純な保持で足りる。 */
class AppState private constructor(private val context: Context) {
    val core: PvCore = PvCore(context.filesDir.resolve("pv").absolutePath)

    /** 開いている本。null なら本棚を表示する */
    var reader: PvReader? by mutableStateOf(null)
        private set

    /** 本棚の再読み込み用。値が変わるたびに一覧を取り直す */
    var shelfVersion by mutableIntStateOf(0)
        private set

    var busy by mutableStateOf(false)
        private set

    /** 閲覧中だけ設定される。音量キーを押したとき処理したら true を返す */
    var volumeHandler: ((Boolean) -> Boolean)? = null

    fun contentResolverOutput(uri: Uri): java.io.OutputStream =
        context.contentResolver.openOutputStream(uri) ?: error("保存先を開けません")

    fun toast(msg: String) = Toast.makeText(context, msg, Toast.LENGTH_SHORT).show()

    /** content:// の書庫を開く。fd は Rust 側へ所有権ごと渡し、以降の読み出しは pread で行う。 */
    suspend fun open(uri: Uri) {
        if (busy) return // 連打・外部インテントとの並行オープンを防ぐ
        busy = true
        try {
            val opened = withContext(Dispatchers.IO) {
                if (DocumentsContract.isTreeUri(uri) && !isDocumentUri(uri)) {
                    // フォルダ（SAF ツリー）。列挙と読み出しは SafTree 経由
                    core.openTree(SafTree(context, uri), uri.toString(), SafTree.displayName(context, uri))
                } else {
                    val pfd = context.contentResolver.openFileDescriptor(uri, "r")
                        ?: error("ファイルを開けません")
                    core.openFd(pfd.detachFd(), uri.toString(), displayName(uri))
                }
            }
            val old = reader
            reader = opened
            old?.let { release(it) } // 閲覧中に別の本を開いた場合、旧 reader を残さない
        } catch (e: SecurityException) {
            toast("この本へのアクセス権がありません。もう一度ファイルを選んでください")
        } catch (e: Exception) {
            toast("開けませんでした: ${e.message}")
        } finally {
            busy = false
        }
    }

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
