//! Čitanje i upis tagova preko `lofty` (ID3v2, Vorbis comments, MP4 ilst).

use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::mpeg::MpegFile;
use lofty::prelude::*;
use lofty::tag::{ItemValue, Tag, TagItem};

use crate::model::{text_field, Fields, Track, TEXT_FIELDS};

pub const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4"];

/// Isti redosled kao `TEXT_FIELDS`.
const TEXT_KEYS: [ItemKey; 7] = [
    ItemKey::TrackTitle,
    ItemKey::TrackArtist,
    ItemKey::AlbumTitle,
    ItemKey::AlbumArtist,
    ItemKey::RecordingDate,
    ItemKey::Genre,
    ItemKey::Comment,
];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn read(path: &Path) -> Track {
    let mut t = Track::new(path.to_path_buf());
    match lofty::read_from_path(path) {
        Ok(file) => {
            if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
                t.fields = fields_from(tag);
            }
        }
        Err(e) => t.error = e.to_string(),
    }
    t
}

fn fields_from(tag: &Tag) -> Fields {
    let mut f = Fields::default();
    for (name, key) in TEXT_FIELDS.iter().zip(TEXT_KEYS) {
        if let (Some(v), Some(slot)) = (get(tag, key), text_field(&mut f, name)) {
            *slot = v.to_string();
        }
    }
    f.track = tag.track().unwrap_or(0);
    f.track_total = tag.track_total().unwrap_or(0);
    f
}

/// Upisuje tagove na disk. Ostala polja (omoti, ReplayGain, iTunNORM…) ostaju netaknuta.
pub fn write(t: &mut Track) -> Result<(), String> {
    let name = t.filename();
    if !t.error.is_empty() {
        return Err(format!("{name}: fajl nije ispravno učitan, preskačem ({})", t.error));
    }
    let is_mp3 = t.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    let res = if is_mp3 { write_mp3(t) } else { write_generic(t) };
    res.map_err(|e| format!("{name}: {e}"))?;
    t.dirty = false;
    Ok(())
}

// ID3v2.3 kao u staroj verziji — najbolja kompatibilnost (Windows Explorer, auto-radija)
fn options() -> WriteOptions {
    WriteOptions::default().use_id3v23(true)
}

fn write_generic(t: &Track) -> Result<(), String> {
    let mut file = lofty::read_from_path(&t.path).map_err(|e| e.to_string())?;
    let tag_type = file.primary_tag_type();
    if file.tag(tag_type).is_none() {
        file.insert_tag(Tag::new(tag_type));
    }
    let tag = file.tag_mut(tag_type).expect("tag je upravo ubačen");
    apply(tag, &t.fields);
    tag.save_to_path(&t.path, options()).map_err(|e| e.to_string())
}

/// MP3 ide preko split/merge: generička konverzija bi obrisala okvire koje
/// `Tag` ne poznaje (npr. COMM:iTunNORM).
fn write_mp3(t: &Track) -> Result<(), String> {
    let mut f = std::fs::File::open(&t.path).map_err(|e| e.to_string())?;
    let mut mp3 = MpegFile::read_from(&mut f, ParseOptions::new()).map_err(|e| e.to_string())?;
    drop(f);
    let id3 = mp3.id3v2_mut().map(std::mem::take).unwrap_or_default();
    let (rest, mut tag) = id3.split_tag();
    apply(&mut tag, &t.fields);
    rest.merge_tag(tag).save_to_path(&t.path, options()).map_err(|e| e.to_string())
}

/// Komentar sa opisom (iTunNORM, iTunSMPB…) nije korisnikov komentar — njega ne diramo.
fn is_main(item: &TagItem, key: ItemKey) -> bool {
    item.key() == key && (key != ItemKey::Comment || item.description().is_empty())
}

fn get(tag: &Tag, key: ItemKey) -> Option<&str> {
    tag.items().find(|i| is_main(i, key)).and_then(|i| i.value().text())
}

fn apply(tag: &mut Tag, f: &Fields) {
    for (name, key) in TEXT_FIELDS.iter().zip(TEXT_KEYS) {
        tag.retain(|i| !is_main(i, key));
        let v = f.get(name);
        if !v.is_empty() {
            let mut item = TagItem::new(key, ItemValue::Text(v.to_string()));
            item.set_lang(*b"eng");
            tag.push(item);
        }
    }
    match f.track {
        0 => tag.remove_track(),
        n => tag.set_track(n),
    }
    match (f.track, f.track_total) {
        (0, _) | (_, 0) => tag.remove_track_total(),
        (_, n) => tag.set_track_total(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Svaki test radi na svojoj kopiji (testovi idu paralelno).
    fn copy_fixture(test: &str, name: &str) -> PathBuf {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        let dir = std::env::temp_dir().join(format!("ate-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join(name);
        std::fs::copy(src, &dst).unwrap();
        dst
    }

    /// Izmena kroz editor ne sme da obriše omot, ReplayGain ni iTunNORM.
    #[test]
    fn roundtrip_preserves_other_data() {
        for name in ["tagged.mp3", "tagged.flac", "tagged.opus", "tagged.m4a"] {
            let path = copy_fixture("roundtrip", name);
            let mut t = read(&path);
            assert_eq!(t.error, "", "{name}");
            assert_eq!((t.fields.title.as_str(), t.fields.track, t.fields.track_total), ("stari", 3, 12), "{name}");

            t.set("title", "Đurđevdan");
            t.set("artist", "Bijelo Dugme");
            t.set("year", "1988");
            t.set("comment", "novi");
            t.set("track", "5");
            write(&mut t).unwrap();
            assert!(!t.dirty);

            let back = read(&path);
            assert_eq!(back.fields.title, "Đurđevdan", "{name}");
            assert_eq!(back.fields.artist, "Bijelo Dugme", "{name}");
            assert_eq!(back.fields.year, "1988", "{name}");
            assert_eq!(back.fields.comment, "novi", "{name}");
            assert_eq!((back.fields.track, back.fields.track_total), (5, 12), "{name}");

            let file = lofty::read_from_path(&path).unwrap();
            let tag = file.primary_tag().unwrap();
            assert_eq!(tag.pictures().len(), 1, "{name}: omot");
            if name.ends_with("mp3") {
                assert!(
                    tag.items().any(|i| i.key() == ItemKey::Comment && i.description() == "iTunNORM"),
                    "mp3: iTunNORM"
                );
            }
            let rg = tag.items().any(|i| format!("{:?}", i.key()).contains("ReplayGain")
                || i.description().eq_ignore_ascii_case("replaygain_track_gain"));
            assert!(rg || name.ends_with("m4a"), "{name}: ReplayGain");
        }
    }

    #[test]
    fn clearing_field_removes_it() {
        let path = copy_fixture("clear", "tagged.flac");
        let mut t = read(&path);
        t.set("title", "");
        t.set("track", "0");
        write(&mut t).unwrap();
        let back = read(&path);
        assert_eq!((back.fields.title.as_str(), back.fields.track, back.fields.track_total), ("", 0, 0));
    }

    #[test]
    fn broken_file_is_not_written() {
        let path = copy_fixture("broken", "broken.flac");
        let mut t = read(&path);
        assert!(!t.error.is_empty());
        assert!(write(&mut t).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not audio");
    }
}
