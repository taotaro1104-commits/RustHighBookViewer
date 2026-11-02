package jp.perfectviewer.rs

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Slider
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.pv_ffi.PvAction
import uniffi.pv_ffi.PvDirection
import uniffi.pv_ffi.PvEvent
import uniffi.pv_ffi.PvFit
import uniffi.pv_ffi.PvReader
import uniffi.pv_ffi.PvViewMode

/** ルーペ中に使うフレームの精細度 */
private const val MAGNIFIER_QUALITY = 2f

/**
 * コアに要求する描画倍率の上限。ズームのたびにページ全体をその倍率で作り直すので、
 * 大きすぎると（8 倍で 8192px 級）描画が終わるまで画面が固まって見える。これを超える拡大は、表示側で引き伸ばす。
 */
private const val MAX_RENDER_SCALE = 2f

/** 画面に出しているフレーム / 要求中のフレームを区別するキー（ページ・ビューポート・精細度） */
private data class FrameKey(val spread: Int, val w: Int, val h: Int, val scale: Float)

@Composable
fun ReaderScreen(state: AppState, reader: PvReader) {
    val scope = rememberCoroutineScope()
    // メニューの表示は AppState に持つ（画面が作り直されても、バックグラウンドから戻っても保たれる）
    var uiVisible by state::readerMenu
    var page by remember { mutableIntStateOf(reader.currentPage().toInt()) }
    var layout by remember { mutableStateOf(reader.layout()) }
    var zoomLevel by remember { mutableFloatStateOf(1f) }
    var viewport by remember { mutableStateOf(0 to 0) }
    var modeMenu by remember { mutableStateOf(false) }
    var showColor by remember { mutableStateOf(false) }
    var showGrid by remember { mutableStateOf(false) }
    var showText by remember { mutableStateOf(false) }
    var showJump by remember { mutableStateOf(false) }
    var showToc by remember { mutableStateOf(false) }
    // EPUB の目次（無ければ空。「目次」ボタンは目次のある本にだけ出す）
    val toc = remember(reader) { runCatching { reader.toc() }.getOrDefault(emptyList()) }
    /** しおりのあるページ（0 始まり） */
    var bookmarks by remember { mutableStateOf(emptyList<Int>()) }
    var magnifier by remember { mutableStateOf(false) }
    var autoMenu by remember { mutableStateOf(false) }
    /** オートめくりの間隔（秒）。0 でオフ */
    var autoSec by remember { mutableIntStateOf(0) }
    /** 最後に要求したフレーム。これ以外のフレームは古い色・古いサイズなので捨てる */
    var latestTicket by remember { mutableStateOf(0uL) }
    val surfaceHolder = remember { arrayOfNulls<PageSurfaceView>(1) }

    var shown: FrameKey? by remember { mutableStateOf(null) }
    var pending: FrameKey? by remember { mutableStateOf(null) }

    fun refreshPage() {
        page = reader.currentPage().toInt()
        layout = reader.layout()
    }

    /** ページ移動・リサイズ・ズーム変更のたびに、必要ならコアへフレームを要求する */
    fun requestIfNeeded() {
        val (w, h) = viewport
        if (w <= 0 || h <= 0) return
        // ルーペ中は拡大して見られるよう、常に高精細なフレームを使う
        val quality = min(if (magnifier) max(zoomLevel, MAGNIFIER_QUALITY) else zoomLevel, MAX_RENDER_SCALE)
        val key = FrameKey(reader.cursor().toInt(), w, h, ceil(quality * 2f) / 2f)
        if (key == shown || key == pending) return
        pending = key
        latestTicket = reader.requestFrame(key.spread.toUInt(), w.toUInt(), h.toUInt(), key.scale)
    }

    /** 色・ルーペなど、同じページでも画像を作り直したいとき */
    fun redraw() {
        shown = null
        pending = null
        requestIfNeeded()
    }

    /** ページ移動の前に、拡大・パンを等倍に戻す。拡大したままだと、新しいページも高倍率で描くまで表示されない */
    fun resetZoom() {
        zoomLevel = 1f
        surfaceHolder[0]?.resetZoom()
    }

    /** 指定ページ（0 始まり）へ移動する。スライダー・ページ一覧・ページ指定・しおりで共通 */
    fun jumpTo(target: Int) {
        val last = (reader.pageCount().toInt() - 1).coerceAtLeast(0)
        resetZoom()
        reader.goToPage(target.coerceIn(0, last).toUInt())
        reader.saveProgress(false)
        refreshPage()
        requestIfNeeded()
    }

    fun toggleBookmark() {
        scope.launch {
            val msg = withContext(Dispatchers.IO) {
                runCatching {
                    val p = reader.currentPage()
                    val text = if (reader.bookmarks().contains(p)) {
                        reader.removeBookmark(p)
                        Tr.f("しおりを外しました: {1} ページ", p + 1u)
                    } else {
                        Tr.f("しおりを追加: {1} ページ", reader.addBookmark() + 1u)
                    }
                    bookmarks = reader.bookmarks().map { it.toInt() }
                    text
                }.getOrElse { Tr.f("しおりを操作できません: {1}", it.message) }
            }
            state.toast(msg)
        }
    }

    val saveLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/*")) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch(Dispatchers.IO) {
            val msg = runCatching {
                val bytes = reader.pageBytes(reader.currentPage())
                state.contentResolverOutput(uri).use { it.write(bytes) }
            }.fold({ "ページを保存しました" }, { Tr.f("保存に失敗: {1}", it.message) })
            withContext(Dispatchers.Main) { state.toast(msg) }
        }
    }

    fun perform(action: PvAction?) {
        when (action) {
            null -> Unit
            PvAction.ZOOM_IN -> surfaceHolder[0]?.let { it.setZoom(it.currentZoom() * 1.25f) }
            PvAction.ZOOM_OUT -> surfaceHolder[0]?.let { it.setZoom(it.currentZoom() / 1.25f) }
            PvAction.OPEN_THUMBNAILS -> uiVisible = !uiVisible // 既定の中央タップ: メニューの表示切替
            PvAction.ADD_BOOKMARK -> toggleBookmark()
            PvAction.SAVE_PAGE -> saveLauncher.launch(reader.pageName(reader.currentPage()).ifEmpty { "page.png" })
            PvAction.TOGGLE_MAGNIFIER -> magnifier = !magnifier
            PvAction.OTHER -> state.toast("この操作は未対応です")
            else -> {
                resetZoom()
                if (reader.apply(action)) {
                    reader.saveProgress(false)
                    refreshPage()
                    requestIfNeeded()
                }
            }
        }
    }

    // ボリュームキーのページ送り
    DisposableEffect(reader) {
        state.volumeHandler = { up -> state.volumeKeys && reader.volumeAction(up)?.also { perform(it) } != null }
        onDispose { state.volumeHandler = null }
    }

    // コアからのフレームを受け取って描画面に渡す（ブロッキング待ちなので IO スレッド）
    LaunchedEffect(reader) {
        withContext(Dispatchers.IO) {
            while (isActive) {
                // 本を閉じた直後に破棄済みになることがある。どこで投げられてもループを終える
                try {
                    val ev = reader.nextEvent(100u) ?: continue
                    when (ev) {
                        is PvEvent.Frame -> {
                            val f = ev.frame
                            if (f.spread != reader.cursor() || f.ticket != latestTicket) continue // 古いフレームは捨てる
                            val bmp = rgbaToBitmap(f.width, f.height, f.rgba)
                            withContext(Dispatchers.Main) {
                                val key = FrameKey(f.spread.toInt(), f.viewportWidth.toInt(), f.viewportHeight.toInt(), f.scale)
                                val newPage = shown?.spread != key.spread
                                val topAligned = reader.layout().fit == PvFit.WIDTH
                                surfaceHolder[0]?.setFrame(bmp, f.scale, newPage, topAligned)
                                shown = key
                                if (pending == key) pending = null
                                if (newPage) zoomLevel = 1f
                                requestIfNeeded()
                            }
                        }
                        // 取り消し・失敗も最新の要求のものだけ扱う（古い要求の遅れた通知で状態を巻き戻さない）
                        is PvEvent.Cancelled -> if (ev.ticket == latestTicket) {
                            withContext(Dispatchers.Main) { pending = null; requestIfNeeded() }
                        }
                        is PvEvent.Failed -> if (ev.ticket == latestTicket) {
                            withContext(Dispatchers.Main) {
                                pending = null
                                state.toast(Tr.f("表示エラー: {1}", ev.message))
                            }
                        }
                    }
                } catch (e: IllegalStateException) {
                    break
                }
            }
        }
    }

    DisposableEffect(reader) { onDispose { runCatching { reader.saveProgress(true) } } }

    // 「全画面」のフィットでは、ステータスバーとナビゲーションバーも隠す（端からスワイプすれば一時的に出る）
    val window = (LocalContext.current as? android.app.Activity)?.window
    val fullscreen = layout.fit == PvFit.FILL
    DisposableEffect(window, fullscreen) {
        val controller = window?.let { androidx.core.view.WindowCompat.getInsetsController(it, it.decorView) }
        if (fullscreen) {
            controller?.systemBarsBehavior =
                androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            controller?.hide(androidx.core.view.WindowInsetsCompat.Type.systemBars())
        }
        onDispose { controller?.show(androidx.core.view.WindowInsetsCompat.Type.systemBars()) }
    }

    // しおりの一覧を読み込む
    LaunchedEffect(reader) {
        bookmarks = withContext(Dispatchers.IO) { runCatching { reader.bookmarks().map { it.toInt() } }.getOrDefault(emptyList()) }
    }

    // ルーペの ON/OFF に合わせて描画面と精細度を切り替える
    LaunchedEffect(magnifier) {
        surfaceHolder[0]?.magnifier = magnifier
        redraw()
    }

    // オートめくり: 操作・メニュー表示・ダイアログ中は待ち直し、最後まで来たら止める
    LaunchedEffect(autoSec, page, uiVisible, showColor, showGrid, showJump, showToc, magnifier, state.keepScreenOn) {
        surfaceHolder[0]?.keepScreenOn = autoSec > 0 || state.keepScreenOn
        if (autoSec <= 0 || uiVisible || showColor || showGrid || showJump || showToc || magnifier) return@LaunchedEffect
        delay(autoSec * 1000L)
        if (reader.cursor() + 1u >= reader.spreadCount()) {
            autoSec = 0
            state.toast("最後のページです。オートめくりを止めました")
        } else {
            perform(PvAction.NEXT_SPREAD)
        }
    }
    BackHandler { state.close() }
    // ルーペ中は画面操作を奪うので、戻るキーで先にルーペを終える
    BackHandler(enabled = magnifier) { magnifier = false }

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { ctx ->
                PageSurfaceView(ctx).also { v ->
                    surfaceHolder[0] = v
                    v.onViewport = { w, h -> viewport = w to h; requestIfNeeded() }
                    v.onTapZone = { z -> perform(reader.tapAction(z.toUByte())) }
                    // ルーペ中の短いタップ: ルーペを解除して、通常のタップ（ページ送りなど）として動かす
                    v.onMagnifierTap = { z ->
                        magnifier = false
                        perform(reader.tapAction(z.toUByte()))
                    }
                    // 左右フリック: 指が右へ = 左側タップ、左へ = 右側タップと同じ（綴じ方向に合わせて進む向きが変わる）
                    v.onFlick = { dir -> perform(reader.tapAction((if (dir > 0) 3 else 5).toUByte())) }
                    v.onDoubleTap = {
                        // 既定の割り当ては ZoomIn。ここでは 1x ↔ 2.5x の切り替えとして扱う
                        if (reader.doubleTapAction() == PvAction.ZOOM_IN) v.setZoom(if (v.currentZoom() > 1f) 1f else 2.5f)
                        else perform(reader.doubleTapAction())
                    }
                    v.onLongPress = { perform(reader.longPressAction()) }
                    v.onZoom = { z -> zoomLevel = z; requestIfNeeded() }
                    v.onEdgeScroll = { dir -> perform(if (dir > 0) PvAction.NEXT_SPREAD else PvAction.PREV_SPREAD) }
                }
            },
        )

        if (showText) {
            TextStyleDialog(
                state,
                onApply = { st -> showText = false; scope.launch { state.applyTextStyle(reader, st) } },
                onClose = { showText = false },
            )
        }
        if (showColor) {
            ColorDialog(
                initial = reader.color(),
                onChange = { c -> scope.launch { withContext(Dispatchers.IO) { reader.setColor(c) }; redraw() } },
                onDismiss = { showColor = false },
            )
        }
        if (showGrid) {
            PageGridDialog(
                reader,
                currentPage = page,
                onPick = { p ->
                    showGrid = false
                    jumpTo(p)
                },
                onDismiss = { showGrid = false },
            )
        }
        if (showToc) {
            TocDialog(
                toc = toc,
                currentPage = page,
                onSelect = { p -> showToc = false; uiVisible = false; jumpTo(p) },
                onDismiss = { showToc = false },
            )
        }
        if (showJump) {
            PageJumpDialog(
                total = reader.pageCount().toInt(),
                current = page,
                bookmarks = bookmarks,
                onJump = { p ->
                    showJump = false
                    jumpTo(p)
                },
                onRemoveBookmark = { p ->
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching { reader.removeBookmark(p.toUInt()) }
                            bookmarks = runCatching { reader.bookmarks().map { it.toInt() } }.getOrDefault(bookmarks)
                        }
                    }
                },
                onDismiss = { showJump = false },
            )
        }

        if (uiVisible) {
            Column(Modifier.fillMaxWidth().background(Color(0xCC000000)).statusBarsPadding().padding(8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    TextButton(onClick = { state.close() }) { Text("← 本棚") }
                    Text("${reader.title()}  ${page + 1}/${reader.pageCount()}", color = Color.White, maxLines = 1)
                }
                Row(
                    modifier = Modifier.horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // 本棚へ戻らずに、フォルダを選び直して別の本を開く
                    TextButton(onClick = { uiVisible = false; state.pickFolderHandler?.invoke() }) { Text("📁 フォルダ参照") }
                    Box {
                        TextButton(onClick = { modeMenu = true }) {
                            Text(
                                when (layout.mode) {
                                    PvViewMode.SINGLE -> "単ページ"
                                    PvViewMode.DOUBLE -> "見開き"
                                    PvViewMode.AUTO -> "自動見開き"
                                    PvViewMode.WEBTOON -> "縦スクロール"
                                },
                            )
                        }
                        DropdownMenu(expanded = modeMenu, onDismissRequest = { modeMenu = false }) {
                            listOf(
                                PvViewMode.SINGLE to "単ページ",
                                PvViewMode.DOUBLE to "見開き",
                                PvViewMode.AUTO to "自動見開き",
                                PvViewMode.WEBTOON to "縦スクロール",
                            ).forEach { (m, label) ->
                                DropdownMenuItem(text = { Text(label) }, onClick = {
                                    modeMenu = false
                                    scope.launch {
                                        withContext(Dispatchers.IO) {
                                            if (m == PvViewMode.WEBTOON) reader.apply(PvAction.FIT_WIDTH)
                                            reader.setLayout(m, layout.direction)
                                        }
                                        refreshPage()
                                        redraw()
                                    }
                                })
                            }
                        }
                    }
                    val rtl = layout.direction == PvDirection.RIGHT_TO_LEFT
                    TextButton(onClick = {
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                reader.setLayout(layout.mode, if (rtl) PvDirection.LEFT_TO_RIGHT else PvDirection.RIGHT_TO_LEFT)
                            }
                            refreshPage()
                            redraw()
                        }
                    }) { Text(if (rtl) "右綴じ" else "左綴じ") }
                    listOf(PvFit.WIDTH to "幅", PvFit.HEIGHT to "高さ", PvFit.PAGE to "全体", PvFit.FILL to "全画面").forEach { (fit, label) ->
                        FilterChip(selected = layout.fit == fit, onClick = {
                            perform(when (fit) {
                                PvFit.WIDTH -> PvAction.FIT_WIDTH
                                PvFit.HEIGHT -> PvAction.FIT_HEIGHT
                                PvFit.PAGE -> PvAction.FIT_PAGE
                                PvFit.FILL -> PvAction.FIT_FILL
                            })
                            shown = null
                            requestIfNeeded()
                        }, label = { Text(label) })
                    }
                    if (toc.isNotEmpty()) TextButton(onClick = { showToc = true }) { Text("目次") }
                    TextButton(onClick = { showJump = true }) { Text("ページ指定") }
                    TextButton(onClick = { showGrid = true }) { Text("一覧") }
                    if (reader.isText()) TextButton(onClick = { showText = true }) { Text("文字") }
                    TextButton(onClick = { showColor = true }) { Text("色調整") }
                    FilterChip(selected = magnifier, onClick = { magnifier = !magnifier; if (magnifier) { uiVisible = false; state.toast("ルーペ: 押したままなぞって拡大。短くタップで終了") } }, label = { Text("ルーペ") })
                    Box {
                        TextButton(onClick = { autoMenu = true }) { Text(if (autoSec > 0) Tr.f("自動 {1}秒", autoSec) else "自動めくり") }
                        DropdownMenu(expanded = autoMenu, onDismissRequest = { autoMenu = false }) {
                            listOf(0, 3, 5, 10, 20).forEach { sec ->
                                DropdownMenuItem(text = { Text(if (sec == 0) "オフ" else Tr.f("{1}秒ごと", sec)) }, onClick = {
                                    autoMenu = false
                                    autoSec = sec
                                    if (sec > 0) uiVisible = false
                                })
                            }
                        }
                    }
                    TextButton(onClick = { toggleBookmark() }) { Text(if (bookmarks.contains(page)) "★ しおり" else "☆ しおり") }
                    TextButton(onClick = { perform(PvAction.SAVE_PAGE) }) { Text("保存") }
                    TextButton(onClick = { state.showSettings = true }) { Text("設定") }
                }
            }
            // シークバー。右綴じでは右から左へ進むよう向きを反転する
            val dir = if (layout.direction == PvDirection.RIGHT_TO_LEFT) LayoutDirection.Rtl else LayoutDirection.Ltr
            Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(Color(0xCC000000)).padding(12.dp)) {
                CompositionLocalProvider(LocalLayoutDirection provides dir) {
                    val last = (reader.pageCount().toInt() - 1).coerceAtLeast(1)
                    var drag by remember(page) { mutableFloatStateOf(page.toFloat()) }
                    Slider(
                        value = drag,
                        onValueChange = { drag = it },
                        onValueChangeFinished = { jumpTo(drag.roundToInt()) },
                        valueRange = 0f..last.toFloat(),
                    )
                }
            }
        }
    }
}
