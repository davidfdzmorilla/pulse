//! The browser's search: everything it can put on a track, by name. Words
//! match the start of a word of the name («comp bus» finds «Compresor de
//! bus»), without minding accents or capitals, and also some useful words in
//! English and Spanish («reverb», «ott», «glue»). Results come grouped by kind,
//! the best first.
//!
//! The index is built from the catalogs, the presets and the folders (see
//! `browser`) and kept a few seconds; a search goes through it in memory.
//! Favourites are kept here too, by the key each entry has.

use crate::chains::Chain;
use crate::presets::Preset;
use engine::model::{Effect, InstrumentKind, MidiEffect};
use engine::plugin::PluginInfo;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The kinds of result, in the order they show when as good.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Instruments,
    MidiEffects,
    Effects,
    Chains,
    Presets,
    Plugins,
    Sounds,
    Files,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Instruments => "INSTRUMENTOS",
            Group::MidiEffects => "EFECTOS MIDI",
            Group::Effects => "EFECTOS DE AUDIO",
            Group::Chains => "CADENAS",
            Group::Presets => "PRESETS",
            Group::Plugins => "PLUGINS",
            Group::Sounds => "SONIDOS DE PULSE",
            Group::Files => "ARCHIVOS",
        }
    }

    /// A little in favour of what one searches for most: devices.
    fn weight(self) -> f32 {
        match self {
            Group::Instruments | Group::MidiEffects | Group::Effects => 2.0,
            Group::Chains => 1.5,
            Group::Plugins => 1.0,
            Group::Presets => 0.8,
            Group::Sounds => 0.5,
            Group::Files => 0.0,
        }
    }
}

/// What an entry puts where it goes.
#[derive(Clone, Debug)]
pub enum What {
    Instrument(InstrumentKind),
    MidiEffect(MidiEffect),
    Effect(Effect),
    /// An instrument preset, the factory's or the user's.
    Preset(Box<Preset>),
    /// An effect's factory setting.
    EffectPreset(Effect),
    Chain(Arc<Chain>),
    /// A kit of «Sonidos de Pulse» not made yet, by its place in `kits()`.
    Kit(usize),
    File(PathBuf),
    Plugin(Box<PluginInfo>),
}

#[derive(Clone, Debug)]
pub struct Item {
    /// What it is known by among the favourites.
    pub key: String,
    pub name: String,
    /// Where it comes from (the instrument of a preset, a sample's folder).
    pub detail: String,
    pub group: Group,
    pub what: What,
    words: Vec<String>,
    extra: Vec<String>,
    folded: String,
}

impl Item {
    pub fn new(key: String, name: impl Into<String>, detail: impl Into<String>, group: Group, what: What, also: &str) -> Item {
        let name = name.into();
        let detail = detail.into();
        let folded = fold(&name);
        let words = folded.split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
        let extra = fold(&format!("{detail} {also}")).split(' ').filter(|w| !w.is_empty()).map(String::from).collect();
        Item { key, name, detail, group, what, words, extra, folded }
    }

    /// How well it answers `query` (folded words), or None if it doesn't.
    fn score(&self, words: &[&str], query: &str) -> Option<f32> {
        let mut total = 0.0;
        for q in words {
            let mut best: f32 = 0.0;
            for (i, w) in self.words.iter().enumerate() {
                let s = if w == q {
                    3.0
                } else if w.starts_with(q) {
                    2.0
                } else if q.len() >= 3 && w.contains(q) {
                    0.8
                } else {
                    continue;
                };
                best = best.max(if i == 0 { s + 0.5 } else { s });
            }
            for w in &self.extra {
                if w == q {
                    best = best.max(1.6);
                } else if w.starts_with(q) {
                    best = best.max(1.2);
                }
            }
            if best == 0.0 {
                return None;
            }
            total += best;
        }
        if self.folded == query {
            total += 3.0;
        } else if words.len() > 1 && self.folded.starts_with(query) {
            // All the words, in order, from the start (one word: as its first word).
            total += 1.5;
        }
        Some(total + self.group.weight() - self.words.len() as f32 * 0.05)
    }
}

