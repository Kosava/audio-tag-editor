// Bez konzolnog prozora na Windows-u
#![cfg_attr(windows, windows_subsystem = "windows")]

mod model;
mod musicbrainz;
mod player;
mod settings;
mod tags;

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, VecModel};

use model::{Track, TEXT_FIELDS};
use settings::Settings;

slint::include_modules!();

const APP_NAME: &str = "Audio Tag Editor";
const LOAD_BATCH: usize = 25;

thread_local! {
    // Stanje živi samo na UI niti; pozadinske niti mu prilaze preko invoke_from_event_loop.
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// None ako je stanje zauzeto: na Windows-u sistemski dijalog vrti petlju poruka
/// dok je `App` pozajmljen, pa tajmer/klik može da stigne "iznutra".
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok()?.as_mut().map(f))
}

/// Pošalji posao UI niti iz pozadinske niti.
fn post(f: impl FnOnce(&mut App) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || run_or_retry(f));
}

/// Rezultat iz pozadine se ne sme izgubiti — ako je stanje zauzeto, probaj malo kasnije.
fn run_or_retry(f: impl FnOnce(&mut App) + 'static) {
    APP.with(|a| match a.try_borrow_mut() {
        Ok(mut app) => {
            if let Some(app) = app.as_mut() {
                f(app);
            }
        }
        Err(_) => slint::Timer::single_shot(Duration::from_millis(50), move || run_or_retry(f)),
    });
}

fn info(title: &str, text: &str) {
    dialog(MessageLevel::Info, title, text);
}

fn error(title: &str, text: &str) {
    dialog(MessageLevel::Error, title, text);
}

fn dialog(level: MessageLevel, title: &str, text: &str) {
    MessageDialog::new().set_level(level).set_title(title).set_description(text).show();
}

