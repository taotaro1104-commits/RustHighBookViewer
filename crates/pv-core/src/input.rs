//! 操作の抽象化。入力（タップ・キー・ジェスチャー）は InputMap で Action に対応づける。

use pv_layout::Direction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorFilter {
    Night,
    Sepia,
    Invert,
    BlueLightCut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    NextPage,
    PrevPage,
    NextSpread,
    PrevSpread,
    FirstPage,
    LastPage,
    /// 見開きずれ補正
    ShiftSpread,
    ToggleMagnifier,
    ToggleAutoFlip,
    TogglePanelMode,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitHeight,
    FitPage,
    FitFill,
    OpenThumbnails,
    OpenNextVolume,
    AddBookmark,
    SavePage,
    RotateView,
    ToggleColorFilter(ColorFilter),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKey {
    /// 画面を 3x3 に分割したゾーン。0..=8 を左上から行優先で数える
    TapZone(u8),
    VolumeUp,
    VolumeDown,
    /// キー名（"ArrowLeft"、"Space"、"B" など）
    Key(String),
    Swipe(Dir),
    LongPress,
    DoubleTap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub key: InputKey,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputMap {
    pub bindings: Vec<Binding>,
    /// "標準" "片手(右)" "e-ink" など
    pub profile: String,
}

impl Default for InputMap {
    fn default() -> Self {
        Self::standard(Direction::RightToLeft)
    }
}

/// 画面座標をタップゾーン（0..=8）に変換する。範囲外は端に丸める。
pub fn tap_zone(x: f32, y: f32, width: f32, height: f32) -> u8 {
    let cell = |v: f32, len: f32| ((v / len.max(1.0)) * 3.0).floor().clamp(0.0, 2.0) as u8;
    cell(y, height) * 3 + cell(x, width)
}

impl InputMap {
    pub fn lookup(&self, key: &InputKey) -> Option<&Action> {
        // 後勝ち: ユーザーが追加した割り当てが標準を上書きできる
        self.bindings
            .iter()
            .rev()
            .find(|b| &b.key == key)
            .map(|b| &b.action)
    }

    pub fn bind(mut self, key: InputKey, action: Action) -> Self {
        self.bindings.push(Binding { key, action });
        self
    }

    /// 標準プロファイル。綴じ方向に応じて「進む」側を入れ替える。
    pub fn standard(dir: Direction) -> Self {
        use Action::*;
        let rtl = dir == Direction::RightToLeft;
        let (fwd, back) = (NextSpread, PrevSpread);
        // 右綴じは左側が「進む」
        let (left, right) = if rtl {
            (fwd.clone(), back.clone())
        } else {
            (back, fwd)
        };
        let key = |k: &str| InputKey::Key(k.to_owned());
        let mut b = Vec::new();
        for z in [0u8, 3, 6] {
            b.push(Binding {
                key: InputKey::TapZone(z),
                action: left.clone(),
            });
        }
        for z in [2u8, 5, 8] {
            b.push(Binding {
                key: InputKey::TapZone(z),
                action: right.clone(),
            });
        }
        // 中央の列（上・中・下）はどこを押してもメニューの開閉
        for z in [1u8, 4, 7] {
            b.push(Binding {
                key: InputKey::TapZone(z),
                action: OpenThumbnails,
            });
        }
        let (arrow_l, arrow_r) = (key("ArrowLeft"), key("ArrowRight"));
        b.push(Binding {
            key: arrow_l,
            action: left.clone(),
        });
        b.push(Binding {
            key: arrow_r,
            action: right.clone(),
        });
        b.push(Binding {
            key: InputKey::Swipe(Dir::Left),
            action: if rtl { NextSpread } else { PrevSpread },
        });
        b.push(Binding {
            key: InputKey::Swipe(Dir::Right),
            action: if rtl { PrevSpread } else { NextSpread },
        });
        for (k, a) in [
            ("Space", NextSpread),
            ("PageDown", NextSpread),
            ("PageUp", PrevSpread),
            ("Backspace", PrevSpread),
            ("Home", FirstPage),
            ("End", LastPage),
            ("Equals", ZoomIn),
            ("Plus", ZoomIn),
            ("Minus", ZoomOut),
            ("W", FitWidth),
            ("H", FitHeight),
            ("P", FitPage),
            ("S", ShiftSpread),
            ("B", AddBookmark),
            ("T", OpenThumbnails),
            ("N", OpenNextVolume),
            ("R", RotateView),
        ] {
            b.push(Binding {
                key: key(k),
                action: a,
            });
        }
        b.push(Binding {
            key: InputKey::VolumeDown,
            action: NextSpread,
        });
        b.push(Binding {
            key: InputKey::VolumeUp,
            action: PrevSpread,
        });
        b.push(Binding {
            key: InputKey::DoubleTap,
            action: ZoomIn,
        });
        b.push(Binding {
            key: InputKey::LongPress,
            action: ToggleMagnifier,
        });
        Self {
            bindings: b,
            profile: "標準".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtl_left_tap_goes_forward_and_ltr_is_mirrored() {
        let rtl = InputMap::standard(Direction::RightToLeft);
        for z in [1u8, 4, 7] {
            assert_eq!(
                rtl.lookup(&InputKey::TapZone(z)),
                Some(&Action::OpenThumbnails),
                "zone {z}"
            );
        }
        assert_eq!(rtl.lookup(&InputKey::TapZone(3)), Some(&Action::NextSpread));
        assert_eq!(rtl.lookup(&InputKey::TapZone(5)), Some(&Action::PrevSpread));
        let ltr = InputMap::standard(Direction::LeftToRight);
        assert_eq!(ltr.lookup(&InputKey::TapZone(3)), Some(&Action::PrevSpread));
        assert_eq!(
            ltr.lookup(&InputKey::Key("ArrowRight".into())),
            Some(&Action::NextSpread)
        );
    }

    #[test]
    fn later_binding_overrides_earlier() {
        let m = InputMap::default().bind(InputKey::TapZone(3), Action::AddBookmark);
        assert_eq!(m.lookup(&InputKey::TapZone(3)), Some(&Action::AddBookmark));
    }

    #[test]
    fn unbound_input_is_none() {
        assert_eq!(
            InputMap::default().lookup(&InputKey::Key("F13".to_owned())),
            None
        );
    }

    #[test]
    fn tap_zone_grid() {
        assert_eq!(tap_zone(0.0, 0.0, 300.0, 300.0), 0);
        assert_eq!(tap_zone(150.0, 150.0, 300.0, 300.0), 4);
        assert_eq!(tap_zone(299.0, 299.0, 300.0, 300.0), 8);
        assert_eq!(tap_zone(-5.0, 1000.0, 300.0, 300.0), 6);
    }

    #[test]
    fn toml_roundtrip() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct W {
            input: InputMap,
        }
        let w = W {
            input: InputMap::default().bind(
                InputKey::Swipe(Dir::Up),
                Action::ToggleColorFilter(ColorFilter::Night),
            ),
        };
        let s = toml::to_string(&w).unwrap();
        assert_eq!(toml::from_str::<W>(&s).unwrap(), w);
    }
}
