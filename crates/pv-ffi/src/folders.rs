//! 本棚の「フォルダ」表示。登録済みの本の場所（`file://` のパス）を階層に見立てて、
//! 指定したフォルダの直下にあるサブフォルダと本、または配下すべての本を返す。
//! SAF（`content://`）で開いた本は場所が分からないので、ルートでだけ「直下の本」として扱う。

use std::collections::BTreeMap;

/// 1 つのサブフォルダ（1 本道の入れ子は、まとめて 1 つにして見せる）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 指定したフォルダからの相対名（例: `Download/sample`）
    pub name: String,
    /// 絶対パス（`/` 区切り）
    pub path: String,
    /// 配下の本の数
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct View {
    pub folders: Vec<Entry>,
    /// 表示する本（渡した `uris` の添字）
    pub shown: Vec<usize>,
}

/// `file://` を外し、`/` 区切りにそろえる。場所が分からない（`content://`）ときは None。
fn path_of(uri: &str) -> Option<String> {
    if uri.starts_with("content://") {
        return None;
    }
    let p = uri.strip_prefix("file://").unwrap_or(uri);
    Some(p.replace('\\', "/").trim_end_matches('/').to_owned())
}

/// `prefix` の配下での相対パス（`prefix` 自身・外は None）
fn rel_under<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        return Some(path.trim_start_matches('/'));
    }
    path.strip_prefix(prefix)?.strip_prefix('/')
}

pub fn view(uris: &[String], prefix: &str, recursive: bool) -> View {
    let prefix = prefix.trim_end_matches('/');
    let paths: Vec<Option<String>> = uris.iter().map(|u| path_of(u)).collect();

    let mut shown = Vec::new();
    let mut firsts: BTreeMap<String, usize> = BTreeMap::new();
    for (i, p) in paths.iter().enumerate() {
        match p {
            None => {
                if prefix.is_empty() {
                    shown.push(i); // 場所が分からない本は、ルートの直下に置く
                }
            }
            Some(p) => {
                let Some(rel) = rel_under(p, prefix) else {
                    continue;
                };
                match rel.split_once('/') {
                    Some((first, _)) => {
                        *firsts.entry(first.to_owned()).or_default() += 1;
                        if recursive {
                            shown.push(i);
                        }
                    }
                    None => shown.push(i),
                }
            }
        }
    }

    let folders = firsts
        .into_iter()
        .map(|(first, count)| {
            let mut path = if prefix.is_empty() {
                format!("/{first}")
            } else {
                format!("{prefix}/{first}")
            };
            // 直下に本が無く、サブフォルダが 1 つだけなら、そのまま奥へ進めて 1 つにまとめる
            loop {
                let mut direct = false;
                let mut children: Vec<&str> = Vec::new();
                for p in paths.iter().flatten() {
                    let Some(rel) = rel_under(p, &path) else {
                        continue;
                    };
                    match rel.split_once('/') {
                        Some((c, _)) => {
                            if !children.contains(&c) {
                                children.push(c);
                            }
                        }
                        None => direct = true,
                    }
                }
                if direct || children.len() != 1 {
                    break;
                }
                path = format!("{path}/{}", children[0]);
            }
            let name = rel_under(&path, prefix).unwrap_or(&path).to_owned();
            Entry { name, path, count }
        })
        .collect();
    View { folders, shown }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn lib() -> Vec<String> {
        uris(&[
            "file:///sd/Download/manga/A/v1.cbz",
            "file:///sd/Download/manga/A/v2.cbz",
            "file:///sd/Download/manga/B",
            "file:///sd/Download/novels/n1.epub",
            "file:///sd/Books/x.cbz",
            "content://tree/abc",
        ])
    }

    #[test]
    fn root_collapses_single_child_chains() {
        let v = view(&lib(), "", false);
        let names: Vec<_> = v
            .folders
            .iter()
            .map(|e| (e.name.as_str(), e.count))
            .collect();
        // 「/sd」の下には Books と Download の 2 つがあるので、そこで止まる
        assert_eq!(names, [("sd", 5)]);
        assert_eq!(v.shown, [5], "場所が分からない本はルート直下");
    }

    #[test]
    fn folder_lists_subfolders_and_direct_books() {
        let v = view(&lib(), "/sd", false);
        let names: Vec<_> = v
            .folders
            .iter()
            .map(|e| (e.name.as_str(), e.count))
            .collect();
        assert_eq!(names, [("Books", 1), ("Download", 4)]);
        assert!(v.shown.is_empty());

        let v = view(&lib(), "/sd/Download", false);
        let names: Vec<_> = v
            .folders
            .iter()
            .map(|e| (e.name.as_str(), e.path.as_str(), e.count))
            .collect();
        assert_eq!(
            names,
            [
                ("manga", "/sd/Download/manga", 3),
                ("novels", "/sd/Download/novels", 1)
            ]
        );

        // 画像フォルダの本（manga/B）は、manga の直下の本
        let v = view(&lib(), "/sd/Download/manga", false);
        assert_eq!(v.shown, [2]);
        assert_eq!(v.folders.len(), 1);
        assert_eq!(v.folders[0].name, "A");
    }

    #[test]
    fn recursive_shows_everything_under_the_folder() {
        let v = view(&lib(), "/sd/Download/manga", true);
        assert_eq!(v.shown, [0, 1, 2]);
        let v = view(&lib(), "/sd/Download", true);
        assert_eq!(v.shown, [0, 1, 2, 3]);
        // 他のフォルダの本や、似た名前のフォルダは含めない
        let v = view(
            &uris(&["file:///a/b/x.cbz", "file:///a/bb/y.cbz"]),
            "/a/b",
            true,
        );
        assert_eq!(v.shown, [0]);
    }

    #[test]
    fn nested_single_chain_is_shown_as_one_entry() {
        let v = view(
            &uris(&["file:///a/b/c/d/x.cbz", "file:///a/b/c/d/y.cbz"]),
            "/a",
            false,
        );
        assert_eq!(
            v.folders,
            vec![Entry {
                name: "b/c/d".into(),
                path: "/a/b/c/d".into(),
                count: 2
            }]
        );
    }

    #[test]
    fn windows_paths_and_trailing_slashes_are_normalized() {
        let v = view(
            &uris(&["C:\\Books\\x\\a.cbz", "C:\\Books\\y.cbz"]),
            "C:/Books/",
            false,
        );
        assert_eq!(v.shown, [1]);
        assert_eq!(v.folders[0].name, "x");
    }
}
