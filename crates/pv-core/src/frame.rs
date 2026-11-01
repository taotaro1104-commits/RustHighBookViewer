use pv_decode::{Bitmap, Size};

/// request_frame の引換券。世代は go_to のたびに進む。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameTicket {
    pub id: u64,
    pub spread: usize,
    pub generation: u64,
}

/// 1 スプレッド分の描画済みビットマップ。
#[derive(Debug)]
pub struct Frame {
    pub spread: usize,
    /// 画面左から右の順のページ番号
    pub pages: Vec<usize>,
    pub bitmap: Bitmap,
    pub viewport: Size,
    pub scale: f32,
}

#[derive(Debug, Clone)]
pub enum CoreEvent {
    FrameReady {
        ticket: FrameTicket,
        frame: std::sync::Arc<Frame>,
    },
    /// ページ移動で世代が進み、破棄された要求
    Cancelled { ticket: FrameTicket },
    Error {
        ticket: FrameTicket,
        message: String,
    },
}
