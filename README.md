# Audio Tag Editor 2 (Rust + Slint)

Editor audio tagova sa srpskim interfejsom, svetlom (aquamarin) i tamnom temom.
Port Python/PyQt6 verzije 1.0 na Rust + Slint — jedan izvršni fajl, bez Python-a i Qt-a.

- Formati: **MP3** (ID3v2.3), **FLAC**, **OGG Vorbis**, **Opus**, **M4A/MP4**
- Omoti, ReplayGain, iTunNORM i ostala polja koja editor ne prikazuje **ostaju netaknuti**
- Online popunjavanje preko **MusicBrainz**
- Batch izmene na označenim (☑) pesmama
- Nesačuvane izmene se pamte po pesmi (●), fajlovi koji ne mogu da se pročitaju su zaključani (⚠)
- Pretpreslušavanje (pusti / pauza / nastavak)
- Otvaranje iz fajl menadžera ili terminala: `audio-tag-editor ~/Muzika pesma.mp3`

## Struktura

```
audio-tag-editor/
├── Cargo.toml, Cargo.lock, build.rs
├── .cargo/config.toml          # Windows: statički runtime, mingw linker
├── src/
│   ├── main.rs                 # povezuje UI i logiku (App)
│   ├── model.rs                # Track, Fields, parse_track      ┐
│   ├── tags.rs                 # čitanje/upis (lofty)            │ bez UI-ja,
│   ├── musicbrainz.rs          # pretraga (ureq + serde_json)    │ pokriveno testovima
│   ├── player.rs               # pretpreslušavanje (rodio)       ┘
│   └── settings.rs             # tema, poslednji folder
├── ui/app.slint                # ceo interfejs
├── data/                       # .desktop, .svg, .ico
├── tests/fixtures/             # mali audio fajlovi sa omotom/ReplayGain/iTunNORM
├── packaging/PKGBUILD          # Arch
└── .github/workflows/windows.yml
```

## Arch Linux

```bash
cd audio-tag-editor/packaging
makepkg -si
```

Preskakanje testova: `makepkg -si --nocheck`. Uklanjanje: `sudo pacman -R audio-tag-editor`.

## Windows

**A) GitHub Actions (najlakše):** postavi projekat na GitHub → *Actions* → *Windows build* → *Run workflow*.
Gotov `audio-tag-editor.exe` je u *Artifacts* na dnu stranice tog pokretanja.

**B) Cross-compile sa Arch-a:**

```bash
sudo pacman -S mingw-w64-gcc
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/audio-tag-editor.exe
```

**C) Na samom Windows-u:** instaliraj Rust sa https://rustup.rs, pa `cargo build --release`.

Exe je samostalan (nema DLL-ova za kopiranje). Podešavanja: `%APPDATA%\audio-tag-editor\`.

## Razvoj

```bash
cargo run -- ~/Muzika      # pokretanje
cargo test                 # testovi
```

Podešavanja na Linux-u: `~/.config/audio-tag-editor/settings.conf`.

## Prečice

| Prečica | Akcija |
|---|---|
| Ctrl+O / Ctrl+Shift+O | Dodaj fajlove / folder |
| Ctrl+S | Sačuvaj trenutnu |
| Ctrl+Shift+S | Sačuvaj sve izmenjene |
| Ctrl+R | Poništi izmene trenutne (vrati sa diska) |
| Ctrl+F | Online popuni (MusicBrainz) |
| Ctrl+P | Pusti / pauza |
| Delete | Ukloni označene (ili trenutnu) iz liste |
