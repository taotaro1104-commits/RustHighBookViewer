package jp.perfectviewer.rs

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.SurfaceHolder
import android.view.SurfaceView
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

/**
 * 閲覧画面の描画面。Rust から受け取ったビットマップを描くだけで、
 * ズーム・パン・タップ判定などの操作は UI 側の責務（設計書 6 章）。
 */
class PageSurfaceView(context: Context) : SurfaceView(context), SurfaceHolder.Callback {
    /** 画面を 3x3 に分けたゾーン (0..8) のタップ */
    var onTapZone: (Int) -> Unit = {}
    var onDoubleTap: () -> Unit = {}
    var onLongPress: () -> Unit = {}
    /** ビューポートの実ピクセルサイズ。Rust へのフレーム要求に使う */
    var onViewport: (Int, Int) -> Unit = { _, _ -> }
    /** ズーム倍率が変わった */
    var onZoom: (Float) -> Unit = {}
    /** 端を超えてスクロールされた (+1: 次, -1: 前) */
    var onEdgeScroll: (Int) -> Unit = {}

    private var bitmap: Bitmap? = null
    private var frameScale = 1f
    private var zoom = 1f
    private var panX = 0f
    private var panY = 0f
    private var overscroll = 0f
    private var edgeFired = false
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private var surfaceReady = false

    /** ルーペモード。ON の間は指の位置を拡大表示し、ページ送り・パンの操作は受け付けない */
    var magnifier = false
        set(value) {
            field = value
            loupeActive = false
            draw()
        }
    private var loupeActive = false
    private var loupeX = 0f
    private var loupeY = 0f
    private val loupeRadius = 84f * resources.displayMetrics.density
    private val loupePath = Path()
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 3f * resources.displayMetrics.density
        color = Color.WHITE
    }

    init {
        holder.addCallback(this)
    }

    /** 新しいフレームを表示する。ページが変わったときは [topAligned] で縦にはみ出す分を上端に寄せる */
    fun setFrame(bmp: Bitmap, scale: Float, newPage: Boolean, topAligned: Boolean) {
        bitmap = bmp
        frameScale = scale
        if (newPage) {
            zoom = 1f
            panX = 0f
            panY = 0f
            edgeFired = false
            overscroll = 0f
            if (topAligned) panY = limitY()
        }
        clampPan()
        draw()
    }

    fun currentZoom(): Float = zoom

    fun setZoom(z: Float, focusX: Float = width / 2f, focusY: Float = height / 2f) {
        val next = z.coerceIn(MIN_ZOOM, MAX_ZOOM)
        val k = next / zoom
        val cx = focusX - width / 2f
        val cy = focusY - height / 2f
        panX = cx - (cx - panX) * k
        panY = cy - (cy - panY) * k
        zoom = next
        clampPan()
        onZoom(zoom)
        draw()
    }

    private fun shownW() = (bitmap?.width ?: 0) / frameScale * zoom
    private fun shownH() = (bitmap?.height ?: 0) / frameScale * zoom
    private fun limitX() = max(0f, (shownW() - width) / 2f)
    private fun limitY() = max(0f, (shownH() - height) / 2f)

    private fun clampPan() {
        panX = panX.coerceIn(-limitX(), limitX())
        panY = panY.coerceIn(-limitY(), limitY())
    }

    private fun draw() {
        if (!surfaceReady) return
        val canvas = holder.lockCanvas() ?: return
        try {
            canvas.drawColor(Color.BLACK)
            val bmp = bitmap ?: return
            val w = shownW()
            val h = shownH()
            val cx = width / 2f + panX
            val cy = height / 2f + panY
            val dst = RectF(cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2)
            canvas.drawBitmap(bmp, null, dst, paint)
            if (magnifier && loupeActive) drawLoupe(canvas, bmp, dst)
        } finally {
            holder.unlockCanvasAndPost(canvas)
        }
    }

    /** 指の位置を中心に LOUPE_ZOOM 倍した円を、指に隠れないよう少し上（上端付近は下）に描く */
    private fun drawLoupe(canvas: android.graphics.Canvas, bmp: Bitmap, dst: RectF) {
        val above = loupeY - loupeRadius * 1.4f
        val oy = if (above - loupeRadius < 0f) loupeY + loupeRadius * 1.4f else above
        loupePath.reset()
        loupePath.addCircle(loupeX, oy, loupeRadius, Path.Direction.CW)
        canvas.save()
        canvas.clipPath(loupePath)
        canvas.drawColor(Color.BLACK)
        canvas.translate(loupeX, oy)
        canvas.scale(LOUPE_ZOOM, LOUPE_ZOOM)
        canvas.translate(-loupeX, -loupeY)
        canvas.drawBitmap(bmp, null, dst, paint)
        canvas.restore()
        canvas.drawCircle(loupeX, oy, loupeRadius, ringPaint)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = true
        draw()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, w: Int, h: Int) {
        clampPan()
        onViewport(w, h)
        draw()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
    }

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(e: MotionEvent) = true

        override fun onSingleTapConfirmed(e: MotionEvent): Boolean {
            val col = min(2, max(0, (e.x / max(1, width) * 3).toInt()))
            val row = min(2, max(0, (e.y / max(1, height) * 3).toInt()))
            onTapZone(row * 3 + col)
            return true
        }

        override fun onDoubleTap(e: MotionEvent): Boolean {
            onDoubleTap()
            return true
        }

        override fun onLongPress(e: MotionEvent) {
            onLongPress()
        }

        override fun onScroll(e1: MotionEvent?, e2: MotionEvent, dx: Float, dy: Float): Boolean {
            if (limitX() == 0f && limitY() == 0f) return true
            panX -= dx
            val wantedY = panY - dy
            panY = wantedY.coerceIn(-limitY(), limitY())
            clampPan()
            // 縦にはみ出すページの端を超えてスクロールしたら次/前のページへ（Webtoon 的な連続閲覧）
            val over = wantedY - panY
            if (limitY() > 0f && over != 0f) {
                overscroll += over
                if (!edgeFired && abs(overscroll) > EDGE_SCROLL_PX) {
                    edgeFired = true
                    onEdgeScroll(if (overscroll < 0) 1 else -1)
                }
            } else {
                overscroll = 0f
            }
            draw()
            return true
        }
    })

    private val scaler = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            setZoom(zoom * d.scaleFactor, d.focusX, d.focusY)
            return true
        }
    })

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (magnifier) {
            loupeX = event.x
            loupeY = event.y
            loupeActive = event.actionMasked != MotionEvent.ACTION_UP && event.actionMasked != MotionEvent.ACTION_CANCEL
            draw()
            return true
        }
        if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
            overscroll = 0f
            edgeFired = false
        }
        scaler.onTouchEvent(event)
        if (!scaler.isInProgress) gestures.onTouchEvent(event)
        return true
    }

    companion object {
        const val MIN_ZOOM = 1f
        const val MAX_ZOOM = 8f
        const val LOUPE_ZOOM = 2f
        private const val EDGE_SCROLL_PX = 160f
    }
}
