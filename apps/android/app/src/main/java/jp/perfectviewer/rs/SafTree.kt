package jp.perfectviewer.rs

import android.content.Context
import android.net.Uri
import android.util.Log
import android.provider.DocumentsContract
import uniffi.pv_ffi.PvException
import uniffi.pv_ffi.PvTree
import uniffi.pv_ffi.PvTreeEntry

/**
 * SAF のフォルダ（OpenDocumentTree で得たツリー URI）を Rust へ見せる。
 * 列挙は ContentResolver の子ドキュメント照会、読み出しは 1 ファイルごとに fd を開いて Rust に渡す。
 */
class SafTree(private val context: Context, private val treeUri: Uri) : PvTree {
    private class Doc(val uri: Uri, val path: String, val size: Long)

    @Volatile
    private var docs: List<Doc> = emptyList()

    override fun entries(): List<PvTreeEntry> {
        val found = ArrayList<Doc>()
        val t0 = System.nanoTime()
        try {
            walk(DocumentsContract.getTreeDocumentId(treeUri), "", 0, found)
        } catch (e: Exception) {
            throw PvException.Failed(Tr.f("フォルダを読めません: {1}", e.message))
        }
        docs = found
        Log.i("PV", "tree listed ${found.size} files in ${(System.nanoTime() - t0) / 1_000_000} ms")
        return found.map { PvTreeEntry(it.path, it.size.coerceAtLeast(0).toULong()) }
    }

    override fun openFd(index: UInt): Int {
        val doc = docs.getOrNull(index.toInt()) ?: throw PvException.Failed(Tr.f("存在しないファイル: {1}", index))
        try {
            val pfd = context.contentResolver.openFileDescriptor(doc.uri, "r")
                ?: throw PvException.Failed(Tr.f("開けません: {1}", doc.path))
            return pfd.detachFd() // 所有権は Rust へ移る
        } catch (e: PvException) {
            throw e
        } catch (e: Exception) {
            throw PvException.Failed(Tr.f("開けません: {1} ({2})", doc.path, e.message))
        }
    }

    private fun walk(parentId: String, prefix: String, depth: Int, out: MutableList<Doc>) {
        if (depth > MAX_DEPTH) return
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentId)
        val cols = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
        )
        val subdirs = ArrayList<Pair<String, String>>()
        context.contentResolver.query(children, cols, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                // 黙って切り詰めるとページ欠けや本の識別子の変化に気付けないので、エラーにする
                if (out.size >= MAX_FILES) throw PvException.Failed(Tr.f("ファイルが多すぎます（上限 {1}）", MAX_FILES))
                val id = c.getString(0)
                val name = c.getString(1) ?: continue
                val path = prefix + name
                if (c.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR) {
                    subdirs += id to "$path/"
                } else {
                    out += Doc(DocumentsContract.buildDocumentUriUsingTree(treeUri, id), path, if (c.isNull(3)) 0 else c.getLong(3))
                }
            }
        }
        for ((id, p) in subdirs) walk(id, p, depth + 1, out)
    }

    companion object {
        private const val MAX_DEPTH = 6
        private const val MAX_FILES = 50_000

        /** ツリー URI のフォルダ名（本のタイトルに使う） */
        fun displayName(context: Context, treeUri: Uri): String {
            val docUri = DocumentsContract.buildDocumentUriUsingTree(
                treeUri,
                DocumentsContract.getTreeDocumentId(treeUri),
            )
            return context.contentResolver
                .query(docUri, arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)
                ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
                ?: "folder"
        }
    }
}
