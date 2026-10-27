//! Drum patterns: a beat in a style (house, techno, trap, drum & bass…) for a
//! drum track's clip, on whatever kit it has (its sounds found by their names),
//! busier or plainer, with a fill at the end of each phrase and, if asked, the
//! kick following the bass. Each take is different; the same seed, the same take.

use crate::project::Track;
use engine::model::Note;
pub use stems::drums::Voice as Sound;

/// The sounds a beat may use, by the drum model's order (`stems::drums::ALL`).
pub type Sounds = [bool; stems::drums::VOICES];

/// The sounds to choose among, as the menu shows them (the three toms go together).
pub const SOUND_CHOICES: [(&str, &[Sound]); 14] = [
    ("Bombo", &[Sound::Kick]),
    ("Caja", &[Sound::Snare]),
    ("Palmada", &[Sound::Clap]),
    ("Charles", &[Sound::Hat]),
    ("Charles abierto", &[Sound::Open]),
    ("Ride", &[Sound::Ride]),
    ("Crash", &[Sound::Crash]),
    ("Rim", &[Sound::Rim]),
    ("Toms", &[Sound::LowTom, Sound::MidTom, Sound::HighTom]),
    ("Shaker", &[Sound::Shaker]),
    ("Pandereta", &[Sound::Tambourine]),
    ("Cencerro", &[Sound::Cowbell]),
    ("Congas", &[Sound::Conga]),
    ("Clave", &[Sound::Clave]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    House,
    Techno,
    Trap,
    DrumAndBass,
    BoomBap,
    PopRock,
    Reggaeton,
    Breakbeat,
    /// Driving and percussive (Ben Sims, Oscar Mulero's grooves): toms and
    /// congas rolling over the kick, hats that come and go.
    Hardgroove,
    /// Fast and hard: a deep, long kick, sometimes galloping, claps rolling.
    HardTechno,
}

impl Style {
    pub const ALL: [Style; 10] = [Style::House, Style::Techno, Style::Hardgroove, Style::HardTechno, Style::Trap, Style::DrumAndBass, Style::BoomBap, Style::PopRock, Style::Reggaeton, Style::Breakbeat];

    pub fn label(self) -> &'static str {
        match self {
            Style::House => "House",
            Style::Techno => "Techno",
            Style::Trap => "Trap",
            Style::DrumAndBass => "Drum & Bass",
            Style::BoomBap => "Hip hop",
            Style::PopRock => "Pop / Rock",
            Style::Reggaeton => "Reggaetón",
            Style::Breakbeat => "Breakbeat",
            Style::Hardgroove => "Hardgroove",
            Style::HardTechno => "Hard techno",
        }
    }

    /// The tempo it usually goes at (for the hint).
    pub fn tempo(self) -> u32 {
        match self {
            Style::House => 124,
            Style::Techno => 130,
            Style::Trap => 140,
            Style::DrumAndBass => 174,
            Style::BoomBap => 90,
            Style::PopRock => 110,
            Style::Reggaeton => 95,
            Style::Breakbeat => 130,
            Style::Hardgroove => 142,
            Style::HardTechno => 152,
        }
    }
}

impl Style {
    /// The sounds the style plays, to start with.
    pub fn sounds(self) -> Sounds {
        use Sound::*;
        let list: &[Sound] = match self {
            Style::House => &[Kick, Clap, Hat, Open, Rim, Shaker, Cowbell, Conga],
            // No ride or cowbell: they ring over the kick.
            Style::Techno => &[Kick, Clap, Hat, Open, Rim],
            Style::Trap => &[Kick, Snare, Clap, Hat, Open],
            Style::DrumAndBass => &[Kick, Snare, Hat, Open, Ride, Crash, Rim],
            Style::BoomBap => &[Kick, Snare, Hat, Open, Rim, Shaker],
            Style::PopRock => &[Kick, Snare, Hat, Open, Ride, Crash, LowTom, MidTom, HighTom, Tambourine],
            Style::Reggaeton => &[Kick, Snare, Hat, Open, Rim, Shaker, Conga],
            Style::Breakbeat => &[Kick, Snare, Hat, Open, Ride, Crash, Rim, LowTom, MidTom, HighTom],
            Style::Hardgroove => &[Kick, Clap, Hat, Open, Rim, LowTom, MidTom, HighTom, Conga, Shaker],
            Style::HardTechno => &[Kick, Clap, Hat, Open, Rim, Crash],
        };
        let mut sounds = [false; stems::drums::VOICES];
        for &sound in list {
            sounds[sound as usize] = true;
        }
        sounds
    }

    /// How its machine is tuned: each style's own sound (the long, deep 808
    /// kick of trap, the short, punchy 909 of techno, drum & bass's high snare…).
    pub fn sound(self) -> engine::model::DrumParams {
        use engine::model::drum_note::*;
        let mut p = engine::model::DrumParams { model: self.machine(), ..Default::default() };
        let voice = |pitch: u8| engine::model::DRUM_VOICES.iter().position(|v| v.0 == pitch).unwrap();
        let mut set = |pitch: u8, tune: f32, decay: f32, level: f32| {
            let v = voice(pitch);
            (p.tune[v], p.decay[v], p.level[v]) = (tune, decay, level);
        };
        match self {
            Style::House => {
                set(KICK, 0.0, 1.0, 0.9);
                set(CLAP, 0.0, 1.2, 0.8);
                set(OPEN_HAT, 0.0, 0.8, 0.7);
            }
            Style::Techno => {
                set(KICK, -1.0, 0.8, 0.85);
                set(CLAP, -1.0, 1.0, 0.7);
                set(CLOSED_HAT, 0.0, 0.8, 0.65);
                set(OPEN_HAT, 1.0, 0.6, 0.65);
                set(RIM, 0.0, 1.0, 0.6);
            }
            Style::Trap => {
                set(KICK, -2.0, 2.2, 0.95);
                set(SNARE, 1.0, 1.0, 0.85);
                set(CLAP, 0.0, 1.0, 0.8);
                set(CLOSED_HAT, 1.0, 0.7, 0.6);
            }
            Style::DrumAndBass => {
                set(KICK, 1.0, 0.6, 0.8);
                set(SNARE, 4.0, 0.8, 0.8);
                set(CLOSED_HAT, 2.0, 0.7, 0.6);
                set(RIDE, 0.0, 0.8, 0.5);
                set(CRASH, 0.0, 1.0, 0.5);
            }
            Style::BoomBap => {
                set(KICK, -1.0, 1.3, 0.95);
                set(SNARE, -2.0, 1.3, 0.85);
                set(CLOSED_HAT, -1.0, 1.0, 0.6);
            }
            Style::PopRock => {
                set(KICK, 0.0, 0.9, 0.8);
                set(SNARE, -1.0, 1.2, 0.85);
                set(CRASH, 0.0, 1.2, 0.55);
                set(RIDE, 0.0, 1.0, 0.5);
            }
            Style::Reggaeton => {
                set(KICK, 0.0, 1.5, 0.95);
                set(SNARE, 2.0, 0.8, 0.8);
                set(CLOSED_HAT, 0.0, 0.8, 0.55);
            }
            Style::Breakbeat => {
                set(KICK, 0.0, 0.7, 0.8);
                set(SNARE, 2.0, 0.9, 0.8);
                set(CRASH, 0.0, 1.0, 0.5);
            }
            Style::Hardgroove => {
                set(KICK, -1.0, 0.9, 0.9);
                set(CLAP, 0.0, 0.8, 0.6);
                set(CLOSED_HAT, 1.0, 0.6, 0.6);
                set(OPEN_HAT, 0.0, 0.5, 0.6);
                set(LOW_TOM, -2.0, 0.8, 0.7);
                set(MID_TOM, -2.0, 0.8, 0.65);
                set(HIGH_TOM, -2.0, 0.8, 0.6);
                set(CONGA, 0.0, 1.0, 0.6);
            }
            Style::HardTechno => {
                // Deep and long, and clipped: the kick carries the bass, and its
                // distortion is what cuts through.
                set(KICK, -3.0, 1.6, 0.7);
                set(CLAP, -1.0, 1.3, 0.75);
                set(CLOSED_HAT, 2.0, 0.6, 0.6);
                set(OPEN_HAT, 1.0, 0.7, 0.65);
                set(RIM, 1.0, 0.8, 0.55);
                p.drive[voice(KICK)] = 0.6;
            }
        }
        // The 909 hits harder than the 808: 2 dB down, so a full kit (a crash
        // over the kick and snare, a fill) stays under 0 dBFS.
        if p.model == engine::model::DrumModel::Tr909 {
            p.level.iter_mut().for_each(|l| *l *= 0.8);
        }
        p
    }
}