fn confirm(title: &str, text: &str) -> bool {
    MessageDialog::new()
        .set_level(MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(MessageButtons::YesNo)
        .show()
        == MessageDialogResult::Yes
}

struct App {
    ui: AppWindow,
    tracks: Vec<Track>,
    rows: Rc<VecModel<Row>>,
    current: Option<usize>,
    loading: bool,
    results: Vec<musicbrainz::Match>,
    results_for: Option<PathBuf>,
    settings: Settings,
    player: player::Player,
}

fn row_of(t: &Track, checked: bool) -> Row {
    let mark = if !t.error.is_empty() { "⚠ " } else if t.dirty { "● " } else { "" };
    let f = &t.fields;
    Row {
        checked,
        mark: mark.into(),
        name: t.filename().into(),
        title: f.title.as_str().into(),
        artist: f.artist.as_str().into(),
        album: f.album.as_str().into(),
        year: f.year.as_str().into(),
        format: t.format().into(),
    }
}

impl App {
    // ------------------------------------------------------------ prikaz

    fn status(&self, text: impl Into<SharedString>) {
        self.ui.set_status(text.into());
    }

    fn refresh_row(&self, i: usize) {
        let checked = self.rows.row_data(i).is_some_and(|r| r.checked);
        self.rows.set_row_data(i, row_of(&self.tracks[i], checked));
    }

    fn checked(&self) -> Vec<usize> {
        self.rows.iter().enumerate().filter(|(_, r)| r.checked).map(|(i, _)| i).collect()
    }

    fn dirty_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.dirty).count()
    }

    fn update_state(&self) {
        let ui = &self.ui;
        let dirty = self.dirty_count();
        ui.set_has_tracks(!self.tracks.is_empty());
        ui.set_any_dirty(dirty > 0);
        ui.set_any_checked(self.rows.iter().any(|r| r.checked));
        ui.set_loading(self.loading);
        ui.set_playing(self.player.is_playing());
        ui.set_current(self.current.map_or(-1, |i| i as i32));
        let mut summary = format!("{} fajlova", self.tracks.len());
        if dirty > 0 {
            summary += &format!(" · {dirty} nesačuvano");
        }
        ui.set_summary(summary.into());
    }

    fn load_form(&self) {
        let ui = &self.ui;
        let t = self.current.map(|i| &self.tracks[i]);
        let empty = Track::new(PathBuf::new());
        let f = &t.unwrap_or(&empty).fields;
        ui.set_f_title(f.title.as_str().into());
        ui.set_f_artist(f.artist.as_str().into());
        ui.set_f_album(f.album.as_str().into());
        ui.set_f_albumartist(f.albumartist.as_str().into());
        ui.set_f_year(f.year.as_str().into());
        ui.set_f_genre(f.genre.as_str().into());
        ui.set_f_track(f.track as i32);
        ui.set_f_comment(f.comment.as_str().into());
        ui.set_form_enabled(t.is_some_and(|t| t.error.is_empty()));
        ui.set_form_title(match t {
            None => "Metapodaci".into(),
            Some(t) if !t.error.is_empty() => format!("{} — greška: {}", t.filename(), t.error).into(),
            Some(t) => format!("Metapodaci — {}", t.filename()).into(),
        });
    }

    fn select(&mut self, i: Option<usize>) {
        self.current = i.filter(|&i| i < self.tracks.len());
        self.load_form();
        self.update_state();
    }

    // ------------------------------------------------------------ učitavanje

    fn pick_files(&mut self) {
        let mut d = FileDialog::new()
            .set_title("Izaberi audio fajlove")
            .add_filter("Audio fajlovi", tags::EXTENSIONS);
        if let Some(dir) = &self.settings.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            self.remember_dir(files[0].parent());
            self.add_paths(files);
        }
    }

    fn pick_folder(&mut self) {
        let mut d = FileDialog::new().set_title("Izaberi folder");
        if let Some(dir) = &self.settings.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(dir) = d.pick_folder() {
            self.remember_dir(Some(&dir));
            self.add_paths(vec![dir]);
        }
    }

    fn remember_dir(&mut self, dir: Option<&Path>) {
        self.settings.last_dir = dir.map(Path::to_path_buf);
        self.settings.save();
    }

    /// Prima fajlove i/ili foldere; traži, uklanja duplikate i čita u pozadini.
    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        if self.loading {
            self.status("Učitavanje je već u toku — sačekaj da se završi.");
            return;
        }
        let known: HashSet<PathBuf> =
            self.tracks.iter().map(|t| t.path.canonicalize().unwrap_or(t.path.clone())).collect();
        self.loading = true;
        self.status("Tražim audio fajlove…");
        self.update_state();

        std::thread::spawn(move || {
            let mut found = Vec::new();
            for p in paths {
                collect(&p, &mut found);
            }
            let mut seen = known;
            found.retain(|p| seen.insert(p.canonicalize().unwrap_or(p.clone())));
            let total = found.len();
            if total == 0 {
                post(|a| a.on_loaded(vec![], 0, 0));
                return;
            }
            let mut done = 0;
            for chunk in found.chunks(LOAD_BATCH) {
                let batch: Vec<Track> = chunk.iter().map(|p| tags::read(p)).collect();
                done += batch.len();
                post(move |a| a.on_loaded(batch, done, total));
            }
        });
    }

    fn on_loaded(&mut self, batch: Vec<Track>, done: usize, total: usize) {
        for t in batch {
            self.rows.push(row_of(&t, false));
            self.tracks.push(t);
        }
        if done < total {
            self.status(format!("Učitavam… {done}/{total}"));
            return;
        }
        self.loading = false;
        if total == 0 {
            self.status("Nema novih podržanih audio fajlova (ili su već u listi).");
        } else {
            let errors = self.tracks.iter().filter(|t| !t.error.is_empty()).count();
            let mut msg = format!("Učitano — ukupno {} fajlova", self.tracks.len());
            if errors > 0 {
                msg += &format!(" ({errors} sa greškom, označeni sa ⚠)");
            }
            self.status(msg);
        }
        if self.current.is_none() && !self.tracks.is_empty() {
            self.select(Some(0));
        }
        self.update_state();
    }

    // ------------------------------------------------------------ izmene

    fn field_edited(&mut self, field: &str, value: &str) {
        if let Some(i) = self.current {
            if self.tracks[i].set(field, value) {
                self.refresh_row(i);
                self.update_state();
            }
        }
    }

    fn batch(&mut self, field: &str, value: &str) {
        let rows = self.checked();
        if rows.is_empty() {
            return;
        }
        let mut changed = 0;
        for &r in &rows {
            if self.tracks[r].set(field, value) {
                changed += 1;
                self.refresh_row(r);
            }
        }
        if self.current.is_some_and(|c| rows.contains(&c)) {
            self.load_form();
        }
        self.update_state();
        let what = if value.trim().is_empty() { "prazno (brisanje)".to_string() } else { format!("„{}”", value.trim()) };
        self.status(format!(
            "{field} = {what} promenjeno na {changed} od {} fajlova — ne zaboravi da sačuvaš.",
            rows.len()
        ));
    }

    fn revert(&mut self) {
        let Some(i) = self.current else { return };
        self.tracks[i] = tags::read(&self.tracks[i].path);
        self.refresh_row(i);
        self.load_form();
        self.update_state();
        self.status(format!("Vraćeni tagovi sa diska: {}", self.tracks[i].filename()));
    }

    // ------------------------------------------------------------ čuvanje

    fn save_rows(&mut self, rows: &[usize]) -> Vec<String> {
        let mut errors = vec![];
        for &r in rows {
            if let Err(e) = tags::write(&mut self.tracks[r]) {
                errors.push(e);
            }
            self.refresh_row(r);
        }
        self.update_state();
        errors
    }

    fn save_current(&mut self) {
        let Some(i) = self.current else { return };
        match self.save_rows(&[i]).first() {
            Some(e) => error("Greška pri čuvanju", e),
            None => self.status(format!("Sačuvano: {}", self.tracks[i].filename())),
        }
    }

    /// Čuva sve izmenjene. Vraća true ako nije bilo grešaka.
    fn save_all(&mut self) -> bool {
        let rows: Vec<usize> = (0..self.tracks.len()).filter(|&i| self.tracks[i].dirty).collect();
        if rows.is_empty() {
            self.status("Nema nesačuvanih izmena.");
            return true;
        }
        let errors = self.save_rows(&rows);
        let mut msg = format!("Sačuvano {} fajlova", rows.len() - errors.len());
        if !errors.is_empty() {
            msg += &format!(", {} neuspešno", errors.len());
            let mut text = errors.iter().take(10).cloned().collect::<Vec<_>>().join("\n");
            if errors.len() > 10 {
                text += &format!("\n… i još {}", errors.len() - 10);
            }
            error("Delimičan uspeh", &text);
        }
        self.status(msg);
        errors.is_empty()
    }

    // ------------------------------------------------------------ uklanjanje

    fn confirm_discard(&self, rows: &[usize], question: &str) -> bool {
        let dirty = rows.iter().filter(|&&r| self.tracks[r].dirty).count();
        dirty == 0 || confirm("Nesačuvane izmene", &format!("{dirty} fajlova ima nesačuvane izmene. {question}"))
    }

    /// Uklanja označene; ako ništa nije označeno — trenutnu pesmu.
    fn remove(&mut self) {
        if self.loading {
            return;
        }
        let mut rows = self.checked();
        if rows.is_empty() {
            rows.extend(self.current);
        }
        if rows.is_empty() || !self.confirm_discard(&rows, "Ukloniti ih iz liste bez čuvanja?") {
            return;
        }
        let gone: HashSet<PathBuf> = rows.iter().map(|&r| self.tracks[r].path.clone()).collect();
        self.player.stop_if(|p| gone.contains(p));
        for &r in rows.iter().rev() {
            self.tracks.remove(r);
            self.rows.remove(r);
        }
        let next = (!self.tracks.is_empty()).then(|| rows[0].min(self.tracks.len() - 1));
        self.select(next);
        self.status(format!("Uklonjeno {} fajlova iz liste", rows.len()));
    }

    fn clear_all(&mut self) {
        if self.tracks.is_empty() || self.loading {
            return;
        }
        let all: Vec<usize> = (0..self.tracks.len()).collect();
        let ok = if self.dirty_count() > 0 {
            self.confirm_discard(&all, "Očistiti listu bez čuvanja?")
        } else {
            confirm("Potvrda", "Očistiti celu listu? (Tagovi na disku se ne menjaju)")
        };
        if !ok {
            return;
        }
        self.player.stop_if(|_| true);
        self.tracks.clear();
        self.rows.set_vec(vec![]);
        self.select(None);
        self.status("Lista očišćena");
    }

    // ------------------------------------------------------------ online

    fn online(&mut self) {
        let Some(i) = self.current else { return };
        if self.ui.get_searching() || !self.tracks[i].error.is_empty() {
            return;
        }
        let t = &self.tracks[i];
        if t.fields.title.is_empty() {
            info("Info", "Naslov je obavezan za online pretragu.");
            return;
        }
        let (path, title, artist) = (t.path.clone(), t.fields.title.clone(), t.fields.artist.clone());
        self.ui.set_searching(true);
        self.status(format!("Pretražujem MusicBrainz: {} — {title}…", if artist.is_empty() { "?" } else { &artist }));
        std::thread::spawn(move || {
            let res = musicbrainz::search(&title, &artist);
            post(move |a| a.on_results(path, res));
        });
    }

    fn on_results(&mut self, path: PathBuf, res: Result<Vec<musicbrainz::Match>, String>) {
        self.ui.set_searching(false);
        self.status("Pretraga završena.");
        let Some(row) = self.tracks.iter().position(|t| t.path == path) else {
            return; // pesma je u međuvremenu uklonjena
        };
        match res {
            Err(e) => error("Greška", &e),
            Ok(m) if m.is_empty() => info(
                "Nema rezultata",
                "MusicBrainz nije našao ništa za taj upit.\nProbaj da pojednostaviš ime izvođača ili naslov.",
            ),
            Ok(m) => {
                let labels: Vec<SharedString> = m.iter().map(|m| m.label().into()).collect();
                self.ui.set_results(ModelRc::new(VecModel::from(labels)));
                self.ui.set_results_header(
                    format!(
                        "{} — nađeno {} mogućih rezultata. Dupli klik ili izaberi pa „Primeni”.",
                        self.tracks[row].filename(),
                        m.len()
                    )
                    .into(),
                );
                self.ui.set_result_index(0);
                self.ui.set_show_results(true);
                self.results = m;
                self.results_for = Some(path);
            }
        }
    }

    fn pick_result(&mut self, idx: usize) {
        self.ui.set_show_results(false);
        let (Some(path), Some(m)) = (self.results_for.take(), self.results.get(idx).cloned()) else { return };
        let Some(row) = self.tracks.iter().position(|t| t.path == path) else { return };
        let t = &mut self.tracks[row];
        // Samo popunjena polja — prazna iz MusicBrainz-a ne brišu postojeće vrednosti.
        for name in TEXT_FIELDS {
            let v = m.fields.get(name);
            if !v.is_empty() {
                t.set(name, v);
            }
        }
        if m.fields.track > 0 {
            t.set("track", &m.fields.track.to_string());
        }
        if m.fields.track_total > 0 {
            t.set("track_total", &m.fields.track_total.to_string());
        }
        let name = t.filename();
        self.refresh_row(row);
        if self.current == Some(row) {
            self.load_form();
        }
        self.update_state();
        self.status(format!("Popunjeno iz MusicBrainz: {name} — sačuvaj (Ctrl+S) da upišeš na disk."));
    }

    // ------------------------------------------------------------ ostalo

    fn toggle_play(&mut self) {
        let path = self.current.map(|i| self.tracks[i].path.clone());
        let res = match path {
            Some(p) => self.player.toggle(&p),
            None if self.player.is_playing() => self.player.toggle(Path::new("")),
            None => Ok(()),
        };
        if let Err(e) = res {
            self.status(format!("Plejer: {e}"));
        }
        self.update_state();
    }

    fn set_dark(&mut self, dark: bool) {
        self.settings.dark = dark;
        self.settings.save();
    }

    fn close_requested(&mut self) -> CloseRequestResponse {
        let dirty = self.dirty_count();
        if dirty > 0 {
            let answer = MessageDialog::new()
                .set_level(MessageLevel::Warning)
                .set_title("Nesačuvane izmene")
                .set_description(format!(
                    "{dirty} fajlova ima nesačuvane izmene. Sačuvati pre izlaska?\n\nDa = sačuvaj, Ne = odbaci izmene"
                ))
                .set_buttons(MessageButtons::YesNoCancel)
                .show();
            let keep = match answer {
                MessageDialogResult::Yes => !self.save_all(),
                MessageDialogResult::No => false,
                _ => true,
            };
            if keep {
                return CloseRequestResponse::KeepWindowShown;
            }
        }
        self.player.stop_if(|_| true);
        CloseRequestResponse::HideWindow
    }
}