/// `text` in lower case, without accents, its words split by single spaces.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        if c == ' ' {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// The entries that answer `query`, by group (the group with the best one
/// first) and, inside each, best first; at most `per_group` a group.
pub fn find<'a>(lists: &[&'a [Item]], query: &str, per_group: usize) -> Vec<(Group, Vec<&'a Item>)> {
    let query = fold(query);
    let words: Vec<&str> = query.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f32, &Item)> = lists.iter().flat_map(|l| l.iter()).filter_map(|item| Some((item.score(&words, &query)?, item))).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.group.cmp(&b.1.group)).then_with(|| a.1.name.len().cmp(&b.1.name.len())).then_with(|| a.1.name.cmp(&b.1.name)));
    let mut groups: Vec<(Group, Vec<&Item>)> = Vec::new();
    for (_, item) in scored {
        match groups.iter_mut().find(|g| g.0 == item.group) {
            Some(g) if g.1.len() < per_group => g.1.push(item),
            Some(_) => {}
            None => groups.push((item.group, vec![item])),
        }
    }
    groups
}

/// Other words an effect goes by.
fn effect_words(effect: &Effect) -> &'static str {
    match effect {
        Effect::Eq3(_) => "eq ecualizador equalizer graves medios agudos",
        Effect::EqEight(_) => "eq ecualizador equalizer parametric parametrico filtro",
        Effect::Compressor(_) => "comp compressor compresor dynamics dinamica",
        Effect::Glue(_) => "glue bus compressor compresor ssl pegar",
        Effect::Multiband(_) => "ott multiband multibanda upward compressor compresor",
        Effect::Limiter(_) => "limiter limitador maximizer maximizador loud volumen",
        Effect::Gate(_) => "gate puerta noise ruido",
        Effect::Transient(_) => "transient transitorios shaper pegada attack",
        Effect::Tuner(_) => "tuner afinador autotune pitch correction afinar",
        Effect::DeEsser(_) => "deesser de esser sibilancia eses",
        Effect::Delay(_) => "delay eco echo retardo",
        Effect::Reverb(_) => "reverb reverberacion sala room",
        Effect::Convolution(_) => "reverb convolution convolucion sala hall room placa plate muelle spring ir",
        Effect::Saturator(_) => "saturator saturacion distortion distorsion drive overdrive tape cinta",
        Effect::AutoFilter(_) => "filter filtro lowpass highpass wah barrido sweep",
        Effect::Chorus(_) => "chorus coro ensemble ancho",
        Effect::Phaser(_) => "phaser fase",
        Effect::Bitcrusher(_) => "bitcrusher crusher lofi decimator bits",
        Effect::Ducker(_) => "ducker sidechain pump bombeo",
        Effect::Utility(_) => "utility utilidad gain ganancia mono width ancho pan fase",
        _ => "",
    }
}

/// Other words an instrument goes by.
fn instrument_words(kind: &InstrumentKind) -> &'static str {
    match kind {
        InstrumentKind::Synth(_) => "synth sintetizador subtractive",
        InstrumentKind::Keys(_) => "keys piano teclas rhodes organo organ epiano",
        InstrumentKind::SuperSaw(_) => "supersaw trance sierra saw",
        InstrumentKind::Acid(_) => "acid 303 bajo bass",
        InstrumentKind::Fm(_) => "fm dx7 sintetizador",
        InstrumentKind::Wavetable(_) => "wavetable serum tablas",
        InstrumentKind::Polychrome(_) => "polychrome",
        InstrumentKind::Modular(_) => "modular",
        InstrumentKind::Sampler(_) => "sampler muestras sample simpler",
        InstrumentKind::Kick(_) => "kick bombo",
        InstrumentKind::DrumMachine(_) | InstrumentKind::Drums => "drums bateria caja de ritmos 808 909",
        InstrumentKind::DrumRack(_) => "drum rack pads bateria kit",
        _ => "",
    }
}

