//! Pretraga MusicBrainz baze (JSON web servis).

use std::time::Duration;

use serde_json::Value;

use crate::model::Fields;

const URL: &str = "https://musicbrainz.org/ws/2/recording";
// MusicBrainz traži ime aplikacije i kontakt URL u User-Agent-u.
const USER_AGENT: &str = concat!(
    "AudioTagEditor/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/alen/audio-tag-editor )"
);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Match {
    pub fields: Fields, // comment je uvek prazan
    pub score: i32,
    pub mbid: String,
}

impl Match {
    pub fn label(&self) -> String {
        let f = &self.fields;
        let or_q = |s: &str| if s.is_empty() { "?".to_string() } else { s.to_string() };
        format!(
            "[{}%]  {} — {}   (Album: {}, {})",
            self.score, f.artist, f.title, or_q(&f.album), or_q(&f.year)
        )
    }
}

pub fn search(title: &str, artist: &str) -> Result<Vec<Match>, String> {
    let mut query = format!("recording:{}", quote(title));
    if !artist.is_empty() {
        query += &format!(" AND artist:{}", quote(artist));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let body = agent
        .get(URL)
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", "15")
        .header("User-Agent", USER_AGENT)
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| format!("MusicBrainz greška: {e}"))?;
    let json: Value = serde_json::from_str(&body).map_err(|e| format!("Neispravan odgovor: {e}"))?;
    Ok(parse(&json))
}

/// Lucene fraza: navodnici + escape, da "AC/DC" ili "Hey!" ne pokvare upit.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn artist_credit(v: &Value) -> String {
    v.get("artist-credit")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|c| format!("{}{}", s(c, "name"), s(c, "joinphrase"))).collect())
        .unwrap_or_default()
}

/// Najpre zvanično, album, bez sekundarnog tipa (kompilacija…), najstarije.
fn release_rank(r: &Value) -> (bool, bool, bool, String) {
    let rg = &r["release-group"];
    let secondary = rg["secondary-types"].as_array().is_some_and(|a| !a.is_empty());
    let date = match s(r, "date") {
        "" => "9999".to_string(),
        d => d.to_string(),
    };
    (s(r, "status") != "Official", s(rg, "primary-type") != "Album", secondary, date)
}

pub fn parse(json: &Value) -> Vec<Match> {
    let empty = vec![];
    let recs = json["recordings"].as_array().unwrap_or(&empty);
    let mut out: Vec<Match> = recs.iter().map(parse_recording).collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.score));
    out
}

fn parse_recording(rec: &Value) -> Match {
    let artist = artist_credit(rec);
    let mut f = Fields {
        title: s(rec, "title").to_string(),
        albumartist: artist.clone(),
        artist,
        ..Default::default()
    };

    if let Some(rel) = rec["releases"].as_array().and_then(|r| r.iter().min_by_key(|r| release_rank(r))) {
        f.album = s(rel, "title").to_string();
        let aa = artist_credit(rel);
        if !aa.is_empty() {
            f.albumartist = aa;
        }
        f.year = s(rel, "date").chars().take(4).collect();
        if let Some(medium) = rel["media"].get(0) {
            f.track = medium["track"][0]["number"].as_str().and_then(|n| n.parse().ok()).unwrap_or(0);
            f.track_total = medium["track-count"].as_u64().unwrap_or(0) as u32;
        }
    }

    if let Some(tag) = rec["tags"].as_array().and_then(|t| t.iter().max_by_key(|t| t["count"].as_i64())) {
        f.genre = title_case(s(tag, "name"));
    }

    Match { fields: f, score: rec["score"].as_i64().unwrap_or(0) as i32, mbid: s(rec, "id").to_string() }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_original_album() {
        let json: Value = serde_json::from_str(include_str!("../tests/fixtures/mb_response.json")).unwrap();
        let m = &parse(&json)[0];
        let f = &m.fields;
        assert_eq!(m.score, 97);
        assert_eq!(f.artist, "Riblja Čorba");
        assert_eq!(f.album, "Kost u grlu");
        assert_eq!(f.year, "1979");
        assert_eq!((f.track, f.track_total), (2, 9));
        assert_eq!(f.genre, "Rock");
        assert!(f.comment.is_empty());
    }

    #[test]
    fn quote_escapes() {
        assert_eq!(quote(r#"Say "Hi""#), r#""Say \"Hi\"""#);
    }
}
