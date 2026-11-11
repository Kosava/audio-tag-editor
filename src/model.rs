//! Model podataka za tagove jedne pesme (bez UI-ja).

use std::path::PathBuf;

/// Tekstualna polja, redom kao u formi.
pub const TEXT_FIELDS: [&str; 7] =
    ["title", "artist", "album", "albumartist", "year", "genre", "comment"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub albumartist: String,
    pub year: String,
    pub genre: String,
    pub track: u32,
    pub track_total: u32,
    pub comment: String,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub path: PathBuf,
    pub fields: Fields,
    /// Greška pri čitanju — takav fajl se ne menja i ne upisuje.
    pub error: String,
    pub dirty: bool,
}

impl Track {
    pub fn new(path: PathBuf) -> Self {
        Self { path, fields: Fields::default(), error: String::new(), dirty: false }
    }

    pub fn filename(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn format(&self) -> String {
        self.path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default()
    }

    /// Postavi jedno polje (tekst se trimuje); vraća true ako se nešto promenilo.
    pub fn set(&mut self, field: &str, value: &str) -> bool {
        if !self.error.is_empty() {
            return false;
        }
        let v = value.trim();
        let f = &mut self.fields;
        let changed = match field {
            "track" => replace(&mut f.track, parse_track(v).0),
            "track_total" => replace(&mut f.track_total, parse_track(v).0),
            _ => match text_field(f, field) {
                Some(slot) if slot != v => {
                    *slot = v.to_string();
                    true
                }
                _ => false,
            },
        };
        self.dirty |= changed;
        changed
    }
}

fn replace(slot: &mut u32, v: u32) -> bool {
    let changed = *slot != v;
    *slot = v;
    changed
}

impl Fields {
    pub fn get(&self, name: &str) -> &str {
        match name {
            "title" => &self.title,
            "artist" => &self.artist,
            "album" => &self.album,
            "albumartist" => &self.albumartist,
            "year" => &self.year,
            "genre" => &self.genre,
            "comment" => &self.comment,
            _ => "",
        }
    }
}

pub fn text_field<'a>(f: &'a mut Fields, name: &str) -> Option<&'a mut String> {
    Some(match name {
        "title" => &mut f.title,
        "artist" => &mut f.artist,
        "album" => &mut f.album,
        "albumartist" => &mut f.albumartist,
        "year" => &mut f.year,
        "genre" => &mut f.genre,
        "comment" => &mut f.comment,
        _ => return None,
    })
}

/// "3/12" -> (3, 12), "07" -> (7, 0), "4a" -> (4, 0), "" -> (0, 0)
pub fn parse_track(s: &str) -> (u32, u32) {
    let lead = |t: &str| {
        let d: String = t.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
        d.parse().unwrap_or(0)
    };
    match s.split_once('/') {
        Some((n, t)) => (lead(n), lead(t)),
        None => (lead(s), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_track_variants() {
        assert_eq!(parse_track("3/12"), (3, 12));
        assert_eq!(parse_track("07"), (7, 0));
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("4a"), (4, 0));
        assert_eq!(parse_track("x"), (0, 0));
    }

    #[test]
    fn set_marks_dirty_only_on_change() {
        let mut t = Track::new("song.mp3".into());
        t.fields.title = "A".into();
        assert!(!t.set("title", " A "));
        assert!(!t.dirty);
        assert!(t.set("title", "B"));
        assert!(t.set("track", "3/9"));
        assert!(!t.set("unknown", "x"));
        assert!(t.dirty);
        assert_eq!((t.fields.title.as_str(), t.fields.track), ("B", 3));
    }

    #[test]
    fn broken_track_is_locked() {
        let mut t = Track::new("bad.flac".into());
        t.error = "loš fajl".into();
        assert!(!t.set("title", "X"));
        assert!(!t.dirty);
    }

    #[test]
    fn format_and_filename() {
        let t = Track::new("/muzika/Pesma.FLAC".into());
        assert_eq!(t.format(), "FLAC");
        assert_eq!(t.filename(), "Pesma.FLAC");
    }
}
