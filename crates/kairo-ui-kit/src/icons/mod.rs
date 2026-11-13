//! The operator interface's icon set.
//!
//! `src/renderer/src/icons/index.tsx` maps names to Hugeicons (free, Stroke
//! Rounded, MIT) and seven Phosphor glyphs (MIT), plus a hand-drawn Bible.
//! Only the icons the renderer imports are bundled (`tools/generate-icons.cjs`
//! writes `assets/icons` and `generated.rs`). Licences are in
//! `assets/licenses`.

mod generated;

pub use generated::KairoIcon;

use gpui_kit::component::IconNamed;
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

const PREFIX: &str = "kairo/icons/";
const FILL_SUFFIX: &str = "-fill.svg";

/// An icon at a weight. Hugeicons' `fill` and `bold` draw a 2px stroke;
/// Phosphor and the Bible have true filled shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WeightedIcon {
    icon: KairoIcon,
    fill: bool,
}

impl KairoIcon {
    /// The filled (or bold) weight of this icon.
    pub fn fill(self) -> WeightedIcon {
        WeightedIcon {
            icon: self,
            fill: true,
        }
    }

    /// This icon at the given weight.
    pub fn weight(self, fill: bool) -> WeightedIcon {
        WeightedIcon { icon: self, fill }
    }

    fn asset_path(self, fill: bool) -> String {
        let suffix = if fill { FILL_SUFFIX } else { ".svg" };
        format!("{PREFIX}{}{suffix}", self.file_stem())
    }

    fn from_file_stem(stem: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|icon| icon.file_stem() == stem)
    }
}

impl IconNamed for KairoIcon {
    fn path(self) -> SharedString {
        self.asset_path(false).into()
    }
}

impl IconNamed for WeightedIcon {
    fn path(self) -> SharedString {
        self.icon.asset_path(self.fill).into()
    }
}

/// Serves the Kairo icons and falls back to GPUI Kit's own assets (the
/// Lucide icons Kit components draw inside their controls).
pub struct KairoAssets;

impl KairoAssets {
    /// Looks up a Kairo icon by asset path.
    pub fn kairo_icon(path: &str) -> Option<&'static [u8]> {
        let name = path.strip_prefix(PREFIX)?;
        let (stem, fill) = match name.strip_suffix(FILL_SUFFIX) {
            Some(stem) => (stem, true),
            None => (name.strip_suffix(".svg")?, false),
        };
        KairoIcon::from_file_stem(stem).map(|icon| icon.svg(fill))
    }
}

impl AssetSource for KairoAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.starts_with(PREFIX) {
            return Ok(Self::kairo_icon(path).map(Cow::Borrowed));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path.starts_with(PREFIX) || PREFIX.starts_with(path) {
            let mut names: Vec<SharedString> = KairoIcon::ALL
                .iter()
                .flat_map(|icon| [icon.asset_path(false), icon.asset_path(true)])
                .filter(|name| name.starts_with(path))
                .map(SharedString::from)
                .collect();
            if !path.starts_with(PREFIX) {
                names.extend(gpui_kit::assets::Assets.list(path)?);
            }
            return Ok(names);
        }
        gpui_kit::assets::Assets.list(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_loads_in_both_weights() {
        for icon in KairoIcon::ALL {
            for fill in [false, true] {
                let path = icon.asset_path(fill);
                let bytes = KairoAssets.load(&path).unwrap().unwrap();
                let text = std::str::from_utf8(&bytes).unwrap();
                assert!(text.starts_with("<svg"), "{path}");
                assert!(text.contains("viewBox"), "{path}");
            }
        }
    }

    #[test]
    fn unknown_kairo_paths_are_missing_not_errors() {
        assert!(KairoAssets.load("kairo/icons/nope.svg").unwrap().is_none());
        assert!(KairoAssets.load("kairo/icons/").unwrap().is_none());
    }

    #[test]
    fn kit_icons_still_load() {
        assert!(KairoAssets.load("icons/check.svg").unwrap().is_some());
    }

    #[test]
    fn names_are_unique() {
        let mut stems: Vec<_> = KairoIcon::ALL.iter().map(|i| i.file_stem()).collect();
        stems.sort();
        stems.dedup();
        assert_eq!(stems.len(), KairoIcon::ALL.len());
    }
}
