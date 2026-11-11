//! Mala podešavanja (tema, poslednji folder) u običnom key=value fajlu.
//! Linux: ~/.config/audio-tag-editor/settings.conf, Windows: %APPDATA%\audio-tag-editor\settings.conf

use std::path::PathBuf;

#[derive(Default)]
pub struct Settings {
    pub dark: bool,
    pub last_dir: Option<PathBuf>,
}

fn file() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("audio-tag-editor").join("settings.conf"))
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Self::default();
        let text = file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("dark", v)) => s.dark = v == "1",
                Some(("last_dir", v)) if !v.is_empty() => s.last_dir = Some(v.into()),
                _ => {}
            }
        }
        s
    }

    /// Greška pri čuvanju podešavanja nije bitna za rad — ignoriše se.
    pub fn save(&self) {
        let Some(f) = file() else { return };
        let dir = self.last_dir.as_deref().map(|d| d.to_string_lossy()).unwrap_or_default();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, format!("dark={}\nlast_dir={dir}\n", self.dark as u8));
    }
}