/// Rekurzivno skupi podržane fajlove (sortirano po imenu, kao fajl menadžer).
fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            collect(&e, out);
        }
    } else if p.is_file() && tags::is_supported(p) {
        out.push(p.to_path_buf());
    }
}

fn about() {
    let exts = tags::EXTENSIONS.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", ");
    info(
        &format!("O programu — {APP_NAME}"),
        &format!(
            "{APP_NAME} {}\n\nEditor audio tagova sa MusicBrainz pretragom.\nRust + Slint.\n\nPodržani formati: {exts}",
            env!("CARGO_PKG_VERSION")
        ),
    );
}

fn main() -> Result<(), slint::PlatformError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Wayland app_id = ime .desktop fajla → ispravna ikonica u panelu
    let _ = slint::set_xdg_app_id("audio-tag-editor");
    let ui = AppWindow::new()?;
    let settings = Settings::load();
    let rows = Rc::new(VecModel::<Row>::default());
    ui.set_rows(ModelRc::from(rows.clone()));
    ui.set_dark(settings.dark);

    // Svi callback-ovi samo prosleđuju u App (bez hvatanja ui handle-a → bez ciklusa).
    ui.on_add_files(|| _ = with_app(App::pick_files));
    ui.on_add_folder(|| _ = with_app(App::pick_folder));
    ui.on_remove(|| _ = with_app(App::remove));
    ui.on_clear_all(|| _ = with_app(App::clear_all));
    ui.on_save_current(|| _ = with_app(App::save_current));
    ui.on_save_all(|| _ = with_app(|a| a.save_all()));
    ui.on_revert(|| _ = with_app(App::revert));
    ui.on_toggle_play(|| _ = with_app(App::toggle_play));
    ui.on_online(|| _ = with_app(App::online));
    ui.on_about(about);
    ui.on_select(|i| _ = with_app(|a| a.select(usize::try_from(i).ok())));
    ui.on_toggle_check(|i, on| {
        with_app(|a| {
            if let Some(mut r) = a.rows.row_data(i as usize) {
                r.checked = on;
                a.rows.set_row_data(i as usize, r);
            }
            a.update_state();
        });
    });
    ui.on_check_all(|on| {
        with_app(|a| {
            for i in 0..a.rows.row_count() {
                let mut r = a.rows.row_data(i).unwrap();
                r.checked = on;
                a.rows.set_row_data(i, r);
            }
            a.update_state();
        });
    });
    ui.on_field_edited(|f, v| _ = with_app(|a| a.field_edited(&f, &v)));
    ui.on_batch(|f, v| _ = with_app(|a| a.batch(&f, &v)));
    ui.on_pick_result(|i| _ = with_app(|a| a.pick_result(i.max(0) as usize)));
    ui.on_cancel_results(|| {
        with_app(|a| {
            a.ui.set_show_results(false);
            a.results_for = None;
        });
    });
    // Ako je stanje zauzeto (otvoren dijalog), prozor ostaje — nikad zatvaranje bez pitanja.
    ui.window().on_close_requested(|| with_app(App::close_requested).unwrap_or(CloseRequestResponse::KeepWindowShown));

    ui.on_theme_changed(|dark| _ = with_app(|a| a.set_dark(dark)));

    // "Pusti/Pauza" se sam vrati kad pesma završi.
    let play_timer = slint::Timer::default();
    play_timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), || {
        with_app(|a| a.ui.set_playing(a.player.is_playing()));
    });

    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            ui: ui.clone_strong(),
            tracks: vec![],
            rows,
            current: None,
            loading: false,
            results: vec![],
            results_for: None,
            settings,
            player: player::Player::default(),
        })
    });
    with_app(|a| a.update_state());

    // Otvaranje iz fajl menadžera / terminala: audio-tag-editor fajl1.mp3 ~/Muzika
    let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();
    if !paths.is_empty() {
        with_app(|a| a.add_paths(paths));
    }

    let res = ui.run();
    APP.with(|a| a.borrow_mut().take()); // oslobodi pre izlaska (zaustavlja zvuk)
    res
}