/// The sounds of a kit, by the note that plays them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kit {
    pub kick: Option<u8>,
    pub snare: Option<u8>,
    pub clap: Option<u8>,
    pub closed: Option<u8>,
    pub open: Option<u8>,
    /// The low tom (or the only one).
    pub tom: Option<u8>,
    pub mid_tom: Option<u8>,
    pub high_tom: Option<u8>,
    pub rim: Option<u8>,
    /// The cowbell, or a percussion with no other name.
    pub perc: Option<u8>,
    pub crash: Option<u8>,
    pub ride: Option<u8>,
    pub shaker: Option<u8>,
    pub tambourine: Option<u8>,
    pub conga: Option<u8>,
    pub clave: Option<u8>,
}

impl Kit {
    /// From a drum track's rows (note and name: a drum machine's sounds, a rack's
    /// samples).
    pub fn from_rows(rows: &[(u8, String)]) -> Self {
        let find = |words: &[&str], not: &[&str]| {
            rows.iter().find(|(_, name)| {
                let n = name.to_lowercase();
                words.iter().any(|w| n.contains(w)) && !not.iter().any(|w| n.contains(w))
            })
            .map(|r| r.0)
        };
        Self {
            kick: find(&["kick", "bombo", "bd", "bass drum"], &[]),
            snare: find(&["snare", "caja", "sd"], &[]),
            clap: find(&["clap", "palma"], &[]),
            closed: find(&["closed", "cerrad", "chh", "hat", "hh", "charles"], &["open", "abiert"]),
            open: find(&["open", "abiert", "ohh", "oh "], &[]),
            tom: find(&["low tom", "floor", "tom grave", "tom bajo"], &[]).or(find(&["tom"], &["mid", "medio", "high", "hi tom", "agudo", "alto"])),
            mid_tom: find(&["mid tom", "tom medio"], &[]),
            high_tom: find(&["high tom", "hi tom", "tom agudo", "tom alto"], &[]),
            rim: find(&["rim", "stick"], &[]),
            perc: find(&["cowbell", "cencerro", "perc"], &[]),
            crash: find(&["crash", "platillo"], &[]),
            ride: find(&["ride"], &[]),
            shaker: find(&["shaker", "maraca", "cabasa"], &[]),
            tambourine: find(&["tambourine", "pandereta", "tamb"], &[]),
            conga: find(&["conga", "bongo"], &[]),
            clave: find(&["clave", "wood"], &[]),
        }
    }

    /// What plays `sound` in this kit: its own, or the nearest it has.
    pub fn pitch(&self, sound: Sound) -> Option<u8> {
        match sound {
            Sound::Kick => self.kick,
            Sound::Snare => self.snare.or(self.clap),
            Sound::Clap => self.clap.or(self.snare),
            Sound::Hat => self.closed.or(self.open),
            Sound::Open => self.open,
            Sound::Ride => self.ride,
            Sound::Crash => self.crash,
            Sound::Rim => self.rim,
            Sound::LowTom => self.tom,
            Sound::MidTom => self.mid_tom.or(self.tom),
            Sound::HighTom => self.high_tom.or(self.mid_tom).or(self.tom),
            Sound::Shaker => self.shaker,
            Sound::Tambourine => self.tambourine,
            Sound::Cowbell => self.perc,
            Sound::Conga => self.conga,
            Sound::Clave => self.clave,
        }
    }
}

/// How busy a beat is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Basic,
    Medium,
    Intense,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Basic, Level::Medium, Level::Intense];

    pub fn label(self) -> &'static str {
        match self {
            Level::Basic => "Básico",
            Level::Medium => "Medio",
            Level::Intense => "Intenso",
        }
    }
}

/// A kit's sound in a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum V {
    Kick,
    /// The backbeat: the snare, or the clap in styles that clap.
    Back,
    Hat,
    Open,
    Rim,
    Perc,
    Tom,
}

/// A bar written out: for each sound sixteen steps, `.` silent, `X` accented,
/// `x` normal, `o` soft, `g` a ghost note, `r` a roll (two thirty-seconds),
/// `p` a soft note that plays only some times round (see `Note::chance`).
type Bar = &'static [(V, &'static str)];

/// A groove: its main bar, and the one that turns the phrase around (every
/// fourth bar).
struct Take {
    a: Bar,
    b: Bar,
}

use V::*;

const FOUR: &str = "X...X...X...X...";
const OFF: &str = "..x...x...x...x.";

