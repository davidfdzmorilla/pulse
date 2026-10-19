//! Drum patterns: a beat in a style (house, techno, trap, drum & bass…) for a
//! drum track's clip, on whatever kit it has (its sounds found by their names),
//! busier or plainer, with a fill at the end of each phrase and, if asked, the
//! kick following the bass. Each take is different; the same seed, the same take.

use crate::project::Track;
use engine::model::Note;

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
}

impl Style {
    pub const ALL: [Style; 8] = [Style::House, Style::Techno, Style::Trap, Style::DrumAndBass, Style::BoomBap, Style::PopRock, Style::Reggaeton, Style::Breakbeat];

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
        }
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
    pub tom: Option<u8>,
    pub rim: Option<u8>,
    pub perc: Option<u8>,
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
            tom: find(&["tom"], &[]),
            rim: find(&["rim", "stick"], &[]),
            perc: find(&["cowbell", "perc", "shaker", "conga", "bongo", "cencerro"], &[]),
        }
    }

    /// The backbeat: the snare, or the clap, or the rim.
    fn back(&self, prefer_clap: bool) -> Option<u8> {
        if prefer_clap { self.clap.or(self.snare) } else { self.snare.or(self.clap) }.or(self.rim)
    }

    fn hat(&self) -> Option<u8> {
        self.closed.or(self.open).or(self.perc)
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
/// `x` normal, `o` soft, `g` a ghost note, `r` a roll (two thirty-seconds).
type Bar = &'static [(V, &'static str)];

/// A groove: its main bar, and the one that turns the phrase around (every
/// fourth bar).
struct Take {
    a: Bar,
    b: Bar,
}

use V::*;

const FOUR: &str = "X...X...X...X...";

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
    }
}

/// How a style swings, if it does.
fn swing(style: Style, level: Level) -> Option<engine::model::Groove> {
    use engine::model::Groove;
    match (style, level) {
        (Style::BoomBap, _) => Some(Groove { grid: 0.25, swing: 0.5, accent: 0.0, humanize: 0.3 }),
        (Style::House, Level::Medium | Level::Intense) => Some(Groove { grid: 0.25, swing: 0.15, accent: 0.0, humanize: 0.15 }),
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
}

impl Style {
    /// The machine the style is played on.
    pub fn machine(self) -> engine::model::DrumModel {
        use engine::model::DrumModel::*;
        match self {
            Style::Trap | Style::BoomBap | Style::Reggaeton => Tr808,
            Style::House | Style::Techno | Style::Breakbeat | Style::DrumAndBass | Style::PopRock => Tr909,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { style: Style::House, level: Level::Medium, fills: true, follow_bass: false, bars: 4, machine: true, ai: false }
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
        _ => None,
    }
}

/// A beat `length` beats long for `kit`, and the swing it wants. `take` picks
/// among the style's grooves (each a different take); `bass` are the beats (in
/// the clip) where the bass starts its notes, for `follow_bass`.
pub fn generate(kit: &Kit, settings: &Settings, length: f64, bass: &[f64], take: u64) -> (Vec<Note>, Option<engine::model::Groove>) {
    let options = takes(settings.style, settings.level);
    let groove = &options[take as usize % options.len()];
    let back_clap = matches!(settings.style, Style::House | Style::Techno | Style::Trap);
    let pitch = |v: V| match v {
        Kick => kit.kick,
        Back => kit.back(back_clap),
        Hat => kit.hat(),
        Open => kit.open.or(kit.closed),
        Rim => kit.rim.or(kit.perc),
        Perc => kit.perc.or(kit.rim),
        Tom => kit.tom.or(kit.back(false)),
    };
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
                if c == 'r' {
                    notes.push(Note { start: beat, length: 0.125, pitch, velocity: v });
                    notes.push(Note { start: beat + 0.125, length: 0.125, pitch, velocity: v * 0.85 });
                } else {
                    notes.push(Note { start: beat, length: 0.25, pitch, velocity: v });
                }
            }
        }
    }
    follow_bass(kit, settings, &mut notes, bass, length);
    (notes, swing(settings.style, settings.level))
}

