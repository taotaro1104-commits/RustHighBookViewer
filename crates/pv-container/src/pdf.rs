//! PDF。各ページを 1 枚の画像（PNG）に描いて返す。ページ数は開くときに決まり、描画は読み出し時に行う。

use std::io::{Cursor, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use bytes::Bytes;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

use crate::container::{Container, ContainerProvider, Probe};
use crate::entry::{EntryId, EntryKind, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::source::Source;
use crate::text_epub::png_head;

const MAGIC: &[u8; 5] = b"%PDF-";
/// 読み込む PDF の大きさの上限
const MAX_PDF_BYTES: u64 = 1 << 30;
/// ページ数の上限
const MAX_PAGES: usize = 20_000;
/// 描画する横幅の目安（ピクセル）
const TARGET_WIDTH: f32 = 1600.0;
/// 描画する 1 辺の上限（ピクセル。`u16` に収める）
const MAX_EDGE: f32 = 4096.0;
const NOMINAL_SIZE: u64 = 400_000;

pub struct PdfProvider;

impl ContainerProvider for PdfProvider {
    fn probe(&self, src: &dyn Source) -> Probe {
        let mut head = [0u8; 5];
        match src.read_at(0, &mut head) {
            Ok(5) if &head == MAGIC => Probe::Match,
            _ => Probe::NoMatch,
        }
    }

    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
        Ok(Arc::new(PdfContainer::open(src.as_ref())?))
    }
}

pub struct PdfContainer {
    pdf: Pdf,
    entries: Vec<EntryMeta>,
    /// ページごとの描画寸法（ピクセル）
    dims: Vec<(u32, u32)>,
    scales: Vec<f32>,
}

/// 描画の倍率。横幅を `TARGET_WIDTH` に合わせ、長辺が `MAX_EDGE` を超えないようにする
fn scale_for(w: f32, h: f32) -> f32 {
    let s = TARGET_WIDTH / w.max(1.0);
    s.min(MAX_EDGE / w.max(h).max(1.0))
}

impl PdfContainer {
    fn open(src: &dyn Source) -> Result<Self> {
        if src.len() > MAX_PDF_BYTES {
            return Err(ContainerError::LimitExceeded("pdf size"));
        }
        let mut data = vec![0u8; src.len() as usize];
        let mut off = 0usize;
        while off < data.len() {
            let n = src.read_at(off as u64, &mut data[off..])?;
            if n == 0 {
                return Err(ContainerError::Archive("truncated pdf".into()));
            }
            off += n;
        }
        // 壊れた・悪意のある PDF でパーサが panic しても、アプリごと落とさずエラーにする
        let pdf = catch_unwind(AssertUnwindSafe(|| Pdf::new(data)))
            .map_err(|_| ContainerError::Archive("pdf: parser panic".into()))?
            .map_err(|e| ContainerError::Archive(format!("pdf: {e:?}")))?;
        let pages = pdf.pages();
        if pages.is_empty() {
            return Err(ContainerError::Unsupported);
        }
        if pages.len() > MAX_PAGES {
            return Err(ContainerError::LimitExceeded("pdf pages"));
        }
        let mut entries = Vec::with_capacity(pages.len());
        let mut dims = Vec::with_capacity(pages.len());
        let mut scales = Vec::with_capacity(pages.len());
        for (i, page) in pages.iter().enumerate() {
            let (w, h) = page.render_dimensions();
            let s = scale_for(w, h);
            dims.push((((w * s) as u32).max(1), ((h * s) as u32).max(1)));
            scales.push(s);
            entries.push(EntryMeta {
                id: EntryId(i as u32),
                path: format!("page_{:05}.png", i + 1),
                size: NOMINAL_SIZE,
                kind: EntryKind::Image,
            });
        }
        Ok(Self {
            pdf,
            entries,
            dims,
            scales,
        })
    }

    fn render(&self, id: EntryId) -> Result<Vec<u8>> {
        let i = id.0 as usize;
        let pages = self.pdf.pages();
        let page = pages.get(i).ok_or(ContainerError::NotFound(id.0))?;
        let s = self.scales[i];
        let pixmap = catch_unwind(AssertUnwindSafe(|| {
            render(
                page,
                &RenderCache::new(),
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: s,
                    y_scale: s,
                    bg_color: WHITE,
                },
            )
        }))
        .map_err(|_| ContainerError::Archive("pdf: render panic".into()))?;
        pixmap
            .into_png()
            .map_err(|e| ContainerError::Archive(e.to_string()))
    }
}

impl Container for PdfContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        Ok(Bytes::from(self.render(id)?))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(Cursor::new(self.render(id)?)))
    }

    /// 寸法の確認に、ページを描かずに答える
    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        let &(w, h) = self
            .dims
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        let mut head = png_head(w, h);
        head.truncate(n);
        Ok(Bytes::from(head))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemorySource;

    /// 空白 1 ページだけの最小の PDF
    fn minimal_pdf() -> Vec<u8> {
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] >>",
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offs {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn pages_become_png_images() {
        let src = MemorySource(Bytes::from(minimal_pdf()));
        assert_eq!(PdfProvider.probe(&src), Probe::Match);
        let c = PdfProvider.open(Arc::new(src)).unwrap();
        assert_eq!(c.entries().len(), 2);
        assert!(c.entries().iter().all(|e| e.kind == EntryKind::Image));
        let png = c.read(EntryId(0)).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let head = c.read_prefix(EntryId(1), 64).unwrap();
        assert_eq!(&head[16..20], &1600u32.to_be_bytes());
        assert_eq!(&head[20..24], &800u32.to_be_bytes());
    }

    #[test]
    fn non_pdf_is_not_matched() {
        let src = MemorySource(Bytes::from_static(b"hello world"));
        assert_eq!(PdfProvider.probe(&src), Probe::NoMatch);
    }

    /// ZIP の中に PDF だけが入っているときは、その PDF を本として開く
    #[test]
    fn pdf_inside_zip_is_opened_as_book() {
        let pdf = minimal_pdf();
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("book/doc.pdf", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, &pdf).unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let c = crate::open_source(Arc::new(MemorySource(Bytes::from(bytes)))).unwrap();
        assert_eq!(c.entries().len(), 2);
        assert_eq!(&c.read(EntryId(0)).unwrap()[1..4], b"PNG");
    }
}