/// Other words a preset goes by, from words of its name: a trap producer
/// looks for «808» and finds the sub basses and glides too.
fn preset_words(name: &str) -> String {
    let name = fold(name);
    let has = |w: &str| name.split(' ').any(|n| n == w);
    let mut words = String::new();
    for (word, also) in [
        ("bajo", "bass"),
        ("sub", "808 trap bass"),
        ("808", "trap sub bass"),
        ("glide", "slide portamento legato"),
        ("trap", "808"),
    ] {
        if has(word) {
            words.push(' ');
            words.push_str(also);
        }
    }
    words
}

fn midi_words(effect: &MidiEffect) -> &'static str {
    match effect {
        MidiEffect::Arpeggiator(_) => "arp arpeggiator",
        MidiEffect::Chord(_) => "chord acordes",
        MidiEffect::Scale(_) => "scale tonalidad key",
        #[allow(unreachable_patterns)]
        _ => "",
    }
}

/// Where the static part of the index comes from (what isn't a file).
pub struct Sources<'a> {
    /// The user's instrument presets (`presets::user_root`).
    pub presets: &'a Path,
    pub chains: Vec<Chain>,
    pub plugins: &'a [PluginInfo],
    /// «Sonidos de Pulse», to know which kits are made.
    pub factory: &'a Path,
}

/// Instruments, effects, presets, chains, kits and plugins.
pub fn catalog(sources: &Sources) -> Vec<Item> {
    let mut items = Vec::new();
    for kind in InstrumentKind::catalog() {
        items.push(Item::new(format!("instrumento:{}", kind.label()), kind.label(), "Instrumento", Group::Instruments, What::Instrument(kind), instrument_words(&kind)));
        for preset in crate::presets::factory(&kind) {
            let words = format!("{}{}", instrument_words(&kind), preset_words(&preset.name));
            items.push(Item::new(format!("preset:{}:{}", kind.label(), preset.name), preset.name.clone(), kind.label(), Group::Presets, What::Preset(Box::new(preset)), &words));
        }
        for preset in crate::presets::list(sources.presets, &kind) {
            let words = format!("{}{}", instrument_words(&kind), preset_words(&preset.name));
            items.push(Item::new(format!("preset-mio:{}:{}", kind.label(), preset.name), preset.name.clone(), format!("{} · mío", kind.label()), Group::Presets, What::Preset(Box::new(preset)), &words));
        }
    }
    for effect in MidiEffect::catalog() {
        items.push(Item::new(format!("efecto-midi:{}", effect.label()), effect.label(), "Efecto MIDI", Group::MidiEffects, What::MidiEffect(effect), midi_words(&effect)));
    }
    for effect in Effect::catalog() {
        items.push(Item::new(format!("efecto:{}", effect.label()), effect.label(), "Efecto", Group::Effects, What::Effect(effect), effect_words(&effect)));
        for (name, preset) in effect.factory_presets() {
            items.push(Item::new(format!("preset-efecto:{}:{name}", effect.label()), name, effect.label(), Group::Presets, What::EffectPreset(preset), effect_words(&effect)));
        }
    }
    for chain in &sources.chains {
        let key = if chain.factory { format!("cadena:{}", chain.name) } else { format!("cadena-mia:{}", chain.name) };
        let detail = if chain.factory { "Cadena" } else { "Cadena · mía" };
        let words = format!("chain rack fx {}", chain.summary());
        items.push(Item::new(key, chain.name.clone(), detail, Group::Chains, What::Chain(Arc::new(chain.clone())), &words));
    }
    for (i, kit) in crate::factory_sounds::kits().iter().enumerate() {
        // Made, it is a file under Kits/ (found with the files).
        if !sources.factory.join(crate::factory_sounds::KITS).join(format!("{}.json", kit.name)).exists() {
            items.push(Item::new(format!("kit:{}", kit.name), format!("Kit {}", kit.name), crate::factory_sounds::NAME, Group::Sounds, What::Kit(i), "kit bateria drums"));
        }
    }
    for info in sources.plugins {
        let detail = if info.vendor.is_empty() { "Plugin".to_string() } else { info.vendor.clone() };
        let words = format!("plugin vst3 clap {}", info.category.replace('|', " "));
        items.push(Item::new(format!("plugin:{}", info.cid), info.name.clone(), detail, Group::Plugins, What::Plugin(Box::new(info.clone())), &words));
    }
    items
}

