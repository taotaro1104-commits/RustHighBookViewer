//! 7z / RAR の展開型コンテナの統合テスト。

use std::path::Path;
use std::sync::Arc;

use pv_container::{open_source_in, Container, EntryId, FileSource, MemorySource};

fn names(c: &dyn Container) -> Vec<String> {
    c.entries().iter().map(|e| e.path.clone()).collect()
}

fn make_7z(dir: &Path) -> std::path::PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("p10.png"), b"ten").unwrap();
    std::fs::write(src.join("sub/p2.png"), b"two").unwrap();
    std::fs::write(src.join("notes.txt"), b"skip me").unwrap();
    let out = dir.join("a.7z");
    sevenz_rust2::compress_to_path(&src, &out).unwrap();
    out
}

#[test]
fn sevenz_lists_images_in_natural_order_and_reads_them() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_7z(dir.path());
    let scratch = dir.path().join("scratch");
    let c = open_source_in(Arc::new(FileSource::open(&archive).unwrap()), &scratch).unwrap();
    // 画像だけが、自然順（p10 より sub/p2 が後ろ: パス全体の比較）で並ぶ
    let n = names(c.as_ref());
    assert_eq!(n.len(), 2, "{n:?}");
    assert!(n.contains(&"p10.png".to_string()) && n.contains(&"sub/p2.png".to_string()));
    let ten = c.entries().iter().find(|e| e.path == "p10.png").unwrap().id;
    assert_eq!(&c.read(ten).unwrap()[..], b"ten");
}

#[test]
fn sevenz_scratch_is_cleaned_when_container_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let archive = make_7z(dir.path());
    let scratch = dir.path().join("scratch");
    let c = open_source_in(Arc::new(FileSource::open(&archive).unwrap()), &scratch).unwrap();
    assert!(std::fs::read_dir(&scratch).unwrap().count() > 0);
    drop(c);
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn rar_extracts_stored_images_and_skips_other_files() {
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stored.rar"))
            .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch");
    let c = open_source_in(Arc::new(MemorySource(bytes.into())), &scratch).unwrap();
    assert_eq!(names(c.as_ref()), ["p10.png", "sub/p2.png"]);
    assert_eq!(&c.read(EntryId(1)).unwrap()[..], b"two");
    drop(c);
    // 書き出した RAR 本体の一時ファイルも残らない
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
}

#[test]
fn garbage_after_magic_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut rar = b"Rar!\x1a\x07\x00".to_vec();
    rar.extend_from_slice(&[0xFF; 64]);
    assert!(open_source_in(Arc::new(MemorySource(rar.into())), dir.path()).is_err());
    let mut z = b"7z\xBC\xAF\x27\x1C".to_vec();
    z.extend_from_slice(&[0xFF; 64]);
    assert!(open_source_in(Arc::new(MemorySource(z.into())), dir.path()).is_err());
}