/// The takes of a style at a level (one or more to choose among), written by
/// hand: what makes the style what it is, and what a drummer adds as it gets busier.
fn takes(style: Style, level: Level) -> &'static [Take] {
    match (style, level) {
        (Style::House, Level::Basic) => &[Take {
            a: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x.")],
            b: &[(Kick, FOUR), (Back, "....X.......X..o"), (Open, "..x...x...x...x.")],
        }],
        (Style::House, Level::Medium) => &[
            Take {
                a: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, ".g.g.g.g.g.g.g.g")],
                b: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, ".g.g.g.g.g.g.g.g"), (Rim, "......x.......x.")],
            },
            Take {
                a: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, "o.g.o.g.o.g.o.g."), (Rim, "...o.....o......")],
                b: &[(Kick, FOUR), (Back, "....X.......X..o"), (Open, "..x...x...x...x."), (Hat, "o.g.o.g.o.g.o.g."), (Rim, "...o.....o....o.")],
            },
        ],
        (Style::House, Level::Intense) => &[
            Take {
                a: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, ".g.o.g.o.g.o.g.o"), (Rim, "...o..o....o..o."), (Perc, "x..o..o...x..o..")],
                b: &[(Kick, FOUR), (Back, "....X.......X.o."), (Open, "..x...x...x...x."), (Hat, ".g.o.g.o.g.o.g.o"), (Rim, "...o..o....o..o."), (Perc, "x..o..o...x..ooo")],
            },
            Take {
                a: &[(Kick, FOUR), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, "g.ggg.ggg.ggg.gg"), (Perc, "..o..o.o..o..o.o")],
                b: &[(Kick, "X...X...X...X..g"), (Back, "....X.......X..."), (Open, "..x...x...x...x."), (Hat, "g.ggg.ggg.ggg.gg"), (Perc, "..o..o.o..o..o.o")],
            },
        ],
        // Techno: the kick on every beat, the open hat on the off-beats, soft
        // closed hats between, the clap on 2 and 4; rims as it gets busier. No
        // cowbell: it rings like a ride.
        (Style::Techno, Level::Basic) => &[Take {
            a: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Back, "....o.......o...")],
            b: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Back, "....o.......o...")],
        }],
        (Style::Techno, Level::Medium) => &[
            Take {
                a: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Hat, ".g.g.g.g.g.g.g.g"), (Back, "....x.......x...")],
                b: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Hat, ".g.g.g.g.g.g.g.g"), (Back, "....x.......x..."), (Rim, "...........o..o.")],
            },
            Take {
                a: &[(Kick, FOUR), (Hat, "g.x.g.x.g.x.g.x."), (Back, "....x.......x...")],
                b: &[(Kick, FOUR), (Hat, "g.x.g.x.g.x.g..."), (Back, "....x.......x..."), (Open, "..............x.")],
            },
        ],
        (Style::Techno, Level::Intense) => &[
            Take {
                a: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Hat, ".g.o.g.o.g.o.g.o"), (Back, "....x.......x..."), (Rim, "...o..o....o....")],
                b: &[(Kick, FOUR), (Open, "..x...x...x...x."), (Hat, ".g.o.g.o.g.o.g.o"), (Back, "....x.......x..."), (Rim, "...o..o....o..o.")],
            },
            Take {
                a: &[(Kick, "X...X...X...X..g"), (Hat, "ggxgggxgggxgggxg"), (Back, "....x.......x..."), (Rim, "......o.......o.")],
                b: &[(Kick, "X...X...X...X.gg"), (Hat, "ggxgggxgggxggg.."), (Open, "..............x."), (Back, "....x.......x..."), (Rim, "......o.......o.")],
            },
        ],
        (Style::Trap, Level::Basic) => &[Take {
            a: &[(Kick, "X.........X....."), (Back, "........X......."), (Hat, "x.x.x.x.x.x.x.x.")],
            b: &[(Kick, "X.........X..x.."), (Back, "........X......."), (Hat, "x.x.x.x.x.x.x.x.")],
        }],
        (Style::Trap, Level::Medium) => &[
            Take {
                a: &[(Kick, "X.....x...X....."), (Back, "........X......."), (Hat, "xoxoxoxoxoxoxoxo")],
                b: &[(Kick, "X.....x.x.X....."), (Back, "........X......."), (Hat, "xoxoxoxoxoxoxorr")],
            },
            Take {
                a: &[(Kick, "X..x......X....."), (Back, "........X......."), (Hat, "x.xox.xox.xox.xo"), (Open, "......o.........")],
                b: &[(Kick, "X..x......X..x.."), (Back, "........X......."), (Hat, "x.xox.xox.xoxrrr"), (Open, "......o.........")],
            },
        ],
        (Style::Trap, Level::Intense) => &[
            Take {
                a: &[(Kick, "X..x..x...X..x.."), (Back, "........X......."), (Hat, "xoxoxorrxoxoxorr")],
                b: &[(Kick, "X..x..x...X....."), (Back, "........X.....g."), (Hat, "xoxoxorrxoxorrrr")],
            },
            Take {
                a: &[(Kick, "X.....xX..X....x"), (Back, "........X......."), (Hat, "xoxoxoxoxorrxoxo"), (Open, "..............o.")],
                b: &[(Kick, "X.....xX..X.x..."), (Back, "........X......g"), (Hat, "xoxoxoxoxorrxrrr"), (Open, "..............o.")],
            },
        ],
        (Style::DrumAndBass, Level::Basic) => &[Take {
            a: &[(Kick, "X.........X....."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
            b: &[(Kick, "X.........X..x.."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
        }],
        (Style::DrumAndBass, Level::Medium) => &[
            Take {
                a: &[(Kick, "X.........X....."), (Back, "....X..g....X..g"), (Hat, "x.xox.xox.xox.xo")],
                b: &[(Kick, "X.......x.X....."), (Back, "....X..g....X..g"), (Hat, "x.xox.xox.xox.xo")],
            },
            Take {
                a: &[(Kick, "X.....x...X....."), (Back, "....X......gX..."), (Hat, "xoxoxoxoxoxoxoxo")],
                b: &[(Kick, "X.....x...X....."), (Back, "....X......gX..."), (Hat, "xoxoxoxoxoxoxo.."), (Open, "..............x.")],
            },
        ],
        (Style::DrumAndBass, Level::Intense) => &[
            Take {
                a: &[(Kick, "X.....x...X..x.."), (Back, "....X..g.g..X..g"), (Hat, "xoxoxoxoxoxoxoxo"), (Rim, "...g......g.....")],
                b: &[(Kick, "X.....x...X..x.."), (Back, "....X..g.g..X.gX"), (Hat, "xoxoxoxoxoxoxoxo"), (Rim, "...g......g.....")],
            },
            Take {
                a: &[(Kick, "X.X.......XX...."), (Back, "....X..g.X..X..g"), (Hat, "x.x.x.x.x.x.x.x."), (Open, "..............o.")],
                b: &[(Kick, "X.X.......X..x.."), (Back, "....X..g.X..X.gX"), (Hat, "x.x.x.x.x.x.x.x."), (Open, "..............o.")],
            },
        ],
        (Style::BoomBap, Level::Basic) => &[Take {
            a: &[(Kick, "X.........X....."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
            b: &[(Kick, "X......x..X....."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
        }],
        (Style::BoomBap, Level::Medium) => &[
            Take {
                a: &[(Kick, "X......x..X....."), (Back, "....X.......X..."), (Hat, "X.x.X.x.X.x.X.x.")],
                b: &[(Kick, "X......x..X..x.."), (Back, "....X.......X..g"), (Hat, "X.x.X.x.X.x.X.x.")],
            },
            Take {
                a: &[(Kick, "X.x.......X....."), (Back, "....X.......X..."), (Hat, "x.xox.xox.xox.xo"), (Open, "..............o.")],
                b: &[(Kick, "X.x.......X.x..."), (Back, "....X.......X..."), (Hat, "x.xox.xox.xox.xo"), (Open, "..............o.")],
            },
        ],
        (Style::BoomBap, Level::Intense) => &[
            Take {
                a: &[(Kick, "X.x....x..X..x.."), (Back, "....X..g....X.g."), (Hat, "x.xox.xox.xox.xo"), (Rim, "......g.........")],
                b: &[(Kick, "X.x....x..X..x.x"), (Back, "....X..g....X.gX"), (Hat, "x.xox.xox.xox.xo"), (Rim, "......g.........")],
            },
            Take {
                a: &[(Kick, "X..x...x.xX....."), (Back, "....X......gX..g"), (Hat, "xoxoxoxoxoxoxoxo"), (Open, "......o.........")],
                b: &[(Kick, "X..x...x.xX..x.."), (Back, "....X......gX.gg"), (Hat, "xoxoxoxoxoxoxoxo"), (Open, "......o.........")],
            },
        ],
        (Style::PopRock, Level::Basic) => &[Take {
            a: &[(Kick, "X.......X......."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
            b: &[(Kick, "X.......X.x....."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
        }],
        (Style::PopRock, Level::Medium) => &[
            Take {
                a: &[(Kick, "X.....x.X.x....."), (Back, "....X.......X..."), (Hat, "X.x.X.x.X.x.X.x.")],
                b: &[(Kick, "X.....x.X.x....."), (Back, "....X.......X..g"), (Hat, "X.x.X.x.X.x.X.x.")],
            },
            Take {
                a: &[(Kick, "X.......X.x...x."), (Back, "....X.......X..."), (Hat, "xoxoxoxoxoxoxoxo")],
                b: &[(Kick, "X.......X.x...x."), (Back, "....X.......X..."), (Hat, "xoxoxoxoxoxoxo.."), (Open, "..............x.")],
            },
        ],
        (Style::PopRock, Level::Intense) => &[
            Take {
                a: &[(Kick, "X.....x.X.x...x."), (Back, "....X..g....X..g"), (Hat, "xoxoxoxoxoxoxoxo"), (Open, "......x.......x.")],
                b: &[(Kick, "X.....x.X.x...x."), (Back, "....X..g....X.gX"), (Hat, "xoxoxoxoxoxoxoxo"), (Open, "......x.......x.")],
            },
            Take {
                a: &[(Kick, "X.x...x.X.x..x.."), (Back, "....X.......X..."), (Hat, "X.x.X.x.X.x.X.x."), (Open, "..............x.")],
                b: &[(Kick, "X.x...x.X.x..x.."), (Back, "....X.......X.gX"), (Hat, "X.x.X.x.X.x.X.x."), (Open, "..............x.")],
            },
        ],
        (Style::Reggaeton, Level::Basic) => &[Take {
            a: &[(Kick, FOUR), (Back, "...x..x....x..x.")],
            b: &[(Kick, FOUR), (Back, "...x..x....x..x.")],
        }],
        (Style::Reggaeton, Level::Medium) => &[
            Take {
                a: &[(Kick, FOUR), (Back, "...x..x....x..x."), (Hat, "o.o.o.o.o.o.o.o.")],
                b: &[(Kick, FOUR), (Back, "...x..x....x..xx"), (Hat, "o.o.o.o.o.o.o.o.")],
            },
            Take {
                a: &[(Kick, FOUR), (Back, "...x..x....x..x."), (Perc, "..o...o...o...o.")],
                b: &[(Kick, FOUR), (Back, "...x..x....x..x."), (Perc, "..o...o...o.o.o.")],
            },
        ],
        (Style::Reggaeton, Level::Intense) => &[
            Take {
                a: &[(Kick, FOUR), (Back, "...x..x....x..x."), (Hat, "xoxoxoxoxoxoxoxo"), (Perc, ".o..o.o..o..o.o."), (Open, "..............o.")],
                b: &[(Kick, FOUR), (Back, "...x..x....x..xx"), (Hat, "xoxoxoxoxoxoxoxo"), (Perc, ".o..o.o..o..o.o."), (Open, "..............o.")],
            },
            Take {
                a: &[(Kick, "X...X...X...X.g."), (Back, "...x..x....x..x."), (Hat, "x.xox.xox.xox.xo"), (Rim, "......g.......g.")],
                b: &[(Kick, "X...X...X...X.g."), (Back, "...x..x....x..x."), (Hat, "x.xox.xox.xox.xo"), (Rim, "......g.....g.g.")],
            },
        ],
        (Style::Breakbeat, Level::Basic) => &[Take {
            a: &[(Kick, "X.........X....."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
            b: &[(Kick, "X.........X..x.."), (Back, "....X.......X..."), (Hat, "x.x.x.x.x.x.x.x.")],
        }],
        (Style::Breakbeat, Level::Medium) => &[
            Take {
                a: &[(Kick, "X.X.......X....."), (Back, "....X..g.g..X..g"), (Hat, "x.x.x.x.x.x.x.x.")],
                b: &[(Kick, "X.X.......X..x.."), (Back, "....X..g.g..X..g"), (Hat, "x.x.x.x.x.x.x.x.")],
            },
            Take {
                a: &[(Kick, "X.........XX...."), (Back, "....X..g....X..."), (Hat, "xoxoxoxoxoxoxoxo")],
                b: &[(Kick, "X.........XX...."), (Back, "....X..g....X.gg"), (Hat, "xoxoxoxoxoxoxoxo")],
            },
        ],
        // Hardgroove: the kick on every beat and the open hat between, with
        // toms and congas rolling over them; hats and percussion that come and
        // go (some only some times round), a light swing.
        (Style::Hardgroove, Level::Basic) => &[Take {
            a: &[(Kick, FOUR), (Open, OFF), (Back, "....o.......o..."), (Perc, "..o..o....o..o..")],
            b: &[(Kick, FOUR), (Open, OFF), (Back, "....o.......o..."), (Perc, "..o..o....o..ooo")],
        }],
        (Style::Hardgroove, Level::Medium) => &[
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "p.g.p.g.p.g.p.g."), (Back, "....x.......x..."), (Tom, "...o..o....o..o."), (Perc, "o..o..o.o..o..o.")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "p.g.p.g.p.g.p.g."), (Back, "....x.......x..."), (Tom, "...o..o....o.oo."), (Perc, "o..o..o.o..o.ooo")],
            },
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxpxp"), (Rim, "...o.....o..o..."), (Perc, "..o...o...o..oo.")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxpxp"), (Rim, "...o.....o..o.oo"), (Perc, "..o...o...o..oo."), (Back, "............o...")],
            },
        ],
        (Style::Hardgroove, Level::Intense) => &[
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "pgpgpgpgpgpgpgpg"), (Back, "....x..g....x..g"), (Tom, "o..o..o..o..o.o."), (Perc, ".o.oo.o..o.oo.o."), (Rim, "......p......p..")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "pgpgpgpgpgpgpgpg"), (Back, "....x..g....x.gg"), (Tom, "o..o..o..o..oooo"), (Perc, ".o.oo.o..o.oo.o."), (Rim, "......p......p..")],
            },
            Take {
                a: &[(Kick, "X...X...X...X.g."), (Open, OFF), (Hat, "xpxxxpxxxpxxxpxx"), (Back, "....x.......x..."), (Tom, "..o..o..o..o..o."), (Perc, "o.p.o.p.o.p.o.p.")],
                b: &[(Kick, "X...X...X...X.gg"), (Open, OFF), (Hat, "xpxxxpxxxpxxxp.."), (Back, "....x.......x.o."), (Tom, "..o..o..o..o.ooo"), (Perc, "o.p.o.p.o.p.o.p.")],
            },
        ],
        // Hard techno: a hard kick on every beat (galloping as it gets busier),
        // the open hat between, claps on 2 and 4 that roll at the turnaround.
        (Style::HardTechno, Level::Basic) => &[Take {
            a: &[(Kick, FOUR), (Open, OFF), (Back, "....X.......X...")],
            b: &[(Kick, FOUR), (Open, OFF), (Back, "....X.......X.xx")],
        }],
        (Style::HardTechno, Level::Medium) => &[
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "gxg.gxg.gxg.gxg."), (Back, "....X.......X...")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "gxg.gxg.gxg.gxg."), (Back, "....X.......X.xx")],
            },
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxpxp"), (Back, "....X.......X..."), (Rim, "..p.......p.....")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxprr"), (Back, "....X.......X..."), (Rim, "..p.......p..o.o")],
            },
        ],
        (Style::HardTechno, Level::Intense) => &[
            Take {
                a: &[(Kick, "X..xX...X..xX..."), (Open, OFF), (Hat, "xxpxxxpxxxpxxxpx"), (Back, "....X..o....X..o"), (Rim, "..p.......p..p..")],
                b: &[(Kick, "X..xX...X..xX.xx"), (Open, OFF), (Hat, "xxpxxxpxxxpxxxpx"), (Back, "....X..o....X.rr"), (Rim, "..p.......p..p..")],
            },
            Take {
                a: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxpxp"), (Back, "..o.X.o...o.X.o.")],
                b: &[(Kick, FOUR), (Open, OFF), (Hat, "xpxpxpxpxpxpxpxp"), (Back, "..o.X.o...o.Xrrr")],
            },
        ],
        (Style::Breakbeat, Level::Intense) => &[
            Take {
                a: &[(Kick, "X.X.......XX...."), (Back, "....X..g.X..X..g"), (Hat, "x.x.x.x.x.x.x.x."), (Open, "..............x.")],
                b: &[(Kick, "X.X.......X..x.."), (Back, "....X..g.X..X.gX"), (Hat, "x.x.x.x.x.x.x.x."), (Open, "..............x.")],
            },
            Take {
                a: &[(Kick, "X.X....x..X..x.."), (Back, "....X..x.x..X..g"), (Hat, "xoxoxoxoxoxoxoxo")],
                b: &[(Kick, "X.X....x..X..x.."), (Back, "....X..x.x..X.gX"), (Hat, "xoxoxoxoxoxoxo.."), (Open, "..............x.")],
            },
        ],
    }
}