/// A file as an entry: «Sonidos de Pulse» or the user's.
pub fn file_item(path: &Path, factory: &Path) -> Item {
    let name = path.file_stem().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let folder = path.parent().and_then(|p| p.file_name()).map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let group = if path.starts_with(factory) { Group::Sounds } else { Group::Files };
    let kit = if path.starts_with(factory.join(crate::factory_sounds::KITS)) { "kit bateria drums" } else { "" };
    Item::new(format!("archivo:{}", path.display()), name, folder, group, What::File(path.to_path_buf()), kit)
}

/// The files the browser shows under `roots`, at most `limit` entries looked
/// at (a huge folder doesn't stall the app).
pub fn walk(roots: &[PathBuf], limit: usize, shows: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending: Vec<PathBuf> = roots.to_vec();
    let mut visited = 0;
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            visited += 1;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => pending.push(path),
                Ok(_) if shows(&path) => found.push(path),
                _ => {}
            }
        }
        if visited > limit {
            break;
        }
    }
    found
}

// ----- Favourites ----------------------------------------------------------------------

/// Factory presets that changed name: a star on the old one stays on it.
const RENAMED: &[(&str, &str)] = &[("preset:Polychrome Synth:Bajo · Sub con glide", "preset:Polychrome Synth:Bajo · 808 con glide")];

/// The entries starred, kept with the user's settings (not in the project).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Favorites {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub keys: Vec<String>,
}

pub fn favorites_path() -> PathBuf {
    // Tests never touch the user's.
    if cfg!(test) {
        return std::env::temp_dir().join(format!("pulse-test-favoritos-{}", std::process::id())).join("favoritos.json");
    }
    paths::config_dir().join("favoritos.json")
}

impl Favorites {
    pub fn load_from(path: &Path) -> Favorites {
        let mut favorites: Favorites = crate::store::read_json(path);
        for key in &mut favorites.keys {
            if let Some(&(_, new)) = RENAMED.iter().find(|(old, _)| old == key) {
                *key = new.to_string();
            }
        }
        favorites
    }

    pub fn save_to(&self, path: &Path) {
        if let Err(e) = crate::store::write_json(path, self) {
            crate::logs::note(&e);
        }
    }

