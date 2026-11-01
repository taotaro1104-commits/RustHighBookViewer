use std::path::Path;

use pv_container::TextStyle;
use pv_layout::{Direction, LayoutOptions, ViewMode};
use pv_process::ColorAdjust;
use pv_render::FitMode;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::input::InputMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheSettings {
    /// None なら端末メモリの 1/8
    pub l1_bytes: Option<u64>,
    pub l2_bytes: u64,
    pub l3_bytes: u64,
    /// 先読み窓（現在位置から見て前・後ろのスプレッド数）
    pub prefetch_back: usize,
    pub prefetch_forward: usize,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            l1_bytes: None,
            l2_bytes: 64 << 20,
            l3_bytes: 512 << 20,
            prefetch_back: 2,
            prefetch_forward: 4,
        }
    }
}

impl CacheSettings {
    pub fn l1_budget(&self) -> u64 {
        self.l1_bytes.unwrap_or_else(|| {
            let total = sysinfo::System::new()
                .map(|mut sys| {
                    sys.refresh_memory();
                    sys.total_memory()
                })
                .unwrap_or(0);
            match total {
                0 => 256 << 20,
                total => total / 8,
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub layout: LayoutOptions,
    pub fit: FitMode,
    pub color: ColorAdjust,
    pub cache: CacheSettings,
    pub input: InputMap,
    /// 文章の EPUB の文字の大きさ・書体
    pub text: TextStyle,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            layout: LayoutOptions::default(),
            fit: FitMode::Page,
            color: ColorAdjust::default(),
            cache: CacheSettings::default(),
            input: InputMap::default(),
            text: TextStyle::default(),
        }
    }
}

/// 書庫ごとの上書き。未指定の項目はグローバル設定を使う。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BookOverride {
    pub mode: Option<ViewMode>,
    pub direction: Option<Direction>,
    pub first_page_single: Option<bool>,
    pub shift: Option<bool>,
    pub fit: Option<FitMode>,
    pub color: Option<ColorAdjust>,
}

impl Settings {
    /// グローバル設定に書庫ごとの上書きを重ねた設定を返す。
    pub fn merged(&self, o: &BookOverride) -> Settings {
        let mut s = self.clone();
        s.layout.mode = o.mode.unwrap_or(s.layout.mode);
        s.layout.direction = o.direction.unwrap_or(s.layout.direction);
        s.layout.first_page_single = o.first_page_single.unwrap_or(s.layout.first_page_single);
        s.layout.shift = o.shift.unwrap_or(s.layout.shift);
        s.fit = o.fit.unwrap_or(s.fit);
        s.color = o.color.unwrap_or(s.color);
        s
    }

    /// ファイルがなければ既定値。新しい版のファイルは読まずにエラーにする。
    pub fn load(path: &Path) -> Result<Settings> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e.into()),
        };
        let mut s: Settings =
            toml::from_str(&text).map_err(|e| CoreError::Settings(e.to_string()))?;
        if s.schema_version > SCHEMA_VERSION {
            return Err(CoreError::Settings(format!(
                "settings schema {} is newer than supported {}",
                s.schema_version, SCHEMA_VERSION
            )));
        }
        // 将来のマイグレーションはここで版ごとに段階適用する
        s.schema_version = SCHEMA_VERSION;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| CoreError::Settings(e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // 書き込み途中の破損を避けるため一時ファイル経由で置換する
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Settings::load(&dir.path().join("none.toml")).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/settings.toml");
        let mut s = Settings::default();
        s.layout.direction = Direction::LeftToRight;
        s.cache.l1_bytes = Some(1234);
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), s);
    }

    #[test]
    fn old_partial_file_keeps_defaults_for_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.toml");
        std::fs::write(&p, "schema_version = 1\n[cache]\nl2_bytes = 5\n").unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.cache.l2_bytes, 5);
        assert_eq!(s.cache.prefetch_forward, 4);
        assert_eq!(s.input, InputMap::default());
    }

    #[test]
    fn newer_schema_and_garbage_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.toml");
        std::fs::write(&p, "schema_version = 99\n").unwrap();
        assert!(Settings::load(&p).is_err());
        std::fs::write(&p, "not = [valid").unwrap();
        assert!(Settings::load(&p).is_err());
    }

    #[test]
    fn override_wins_only_where_set() {
        let g = Settings::default();
        let o = BookOverride {
            direction: Some(Direction::LeftToRight),
            shift: Some(true),
            ..Default::default()
        };
        let m = g.merged(&o);
        assert_eq!(m.layout.direction, Direction::LeftToRight);
        assert!(m.layout.shift);
        assert_eq!(m.layout.mode, g.layout.mode);
        assert_eq!(m.fit, g.fit);
    }

    #[test]
    fn l1_budget_auto_is_nonzero() {
        assert!(CacheSettings::default().l1_budget() > 0);
        assert_eq!(
            CacheSettings {
                l1_bytes: Some(7),
                ..Default::default()
            }
            .l1_budget(),
            7
        );
    }
}