/// How a style ends a phrase, over its last beat: what plays (the rest of the
/// beat is silent but the kick, unless it says otherwise), or nothing.
fn fill(style: Style) -> Option<(Bar, bool)> {
    // (The bar's steps 12–15 only; `true`: the kick stops too.)
    match style {
        Style::House => Some((&[(Back, "............oxXX")], false)),
        Style::Techno => Some((&[(Open, "..............x."), (Back, "...............o")], false)),
        Style::Trap => Some((&[(Hat, "............rrrr"), (Back, "............X...")], true)),
        Style::DrumAndBass => Some((&[(Back, "............gXgX")], false)),
        Style::BoomBap => Some((&[(Back, "............X.gX"), (Kick, "..............X.")], true)),
        Style::PopRock => Some((&[(Back, "............xX.."), (Tom, "..............xX")], false)),
        Style::Reggaeton => None,
        Style::Breakbeat => Some((&[(Back, "............gXXX")], false)),
        Style::Hardgroove => Some((&[(Tom, "............xoXx"), (Perc, "............o.o.")], false)),
        // A clap roll over the kick into the next phrase.
        Style::HardTechno => Some((&[(Back, "............rrrr")], false)),
    }
}

/// How a style swings, if it does.
fn swing(style: Style, level: Level) -> Option<engine::model::Groove> {
    use engine::model::Groove;
    match (style, level) {
        (Style::BoomBap, _) => Some(Groove { grid: 0.25, swing: 0.5, accent: 0.0, humanize: 0.3, template: None }),
        (Style::House, Level::Medium | Level::Intense) => Some(Groove { grid: 0.25, swing: 0.15, accent: 0.0, humanize: 0.15, template: None }),
        (Style::Hardgroove, Level::Medium | Level::Intense) => Some(Groove { grid: 0.25, swing: 0.2, accent: 0.0, humanize: 0.1, template: None }),
        _ => None,
    }
}