    /// Stars or unstars `key`; true if it is a favourite now.
    pub fn toggle(&mut self, key: &str) -> bool {
        self.version = 1;
        if let Some(i) = self.keys.iter().position(|k| k == key) {
            self.keys.remove(i);
            false
        } else {
            self.keys.push(key.to_string());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Vec<Item> {
        let empty = std::env::temp_dir().join("pulse-busqueda-nada");
        catalog(&Sources { presets: &empty, chains: crate::chains::factory(), plugins: &[], factory: &empty })
    }

    fn names(items: &[Item], query: &str) -> Vec<String> {
        find(&[items], query, 50).into_iter().flat_map(|(_, g)| g.into_iter().map(|i| i.name.clone())).collect()
    }

    fn first(items: &[Item], query: &str) -> String {
        names(items, query).into_iter().next().unwrap_or_default()
    }

    #[test]
    fn words_fold_without_accents_or_capitals() {
        assert_eq!(fold("  Batería · Bus de BATERÍA!  "), "bateria bus de bateria");
        assert_eq!(fold("Multibanda (OTT)"), "multibanda ott");
        assert_eq!(fold("Pequeño Ñu"), "pequeno nu");
    }

    #[test]
    fn the_search_finds_by_prefix_accent_and_synonym() {
        let items = index();
        assert_eq!(first(&items, "comp bus"), "Compresor de bus");
        assert_eq!(first(&items, "espa"), "Espacio");
        assert_eq!(first(&items, "ESPACIO"), "Espacio");
        assert_eq!(first(&items, "bateria"), first(&items, "batería"));
        assert_eq!(first(&items, "ott"), "Multibanda (OTT)");
        assert_eq!(first(&items, "glue"), "Compresor de bus");
        assert_eq!(first(&items, "multib"), "Multibanda (OTT)");
        let reverb = names(&items, "reverb");
        assert!(reverb[..2].contains(&"Reverb".to_string()) && reverb[..2].contains(&"Espacio".to_string()), "{reverb:?}");
        assert!(reverb.iter().any(|n| n == "Voces · Placa brillante"), "the space's presets come too");
        // Presets «Categoría · Nombre» and chains.
        assert!(names(&items, "placa brill").contains(&"Voces · Placa brillante".to_string()));
        assert_eq!(first(&items, "voz pop"), "Voz · Pop");
        assert!(names(&items, "zarandajo inventado").is_empty());
        assert!(names(&items, "   ").is_empty());
    }

    /// A star on a preset that changed name stays on it.
    #[test]
    fn renamed_presets_keep_their_star() {
        let dir = std::env::temp_dir().join(format!("pulse-test-renombrado-{}", std::process::id()));
        let path = dir.join("favoritos.json");
        let old = Favorites { version: 1, keys: vec!["preset:Polychrome Synth:Bajo · Sub con glide".into(), "efecto:Reverb".into()] };
        old.save_to(&path);
        let loaded = Favorites::load_from(&path);
        assert_eq!(loaded.keys, ["preset:Polychrome Synth:Bajo · 808 con glide", "efecto:Reverb"]);
        assert!(index().iter().any(|i| i.key == loaded.keys[0]), "the new key is a real entry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A trap producer finds the melodic 808 by «808», and the subs and glides
    /// by their usual words.
    #[test]
    fn trap_words_find_the_808_bass() {
        let items = index();
        let found = names(&items, "808");
        assert!(found.iter().any(|n| n == "Bajo · 808 con glide"), "{found:?}");
        assert_eq!(first(&items, "808 bajo"), "Bajo · 808 con glide");
        assert!(names(&items, "slide").iter().any(|n| n == "Bajo · 808 con glide"));
        assert!(names(&items, "portamento").iter().any(|n| n == "Bajo · 808 con glide"));
        assert!(names(&items, "808").iter().any(|n| n == "Bajo · Sub"), "subs come with «808» too");
        assert!(names(&items, "sub bass").iter().any(|n| n == "Bajo · 808 con glide"));
    }

    /// The device itself before its presets, presets before files; whole words
    /// before parts of them.
    #[test]
    fn results_are_grouped_and_best_first() {
        let mut items = index();
        let dir = Path::new("/samples");
        for name in ["Compresión rara.wav", "Bus 3.wav", "comp bus loop 128.wav"] {
            items.push(file_item(&dir.join(name), Path::new("/fabrica")));
        }
        let catalog = index();
        let devices = find(&[&catalog], "comp bus", 50);
        assert_eq!(devices[0].0, Group::Effects);
        assert_eq!(devices[0].1[0].name, "Compresor de bus");
        let groups = find(&[&items], "comp bus", 50);
        let order: Vec<Group> = groups.iter().map(|g| g.0).collect();
        let at = |g: Group| order.iter().position(|&o| o == g).unwrap();
        assert!(at(Group::Effects) < at(Group::Presets), "{order:?}");
        assert_eq!(groups[at(Group::Effects)].1[0].name, "Compresor de bus");
        let files = &groups.iter().find(|g| g.0 == Group::Files).unwrap().1;
        assert_eq!(files[0].name, "comp bus loop 128", "both words, from the start");
        assert_eq!(files.len(), 1, "every word must be there");
        // A word inside another counts, less than at its start.
        let kick = names(&items, "bombo");
        assert_eq!(kick[0], "Kick Designer", "{kick:?}");
    }

    /// Thousands of samples: a search, and walking the folders, stay quick.
    #[test]
    fn the_search_is_quick_with_thousands_of_samples() {
        let dir = std::env::temp_dir().join(format!("pulse-busqueda-miles-{}", std::process::id()));
        let words = ["Kick", "Snare", "Hat", "Clap", "Bass", "Pad", "Vox", "Perc", "FX", "Loop"];
        for i in 0..5000 {
            let folder = dir.join(words[i % 10]).join(format!("Pack {}", i / 500));
            if i % 500 < 10 {
                std::fs::create_dir_all(&folder).unwrap();
            }
            std::fs::write(folder.join(format!("{} {} {} {}.wav", words[i % 10], words[(i / 10) % 10], i, if i % 3 == 0 { "Seco" } else { "Húmedo" })), b"").unwrap();
        }
        let started = std::time::Instant::now();
        let files = walk(&[dir.clone()], 50_000, |p| crate::app::is_audio_file(p));
        let walked = started.elapsed();
        assert_eq!(files.len(), 5000);
        // Each timed a few times, the fastest kept: a busy machine (other tests,
        // other builds) only ever makes one slower.
        let fastest = |f: &mut dyn FnMut()| {
            (0..5)
                .map(|_| {
                    let started = std::time::Instant::now();
                    f();
                    started.elapsed()
                })
                .min()
                .unwrap()
        };
        let build = || {
            let mut items = index();
            items.extend(files.iter().map(|p| file_item(p, Path::new("/fabrica"))));
            items
        };
        let built = fastest(&mut || drop(build()));
        let items = build();
        let mut slowest = std::time::Duration::ZERO;
        for query in ["k", "kick", "kick sec", "humedo pad 12", "comp bus", "reverb", "zzz"] {
            slowest = slowest.max(fastest(&mut || drop(find(&[&items], query, 50))));
            let found = find(&[&items], query, 50);
            if query == "kick sec" {
                let files = &found.iter().find(|g| g.0 == Group::Files).unwrap().1;
                assert!(files.iter().all(|f| fold(&f.name).contains("kick") && fold(&f.name).contains("seco")));
            }
        }
        println!("{} entradas: recorrer {walked:?}, índice {built:?}, la búsqueda más lenta {slowest:?}", items.len());
        assert!(slowest < std::time::Duration::from_millis(30), "{slowest:?}");
        assert!(built < std::time::Duration::from_millis(200), "{built:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn favorites_persist_and_a_damaged_file_starts_empty() {
        let path = std::env::temp_dir().join(format!("pulse-favoritos-prueba-{}", std::process::id())).join("favoritos.json");
        let mut favorites = Favorites::load_from(&path);
        assert!(favorites.keys.is_empty());
        assert!(favorites.toggle("efecto:Espacio"));
        assert!(favorites.toggle("cadena:Voz · Pop"));
        favorites.save_to(&path);
        let again = Favorites::load_from(&path);
        assert_eq!(again.keys, ["efecto:Espacio", "cadena:Voz · Pop"]);
        let mut again = again;
        assert!(!again.toggle("efecto:Espacio"));
        again.save_to(&path);
        assert_eq!(Favorites::load_from(&path).keys, ["cadena:Voz · Pop"]);
        std::fs::write(&path, b"{ roto").unwrap();
        assert!(Favorites::load_from(&path).keys.is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
