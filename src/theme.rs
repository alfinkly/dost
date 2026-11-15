//! Colours used by widgets and scenes; overridable from `.dost/theme.yml`.

use std::str::FromStr;

use ratatui::style::Color;

use crate::catalog::Theme as CatalogTheme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub ok: Color,
    pub fail: Color,
    pub running: Color,
    pub pending: Color,
    pub accent: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            ok: Color::Green,
            fail: Color::Red,
            running: Color::Yellow,
            pending: Color::DarkGray,
            accent: Color::Cyan,
        }
    }
}

impl Theme {
    /// Colours from the catalog theme; unknown names keep the default.
    pub fn from_catalog(c: &CatalogTheme) -> Theme {
        let mut t = Theme::default();
        let pick = |name: &str, slot: &mut Color| {
            if let Some(v) = c.colors.get(name) {
                if let Ok(col) = Color::from_str(v) {
                    *slot = col;
                }
            }
        };
        pick("ok", &mut t.ok);
        pick("fail", &mut t.fail);
        pick("running", &mut t.running);
        pick("pending", &mut t.pending);
        pick("accent", &mut t.accent);
        t
    }
}