/// With the bass: a kick where it starts a note on an eighth and none is there.
fn follow_bass(kit: &Kit, settings: &Settings, notes: &mut Vec<Note>, bass: &[f64], length: f64) {
    if settings.follow_bass {
        if let Some(kick) = kit.kick {
            for &b in bass {
                let on_eighth = ((b * 2.0) - (b * 2.0).round()).abs() < 1e-6;
                let taken = notes.iter().any(|n| n.pitch == kick && (n.start - b).abs() < 0.1);
                if on_eighth && !taken && b < length {
                    notes.push(Note { start: b, length: 0.25, pitch: kick, velocity: 0.85 });
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
    use stems::drums::{Voice, VOICES};
    let options = takes(settings.style, settings.level);
    let groove = &options[take as usize % options.len()];
    let back_clap = matches!(settings.style, Style::House | Style::Techno | Style::Trap);
    let voice = |v: V| match v {
        Kick => Voice::Kick,
        Back if back_clap => Voice::Clap,
        Back => Voice::Snare,
        Hat => Voice::Hat,
        Open => Voice::Open,
        Rim => Voice::Rim,
        Perc => Voice::Perc,
        Tom => Voice::Tom,
    };
    let mut seed = vec![[0u8; VOICES]; 64];
    for bar in 0..4 {
        for &(v, steps) in if bar == 3 { groove.b } else { groove.a } {
            for (s, c) in steps.char_indices() {
                let level = match c {
                    'X' => 3,
                    'x' | 'o' | 'r' => 2,
                    'g' => 1,
                    _ => continue,
                };
                let cell = &mut seed[bar * 16 + s][voice(v) as usize];
                *cell = (*cell).max(level);
            }
        }
    }
    let pitch = |v: Voice| match v {
        Voice::Kick => kit.kick,
        Voice::Snare => kit.snare.or(kit.clap),
        Voice::Clap => kit.clap.or(kit.snare),
        Voice::Hat => kit.closed.or(kit.open),
        Voice::Open => kit.open,
        Voice::Rim => kit.rim,
        Voice::Perc => kit.perc,
        Voice::Tom => kit.tom,
    };
    let playable = stems::drums::ALL.map(|v| pitch(v).is_some());
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
            if level == 0 || (v == Voice::Hat && step[Voice::Open as usize] > 0) {
                continue;
            }
            let Some(pitch) = pitch(v) else { continue };
            let base = [0.0, 0.38, 0.8, 1.0][level as usize];
            let base = if matches!(v, Voice::Hat | Voice::Open) { base * 0.85 } else { base };
            let velocity = (base * (0.96 + 0.08 * dice.roll())).clamp(0.05, 1.0);
            notes.push(Note { start: s as f64 * 0.25, length: 0.25, pitch, velocity });
        }
    }
    follow_bass(kit, settings, &mut notes, bass, length);
    Ok((notes, swing(settings.style, settings.level)))
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
        let params = engine::model::DrumParams { model: style.machine(), ..Default::default() };
        let mut track = crate::project::Track::new("Drums", [0; 3], engine::model::InstrumentKind::DrumMachine(params));
        let settings = Settings { style, level, fills: true, follow_bass: false, bars: 8, machine: true, ai };
        let (notes, groove) = if ai { generate_ai(&machine(), &settings, 32.0, &[], take).unwrap() } else { generate(&machine(), &settings, 32.0, &[], take) };
        let clip = engine::model::MidiClip { notes, groove, ..engine::model::MidiClip::new("Beat", 32.0, [0; 3]) };
        track.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 32.0, clip: engine::model::Clip::Midi(clip) });
        project.tracks = vec![track];
        let mut samples = crate::sync::SampleCache::default();
        let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
        let (l, r) = crate::export::render(commands, (0.0, 32.0), project.tempo, 48_000.0, &Default::default(), &|_| {}).unwrap();
        let by = if ai { " IA" } else { "" };
        let name = format!("{} {} {}{by} v{}.wav", style.label().replace(['/', '&'], "y"), level.label(), style.machine().label(), take + 1);
        crate::export::write(&dir.join(&name), &l, &r, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
        name
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
        generate(&machine(), &Settings { style, level, fills, follow_bass: false, bars: 4, machine: false, ai: false }, bars * 4.0, &[], 0).0
    }

    #[test]
    fn every_style_and_level_is_written_out_right() {
        for style in Style::ALL {
            for level in Level::ALL {
                for take in takes(style, level) {
                    for bar in [take.a, take.b] {
                        for (_, steps) in bar {
                            assert_eq!(steps.len(), 16, "{style:?} {level:?}: {steps}");
                            assert!(steps.chars().all(|c| ".Xxogr".contains(c)), "{steps}");
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

    #[test]
    fn a_bar_repeats_and_the_fourth_turns_it_around() {
        // A groove, not noise: bars one to three alike.
        let notes = beat(Style::BoomBap, Level::Medium, false, 4.0);
        let bar = |b: usize| -> Vec<(u8, usize)> { notes.iter().filter(|n| (n.start / 4.0) as usize == b).map(|n| (n.pitch, ((n.start % 4.0) * 4.0).round() as usize)).collect() };
        assert_eq!(bar(0), bar(1));
        assert_eq!(bar(1), bar(2));
        assert_ne!(bar(2), bar(3), "the fourth turns it around");
        // Takes differ.
        let kit = machine();
        let settings = Settings { style: Style::Techno, level: Level::Intense, fills: false, follow_bass: false, bars: 4, machine: false, ai: false };
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
        let settings = Settings { style: Style::PopRock, level: Level::Basic, fills: false, follow_bass: true, bars: 1, machine: false, ai: false };
        let (notes, _) = generate(&kit, &settings, 4.0, &[1.5, 3.0, 2.3], 0);
        let kicks: Vec<f64> = notes.iter().filter(|n| n.pitch == drum_note::KICK).map(|n| n.start).collect();
        assert!(kicks.contains(&1.5) && kicks.contains(&3.0) && !kicks.contains(&2.3), "{kicks:?}");
        // No snare: the clap stands in.
        let small = Kit { kick: Some(36), clap: Some(39), ..Default::default() };
        let (beat, _) = generate(&small, &Settings { style: Style::DrumAndBass, ..Default::default() }, 8.0, &[], 0);
        assert!(beat.iter().any(|n| n.pitch == 39));
    }
}
