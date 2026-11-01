//! ページ列から表示単位（スプレッド）の列を作る純粋関数群。UI に依存しない。

use serde::{Deserialize, Serialize};

pub type PageIdx = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageMeta {
    pub width: u32,
    pub height: u32,
    /// ComicInfo.xml の DoublePage 指定など、常に単独表示するページ
    pub force_single: bool,
}

impl PageMeta {
    pub fn is_wide(&self) -> bool {
        self.width > self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    Single,
    Double,
    /// 横長ページは単独、それ以外は見開き
    Auto,
    /// 縦スクロール。各ページを単独で扱う
    Webtoon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// `Double` の left / right は画面上の位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    Single(PageIdx),
    Double { left: PageIdx, right: PageIdx },
}

impl Spread {
    /// 読む順（綴じ方向に沿った順）のページ列。
    pub fn pages(&self, dir: Direction) -> Vec<PageIdx> {
        match (*self, dir) {
            (Spread::Single(p), _) => vec![p],
            (Spread::Double { left, right }, Direction::LeftToRight) => vec![left, right],
            (Spread::Double { left, right }, Direction::RightToLeft) => vec![right, left],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutOptions {
    pub mode: ViewMode,
    pub direction: Direction,
    pub first_page_single: bool,
    pub wide_page_alone: bool,
    /// 見開きずれ補正（先頭に空ページを1枚挿入した扱い）
    pub shift: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            mode: ViewMode::Double,
            direction: Direction::RightToLeft,
            first_page_single: true,
            wide_page_alone: true,
            shift: false,
        }
    }
}

pub fn build_spreads(pages: &[PageMeta], opt: &LayoutOptions) -> Vec<Spread> {
    if matches!(opt.mode, ViewMode::Single | ViewMode::Webtoon) {
        return (0..pages.len()).map(Spread::Single).collect();
    }
    let wide_alone = opt.wide_page_alone || opt.mode == ViewMode::Auto;
    let alone = |i: usize| pages[i].force_single || (wide_alone && pages[i].is_wide());
    // ずれ補正は先頭単独の有無を反転させる（空ページ1枚分の位相ずれ）
    let first_single = opt.first_page_single != opt.shift;

    let mut out = Vec::with_capacity(pages.len() / 2 + 1);
    let mut i = 0;
    while i < pages.len() {
        let must_single = (i == 0 && first_single) || alone(i);
        if must_single || i + 1 >= pages.len() || alone(i + 1) {
            out.push(Spread::Single(i));
            i += 1;
            continue;
        }
        out.push(match opt.direction {
            Direction::LeftToRight => Spread::Double {
                left: i,
                right: i + 1,
            },
            Direction::RightToLeft => Spread::Double {
                left: i + 1,
                right: i,
            },
        });
        i += 2;
    }
    out
}

/// ページ番号が属するスプレッドの添字。
pub fn spread_of_page(spreads: &[Spread], page: PageIdx) -> Option<usize> {
    spreads.iter().position(|s| match *s {
        Spread::Single(p) => p == page,
        Spread::Double { left, right } => left == page || right == page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn page(w: u32, h: u32) -> PageMeta {
        PageMeta {
            width: w,
            height: h,
            force_single: false,
        }
    }
    fn portrait(n: usize) -> Vec<PageMeta> {
        vec![page(100, 150); n]
    }

    #[test]
    fn rtl_double_with_cover() {
        let s = build_spreads(&portrait(5), &LayoutOptions::default());
        assert_eq!(
            s,
            [
                Spread::Single(0),
                Spread::Double { left: 2, right: 1 },
                Spread::Double { left: 4, right: 3 }
            ]
        );
    }

    #[test]
    fn ltr_order() {
        let opt = LayoutOptions {
            direction: Direction::LeftToRight,
            ..Default::default()
        };
        let s = build_spreads(&portrait(3), &opt);
        assert_eq!(s[1], Spread::Double { left: 1, right: 2 });
    }

    #[test]
    fn wide_page_stands_alone() {
        let mut p = portrait(5);
        p[2] = page(300, 150);
        let opt = LayoutOptions {
            first_page_single: false,
            ..Default::default()
        };
        let s = build_spreads(&p, &opt);
        assert_eq!(
            s,
            [
                Spread::Double { left: 1, right: 0 },
                Spread::Single(2),
                Spread::Double { left: 4, right: 3 }
            ]
        );
    }

    #[test]
    fn wide_pages_pair_when_not_alone() {
        let p = vec![page(300, 150); 2];
        let opt = LayoutOptions {
            first_page_single: false,
            wide_page_alone: false,
            ..Default::default()
        };
        assert_eq!(build_spreads(&p, &opt).len(), 1);
        let auto = LayoutOptions {
            mode: ViewMode::Auto,
            ..opt
        };
        assert_eq!(build_spreads(&p, &auto).len(), 2);
    }

    #[test]
    fn force_single_page() {
        let mut p = portrait(4);
        p[1].force_single = true;
        let opt = LayoutOptions {
            first_page_single: false,
            ..Default::default()
        };
        let s = build_spreads(&p, &opt);
        assert_eq!(s[0], Spread::Single(0));
        assert_eq!(s[1], Spread::Single(1));
    }

    #[test]
    fn shift_flips_pairing_phase() {
        let base = build_spreads(&portrait(4), &LayoutOptions::default());
        let shifted = build_spreads(
            &portrait(4),
            &LayoutOptions {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(base[0], Spread::Single(0));
        assert_eq!(shifted[0], Spread::Double { left: 1, right: 0 });
    }

    #[test]
    fn single_and_webtoon_modes() {
        for mode in [ViewMode::Single, ViewMode::Webtoon] {
            let s = build_spreads(
                &portrait(3),
                &LayoutOptions {
                    mode,
                    ..Default::default()
                },
            );
            assert_eq!(s, [Spread::Single(0), Spread::Single(1), Spread::Single(2)]);
        }
    }

    #[test]
    fn empty_and_lookup() {
        assert!(build_spreads(&[], &LayoutOptions::default()).is_empty());
        let s = build_spreads(&portrait(5), &LayoutOptions::default());
        assert_eq!(spread_of_page(&s, 3), Some(2));
        assert_eq!(spread_of_page(&s, 9), None);
    }

    #[test]
    fn spread_reading_order() {
        let d = Spread::Double { left: 2, right: 1 };
        assert_eq!(d.pages(Direction::RightToLeft), [1, 2]);
        assert_eq!(d.pages(Direction::LeftToRight), [2, 1]);
    }

    fn arb_mode() -> impl Strategy<Value = ViewMode> {
        prop_oneof![
            Just(ViewMode::Single),
            Just(ViewMode::Double),
            Just(ViewMode::Auto),
            Just(ViewMode::Webtoon)
        ]
    }

    proptest! {
        #[test]
        fn every_page_appears_exactly_once_in_order(
            dims in prop::collection::vec((1u32..500, 1u32..500, any::<bool>()), 0..60),
            mode in arb_mode(),
            rtl in any::<bool>(),
            first in any::<bool>(),
            wide in any::<bool>(),
            shift in any::<bool>(),
        ) {
            let pages: Vec<_> = dims.iter()
                .map(|&(w, h, f)| PageMeta { width: w, height: h, force_single: f })
                .collect();
            let dir = if rtl { Direction::RightToLeft } else { Direction::LeftToRight };
            let opt = LayoutOptions {
                mode, direction: dir, first_page_single: first, wide_page_alone: wide, shift,
            };
            let flat: Vec<_> = build_spreads(&pages, &opt)
                .iter().flat_map(|s| s.pages(dir)).collect();
            prop_assert_eq!(flat, (0..pages.len()).collect::<Vec<_>>());
        }
    }
}
