//! 表示倍率・パンの計算（純粋関数）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitMode {
    Width,
    Height,
    Page,
    /// 全画面（画像の縦横比を無視して、画面ちょうどに引き伸ばし、全体を収める）。
    /// 倍率が縦横で違うので `fit_scale` では表せず、描画側（`dest_size`）が枠の大きさをそのまま使う。
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// コンテンツをビューポートへフィットさせる倍率。
pub fn fit_scale(content: Vec2, viewport: Vec2, mode: FitMode) -> f32 {
    if content.x <= 0.0 || content.y <= 0.0 {
        return 1.0;
    }
    let (sx, sy) = (viewport.x / content.x, viewport.y / content.y);
    match mode {
        FitMode::Width => sx,
        FitMode::Height => sy,
        FitMode::Page => sx.min(sy),
        FitMode::Fill => sx.max(sy),
    }
}

pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 8.0;

pub fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// 表示サイズ（フィット × ズーム）に対して、パン量を画面外へ出ない範囲に収める。
/// パンは中心からのオフセット。
pub fn clamp_pan(pan: Vec2, shown: Vec2, viewport: Vec2) -> Vec2 {
    let limit = |s: f32, v: f32| ((s - v) / 2.0).max(0.0);
    let (lx, ly) = (limit(shown.x, viewport.x), limit(shown.y, viewport.y));
    Vec2::new(pan.x.clamp(-lx, lx), pan.y.clamp(-ly, ly))
}

/// ピンチ / ホイールでの拡大。`anchor` はビューポート中心からのカーソル位置。
/// アンカー下の点が動かないようパンを補正する。
pub fn zoom_at(zoom: f32, pan: Vec2, anchor: Vec2, factor: f32) -> (f32, Vec2) {
    let next = clamp_zoom(zoom * factor);
    let k = next / zoom;
    (
        next,
        Vec2::new(
            anchor.x - (anchor.x - pan.x) * k,
            anchor.y - (anchor.y - pan.y) * k,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_modes() {
        let c = Vec2::new(100.0, 200.0);
        let v = Vec2::new(400.0, 400.0);
        assert_eq!(fit_scale(c, v, FitMode::Width), 4.0);
        assert_eq!(fit_scale(c, v, FitMode::Height), 2.0);
        assert_eq!(fit_scale(c, v, FitMode::Page), 2.0);
        assert_eq!(fit_scale(c, v, FitMode::Fill), 4.0);
        assert_eq!(fit_scale(Vec2::ZERO, v, FitMode::Page), 1.0);
    }

    #[test]
    fn pan_is_clamped_to_overflow() {
        let p = clamp_pan(
            Vec2::new(500.0, -500.0),
            Vec2::new(800.0, 300.0),
            Vec2::new(400.0, 400.0),
        );
        assert_eq!(p, Vec2::new(200.0, 0.0));
    }

    #[test]
    fn zoom_is_clamped_and_anchored() {
        let (z, p) = zoom_at(1.0, Vec2::ZERO, Vec2::new(10.0, 0.0), 2.0);
        assert_eq!(z, 2.0);
        assert_eq!(p, Vec2::new(-10.0, 0.0));
        assert_eq!(zoom_at(8.0, Vec2::ZERO, Vec2::ZERO, 2.0).0, MAX_ZOOM);
        assert_eq!(clamp_zoom(0.1), MIN_ZOOM);
    }
}