/// How a beat is made.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub style: Style,
    pub level: Level,
    /// A fill to end each phrase of eight bars (or the clip, of four or more).
    pub fills: bool,
    /// The kick plays with the bass (where it falls on eighths).
    pub follow_bass: bool,
    /// Bars the clip is made (it takes four for its turnaround, eight for a fill).
    pub bars: u32,
    /// A drum machine is switched to the style's machine (808 or 909).
    pub machine: bool,
    /// The drum model plays the clip, carrying the written beat on its way.
    pub ai: bool,
    /// The sounds it may use (the style's, unless changed).
    pub sounds: Sounds,
}

impl Style {
    /// The machine the style is played on.
    pub fn machine(self) -> engine::model::DrumModel {
        use engine::model::DrumModel::*;
        match self {
            Style::Trap | Style::BoomBap | Style::Reggaeton => Tr808,
            Style::House | Style::Techno | Style::Hardgroove | Style::HardTechno | Style::Breakbeat | Style::DrumAndBass | Style::PopRock => Tr909,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { style: Style::House, level: Level::Medium, fills: true, follow_bass: false, bars: 4, machine: true, ai: false, sounds: Style::House.sounds() }
    }
}

/// A small, repeatable random sequence (for a touch of humanity in the velocities).
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn velocity(c: char) -> Option<f32> {
    match c {
        'X' => Some(1.0),
        'x' | 'r' => Some(0.8),
        'o' => Some(0.6),
        'g' => Some(0.38),
        'p' => Some(0.5),
        _ => None,
    }
}

/// A beat `length` beats long for `kit`, and the swing it wants. `take` picks
/// among the style's grooves (each a different take); `bass` are the beats (in
/// the clip) where the bass starts its notes, for `follow_bass`.
pub fn generate(kit: &Kit, settings: &Settings, length: f64, bass: &[f64], take: u64) -> (Vec<Note>, Option<engine::model::Groove>) {
    let options = takes(settings.style, settings.level);
    let groove = &options[take as usize % options.len()];
    let pitch = |v: V| sound_of(settings, v).and_then(|s| kit.pitch(s));
    let mut dice = Dice(take.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let bars = (length / 4.0).ceil().max(1.0) as usize;
    let mut notes = Vec::new();
    for bar in 0..bars {
        let start = bar as f64 * 4.0;
        // A A A B: the fourth bar of each four turns the phrase around.
        let written = if bar % 4 == 3 { groove.b } else { groove.a };
        // The phrase's end (every eight bars, or the clip's if it has four or more).
        let phrase_end = bars >= 4 && (bar + 1 == bars || (bar + 1) % 8 == 0);
        let filled = if settings.fills && phrase_end { fill(settings.style) } else { None };
        let mut lines: Vec<(V, &str)> = written.to_vec();
        if let Some((fill_bar, kick_stops)) = filled {
            // The fill's beat: its sounds, and nothing else but the kick.
            for line in &mut lines {
                if line.0 != Kick || kick_stops {
                    line.1 = &line.1[..12];
                }
            }
            lines.extend(fill_bar.iter().copied());
        }
        // Where the open hat plays, the closed one doesn't (it would choke it).
        let open_steps: Vec<usize> = lines.iter().filter(|l| l.0 == Open).flat_map(|l| l.1.char_indices().filter(|c| velocity(c.1).is_some()).map(|c| c.0)).collect();
        for &(voice, steps) in &lines {
            let Some(pitch) = pitch(voice) else { continue };
            for (s, c) in steps.char_indices() {
                let Some(v) = velocity(c) else { continue };
                if voice == Hat && open_steps.contains(&s) {
                    continue;
                }
                let beat = start + s as f64 * 0.25;
                if beat >= length - 1e-9 {
                    continue;
                }
                // Hats a little under the rest, and every hit a hair different.
                let level = if matches!(voice, Hat | Open) { 0.85 } else { 1.0 };
                let v = (v * level * (0.96 + 0.08 * dice.roll())).clamp(0.05, 1.0);
                // Some times round only.
                let chance = if c == 'p' { 0.55 } else { 1.0 };
                if c == 'r' {
                    notes.push(Note { start: beat, length: 0.125, pitch, velocity: v, chance: 1.0 });
                    notes.push(Note { start: beat + 0.125, length: 0.125, pitch, velocity: v * 0.85, chance: 1.0 });
                } else {
                    notes.push(Note { start: beat, length: 0.25, pitch, velocity: v, chance });
                }
            }
        }
    }
    crash_phrases(kit, settings, &mut notes, length);
    follow_bass(kit, settings, &mut notes, bass, length);
    (notes, swing(settings.style, settings.level))
}

/// Which of the chosen sounds plays a written line: the backbeat on the clap
/// or the snare (as the style likes), the percussion on what there is.
fn sound_of(settings: &Settings, v: V) -> Option<Sound> {
    let on = |s: Sound| settings.sounds[s as usize];
    let first = |list: &[Sound]| list.iter().copied().find(|&s| on(s));
    let back_clap = matches!(settings.style, Style::House | Style::Techno | Style::Hardgroove | Style::HardTechno | Style::Trap);
    match v {
        Kick => first(&[Sound::Kick]),
        Back if back_clap => first(&[Sound::Clap, Sound::Snare, Sound::Rim]),
        Back => first(&[Sound::Snare, Sound::Clap, Sound::Rim]),
        Hat => first(&[Sound::Hat, Sound::Shaker]),
        Open => first(&[Sound::Open, Sound::Hat]),
        Rim => first(&[Sound::Rim]),
        Perc => first(&[Sound::Cowbell, Sound::Conga, Sound::Shaker]),
        Tom => first(&[Sound::LowTom, Sound::Snare]),
    }
}

/// A crash where each phrase starts (every eight bars, from the first), if
/// it's chosen and the clip is long enough for it not to crash every bar.
fn crash_phrases(kit: &Kit, settings: &Settings, notes: &mut Vec<Note>, length: f64) {
    let Some(crash) = kit.pitch(Sound::Crash).filter(|_| settings.sounds[Sound::Crash as usize]) else { return };
    if length < 16.0 {
        return;
    }
    let mut beat = 0.0;
    while beat < length {
        notes.push(Note { start: beat, length: 0.25, pitch: crash, velocity: 0.9, chance: 1.0 });
        beat += 32.0;
    }
}

/// With the bass: a kick where it starts a note on an eighth and none is there.
fn follow_bass(kit: &Kit, settings: &Settings, notes: &mut Vec<Note>, bass: &[f64], length: f64) {
    if settings.follow_bass {
        if let Some(kick) = kit.kick {
            for &b in bass {
                let on_eighth = ((b * 2.0) - (b * 2.0).round()).abs() < 1e-6;
                let taken = notes.iter().any(|n| n.pitch == kick && (n.start - b).abs() < 0.1);
                if on_eighth && !taken && b < length {
                    notes.push(Note { start: b, length: 0.25, pitch: kick, velocity: 0.85, chance: 1.0 });
                }
            }
        }
    }
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
    notes.dedup_by(|a, b| a.pitch == b.pitch && (a.start - b.start).abs() < 1e-9);
}

/// The drum model, loaded the first time it plays.
static WRITER: std::sync::Mutex<Option<stems::drums::DrumWriter>> = std::sync::Mutex::new(None);

/// A beat the drum model plays: it hears the style's written beat (four bars,
/// A A A B, the `take`-th groove) and carries it on for the whole clip, a
/// drummer's way, with its own variations and fills where phrases end. Every
/// `take` is another performance. Plainer levels play it safer.
pub fn generate_ai(kit: &Kit, settings: &Settings, length: f64, bass: &[f64], take: u64) -> Result<(Vec<Note>, Option<engine::model::Groove>), String> {
    use stems::drums::VOICES;
    let options = takes(settings.style, settings.level);
    let groove = &options[take as usize % options.len()];
    let mut seed = vec![[0u8; VOICES]; 64];
    for bar in 0..4 {
        for &(v, steps) in if bar == 3 { groove.b } else { groove.a } {
            let Some(sound) = sound_of(settings, v) else { continue };
            for (s, c) in steps.char_indices() {
                let level = match c {
                    'X' => 3,
                    'x' | 'o' | 'r' => 2,
                    'g' | 'p' => 1,
                    _ => continue,
                };
                let cell = &mut seed[bar * 16 + s][sound as usize];
                *cell = (*cell).max(level);
            }
        }
    }
    extras(settings, &mut seed);
    let playable = stems::drums::ALL.map(|s| settings.sounds[s as usize] && kit.pitch(s).is_some());
    let heat = match settings.level {
        Level::Basic => 0.85,
        Level::Medium => 1.0,
        Level::Intense => 1.1,
    };
    let steps = (length * 4.0).ceil() as usize;
    let played = {
        let mut writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
        if writer.is_none() {
            *writer = Some(stems::drums::DrumWriter::load()?);
        }
        writer.as_mut().unwrap().play_on(&seed, steps, heat, playable, take)?
    };
    let mut dice = Dice(take.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut notes = Vec::new();
    for (s, step) in played.iter().enumerate() {
        for v in stems::drums::ALL {
            let level = step[v as usize];
            // Where the open hat plays, the closed one doesn't (it would choke it).
            if level == 0 || (v == Sound::Hat && step[Sound::Open as usize] > 0) {
                continue;
            }
            let Some(pitch) = kit.pitch(v) else { continue };
            let base = [0.0, 0.38, 0.8, 1.0][level as usize];
            let base = if matches!(v, Sound::Hat | Sound::Open | Sound::Ride | Sound::Shaker) { base * 0.85 } else { base };
            let velocity = (base * (0.96 + 0.08 * dice.roll())).clamp(0.05, 1.0);
            notes.push(Note { start: s as f64 * 0.25, length: 0.25, pitch, velocity, chance: 1.0 });
        }
    }
    // The kick the written beat has on the beat stays (a four on the floor
    // the model left a gap in would not be the style); the rest is the model's.
    if let Some(kick) = kit.pitch(Sound::Kick).filter(|_| playable[Sound::Kick as usize]) {
        for s in (0..steps).step_by(4) {
            let written = seed[s % seed.len()][Sound::Kick as usize] >= 2;
            let start = s as f64 * 0.25;
            if written && start < length && !notes.iter().any(|n| n.pitch == kick && (n.start - start).abs() < 1e-9) {
                notes.push(Note { start, length: 0.25, pitch: kick, velocity: 0.8 * (0.96 + 0.08 * dice.roll()), chance: 1.0 });
            }
        }
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    follow_bass(kit, settings, &mut notes, bass, length);
    Ok((notes, swing(settings.style, settings.level)))
}

/// The chosen sounds the written grooves leave out, put into the four bars
/// the model starts from: it carries on with the sounds it hears, so
/// without them it would never play them. A tom fill ends the fourth bar;
/// from Medium a tambourine on the backbeat and a shaker on the sixteenths;
/// at Intense congas and a cowbell too.
fn extras(settings: &Settings, seed: &mut [stems::drums::Step]) {
    let on = |s: Sound| settings.sounds[s as usize];
    let toms: Vec<Sound> = [Sound::HighTom, Sound::MidTom, Sound::LowTom].into_iter().filter(|&t| on(t)).collect();
    if settings.fills && !toms.is_empty() {
        for (i, step) in seed[60..64].iter_mut().enumerate() {
            *step = [0; stems::drums::VOICES];
            step[toms[i * toms.len() / 4] as usize] = if i == 0 { 3 } else { 2 };
        }
    }
    let mut lay = |sound: Sound, line: &str| {
        if !on(sound) || seed.iter().any(|s| s[sound as usize] > 0) {
            return;
        }
        // Not over the fill.
        for (i, step) in seed[..60].iter_mut().enumerate() {
            let level = match line.as_bytes()[i % 16] {
                b'x' => 2,
                b'g' => 1,
                _ => 0,
            };
            step[sound as usize] = level;
        }
    };
    if settings.level != Level::Basic {
        lay(Sound::Tambourine, "....x.......x...");
        lay(Sound::Shaker, "gxgxgxgxgxgxgxgx");
    }
    if settings.level == Level::Intense {
        lay(Sound::Conga, "......x..x...x.x");
        lay(Sound::Cowbell, "..x...x...x...x.");
    }
}

/// How many takes a style has at a level.
pub fn take_count(style: Style, level: Level) -> usize {
    takes(style, level).len()
}

/// Where the bass tracks start notes over a clip's span: the session clips in the
/// same scene, or the arrangement clips under it (in its beats).
pub fn bass_onsets(tracks: &[Track], slot: Option<usize>, span: Option<(f64, f64)>, length: f64) -> Vec<f64> {
    let mut onsets = Vec::new();
    for track in tracks.iter().filter(|t| crate::arrange::role(t) == crate::arrange::Role::Bass) {
        if let Some(slot) = slot {
            if let Some(engine::model::Clip::Midi(m)) = &track.clips[slot] {
                let mut lap = 0.0;
                while lap < length && m.length > 0.0 {
                    onsets.extend(m.notes.iter().map(|n| lap + n.start).filter(|&b| b < length));
                    lap += m.length;
                }
            }
        }
        if let Some((from, to)) = span {
            for placed in &track.arrangement {
                let engine::model::Clip::Midi(m) = &placed.clip else { continue };
                if m.length <= 0.0 {
                    continue;
                }
                let mut lap = placed.start;
                while lap < placed.end().min(to) {
                    for n in &m.notes {
                        let b = lap + n.start;
                        if b >= from && b < to && b < placed.end() {
                            onsets.push(b - from);
                        }
                    }
                    lap += m.length;
                }
            }
        }
    }
    onsets.sort_by(f64::total_cmp);
    onsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::model::drum_note;

    fn machine() -> Kit {
        let rows: Vec<(u8, String)> = engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect();
        Kit::from_rows(&rows)
    }

    /// A beat of `style` on its machine, at its tempo, eight bars, to a WAV in
    /// `dir`; written, or played by the drum model (`ai`).
    fn beat_to_wav(dir: &std::path::Path, style: Style, level: Level, take: u64, ai: bool) -> String {
        let mut project = crate::project::Project::empty();
        project.tempo = style.tempo() as f64;
        let params = style.sound();
        let mut track = crate::project::Track::new("Drums", [0; 3], engine::model::InstrumentKind::DrumMachine(params));
        let settings = Settings { style, level, fills: true, follow_bass: false, bars: 8, machine: true, ai, sounds: style.sounds() };
        let (notes, groove) = if ai { generate_ai(&machine(), &settings, 32.0, &[], take).unwrap() } else { generate(&machine(), &settings, 32.0, &[], take) };
        let clip = engine::model::MidiClip { notes, groove, ..engine::model::MidiClip::new("Beat", 32.0, [0; 3]) };
        track.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 32.0, clip: engine::model::Clip::Midi(clip) });
        project.tracks = vec![track];
        let mut samples = crate::sync::SampleCache::default();
        let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
        let (l, r) = crate::export::render(commands, (0.0, 32.0), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap();
        let by = if ai { " IA" } else { "" };
        let name = format!("{} {} {}{by} v{}.wav", style.label().replace(['/', '&'], "y"), level.label(), style.machine().label(), take + 1);
        crate::export::write(&dir.join(&name), &l, &r, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
        let peak = l.iter().chain(&r).fold(0.0f32, |p, s| p.max(s.abs()));
        format!("{name}: pico {:.1} dBFS", 20.0 * peak.log10())
    }

    /// The Hard techno machine's kick is clipped: as much presence (1–4 kHz
    /// against its lows) as the Kick Designer's «Hard · Kick», a little louder
    /// than it was clean. The other styles stay clean.
    #[test]
    fn the_hard_techno_kick_is_distorted() {
        let measure = |p: engine::model::DrumParams| {
            let (hit, _) = engine::render_drum_hit(p, engine::model::drum_note::KICK, 48_000.0);
            // Four on the floor at 150, each kick cutting the last.
            let beat = (60.0 / 150.0 * 48_000.0) as usize;
            let mut l = vec![0.0f32; 32 * beat];
            for b in 0..32 {
                let n = hit.len().min(beat);
                l[b * beat..b * beat + n].copy_from_slice(&hit[..n]);
            }
            let bands = stems::loudness::bands(&l, &l, 48_000.0);
            let mean = |b: &[f64]| 10.0 * (b.iter().map(|d| 10f64.powf(d / 10.0)).sum::<f64>() / b.len() as f64).log10();
            (mean(&bands[0..2]) - mean(&bands[5..8]), stems::loudness::integrated(&l, &l, 48_000.0).unwrap())
        };
        let hard = Style::HardTechno.sound();
        // As it was: clean, at 0.95 (less the 909's 2 dB).
        let mut clean = engine::model::DrumParams { drive: [0.0; engine::model::DRUM_SOUNDS], ..hard };
        clean.level[0] = 0.95 * 0.8;
        let ((presence, lufs), (clean_presence, clean_lufs)) = (measure(hard), measure(clean));
        println!("bombo Hard techno: graves frente a 1–4 kHz {presence:.1} dB ({clean_presence:.1} antes), {lufs:.1} LUFS ({clean_lufs:.1} antes)");
        assert!(presence < 22.0 && presence < clean_presence - 6.0, "{presence:.1} vs {clean_presence:.1}");
        assert!(lufs > clean_lufs && lufs < clean_lufs + 3.0, "{lufs:.1} vs {clean_lufs:.1}");
        assert!(Style::ALL.iter().filter(|&&s| s != Style::HardTechno).all(|s| s.sound().drive == [0.0; engine::model::DRUM_SOUNDS]));
    }

    /// Renders beats to WAVs in `PULSE_OUT` (for listening): every level and take
    /// of the styles in `PULSE_STYLES` (labels, comma-separated), or one of each.
    /// With `PULSE_AI` set, the drum model plays them, that many takes each.
    #[test]
    #[ignore]
    fn beats_to_wav() {
        let Ok(dir) = std::env::var("PULSE_OUT") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wanted = std::env::var("PULSE_STYLES").unwrap_or_default();
        let ai_takes: u64 = std::env::var("PULSE_AI").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
        for style in Style::ALL {
            if ai_takes > 0 {
                if wanted.is_empty() || wanted.split(',').any(|w| w.trim().eq_ignore_ascii_case(style.label())) {
                    for take in 0..ai_takes {
                        println!("{}", beat_to_wav(&dir, style, Level::Medium, take, true));
                    }
                }
                continue;
            }
            if !wanted.is_empty() {
                if !wanted.split(',').any(|w| w.trim().eq_ignore_ascii_case(style.label())) {
                    continue;
                }
                for level in Level::ALL {
                    for take in 0..take_count(style, level) as u64 {
                        println!("{}", beat_to_wav(&dir, style, level, take, false));
                    }
                }
            } else {
                println!("{}", beat_to_wav(&dir, style, Level::Medium, 0, false));
            }
        }
    }

    #[test]
    fn the_bass_of_the_same_scene_is_found() {
        let demo = crate::project::Project::demo();
        let onsets = bass_onsets(&demo.tracks, Some(0), None, 4.0);
        assert!(!onsets.is_empty() && onsets.iter().all(|&b| (0.0..4.0).contains(&b)), "{onsets:?}");
        // The second scene has no bass.
        assert!(bass_onsets(&demo.tracks, Some(1), None, 4.0).is_empty());
    }

    #[test]
    fn kits_are_read_from_their_names() {
        let kit = machine();
        assert_eq!(kit.kick, Some(drum_note::KICK));
        assert_eq!(kit.snare, Some(drum_note::SNARE));
        assert_eq!(kit.clap, Some(drum_note::CLAP));
        assert_eq!(kit.closed, Some(drum_note::CLOSED_HAT));
        assert_eq!(kit.open, Some(drum_note::OPEN_HAT));
        // A rack named in Spanish.
        let rack = Kit::from_rows(&[(36, "Bombo 909".into()), (37, "Caja seca".into()), (38, "Charles abierto".into()), (39, "Charles cerrado".into())]);
        assert_eq!((rack.kick, rack.snare, rack.open, rack.closed), (Some(36), Some(37), Some(38), Some(39)));
    }

    fn hits(notes: &[Note], pitch: u8, bar: usize) -> Vec<usize> {
        notes.iter().filter(|n| n.pitch == pitch && (n.start / 4.0) as usize == bar).map(|n| ((n.start % 4.0) * 4.0).round() as usize).collect()
    }

    fn beat(style: Style, level: Level, fills: bool, bars: f64) -> Vec<Note> {
        generate(&machine(), &Settings { style, level, fills, follow_bass: false, bars: 4, machine: false, ai: false, sounds: [true; stems::drums::VOICES] }, bars * 4.0, &[], 0).0
    }

    #[test]
    fn every_style_and_level_is_written_out_right() {
        for style in Style::ALL {
            for level in Level::ALL {
                for take in takes(style, level) {
                    for bar in [take.a, take.b] {
                        for (_, steps) in bar {
                            assert_eq!(steps.len(), 16, "{style:?} {level:?}: {steps}");
                            assert!(steps.chars().all(|c| ".Xxogrp".contains(c)), "{steps}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn each_style_has_its_backbone() {
        let house = beat(Style::House, Level::Basic, false, 4.0);
        assert_eq!(hits(&house, drum_note::KICK, 0), vec![0, 4, 8, 12]);
        assert_eq!(hits(&house, drum_note::CLAP, 0), vec![4, 12]);
        assert_eq!(hits(&house, drum_note::OPEN_HAT, 0), vec![2, 6, 10, 14]);
        assert_eq!(hits(&beat(Style::Trap, Level::Medium, false, 1.0), drum_note::CLAP, 0), vec![8], "half time");
        let dnb = beat(Style::DrumAndBass, Level::Basic, false, 1.0);
        assert_eq!(hits(&dnb, drum_note::KICK, 0), vec![0, 10]);
        assert_eq!(hits(&dnb, drum_note::SNARE, 0), vec![4, 12]);
        assert_eq!(hits(&beat(Style::Reggaeton, Level::Intense, false, 1.0), drum_note::SNARE, 0), vec![3, 6, 11, 14], "the dembow");
        // Every level keeps the backbone: the kick on one, the backbeat where it goes.
        for style in Style::ALL {
            for level in Level::ALL {
                let b = beat(style, level, false, 1.0);
                assert!(b.iter().any(|n| n.pitch == drum_note::KICK && n.start == 0.0), "{style:?} {level:?}");
            }
        }
    }

    /// Hardgroove rolls toms and congas over the four on the floor, with hats
    /// that play only some times round; hard techno's kick gallops when busy
    /// and its claps roll at the end of a phrase. Both on the 909.
    #[test]
    fn hardgroove_and_hard_techno_have_their_backbone() {
        let own = |style: Style, level: Level| generate(&machine(), &Settings { style, level, fills: false, sounds: style.sounds(), ..Settings::default() }, 4.0, &[], 0).0;
        let groove = own(Style::Hardgroove, Level::Medium);
        assert_eq!(hits(&groove, drum_note::KICK, 0), vec![0, 4, 8, 12]);
        assert_eq!(hits(&groove, drum_note::OPEN_HAT, 0), vec![2, 6, 10, 14]);
        assert!(!hits(&groove, drum_note::LOW_TOM, 0).is_empty() && !hits(&groove, drum_note::CONGA, 0).is_empty(), "toms and congas");
        assert!(groove.iter().any(|n| n.chance < 1.0), "hats that come and go");
        assert!(groove.iter().filter(|n| n.pitch == drum_note::KICK).all(|n| n.chance == 1.0), "the kick always");
        let hard = beat(Style::HardTechno, Level::Intense, false, 1.0);
        assert_eq!(hits(&hard, drum_note::KICK, 0), vec![0, 3, 4, 8, 11, 12], "galloping");
        assert_eq!(hits(&hard, drum_note::CLAP, 0), vec![4, 7, 12, 15]);
        let phrase = beat(Style::HardTechno, Level::Basic, true, 4.0);
        let rolled = phrase.iter().filter(|n| n.pitch == drum_note::CLAP && n.start >= 15.0).count();
        assert_eq!(rolled, 8, "a clap roll in the last beat");
        assert_eq!(hits(&phrase, drum_note::KICK, 3), vec![0, 4, 8, 12], "over the kick");
        for style in [Style::Hardgroove, Style::HardTechno] {
            assert_eq!(style.machine(), engine::model::DrumModel::Tr909);
            assert!(style.tempo() >= 140);
        }
        assert!(swing(Style::Hardgroove, Level::Medium).is_some());
    }

    #[test]
    fn only_the_chosen_sounds_play_and_a_crash_opens_each_phrase() {
        let kit = machine();
        let mut settings = Settings { style: Style::PopRock, level: Level::Intense, bars: 16, ..Settings::default() };
        settings.sounds = Style::PopRock.sounds();
        let (notes, _) = generate(&kit, &settings, 64.0, &[], 0);
        let crashes: Vec<f64> = notes.iter().filter(|n| n.pitch == drum_note::CRASH).map(|n| n.start).collect();
        assert_eq!(crashes, vec![0.0, 32.0]);
        // Without hats the hat lines go silent; without the crash, no crash.
        settings.sounds[Sound::Hat as usize] = false;
        settings.sounds[Sound::Open as usize] = false;
        settings.sounds[Sound::Crash as usize] = false;
        let (notes, _) = generate(&kit, &settings, 64.0, &[], 0);
        assert!(notes.iter().all(|n| ![drum_note::CLOSED_HAT, drum_note::OPEN_HAT, drum_note::CRASH].contains(&n.pitch)));
        assert!(notes.iter().any(|n| n.pitch == drum_note::KICK) && notes.iter().any(|n| n.pitch == drum_note::SNARE));
        // Techno keeps its ride and cowbell out.
        let techno = Style::Techno.sounds();
        assert!(!techno[Sound::Ride as usize] && !techno[Sound::Cowbell as usize]);
    }

    #[test]
    fn each_style_has_its_own_sound() {
        let trap = Style::Trap.sound();
        let techno = Style::Techno.sound();
        assert_eq!((trap.model, techno.model), (engine::model::DrumModel::Tr808, engine::model::DrumModel::Tr909));
        // The trap kick long and deep; the techno one short.
        assert!(trap.decay[0] > 2.0 && trap.tune[0] < 0.0 && techno.decay[0] < 1.0);
        let sounds: Vec<_> = Style::ALL.iter().map(|s| s.sound()).collect();
        assert!(sounds.iter().enumerate().all(|(i, a)| sounds[i + 1..].iter().all(|b| a != b)), "no two styles sound alike");
    }

    #[test]
    fn a_bar_repeats_and_the_fourth_turns_it_around() {
        // A groove, not noise: bars one to three alike (but for the crash that opens the phrase).
        let notes = beat(Style::BoomBap, Level::Medium, false, 4.0);
        let bar = |b: usize| -> Vec<(u8, usize)> {
            notes.iter().filter(|n| (n.start / 4.0) as usize == b && n.pitch != drum_note::CRASH).map(|n| (n.pitch, ((n.start % 4.0) * 4.0).round() as usize)).collect()
        };
        assert_eq!(bar(0), bar(1));
        assert_eq!(bar(1), bar(2));
        assert_ne!(bar(2), bar(3), "the fourth turns it around");
        // Takes differ.
        let kit = machine();
        let settings = Settings { style: Style::Techno, level: Level::Intense, fills: false, follow_bass: false, bars: 4, machine: false, ai: false, sounds: [true; stems::drums::VOICES] };
        assert_ne!(generate(&kit, &settings, 16.0, &[], 0).0, generate(&kit, &settings, 16.0, &[], 1).0);
        // Busier levels have more.
        let count = |level| beat(Style::PopRock, level, false, 4.0).len();
        assert!(count(Level::Intense) > count(Level::Medium) && count(Level::Medium) > count(Level::Basic));
    }

    #[test]
    fn fills_end_phrases_only() {
        // A bar or two: no fill at all.
        let short = beat(Style::PopRock, Level::Basic, true, 2.0);
        assert!(!short.iter().any(|n| n.pitch == drum_note::LOW_TOM));
        // Eight bars: one fill, at the end; none at bar four.
        let long = beat(Style::PopRock, Level::Basic, true, 8.0);
        let toms: Vec<f64> = long.iter().filter(|n| n.pitch == drum_note::LOW_TOM).map(|n| n.start).collect();
        assert!(!toms.is_empty() && toms.iter().all(|&b| b >= 31.0), "{toms:?}");
        assert!(!long.iter().any(|n| n.pitch == drum_note::CLOSED_HAT && n.start >= 31.0), "the fill's beat has no hats");
        // Reggaetón has none.
        let dembow = beat(Style::Reggaeton, Level::Basic, true, 8.0);
        assert_eq!(dembow, beat(Style::Reggaeton, Level::Basic, false, 8.0));
        // Hip hop swings; techno doesn't.
        assert!(generate(&machine(), &Settings { style: Style::BoomBap, ..Default::default() }, 4.0, &[], 0).1.is_some());
        assert!(generate(&machine(), &Settings { style: Style::Techno, ..Default::default() }, 4.0, &[], 0).1.is_none());
    }

    #[test]
    fn the_kick_can_follow_the_bass_and_small_kits_still_play() {
        let kit = machine();
        let settings = Settings { style: Style::PopRock, level: Level::Basic, fills: false, follow_bass: true, bars: 1, machine: false, ai: false, sounds: [true; stems::drums::VOICES] };
        let (notes, _) = generate(&kit, &settings, 4.0, &[1.5, 3.0, 2.3], 0);
        let kicks: Vec<f64> = notes.iter().filter(|n| n.pitch == drum_note::KICK).map(|n| n.start).collect();
        assert!(kicks.contains(&1.5) && kicks.contains(&3.0) && !kicks.contains(&2.3), "{kicks:?}");
        // No snare: the clap stands in.
        let small = Kit { kick: Some(36), clap: Some(39), ..Default::default() };
        let (beat, _) = generate(&small, &Settings { style: Style::DrumAndBass, ..Default::default() }, 8.0, &[], 0);
        assert!(beat.iter().any(|n| n.pitch == 39));
    }

    /// The drum model keeps the four on the floor: wherever the written beat
    /// has its kick on the beat, so does what it plays (in the 2026-10b audit
    /// a Hard techno take had three bars of eight with a beat missing). Fills
    /// that leave a beat out are the written beat's, and stay.
    #[test]
    fn the_drum_model_keeps_the_four_on_the_floor() {
        let kick = drum_note::KICK;
        let on_beats = |notes: &[Note]| -> Vec<usize> { (0..32).filter(|&b| notes.iter().any(|n| n.pitch == kick && (n.start - b as f64).abs() < 1e-6)).collect() };
        for style in [Style::Techno, Style::Hardgroove, Style::HardTechno] {
            for level in [Level::Basic, Level::Intense] {
                for take in [0, 2] {
                    let settings = Settings { style, level, bars: 8, sounds: style.sounds(), ..Default::default() };
                    let (written, _) = generate(&machine(), &settings, 32.0, &[], take);
                    let (played, _) = generate_ai(&machine(), &Settings { ai: true, ..settings }, 32.0, &[], take).unwrap();
                    let (written, played) = (on_beats(&written), on_beats(&played));
                    let missing: Vec<usize> = written.iter().filter(|b| !played.contains(b)).copied().collect();
                    assert!(missing.is_empty(), "{} {} {take}: no kick on beats {missing:?}", style.label(), level.label());
                    assert!(written.len() >= 28, "{} {} {take}: a four on the floor ({} beats)", style.label(), level.label(), written.len());
                }
            }
        }
    }
}
