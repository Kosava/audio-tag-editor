//! Pretpreslušavanje preko rodio (zvučni uređaj se otvara tek pri prvom puštanju).

use std::fs::File;
use std::path::{Path, PathBuf};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink};

#[derive(Default)]
pub struct Player {
    sink: Option<MixerDeviceSink>,
    current: Option<(PathBuf, rodio::Player)>,
}

impl Player {
    pub fn is_playing(&self) -> bool {
        matches!(&self.current, Some((_, p)) if !p.is_paused() && !p.empty())
    }

    /// Svira → pauza; pauzirana ista pesma → nastavak; inače kreće nova.
    pub fn toggle(&mut self, path: &Path) -> Result<(), String> {
        if let Some((p, pl)) = &self.current {
            if self.is_playing() {
                pl.pause();
                return Ok(());
            }
            if p == path && !pl.empty() {
                pl.play();
                return Ok(());
            }
        }
        self.current = None;

        if self.sink.is_none() {
            let mut sink = DeviceSinkBuilder::open_default_sink()
                .map_err(|e| format!("Zvučni uređaj nije dostupan: {e}"))?;
            sink.log_on_drop(false);
            self.sink = Some(sink);
        }
        let file = File::open(path).map_err(|e| e.to_string())?;
        let source = Decoder::try_from(file).map_err(|e| format!("Ne mogu da pustim ovaj fajl: {e}"))?;
        let pl = rodio::Player::connect_new(self.sink.as_ref().unwrap().mixer());
        pl.append(source);
        self.current = Some((path.to_path_buf(), pl));
        Ok(())
    }

    /// Zaustavi ako svira neki od ovih fajlova (npr. pre uklanjanja iz liste).
    pub fn stop_if(&mut self, f: impl Fn(&Path) -> bool) {
        if self.current.as_ref().is_some_and(|(p, _)| f(p)) {
            self.current = None;
        }
    }
}
