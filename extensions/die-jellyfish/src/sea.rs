//! ASCII Jellyfish
//!
//! A jellyfish swimming through a page full of letters. Nothing is remembered between frames:
//! every glyph on screen is worked out from the clock alone, so any moment can be drawn on its own.
//!
//! Inspired by the "ASCII Jellyfish" reel by @dwcminh.ascii. This version is written from scratch.

use std::f64::consts::{PI, TAU};

fn lerp(a: f64, b: f64, k: f64) -> f64 {
    a + (b - a) * k
}

fn ease(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn frac(x: f64) -> f64 {
    x - x.floor()
}

// A repeatable scramble of up to three whole numbers into 0…1. It is why the same letter stays
// on the same patch of skin from one frame to the next.
fn hash(a: i64, b: i64, c: i64) -> f64 {
    let mut h = (a as u32).wrapping_mul(0x9e3779b1) ^ (b as u32).wrapping_mul(0x85ebca77) ^ (c as u32).wrapping_mul(0xc2b2ae3d);
    h = (h ^ (h >> 15)).wrapping_mul(0x2c1b3c6d);
    h = (h ^ (h >> 12)).wrapping_mul(0x297a2d39);
    (h ^ (h >> 15)) as f64 / 4294967296.0
}

// A small repeatable random stream, so every run meets the same jellyfish.
struct Dice(u32);

impl Dice {
    fn roll(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let mut x = (self.0 ^ (self.0 >> 15)).wrapping_mul(1 | self.0);
        x = x.wrapping_add((x ^ (x >> 7)).wrapping_mul(61 | x)) ^ x;
        (x ^ (x >> 14)) as f64 / 4294967296.0
    }
}


// ── colour ──────────────────────────────────────────────────────────────────────────────────
// A colour is one number, 0xRRGGBB.

const WHITE: u32 = 0xffffff;

fn mix(a: u32, b: u32, k: f64) -> u32 {
    let channel = |shift: u32| {
        let (from, to) = (((a >> shift) & 255) as f64, ((b >> shift) & 255) as f64);
        ((from + (to - from) * k) as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

// A colour somewhere along a list of colours.
fn ramp(stops: &[u32], x: f64) -> u32 {
    let last = stops.len() - 1;
    if last == 0 {
        return stops[0];
    }
    let f = x.clamp(0.0, 1.0) * last as f64;
    let i = (f.floor() as usize).min(last - 1);
    mix(stops[i], stops[i + 1], f - i as f64)
}

// How bright a colour looks, 0…1.
fn luminance(c: u32) -> f64 {
    let channel = |shift: u32| {
        let v = ((c >> shift) & 255) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

/// The page the sea is drawn on, as its paints need to know it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tones {
    /// What every colour fades into: the theme's background.
    pub paper: u32,
    /// What a highlight bleaches towards: white on a dark paper, the colour of the text on a light one.
    pub bright: u32,
    /// How much of each pigment is kept, the rest going to black: all of it on a dark paper.
    pub deepen: f64,
}

impl Tones {
    /// The tones for a theme, from its background and the colour of its text.
    pub fn for_theme(background: u32, foreground: u32) -> Tones {
        if luminance(background) < 0.4 {
            Tones { paper: background, bright: WHITE, deepen: 1.0 }
        } else {
            // on a pale paper white would not show and the pastels hardly, so highlights go to
            // the colour of the text and every pigment is deepened
            Tones { paper: background, bright: foreground, deepen: 0.62 }
        }
    }

    // A pigment, as deep as this paper wants it.
    fn deep(self, c: u32) -> u32 {
        mix(0, c, self.deepen)
    }
}

#[derive(Clone, Copy)]
enum Coat {
    Bell,
    Stripe,
    Inner,
    Arm,
    Tent,
}

struct Palette {
    bell: &'static [u32],
    stripe: &'static [u32],
    inner: &'static [u32],
    arm: &'static [u32],
    tent: &'static [u32],
}

// One coat per pulse. Bell and arms are painted crown-to-tip along their list of colours.
#[rustfmt::skip]
static PALETTES: [Palette; 8] = [
    Palette { bell: &[0x6f93ff, 0x3d62f2], stripe: &[0xf3f6ff], inner: &[0x24399c], // abyss
              arm: &[0xc4b5ff, 0x7c61f0], tent: &[0xbfcdf8] },
    Palette { bell: &[0x3fdfd0, 0x1c9fc2], stripe: &[0xff74c8], inner: &[0x2c3794], // lagoon
              arm: &[0xff7f9e, 0xe2416f], tent: &[0xa6b8f6] },
    Palette { bell: &[0xffe45e, 0xff8f5e, 0xff62b0], stripe: &[0xfff7d4], inner: &[0x8b49da], // sorbet
              arm: &[0xb875ff, 0x47dcea], tent: &[0xf6ccdf] },
    Palette { bell: &[0xf5eb50, 0x4bdb8d, 0x2ebbdd, 0x4071f3], stripe: &[0xf0fff4], inner: &[0x2b3192], // prism
              arm: &[0x408df3, 0x7d5ef6, 0xd466ea], tent: &[0xffa0a0] },
    Palette { bell: &[0xff4d2c, 0xff9c30, 0xffd44d], stripe: &[0xffecb0], inner: &[0x7c2030], // ember
              arm: &[0xa26dff, 0x6c4ae2], tent: &[0xffdb8c] },
    Palette { bell: &[0x9c5eff, 0x6c41e2], stripe: &[0x42e2f2], inner: &[0x3b2b9c], // orchid
              arm: &[0xffe65e, 0xf3b834], tent: &[0xf3de7c] },
    Palette { bell: &[0xff4343, 0xe22c3c], stripe: &[0xffda40], inner: &[0x8c203c], // coral
              arm: &[0xffe5ec, 0xff9cba], tent: &[0xffb5c6] },
    Palette { bell: &[0xe9edf6, 0xb0b8cb], stripe: &[0x31cff6], inner: &[0x2b3caa], // ghost
              arm: &[0xff7cba, 0xf2428e], tent: &[0xe7eaf3] },
];

impl Palette {
    fn of(&self, coat: Coat) -> &'static [u32] {
        match coat {
            Coat::Bell => self.bell,
            Coat::Stripe => self.stripe,
            Coat::Inner => self.inner,
            Coat::Arm => self.arm,
            Coat::Tent => self.tent,
        }
    }
}


// ── the clock ───────────────────────────────────────────────────────────────────────────────

const BEAT: f64 = 2.2; // seconds from one squeeze of the bell to the next

// It forms on beat 0, grows its arms on beat 1, and from beat 3 puts on a new coat every beat.
fn palette_at(beat: i64) -> usize {
    if beat < 3 { 0 } else { (beat - 2) as usize % PALETTES.len() }
}

fn is_finale(beat: i64) -> bool {
    beat >= 3 && palette_at(beat) >= PALETTES.len() - 2
}

// 0 = open and relaxed, 1 = clenched.
fn squeeze(t: f64) -> f64 {
    if t < BEAT {
        return 0.0; // it only starts to swim once it has formed
    }
    let f = frac(t / BEAT);
    if f < 0.3 { ease(f / 0.3) } else { 1.0 - ease(((f - 0.3) / 0.7).powf(0.85)) }
}

// The colour of a part at time `when`. Callers hand in a later `when` the further down the body
// they are, so a new coat (and the white flash that brings it) runs from crown to tentacle tip.
fn tint(tones: Tones, coat: Coat, along: f64, when: f64) -> u32 {
    let beat = (when / BEAT).floor() as i64;
    let since = when - beat as f64 * BEAT;
    let (now, was) = (palette_at(beat), palette_at(beat - 1));
    let mut c = tones.deep(ramp(PALETTES[now].of(coat), along));
    if now != was {
        c = mix(tones.deep(ramp(PALETTES[was].of(coat), along)), c, ease(since / 0.4));
    }
    let flash = if now != was || beat == 1 { (-since / 0.18).exp() } else { 0.0 };
    if flash > 0.01 { mix(c, tones.bright, 0.9 * flash) } else { c }
}


// ── the page of letters ─────────────────────────────────────────────────────────────────────

const EMPTY: u8 = 0;
const WATER: u8 = 1;
const BELL: u8 = 2;
const LIP: u8 = 3;
const ARM: u8 = 4;
const TENTACLE: u8 = 5;
const FX: u8 = 6;

const FAR: f64 = -1e6;

const STAR: u16 = b'*' as u16;
const PLUS: u16 = b'+' as u16;
const DASH: u16 = b'-' as u16;
const DOT: u16 = b'.' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const BACK: u16 = b'\\' as u16;
const LOW: u16 = b'_' as u16;
const BAR: u16 = b'|' as u16;

// Darker skin gets thin letters, lit skin gets heavy ones.
const WEIGHTS: [&[u8]; 5] = [b":;!.", b"1l+=!;", b"tvrcxzj", b"nuaeo{}[]<>", b"08&%B$#Q@0"];

fn letter_for(light: f64, h1: f64, h2: f64) -> u16 {
    let set = WEIGHTS[(light * 5.0 + h1 - 0.5).floor().clamp(0.0, 4.0) as usize];
    set[(h2 * set.len() as f64) as usize] as u16
}


// ── the bell, in numbers ────────────────────────────────────────────────────────────────────
// A dome spun round its axis. v runs from 0 at the crown to 1 at the lip, a is the angle round.

const NV: usize = 72; // how finely the skin is sampled, down…
const NA: usize = 224; // …and round
const DOME: f64 = 2.3; // 2 is a ball; more gives it shoulders
const SWEEP: f64 = 0.56 * PI; // a little past the equator, so the lip tucks under
const TALL: f64 = 1.05;
const FRILLS: f64 = 16.0;
const STRIPES: f64 = 8.0;
const LAG: f64 = 0.22; // the squeeze reaches the lip this long after the crown

// The light: above, to the left, a little in front.
const LX: f64 = -0.52;
const LY: f64 = 0.66;
const LZ: f64 = 0.54;

// The pale bands painted from crown to shoulder.
fn stripe(v: f64, j: usize) -> f64 {
    let along = ease((v - 0.08) / 0.2) * (1.0 - ease((v - 0.62) / 0.2));
    along * ease((0.5 + 0.5 * (STRIPES * TAU * j as f64 / NA as f64).cos() - 0.55) / 0.25)
}

const ARMS: usize = 4;
const ARM_LONG: f64 = 2.0;
const ARM_STEPS: usize = 64;
const ARM_WIDE: usize = 17;
const TRAIL: f64 = 0.30; // each unit further down follows the bell this much later

const TENTACLES: usize = 14;
const TENT_STEPS: usize = 60;

#[derive(Clone, Copy)]
struct Tentacle {
    root: f64,
    long: f64,
    a: f64,
    b: f64,
    born: f64,
}

#[derive(Clone, Copy)]
struct Mote {
    far: f64,
    lift: f64,
    ph: f64,
    turn: f64,
    lands: f64,
}

#[derive(Clone, Copy)]
struct Speck {
    x: f64,
    y: f64,
    near: f64,
    rate: f64,
    ph: f64,
    ink: u32,
}

#[derive(Clone, Copy)]
struct Tiny {
    x: f64,
    y: f64,
    rise: f64,
    beat: f64,
    ph: f64,
    ink: u32,
}

const HALO: usize = 16;
const HALO_LIFE: f64 = 1.8;
const SPARKS: usize = 110;
const SPARK_LIFE: f64 = 2.8;
const DRAG: f64 = 1.7;
const SPARK_INKS: [u32; 7] = [0xffd84a, 0xff6fb5, 0x4fe3ee, 0xff8a4a, 0xb48cff, 0x7dffb0, 0xffffff];
const WATER_INKS: [u32; 4] = [0x5a78d8, 0x7a6ad8, 0x4a9ad0, 0x8894c8];
const SHOAL_INKS: [u32; 4] = [0xd85a8a, 0x3fb8c8, 0x8a6ae0, 0xc8784a];

// A far-off jellyfish, open and shut.
const TINY: [[&[u8]; 6]; 2] = [
    [b" .-\"\"-. ", b"/      \\", b"\\_.--._/", b" ( )( ) ", b"  ) )(  ", b" (  ) ) "],
    [b" .-\"\"-. ", b" |    | ", b" `.--.' ", b"  )()(  ", b"  ( )(  ", b"   )(   "],
];

/// What is drawn besides the jellyfish and its own glitter.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Extras {
    /// The water behind: drifting dust, and a four-point glint now and then.
    pub water: bool,
    /// The far-off little jellyfish at the sides.
    pub shoal: bool,
}


// ── the sea ─────────────────────────────────────────────────────────────────────────────────
// Everything that is drawn, and the grid of cells it is drawn on.

/// The jellyfish and the water round it, and the grid of cells they are drawn on.
pub struct Sea {
    // the whole area, one entry per cell
    cols: usize,
    rows: usize,
    cw: f64, // cw : ch is the shape of one cell
    ch: f64,
    glyph: Vec<u16>, // which character
    ink: Vec<u32>,   // what colour
    tones: Tones,    // on what paper
    depth: Vec<f32>, // how near the thing drawn there is
    part: Vec<u8>,   // what it belongs to
    owner: Vec<u32>, // for the bell: which point of its skin won the cell

    unit: f64,    // one bell-radius is this many columns
    lip_row: f64, // the row the lip of the bell rests on
    rise: f64,    // rows per unit
    cx: f64,
    cy: f64,

    // the pose: a turn, a place, a size
    m: [f64; 9],
    px: f64,
    py: f64,
    pk: f64,
    // the last point placed, in space and on the grid
    vx: f64,
    vy: f64,
    vz: f64,
    sx: f64,
    sy: f64,

    // one hoop of the bell
    ring_r: f64,
    ring_y: f64,
    ring_c: f64,
    ring_frill: f64,
    skirt: f64,
    cos_a: Vec<f32>,
    sin_a: Vec<f32>,
    lobe: Vec<f32>,
    bx: Vec<f32>,
    by: Vec<f32>,
    bz: Vec<f32>,
    bc: Vec<f32>,
    br: Vec<f32>,
    ax: [f32; ARM_WIDE],
    ay: [f32; ARM_WIDE],
    az: [f32; ARM_WIDE],

    tent: Vec<Tentacle>,
    motes: Vec<Mote>,
    dust: Vec<Speck>,
    shoal: Vec<Tiny>,
}

impl Default for Sea {
    fn default() -> Sea {
        Sea::new()
    }
}

impl Sea {
    /// A sea with nothing drawn yet. Every one holds the same jellyfish.
    pub fn new() -> Sea {
        let mut dice = Dice(11);
        let tent = (0..TENTACLES)
            .map(|i| Tentacle {
                root: TAU * (i as f64 + 0.7 * dice.roll()) / TENTACLES as f64,
                long: 2.1 + 0.8 * dice.roll(),
                a: dice.roll() * TAU,
                b: dice.roll() * TAU,
                born: 2.9 + 0.09 * ((i * 5) % TENTACLES) as f64, // they do not all sprout at once
            })
            .collect();
        let motes = (0..230)
            .map(|_| {
                let far = 0.9 + 1.9 * dice.roll().powf(0.8);
                let lift = 0.5 * (dice.roll() - 0.5);
                let ph = dice.roll() * TAU;
                let arm = (dice.roll() * 3.0).floor() * TAU / 3.0; // three spiral arms
                let turn = arm + 1.3 * far + 0.8 * (dice.roll() - 0.5);
                Mote { far, lift, ph, turn, lands: 0.45 + 0.6 * (far - 0.9) + 0.35 * dice.roll() }
            })
            .collect();
        let dust = (0..260)
            .map(|_| Speck {
                x: dice.roll(),
                y: dice.roll(),
                near: 0.25 + 0.75 * dice.roll(),
                rate: 0.4 + 1.6 * dice.roll(),
                ph: dice.roll() * TAU,
                ink: WATER_INKS[(dice.roll() * WATER_INKS.len() as f64) as usize],
            })
            .collect();
        let shoal = (0..10)
            .map(|i| Tiny {
                x: dice.roll(),
                y: dice.roll(),
                rise: 0.012 * (dice.roll() - 0.35),
                beat: 2.4 + 2.0 * dice.roll(),
                ph: dice.roll(),
                ink: SHOAL_INKS[i % SHOAL_INKS.len()],
            })
            .collect();
        let round = |f: fn(f64) -> f64| (0..NA).map(|j| f(TAU * j as f64 / NA as f64) as f32).collect::<Vec<f32>>();
        Sea {
            cols: 0,
            rows: 0,
            cw: 0.5,
            ch: 1.0,
            glyph: Vec::new(),
            ink: Vec::new(),
            tones: Tones { paper: 0, bright: WHITE, deepen: 1.0 },
            depth: Vec::new(),
            part: Vec::new(),
            owner: Vec::new(),
            unit: 20.0,
            lip_row: 20.0,
            rise: 10.0,
            cx: 0.0,
            cy: 0.0,
            m: [0.0; 9],
            px: 0.0,
            py: 0.0,
            pk: 1.0,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            sx: 0.0,
            sy: 0.0,
            ring_r: 0.0,
            ring_y: 0.0,
            ring_c: 0.0,
            ring_frill: 0.0,
            skirt: SWEEP.cos().abs().powf(2.0 / DOME),
            cos_a: round(f64::cos),
            sin_a: round(f64::sin),
            lobe: round(|a| 0.5 + 0.5 * (FRILLS * a).cos()),
            bx: vec![0.0; NV * NA],
            by: vec![0.0; NV * NA],
            bz: vec![0.0; NV * NA],
            bc: vec![0.0; NV * NA],
            br: vec![0.0; NV * NA],
            ax: [0.0; ARM_WIDE],
            ay: [0.0; ARM_WIDE],
            az: [0.0; ARM_WIDE],
            tent,
            motes,
            dust,
            shoal,
        }
    }

    // Nearer things win a cell.
    fn put(&mut self, col: i64, row: i64, z: f64, code: u16, ink: u32, part: u8) {
        if col < 0 || row < 0 || col >= self.cols as i64 || row >= self.rows as i64 {
            return;
        }
        let cell = row as usize * self.cols + col as usize;
        if z < self.depth[cell] as f64 {
            return;
        }
        self.depth[cell] = z as f32;
        self.glyph[cell] = code;
        self.ink[cell] = ink;
        self.part[cell] = part;
    }

    // Fade a colour into the paper.
    fn dim(&self, c: u32, k: f64) -> u32 {
        mix(self.tones.paper, c, k.clamp(0.0, 1.0))
    }

    // Shade a colour, bleach its highlights, fade it with distance.
    fn lit(&self, ink: u32, light: f64, z: f64) -> u32 {
        let ink = self.dim(ink, 0.5 + 0.5 * light);
        let ink = mix(ink, self.tones.bright, 0.5 * ease((light - 0.78) / 0.22));
        self.dim(ink, 0.72 + 0.28 * ease((z + 1.0) / 2.0))
    }

    // A stroke between two points of the grid, written with whichever of _ - / \ | leans the same way.
    #[allow(clippy::too_many_arguments)]
    fn stroke_to(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, ink: u32, part: u8) {
        let (dx, dy) = ((x1 - x0) * self.cw, (y1 - y0) * self.ch); // a cell is taller than it is wide
        let slope = dy.abs().atan2(dx.abs());
        let leaning = if dx * dy > 0.0 { BACK } else { SLASH };
        let steps = ((((x1 - x0).abs().max((y1 - y0).abs())) * 2.0).ceil() as i64).max(1);
        for k in 0..steps {
            let f = (k as f64 + 0.5) / steps as f64;
            let (x, y) = (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
            let mut row = y.floor() as i64;
            let mut code = leaning;
            if slope > 1.25 {
                code = BAR;
            } else if slope < 0.26 {
                // nearly flat: pick the dash that sits at the right height
                let high = y - row as f64;
                code = if !(0.3..0.7).contains(&high) { LOW } else { DASH };
                if high < 0.3 {
                    row -= 1;
                }
            }
            self.put(x.floor() as i64, row, z0 + (z1 - z0) * f, code, ink, part);
        }
    }


    // ── where the jellyfish is ──────────────────────────────────────────────────────────────
    // Its own space has the lip of the bell on the ground (y = 0), the crown above, the tentacles
    // below. The viewer's space has x to the right, y up and z towards the eye.

    fn set_pose(&mut self, t: f64) {
        let awake = ease((t - 1.0) / 1.6); // 0 while it forms, seen from above; 1 once it swims
        let lean = awake * 0.30 * (0.33 * t - 0.5).sin();
        let pitch = lerp(1.0, -0.30 + 0.10 * (0.47 * t).sin(), awake);
        let spin = 0.5 + 0.28 * t;
        let (cl, sl, cp, sp, cs, ss) = (lean.cos(), lean.sin(), pitch.cos(), pitch.sin(), spin.cos(), spin.sin());
        self.m = [
            cl * cs - sl * sp * ss, -sl * cp, cl * ss + sl * sp * cs,
            sl * cs + cl * sp * ss, cl * cp, sl * ss - cl * sp * cs,
            -cp * ss, sp, cp * cs,
        ];
        let f = frac(t / BEAT); // each squeeze shoves it upward, then it coasts back
        let surge = if t < BEAT {
            0.0
        } else if f < 0.45 {
            ease((f - 0.05) / 0.4)
        } else {
            1.0 - ease((f - 0.45) / 0.55)
        };
        self.px = awake * 0.42 * (0.33 * t - 0.5).cos(); // it drifts the way it leans
        self.py = 0.13 * surge + awake * 0.10 * (0.21 * t).sin();
        self.pk = 0.1 + 0.9 * (1.0 - (1.0 - (t / 2.0).clamp(0.0, 1.0)).powf(3.0));
    }

    // From the jellyfish's space into the viewer's.
    fn place(&mut self, x: f64, y: f64, z: f64) {
        let m = &self.m;
        self.vx = (m[0] * x + m[1] * y + m[2] * z) * self.pk + self.px;
        self.vy = (m[3] * x + m[4] * y + m[5] * z) * self.pk + self.py;
        self.vz = (m[6] * x + m[7] * y + m[8] * z) * self.pk;
    }

    // And from there onto the grid; nearer is a touch bigger.
    fn project(&mut self) {
        let near = 1.0 + 0.05 * self.vz;
        self.sx = self.cx + self.vx * near * self.unit;
        self.sy = self.cy - self.vy * near * self.rise;
    }


    // ── the bell ────────────────────────────────────────────────────────────────────────────

    // One hoop of the bell: how wide, how high, how clenched.
    fn ring(&mut self, v: f64, t: f64) {
        let a = v * SWEEP;
        let c = a.cos();
        self.ring_c = squeeze(t - LAG * v);
        self.ring_r = a.sin().powf(2.0 / DOME) * (1.0 - 0.26 * self.ring_c * ease(v)) + 0.07 * v.powf(8.0) * (1.0 - self.ring_c);
        self.ring_y = TALL * (c.signum() * c.abs().powf(2.0 / DOME) + self.skirt) * (1.0 + 0.16 * self.ring_c);
        self.ring_frill = v.powf(7.0); // the scallops only show near the lip
    }

    fn draw_bell(&mut self, t: f64) {
        self.set_pose(t);

        // 1. find which point of the skin is nearest the eye in every cell
        for i in 0..NV {
            self.ring(i as f64 / (NV - 1) as f64, t);
            for j in 0..NA {
                let k = i * NA + j;
                let lobe = self.lobe[j] as f64;
                let r = self.ring_r * (1.0 + 0.05 * self.ring_frill * lobe);
                self.place(r * self.cos_a[j] as f64, self.ring_y - 0.10 * self.ring_frill * lobe, r * self.sin_a[j] as f64);
                self.project();
                self.bx[k] = self.vx as f32;
                self.by[k] = self.vy as f32;
                self.bz[k] = self.vz as f32;
                self.bc[k] = self.sx as f32;
                self.br[k] = self.sy as f32;
                let (col, row) = (self.sx.floor() as i64, self.sy.floor() as i64);
                if col < 0 || row < 0 || col >= self.cols as i64 || row >= self.rows as i64 {
                    continue;
                }
                let cell = row as usize * self.cols + col as usize;
                if self.vz > self.depth[cell] as f64 {
                    self.depth[cell] = self.vz as f32;
                    self.part[cell] = BELL;
                    self.owner[cell] = k as u32;
                }
            }
        }

        // 2. light those points and give each a letter
        let (patches_v, patches_a) = (1.5 * self.unit, 3.2 * self.unit); // a patch of skin is about two cells wide
        for cell in 0..self.cols * self.rows {
            if self.part[cell] != BELL {
                continue;
            }
            let k = self.owner[cell] as usize;
            let (i, j) = (k / NA, k % NA);
            let v = i as f64 / (NV - 1) as f64;
            let up = i.saturating_sub(1) * NA + j;
            let down = (i + 1).min(NV - 1) * NA + j;
            let hoop = i.max(1) * NA;
            let (west, east) = (hoop + (j + NA - 2) % NA, hoop + (j + 2) % NA);
            let at = |list: &[f32], n: usize| list[n] as f64;
            let (ux, uy, uz) = (at(&self.bx, down) - at(&self.bx, up), at(&self.by, down) - at(&self.by, up), at(&self.bz, down) - at(&self.bz, up));
            let (ex, ey, ez) = (at(&self.bx, east) - at(&self.bx, west), at(&self.by, east) - at(&self.by, west), at(&self.bz, east) - at(&self.bz, west));
            let (mut nx, mut ny, mut nz) = (ey * uz - ez * uy, ez * ux - ex * uz, ex * uy - ey * ux); // which way the skin faces
            let size = (nx * nx + ny * ny + nz * nz).sqrt();
            let size = if size > 0.0 { size } else { 1.0 };
            nx /= size;
            ny /= size;
            nz /= size;
            let sun = nx * LX + ny * LY + nz * LZ;
            let when = t - 0.22 * v;
            let (light, tone);
            if nz >= 0.0 {
                // the outside of the dome
                let band = stripe(v, j);
                tone = mix(tint(self.tones, Coat::Bell, v, when), tint(self.tones, Coat::Stripe, v, when), band);
                light = 0.16 + 0.7 * sun.max(0.0) + 0.25 * (1.0 - nz).powf(3.0) + 0.3 * band;
            } else {
                // looking up into it
                tone = mix(tint(self.tones, Coat::Inner, v, when), tint(self.tones, Coat::Bell, v, when), 0.25);
                light = 0.14 + 0.22 * sun.abs();
            }
            let light = light.clamp(0.0, 1.0);
            let (patch_v, patch_a) = ((v * patches_v).floor() as i64, (j as f64 * patches_a / NA as f64).floor() as i64);
            self.glyph[cell] = letter_for(light, hash(patch_v, patch_a, 1), hash(patch_v, patch_a, 2));
            self.ink[cell] = self.lit(tone, light, self.bz[k] as f64);
        }

        // 3. the scalloped lip, as a bright line
        let lip = (NV - 1) * NA;
        let lip_ink = mix(tint(self.tones, Coat::Bell, 1.0, t - 0.22), self.tones.bright, 0.72);
        for j in 0..NA {
            let (a, b) = (lip + j, lip + (j + 1) % NA);
            let shade = self.dim(lip_ink, 0.5 + 0.5 * ease((self.bz[a] as f64 + 1.0) / 2.0));
            let (x0, y0, z0) = (self.bc[a] as f64, self.br[a] as f64, self.bz[a] as f64 + 0.04);
            let (x1, y1, z1) = (self.bc[b] as f64, self.br[b] as f64, self.bz[b] as f64 + 0.04);
            self.stroke_to(x0, y0, z0, x1, y1, z1, shade, LIP);
        }

        // 4. where the dome meets open water, swap the letter for a stroke that follows the outline
        let (cols, wide, high) = (self.cols as i64, self.cols as i64, self.rows as i64);
        for row in 0..high {
            for col in 0..wide {
                let cell = (row * cols + col) as usize;
                if self.part[cell] != BELL {
                    continue;
                }
                let part = &self.part;
                let solid = |c: i64, r: i64| -> i32 {
                    let inside = c >= 0 && r >= 0 && c < wide && r < high;
                    (inside && matches!(part[(r * cols + c) as usize], BELL | LIP)) as i32
                };
                let (w, e, u, d) = (solid(col - 1, row), solid(col + 1, row), solid(col, row - 1), solid(col, row + 1));
                if w + e + u + d == 4 {
                    continue;
                }
                let (nw, ne) = (solid(col - 1, row - 1), solid(col + 1, row - 1));
                let (sw, se) = (solid(col - 1, row + 1), solid(col + 1, row + 1));
                let (gx, gy) = (2 * (e - w) + ne + se - nw - sw, 2 * (d - u) + sw + se - nw - ne);
                let tilt = (gx.abs() as f64 * self.ch).atan2(gy.abs() as f64 * self.cw);
                self.glyph[cell] = if tilt < 0.5 {
                    if gy > 0 { DASH } else { LOW }
                } else if tilt > 1.2 {
                    BAR
                } else if gx * gy > 0 {
                    SLASH
                } else {
                    BACK
                };
                self.ink[cell] = mix(self.ink[cell], self.tones.bright, 0.5);
            }
        }
    }


    // ── the oral arms ───────────────────────────────────────────────────────────────────────
    // Four frilled ribbons hanging from the middle of the bell, winding slowly round each other.

    fn draw_arms(&mut self, t: f64) {
        let grown = ease((t - 2.5) / 1.8);
        if grown <= 0.0 {
            return;
        }
        let (mid, patches) = ((ARM_WIDE - 1) / 2, 1.8 * self.unit);
        for k in 0..ARMS {
            let kf = k as f64;
            for i in 0..ARM_STEPS {
                let s = i as f64 / (ARM_STEPS - 1) as f64;
                let reach = s * grown * ARM_LONG;
                let c = squeeze(t - 0.1 - 0.18 * reach);
                self.set_pose(t - TRAIL * reach); // carried by where the bell was a moment ago
                let turn = kf * TAU / ARMS as f64 + 0.5 + 0.5 * reach + 0.22 * (0.7 * t - 1.6 * reach + 1.7 * kf).sin();
                let out = (0.06 + 0.15 * (PI * s).sin()) * (1.0 - 0.25 * c);
                let half = 0.24 * (PI * s.powf(0.6)).sin().powf(0.7) * (1.0 - 0.3 * s) + 0.015;
                let y = 0.5 - reach * (1.0 + 0.05 * c);
                let (ca, sa) = (turn.cos(), turn.sin());
                let sun_out = LX * self.m[0] + LY * self.m[3] + LZ * self.m[6];
                let sun_side = LX * self.m[2] + LY * self.m[5] + LZ * self.m[8];
                let tone = tint(self.tones, Coat::Arm, s, t - 0.25 - 0.16 * reach);
                let edge = mix(tone, self.tones.bright, 0.45);
                for j in 0..ARM_WIDE {
                    let w = j as f64 / (ARM_WIDE - 1) as f64 * 2.0 - 1.0;
                    let wave = 8.0 * reach + 2.6 * w - 2.2 * t + 2.1 * kf;
                    let ruffle = 0.08 * (0.3 + 0.7 * w * w);
                    let (off, side) = (out + ruffle * wave.sin(), half * w);
                    self.place(ca * off - sa * side, y - 0.05 * w * w, sa * off + ca * side);
                    self.project();
                    let (sx, sy, vz) = (self.sx, self.sy, self.vz);
                    let fade = 0.55 + 0.45 * ease((vz + 1.0) / 2.0);
                    if j == 0 || j == ARM_WIDE - 1 || j == mid {
                        // the hems and the midrib are drawn as lines
                        let shade = self.dim(edge, if j == mid { 0.6 * fade } else { fade });
                        if i > 0 {
                            let (x0, y0, z0) = (self.ax[j] as f64, self.ay[j] as f64, self.az[j] as f64 + 0.02);
                            self.stroke_to(x0, y0, z0, sx, sy, vz + 0.02, shade, ARM);
                        }
                    } else {
                        // the cloth between is letters, lit by how each ruffle faces
                        let tip = (ruffle * wave.cos() * 2.6 / half).clamp(-3.0, 3.0);
                        let len = (1.0 + tip * tip).sqrt();
                        let (fx, fz) = ((ca + sa * tip) / len, (sa - ca * tip) / len);
                        let light = (0.24 + 0.62 * (fx * sun_out + fz * sun_side).abs() + 0.14 * wave.sin()).clamp(0.0, 1.0);
                        let (patch, lane) = ((s * patches).floor() as i64, (j / 3 + 8 * k) as i64);
                        let code = letter_for(light, hash(patch, lane, 3), hash(patch, lane, 4));
                        self.put(sx.floor() as i64, sy.floor() as i64, vz, code, self.lit(tone, light, vz), ARM);
                    }
                    self.ax[j] = sx as f32;
                    self.ay[j] = sy as f32;
                    self.az[j] = vz as f32;
                }
            }
        }
    }


    // ── the tentacles ───────────────────────────────────────────────────────────────────────

    // The point `reach` down a tentacle; the answer is left in sx, sy, vz.
    fn tentacle_point(&mut self, thread: Tentacle, reach: f64, t: f64) {
        self.ring(1.0, t - 0.2 * reach); // the lip as it was when this squeeze set off down the thread
        let hang = self.ring_r * (0.97 - 0.30 * ease(reach / 1.4));
        let r = hang + 0.17 * reach.powf(0.8) * (1.5 * t - 2.4 * reach + thread.a).sin();
        let round = thread.root + 0.24 * reach * (0.8 * t - 1.7 * reach + thread.b).sin();
        let y = -0.04 - reach * (1.0 + 0.06 * self.ring_c) + 0.03 * reach * (2.2 * t - 5.0 * reach + thread.b).sin();
        self.set_pose(t - TRAIL * reach);
        self.place(r * round.cos(), y, r * round.sin());
        self.project();
    }

    fn draw_tentacles(&mut self, t: f64) {
        for n in 0..TENTACLES {
            let thread = self.tent[n];
            let grown = ease((t - thread.born) / 1.5);
            if grown <= 0.0 {
                continue;
            }
            let (mut x0, mut y0, mut z0) = (0.0, 0.0, 0.0);
            for i in 0..TENT_STEPS {
                let s = i as f64 / (TENT_STEPS - 1) as f64;
                let reach = s * grown * thread.long;
                self.tentacle_point(thread, reach, t);
                let (sx, sy, vz) = (self.sx, self.sy, self.vz);
                if i > 0 {
                    let near = 0.6 + 0.4 * ease((vz + 1.1) / 2.2);
                    let tone = mix(tint(self.tones, Coat::Tent, s, t - 0.25 - 0.16 * reach), self.tones.bright, 0.2 * near);
                    self.stroke_to(x0, y0, z0, sx, sy, vz, self.dim(tone, near * (1.0 - 0.3 * s)), TENTACLE);
                }
                (x0, y0, z0) = (sx, sy, vz);
            }
            if grown > 0.9 && hash(n as i64, (t * 3.0).floor() as i64, 0) > 0.6 {
                self.put(self.sx.floor() as i64, self.sy.floor() as i64, self.vz + 0.01, STAR, self.tones.deep(0xe8eeff), TENTACLE);
            }
        }
    }


    // ── glitter and fireworks ───────────────────────────────────────────────────────────────

    // The opening: a whirl of + falls inward and becomes the bell.
    fn draw_swirl(&mut self, t: f64) {
        if t > 2.6 {
            return;
        }
        self.set_pose(t);
        for n in 0..self.motes.len() {
            let mote = self.motes[n];
            let f = t / mote.lands;
            if f >= 1.0 {
                continue;
            }
            let pull = f * f; // slow at first, then it drops in
            let r = lerp(mote.far, 0.9 * self.pk, pull) / self.pk;
            let round = mote.turn + 1.8 * pull + 0.4 * t;
            self.place(r * round.cos(), mote.lift * (1.0 - pull) / self.pk, r * round.sin());
            self.project();
            let glow = 0.3 + 0.7 * (0.5 + 0.5 * (6.0 * t + mote.ph).sin()).powf(2.0);
            let code = if glow > 0.8 { STAR } else { PLUS };
            self.put(self.sx.floor() as i64, self.sy.floor() as i64, self.vz, code, self.dim(self.tones.deep(0xdce6ff), glow * (0.45 + 0.55 * pull)), FX);
        }
    }

    // Every squeeze puffs a ring of glitter off the lip.
    fn draw_halos(&mut self, t: f64) {
        for beat in ((t - 0.32 - HALO_LIFE) / BEAT).floor() as i64..=(t / BEAT).floor() as i64 {
            let from = beat as f64 * BEAT + 0.32;
            let age = t - from;
            if beat < 2 || !(0.0..HALO_LIFE).contains(&age) {
                continue;
            }
            self.set_pose(from); // the ring stays where the lip was when it let go
            self.ring(1.0, from);
            let r = self.ring_r + 1.25 * age / (age + 0.7);
            let sink = 0.14 * age * (1.0 + 0.5 * age);
            let left = 1.0 - age / HALO_LIFE;
            let tone = mix(tint(self.tones, Coat::Tent, 0.0, t), self.tones.bright, 0.65);
            for n in 0..HALO as i64 {
                let round = TAU * (n as f64 + hash(beat, n, 0)) / HALO as f64 + 0.3 * age;
                self.place(r * round.cos(), -sink, r * round.sin());
                self.project();
                let blink = hash(beat * 31 + n, (t * 10.0).floor() as i64, 0); // it flickers ten times a second
                let code = if left < 0.3 {
                    DOT
                } else if blink > 0.55 {
                    STAR
                } else {
                    PLUS
                };
                let shade = self.dim(tone, left.powf(0.6) * (0.4 + 0.6 * blink));
                self.put(self.sx.floor() as i64, self.sy.floor() as i64, self.vz, code, shade, FX);
            }
        }
    }

    // The last two coats of the cycle go off like fireworks.
    fn draw_sparks(&mut self, t: f64) {
        for beat in ((t - SPARK_LIFE) / BEAT).floor() as i64 - 1..=(t / BEAT).floor() as i64 {
            let age = t - beat as f64 * BEAT - 0.28;
            if !is_finale(beat) || !(0.0..=SPARK_LIFE).contains(&age) {
                continue;
            }
            self.set_pose(beat as f64 * BEAT + 0.28);
            self.place(0.0, 0.5, 0.0);
            let (ox, oy, oz) = (self.vx, self.vy, self.vz); // the middle of the bell at the moment it fired
            for n in 0..SPARKS as i64 {
                let life = SPARK_LIFE * (0.6 + 0.4 * hash(beat, n, 1));
                if age > life {
                    continue;
                }
                // a direction picked evenly over a ball, folded to the far side so nothing crosses the bell
                let (up, round) = (1.0 - 2.0 * hash(beat, n, 2), TAU * hash(beat, n, 3));
                let flat = (1.0 - up * up).sqrt();
                let (dx, dy, dz) = (flat * round.cos(), 0.8 * up + 0.15, -0.6 * (flat * round.sin()).abs());
                let speed = 2.0 + 3.2 * hash(beat, n, 4);
                let tone = self.tones.deep(SPARK_INKS[(hash(beat, n, 5) * SPARK_INKS.len() as f64) as usize]);
                for tail in (0..=5).rev() {
                    // the head is drawn last so it keeps its cell
                    let a = age - tail as f64 * 0.045;
                    if a < 0.0 {
                        continue;
                    }
                    let gone = speed * (1.0 - (-DRAG * a).exp()) / DRAG; // the water slows it to a stop
                    self.vx = ox + dx * gone;
                    self.vy = oy + dy * gone - 0.07 * a * a;
                    self.vz = oz + dz * gone - 0.3;
                    self.project();
                    let left = 1.0 - a / life;
                    if left < 0.25 && tail > 0 {
                        continue; // at the end it crumbles to a single dot
                    }
                    let flick = 0.8 + 0.2 * (a * (11.0 + 9.0 * hash(beat, n, 6)) + n as f64).sin();
                    let code = match tail {
                        _ if left < 0.25 => DOT,
                        0 => STAR,
                        1 | 2 => PLUS,
                        3 | 4 => COLON,
                        _ => DOT,
                    };
                    let shade = self.dim(tone, (2.4 * left).clamp(0.0, 1.0) * flick * (1.0 - 0.11 * tail as f64));
                    self.put(self.sx.floor() as i64, self.sy.floor() as i64, self.vz, code, shade, FX);
                }
            }
        }
    }


    // ── the water behind ────────────────────────────────────────────────────────────────────

    fn draw_dust(&mut self, t: f64) {
        let sunk = (t - 2.4).max(0.0) * 0.016; // the eye rises with the jellyfish, so the water falls past
        let (wide, high) = (self.cols as f64, self.rows as f64);
        let count = self.dust.len().min((wide * high / 45.0) as usize);
        for n in 0..count {
            let d = self.dust[n];
            let col = (frac(d.x + 0.004 * (0.4 * t + d.ph).sin()) * wide).floor() as i64;
            let row = (frac(d.y + sunk * d.near) * high).floor() as i64;
            let glow = d.near * (0.3 + 0.7 * (0.5 + 0.5 * (d.rate * t + d.ph).sin()).powf(3.0));
            let code = if glow > 0.7 {
                STAR
            } else if glow > 0.33 {
                PLUS
            } else {
                DOT
            };
            self.put(col, row, FAR, code, self.dim(self.tones.deep(d.ink), 0.22 + 0.6 * glow), WATER);
        }
    }

    fn draw_shoal(&mut self, t: f64) {
        let (wide, high) = (self.cols as f64, self.rows as f64);
        let room = wide / 2.0 - 1.6 * self.unit - 9.0; // kept to the sides, clear of the big one
        if room < 2.0 {
            return;
        }
        let count = self.shoal.len().min(2 + (room / 7.0) as usize).min(2 * (self.rows / 14));
        for n in 0..count {
            let s = self.shoal[n];
            let art = &TINY[(frac(t / s.beat + s.ph) < 0.35) as usize];
            let aside = 1.6 * self.unit + s.x * room;
            let side = if n % 2 == 1 { aside } else { -aside - 8.0 };
            let left = (wide / 2.0 + side + 1.5 * (0.3 * t + 9.0 * s.ph).sin()).floor() as i64;
            let top = (frac(s.y - s.rise * t) * (high + 8.0)).floor() as i64 - 7;
            for (r, line) in art.iter().enumerate() {
                for (c, &code) in line.iter().enumerate() {
                    if code != b' ' {
                        let shade = self.dim(self.tones.deep(s.ink), if r < 3 { 0.5 } else { 0.36 });
                        self.put(left + c as i64, top + r as i64, FAR + 1.0, code as u16, shade, WATER);
                    }
                }
            }
        }
    }

    // A four-point glint; open runs 0…1.
    fn twinkle(&mut self, col: i64, row: i64, open: f64) {
        let ink = self.dim(self.tones.deep(0xdfe8ff), 0.3 + 0.7 * open);
        for d in (2..=(10.0 * open).floor() as i64).step_by(2) {
            let faint = self.dim(ink, 1.0 - d as f64 / 13.0);
            self.put(col - d, row, FAR + 2.0, DASH, faint, WATER);
            self.put(col + d, row, FAR + 2.0, DASH, faint, WATER);
        }
        for d in 1..=(3.0 * open).floor() as i64 {
            let faint = self.dim(ink, 1.0 - d as f64 / 5.0);
            self.put(col, row - d, FAR + 2.0, BAR, faint, WATER);
            self.put(col, row + d, FAR + 2.0, BAR, faint, WATER);
        }
        if open > 0.55 {
            self.put(col - 1, row - 1, FAR + 2.0, BACK, ink, WATER);
            self.put(col + 1, row - 1, FAR + 2.0, SLASH, ink, WATER);
            self.put(col - 1, row + 1, FAR + 2.0, SLASH, ink, WATER);
            self.put(col + 1, row + 1, FAR + 2.0, BACK, ink, WATER);
        }
        self.put(col, row, FAR + 3.0, if open > 0.4 { STAR } else { PLUS }, self.tones.bright, WATER);
    }

    fn draw_twinkles(&mut self, t: f64, water: bool) {
        if t < 0.9 {
            // one where the jellyfish is about to be
            self.set_pose(t);
            self.place(0.0, 0.6, 0.0);
            self.project();
            self.twinkle(self.sx.floor() as i64, self.sy.floor() as i64, (PI * t / 0.9).sin());
        }
        if !water {
            return; // the rest are the water's
        }
        let latest = (t / 1.7).floor() as i64;
        for n in latest - 1..=latest {
            let age = t - (n as f64 + 0.6 * hash(n, 1, 0)) * 1.7;
            if n < 0 || !(0.0..=0.8).contains(&age) {
                continue;
            }
            let col = (hash(n, 2, 0) * self.cols as f64).floor() as i64;
            let row = (hash(n, 3, 0) * self.rows as f64 * 0.9).floor() as i64;
            self.twinkle(col, row, (PI * age / 0.8).sin());
        }
    }


    // ── one frame of the sea ────────────────────────────────────────────────────────────────

    /// Draw the moment `t`, in seconds since the first spark, on `cols` by `rows` cells, each
    /// `aspect` times as wide as it is tall, in these tones. It takes whatever it is given: a time
    /// before the start or no number at all is the start, a cell of no shape is twice as tall as
    /// it is wide, and a page with no cells stays empty.
    pub fn draw(&mut self, cols: usize, rows: usize, aspect: f64, tones: Tones, extras: Extras, t: f64) {
        let t = if t.is_finite() { t.clamp(0.0, 1e12) } else { 0.0 };
        let aspect = if aspect > 0.0 { aspect.clamp(0.3, 1.0) } else { 0.5 };
        self.layout(cols, rows, aspect);
        self.tones = tones;
        self.glyph.fill(0);
        self.ink.fill(0);
        self.part.fill(EMPTY);
        self.depth.fill(-1e9);
        if cols == 0 || rows == 0 {
            return;
        }
        self.cx = self.cols as f64 / 2.0;
        self.cy = self.lip_row;
        self.rise = self.unit * self.cw / self.ch;
        if extras.water {
            self.draw_dust(t);
        }
        if extras.shoal {
            self.draw_shoal(t);
        }
        self.draw_twinkles(t, extras.water);
        self.draw_bell(t);
        self.draw_arms(t);
        self.draw_tentacles(t);
        self.draw_swirl(t);
        self.draw_halos(t);
        self.draw_sparks(t);
    }

    /// The characters drawn, row after row: 0 where there is none.
    pub fn glyphs(&self) -> &[u16] {
        &self.glyph
    }

    /// The colour of each, 0xRRGGBB: 0 where there is none.
    pub fn inks(&self) -> &[u32] {
        &self.ink
    }

    /// Whether a cell shows the jellyfish itself (bell, lip, arm or tentacle), and not its glitter or the water.
    pub fn is_animal(&self, cell: usize) -> bool {
        self.part.get(cell).is_some_and(|part| (BELL..=TENTACLE).contains(part))
    }

    // ── fitting it to the window ────────────────────────────────────────────────────────────

    fn layout(&mut self, cols: usize, rows: usize, aspect: f64) {
        self.cw = aspect;
        self.ch = 1.0;
        if (cols, rows) != (self.cols, self.rows) {
            let cells = cols * rows;
            self.cols = cols;
            self.rows = rows;
            self.glyph = vec![0; cells];
            self.ink = vec![0; cells];
            self.depth = vec![0.0; cells];
            self.part = vec![0; cells];
            self.owner = vec![0; cells];
        }

        // the jellyfish is five bell-radii tall and wants a little over four of them across
        self.unit = ((rows as f64 - 2.0) / (5.0 * aspect)).min(cols as f64 / 4.4).clamp(11.0, 30.0);
        let rise = self.unit * aspect;
        let spare = rows as f64 - 2.0 - 5.0 * rise; // a row above it and a row below
        // in a small window the tentacles run off the bottom
        self.lip_row = 1.0 + (spare / 2.0).max(0.0) + 1.8 * rise;
    }
}
