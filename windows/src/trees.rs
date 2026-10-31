//! "Window bonsai": pixel-art snowy pines that grow on the top edges of the window being used and on the taskbar,
//! and fall to a quick swipe of the pointer.
//!
//! Everything here is plain data and maths, so it is tested on any platform. The trees, the falling needles and
//! snow, the little score text and the swipe trail all end up as square [`Sprite`]s (see `PIXEL_LUMP`) that
//! the overlay draws after the weather, in the same pass.
//!
//! Coordinates are the overlay's: DIPs, y pointing up from the bottom of the work area. The taskbar is the
//! ground at y = 0.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::geometry::{Rect, V2};
use crate::simulation::{Pace, Sprite, WindowId, WindowSurface};

/// Size of one art pixel in DIPs.
pub const PX: f32 = 2.0;
/// The taskbar, which is the ground. No window has this id.
pub const GROUND: WindowId = 0;
/// Marks a sprite as a flat, hard-edged coloured square; the shaders draw it as such.
pub const PIXEL_LUMP: f32 = -1000.0;

const VARIANTS: usize = 8;
const MAX_TREES: usize = 36;
const MAX_BITS: usize = 1500;
const CUT_SPEED: f32 = 1000.0;
const GRAVITY: f32 = 620.0;
const ACTIVE_FOR: f32 = 2.5;

static WOOD: AtomicU32 = AtomicU32::new(0);
static SNOW: AtomicU32 = AtomicU32::new(0);

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

const OUT: u8 = 0;
const G1: u8 = 1;
const G2: u8 = 2;
const G3: u8 = 3;
const FROST: u8 = 4;
const FROST2: u8 = 5;
const FROST3: u8 = 6;
const TRUNK: u8 = 7;
const TRUNK2: u8 = 8;
const WOOD_TEXT: u8 = 9;
const SNOW_TEXT: u8 = 10;
const SHADOW: u8 = 11;

const PALETTE: [[f32; 3]; 12] = [
    rgb(11, 36, 23),
    rgb(23, 73, 44),
    rgb(34, 112, 58),
    rgb(52, 147, 63),
    rgb(246, 251, 255),
    rgb(196, 227, 244),
    rgb(141, 191, 220),
    rgb(122, 74, 40),
    rgb(77, 46, 25),
    rgb(134, 239, 172),
    rgb(224, 242, 254),
    rgb(2, 6, 23),
];

// ---------------------------------------------------------------------------------------------- random numbers

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    /// 0 up to, not including, 1.
    fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.f()
    }
}

// ---------------------------------------------------------------------------------------------- pixel art

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Sprout,
    Sapling,
    Mature,
}

impl Stage {
    fn of(growth: f32) -> Stage {
        if growth < 0.22 {
            Stage::Sprout
        } else if growth < 0.55 {
            Stage::Sapling
        } else {
            Stage::Mature
        }
    }
}

struct Preset {
    tiers: usize,
    tier_height: usize,
    step: f32,
    trunk: usize,
    trunk_width: usize,
}

const PRESETS: [Preset; 3] = [
    Preset { tiers: 1, tier_height: 4, step: 1.2, trunk: 2, trunk_width: 1 },
    Preset { tiers: 3, tier_height: 6, step: 1.8, trunk: 3, trunk_width: 2 },
    Preset { tiers: 6, tier_height: 8, step: 2.2, trunk: 6, trunk_width: 3 },
];

/// A tree drawn once as a list of coloured cells: column from the trunk's centre, row up from the base, palette index.
pub struct Art {
    pub width: usize,
    pub height: usize,
    cells: Vec<(i16, i16, u8)>,
}

fn half_top(p: &Preset, tier: usize) -> f32 {
    0.4 + tier as f32 * p.step * 0.9
}

fn half_bottom(p: &Preset, tier: usize) -> f32 {
    half_top(p, tier) + p.step * 1.5 + 0.8
}

/// A pine of stacked, ragged cones. The open top edge of every cone gets frost, the underside gets shade.
fn build_art(p: &Preset, seed: u64) -> Art {
    let mut rng = Rng(seed | 1);
    let last = p.tiers - 1;
    let max_half = half_bottom(p, last).ceil() as usize + 2;
    let width = max_half * 2 + 1;
    let centre = max_half;
    let height = last * (p.tier_height - 3) + p.tier_height + p.trunk;
    let mut grid = vec![vec![-1i8; width]; height];

    for tier in 0..p.tiers {
        let y0 = tier * (p.tier_height - 3); // tiers overlap by three rows
        for row in 0..p.tier_height {
            let t = row as f32 / (p.tier_height - 1) as f32;
            let v = half_top(p, tier) + (half_bottom(p, tier) - half_top(p, tier)) * t;
            let mut half = (v + (rng.f() - 0.5) * 0.9).round().max(0.0) as i32;
            if tier == 0 && row == 0 {
                half = 0; // a sharp tip
            }
            if row == p.tier_height - 1 {
                half += 1; // the boughs droop past the tier
            }
            for dx in -half..=half {
                // ragged lower edge
                if row == p.tier_height - 1 && dx.abs() >= half - 1 && ((dx + tier as i32) & 1) != 0 {
                    continue;
                }
                grid[y0 + row][(centre as i32 + dx) as usize] = tier as i8;
            }
        }
    }
    let trunk_left = centre - (p.trunk_width >> 1);
    for row in grid.iter_mut().skip(height - p.trunk) {
        for cell in &mut row[trunk_left..trunk_left + p.trunk_width] {
            *cell = 100;
        }
    }

    let at = |y: i32, x: i32| -> i8 {
        if y < 0 || y >= height as i32 || x < 0 || x >= width as i32 { -1 } else { grid[y as usize][x as usize] }
    };
    let mut cells = Vec::new();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let k = at(y, x);
            if k < 0 {
                continue;
            }
            let color = if k == 100 {
                if x == (trunk_left + p.trunk_width - 1) as i32 || rng.f() < 0.25 { TRUNK2 } else { TRUNK }
            } else {
                let (up, up2, down) = (at(y - 1, x), at(y - 2, x), at(y + 1, x));
                let (left, right) = (at(y, x - 1), at(y, x + 1));
                if up < 0 {
                    if rng.f() < 0.88 { FROST } else { FROST2 } // snow cap of the tier
                } else if up2 < 0 && rng.f() < 0.5 {
                    FROST2
                } else if (left < 0 || right < 0) && rng.f() < 0.28 {
                    FROST3 // rime on the sides
                } else if down != k {
                    if rng.f() < 0.5 { OUT } else { G1 } // shade under the tier
                } else {
                    let v = rng.f();
                    if x > centre as i32 && v < 0.5 {
                        G1
                    } else if v < 0.2 {
                        G3
                    } else if v < 0.7 {
                        G2
                    } else {
                        G1
                    }
                }
            };
            cells.push(((x - centre as i32) as i16, (height as i32 - 1 - y) as i16, color));
        }
    }
    Art { width, height, cells }
}

// ---------------------------------------------------------------------------------------------- tiny pixel font

/// 3x5 glyphs for the few characters the score and the floating texts need; rows from the top, three bits each.
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        'W' => [0b101, 0b101, 0b101, 0b111, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'o' => [0b000, 0b111, 0b101, 0b101, 0b111],
        'd' => [0b001, 0b001, 0b111, 0b101, 0b111],
        'n' => [0b000, 0b110, 0b101, 0b101, 0b101],
        'w' => [0b000, 0b101, 0b101, 0b111, 0b010],
        _ => [0; 5],
    }
}

fn pixel(position: V2, size: f32, color: u8, alpha: f32) -> Sprite {
    Sprite {
        position,
        velocity: V2::ZERO,
        size,
        alpha,
        soft: 0.0,
        tint_amount: 1.0,
        tint: PALETTE[color as usize],
        lump: PIXEL_LUMP,
    }
}

/// `origin` is the text's top left.
fn push_text(out: &mut Vec<Sprite>, text: &str, origin: V2, color: u8, alpha: f32) {
    for pass in 0..2 {
        let (shift, tint, a) = if pass == 0 { (V2::new(1.0, -1.0), SHADOW, alpha * 0.7) } else { (V2::ZERO, color, alpha) };
        for (i, c) in text.chars().enumerate() {
            let rows = glyph(c);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..3 {
                    if (bits >> (2 - col)) & 1 == 1 {
                        let x = origin.x + (i * 4 + col) as f32 * PX + PX / 2.0;
                        let y = origin.y - row as f32 * PX - PX / 2.0;
                        out.push(pixel(V2::new(x, y) + shift, PX, tint, a));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- the forest

/// What the user is doing, which decides where trees grow.
pub struct Input {
    /// The window that has the focus; [`GROUND`] if none.
    pub foreground: WindowId,
    /// Typing or clicking lately: the taskbar grows trees too.
    pub typing: bool,
}

/// A place trees can stand: the top edge of a window, or the ground.
#[derive(Clone, PartialEq)]
struct Slot {
    id: WindowId,
    frame: Rect,
    blockers: Vec<Rect>,
}

impl Slot {
    fn top(&self) -> f32 {
        self.frame.max_y()
    }

    /// Is the point hidden behind a window in front?
    fn blocked(&self, x: f32, y: f32) -> bool {
        self.id != GROUND && self.blockers.iter().any(|b| x >= b.min_x() && x <= b.max_x() && y >= b.min_y() && y <= b.max_y())
    }
}

struct Tree {
    slot: WindowId,
    /// Distance from the slot's left edge.
    x: f32,
    growth: f32,
    variant: usize,
    stage: Stage,
    /// Seconds without a place to stand, as when its window is minimized.
    orphaned: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Needle,
    Flake,
}

struct Rest {
    slot: WindowId,
    column: usize,
    level: u8,
    life: f32,
}

/// A needle or a bit of snow, falling or lying in a small pile that thins away.
struct Bit {
    position: V2,
    velocity: V2,
    kind: Kind,
    color: u8,
    bounces: u8,
    phase: f32,
    rest: Option<Rest>,
}

struct Floating {
    position: V2,
    age: f32,
    text: &'static str,
    color: u8,
}

struct Streak {
    from: V2,
    to: V2,
    age: f32,
}

pub struct Forest {
    width: f32,
    height: f32,
    arts: Vec<Vec<Art>>,
    slots: Vec<Slot>,
    trees: Vec<Tree>,
    bits: Vec<Bit>,
    texts: Vec<Floating>,
    streaks: Vec<Streak>,
    piles: HashMap<WindowId, Vec<u8>>,
    planting: HashMap<WindowId, f32>,
    active: HashMap<WindowId, f32>,
    cursor: Option<V2>,
    rng: Rng,
    time: f32,
    score_seen: (u32, u32),
    out: Vec<Sprite>,
}

impl Forest {
    pub fn new(width: f32, height: f32) -> Forest {
        let arts = PRESETS
            .iter()
            .enumerate()
            .map(|(stage, preset)| (0..VARIANTS).map(|v| build_art(preset, (stage * 977 + v * 7919 + 13) as u64)).collect())
            .collect();
        Forest {
            width,
            height,
            arts,
            slots: Vec::new(),
            trees: Vec::new(),
            bits: Vec::new(),
            texts: Vec::new(),
            streaks: Vec::new(),
            piles: HashMap::new(),
            planting: HashMap::new(),
            active: HashMap::new(),
            cursor: None,
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            time: 0.0,
            score_seen: (u32::MAX, 0),
            out: Vec::new(),
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
        self.slots.clear();
    }

    /// How many sprites [`Forest::write`] will produce.
    pub fn sprite_count(&self) -> usize {
        self.out.len()
    }

    pub fn write(&self, sprites: &mut [Sprite]) -> usize {
        let n = self.out.len().min(sprites.len());
        sprites[..n].copy_from_slice(&self.out[..n]);
        n
    }

    pub fn pace(&self) -> Pace {
        if !self.streaks.is_empty() || self.bits.iter().any(|b| b.rest.is_none()) {
            Pace::Interactive
        } else if !self.bits.is_empty() || !self.texts.is_empty() {
            Pace::Lively
        } else {
            Pace::Calm
        }
    }

    fn slot(&self, id: WindowId) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// Advances everything; returns whether the picture changed.
    pub fn step(&mut self, dt: f32, surfaces: &[WindowSurface], cursor: V2, input: &Input) -> bool {
        let dt = dt.clamp(0.0, 0.05);
        self.time += dt;
        let mut dirty = false;

        let mut slots: Vec<Slot> = surfaces.iter().map(|s| Slot { id: s.id, frame: s.frame, blockers: s.blockers.clone() }).collect();
        slots.push(Slot { id: GROUND, frame: Rect { x: 0.0, y: 0.0, width: self.width, height: 0.0 }, blockers: Vec::new() });
        if slots != self.slots {
            self.slots = slots;
            dirty = true;
        }

        // Where the user is working, trees grow, for a moment after as well.
        if input.foreground != GROUND {
            self.active.insert(input.foreground, ACTIVE_FOR);
        }
        if input.typing {
            self.active.insert(GROUND, ACTIVE_FOR);
        }
        self.active.retain(|_, left| {
            *left -= dt;
            *left > 0.0
        });

        for i in 0..self.slots.len() {
            let id = self.slots[i].id;
            if !self.active.contains_key(&id) {
                continue;
            }
            let clock = self.planting.entry(id).or_insert(0.0);
            *clock += dt;
            if *clock > 0.8 {
                *clock = 0.0;
                dirty |= self.plant(i);
            }
        }

        dirty |= self.grow(dt);
        dirty |= self.harvest(dt, cursor);
        self.move_bits(dt);

        for text in &mut self.texts {
            text.age += dt;
            if text.age > 0.0 {
                text.position.y += 30.0 * dt;
            }
        }
        self.texts.retain(|t| t.age < 1.3);
        for streak in &mut self.streaks {
            streak.age += dt;
        }
        self.streaks.retain(|s| s.age < 0.14);

        let score = (WOOD.load(Ordering::Relaxed), SNOW.load(Ordering::Relaxed));
        let animated = !self.bits.is_empty() || !self.texts.is_empty() || !self.streaks.is_empty();
        if dirty || animated || score != self.score_seen {
            self.score_seen = score;
            self.rebuild();
            return true;
        }
        false
    }

    /// Tries to start a sprout on a slot; returns whether one was planted.
    fn plant(&mut self, i: usize) -> bool {
        let (id, left, width) = (self.slots[i].id, self.slots[i].frame.x, self.slots[i].frame.width);
        let top = self.slots[i].top();
        if width < 48.0 || self.trees.len() >= MAX_TREES {
            return false;
        }
        let cap = ((width / (PX * 16.0)) as usize).clamp(1, 8);
        if self.trees.iter().filter(|t| t.slot == id).count() >= cap {
            return false;
        }
        let x = self.rng.range(14.0, width - 14.0);
        if self.trees.iter().any(|t| t.slot == id && (t.x - x).abs() < 26.0) {
            return false;
        }
        let at = left + x;
        if at < 10.0 || at > self.width - 10.0 || self.slots[i].blocked(at, top - 1.0) {
            return false;
        }
        let variant = (self.rng.next() as usize) % VARIANTS;
        self.trees.push(Tree { slot: id, x, growth: 0.0, variant, stage: Stage::Sprout, orphaned: 0.0 });
        true
    }

    fn grow(&mut self, dt: f32) -> bool {
        let mut changed = false;
        let mut puffs = Vec::new();
        let mut gone = Vec::new();
        for (index, tree) in self.trees.iter_mut().enumerate() {
            let Some(slot) = self.slots.iter().find(|s| s.id == tree.slot) else {
                tree.orphaned += dt;
                if tree.orphaned > 30.0 {
                    gone.push(index);
                }
                continue;
            };
            tree.orphaned = 0.0;
            if tree.x > slot.frame.width - 6.0 {
                gone.push(index); // the window was made too narrow for it
                continue;
            }
            if tree.growth < 1.0 && self.active.contains_key(&tree.slot) {
                tree.growth = (tree.growth + dt / if tree.slot == GROUND { 22.0 } else { 16.0 }).min(1.0);
                let stage = Stage::of(tree.growth);
                if stage != tree.stage {
                    tree.stage = stage;
                    changed = true;
                    puffs.push(V2::new(slot.frame.x + tree.x, slot.top() + 6.0));
                }
            }
        }
        for &index in gone.iter().rev() {
            self.trees.remove(index);
            changed = true;
        }
        for at in puffs {
            for _ in 0..6 {
                let (x, y) = (self.rng.range(-8.0, 8.0), self.rng.range(0.0, 14.0));
                let vx = self.rng.range(-20.0, 20.0);
                self.spawn_bit(Kind::Flake, V2::new(at.x + x, at.y + y), V2::new(vx, 10.0));
            }
        }
        changed
    }

    /// The pointer is a pruner: a fast swipe through a full-grown tree cuts it down.
    fn harvest(&mut self, dt: f32, cursor: V2) -> bool {
        if cursor.x < -5000.0 || cursor.y < -5000.0 || dt <= 0.0 {
            self.cursor = None;
            return false;
        }
        let Some(previous) = self.cursor.replace(cursor) else { return false };
        let travel = cursor - previous;
        let distance = travel.length();
        if distance / dt < CUT_SPEED {
            return false;
        }
        self.streaks.push(Streak { from: previous, to: cursor, age: 0.0 });

        let steps = ((distance / 6.0).ceil() as usize).max(1);
        let mut hit = Vec::new();
        for (index, tree) in self.trees.iter().enumerate() {
            if tree.stage != Stage::Mature {
                continue;
            }
            let Some(slot) = self.slot(tree.slot) else { continue };
            let x = slot.frame.x + tree.x;
            if slot.blocked(x, slot.top() - 1.0) {
                continue;
            }
            let art = &self.arts[2][tree.variant];
            let half = art.width as f32 * PX * 0.4;
            let (low, high) = (slot.top() + 3.0, slot.top() + art.height as f32 * PX);
            let touched = (0..=steps).any(|k| {
                let p = previous + travel * (k as f32 / steps as f32);
                p.x > x - half && p.x < x + half && p.y > low && p.y < high
            });
            if touched {
                hit.push(index);
            }
        }
        for &index in hit.iter().rev() {
            self.cut(index, travel);
        }
        !hit.is_empty()
    }

    fn cut(&mut self, index: usize, swipe: V2) {
        let tree = self.trees.remove(index);
        let Some(slot) = self.slot(tree.slot) else { return };
        let (x, top) = (slot.frame.x + tree.x, slot.top());
        let art = &self.arts[2][tree.variant];
        let (w, h) = (art.width as f32 * PX, art.height as f32 * PX);
        for _ in 0..30 {
            let position = V2::new(x + self.rng.range(-0.35, 0.35) * w, top + self.rng.range(0.0, 1.0) * h);
            let velocity = V2::new(self.rng.range(-130.0, 130.0) + swipe.x * 4.0, self.rng.range(60.0, 280.0));
            self.spawn_bit(Kind::Needle, position, velocity);
        }
        for _ in 0..40 {
            let position = V2::new(x + self.rng.range(-0.4, 0.4) * w, top + self.rng.range(0.0, 1.0) * h);
            let velocity = V2::new(self.rng.range(-60.0, 60.0), self.rng.range(0.0, 90.0));
            self.spawn_bit(Kind::Flake, position, velocity);
        }
        WOOD.fetch_add(10, Ordering::Relaxed);
        SNOW.fetch_add(25, Ordering::Relaxed);
        let y = top + h * 0.6;
        self.texts.push(Floating { position: V2::new(x - 4.0 * PX * 4.0, y), age: 0.0, text: "+10 Wood", color: WOOD_TEXT });
        self.texts.push(Floating { position: V2::new(x - 4.0 * PX * 4.0, y + 8.0 * PX), age: -0.12, text: "+25 Snow", color: SNOW_TEXT });
    }

    fn spawn_bit(&mut self, kind: Kind, position: V2, velocity: V2) {
        if self.bits.len() >= MAX_BITS {
            return;
        }
        let color = match kind {
            Kind::Needle => [G1, G2, G3][(self.rng.next() % 3) as usize],
            Kind::Flake => if self.rng.f() < 0.6 { FROST } else { FROST2 },
        };
        let phase = self.rng.range(0.0, 6.28);
        self.bits.push(Bit { position, velocity, kind, color, bounces: 0, phase, rest: None });
    }

    /// Gravity, bounces off the top edges of windows and the taskbar, and the piles that lie there and thin away.
    fn move_bits(&mut self, dt: f32) {
        let (slots, piles, rng, time, width) = (&self.slots, &mut self.piles, &mut self.rng, self.time, self.width);
        self.bits.retain_mut(|b| {
            if let Some(rest) = &mut b.rest {
                rest.life -= dt;
                let present = slots.iter().any(|s| s.id == rest.slot && (rest.column as f32) * PX < s.frame.width);
                if rest.life > 0.0 && present {
                    return true;
                }
                if let Some(h) = piles.get_mut(&rest.slot).and_then(|v| v.get_mut(rest.column)) {
                    *h = h.saturating_sub(1);
                }
                return false;
            }
            match b.kind {
                Kind::Needle => b.velocity.y -= GRAVITY * dt,
                Kind::Flake => {
                    b.velocity.y = (b.velocity.y - 140.0 * dt).max(-70.0);
                    let sway = (time * 2.0 + b.phase).sin() * 14.0;
                    b.velocity.x += (sway - b.velocity.x) * (dt * 2.0).min(1.0);
                }
            }
            let before = b.position;
            b.position = b.position + b.velocity * dt;
            if b.velocity.y < 0.0 {
                let mut landing: Option<&Slot> = None;
                for s in slots {
                    let top = s.top();
                    if b.position.x >= s.frame.min_x()
                        && b.position.x <= s.frame.max_x()
                        && before.y >= top
                        && b.position.y <= top
                        && !s.blocked(b.position.x, top - 1.0)
                        && landing.is_none_or(|l| top > l.top())
                    {
                        landing = Some(s);
                    }
                }
                if let Some(s) = landing {
                    if b.kind == Kind::Needle && b.bounces < 2 {
                        b.position.y = s.top() + 0.5;
                        b.velocity.y *= -0.35;
                        b.velocity.x *= 0.6;
                        b.bounces += 1;
                        return true;
                    }
                    let column = ((b.position.x - s.frame.x) / PX).max(0.0) as usize;
                    let heights = piles.entry(s.id).or_default();
                    if heights.len() <= column {
                        heights.resize(column + 1, 0);
                    }
                    if heights[column] >= 4 {
                        return false;
                    }
                    let level = heights[column];
                    heights[column] += 1;
                    let life = (4.0 + rng.f() * 4.0 - level as f32 * 0.9).max(1.2);
                    b.rest = Some(Rest { slot: s.id, column, level, life });
                    b.velocity = V2::ZERO;
                    return true;
                }
            }
            b.position.y > -20.0 && b.position.x > -20.0 && b.position.x < width + 20.0
        });
    }

    /// Lays out every visible square, back to front.
    fn rebuild(&mut self) {
        let mut out = std::mem::take(&mut self.out);
        out.clear();

        for tree in &self.trees {
            let Some(slot) = self.slot(tree.slot) else { continue };
            let x = slot.frame.x + tree.x;
            if slot.blocked(x, slot.top() - 1.0) {
                continue;
            }
            let centre = (x / PX).round() * PX;
            let base = slot.top();
            for &(dx, dy, color) in &self.arts[tree.stage as usize][tree.variant].cells {
                out.push(pixel(V2::new(centre + dx as f32 * PX, base + (dy as f32 + 0.5) * PX), PX, color, 1.0));
            }
        }

        for bit in &self.bits {
            match &bit.rest {
                Some(rest) => {
                    let Some(slot) = self.slot(rest.slot) else { continue };
                    let x = slot.frame.x + rest.column as f32 * PX + PX / 2.0;
                    if slot.blocked(x, slot.top() - 1.0) {
                        continue;
                    }
                    let y = slot.top() + (rest.level as f32 + 0.5) * PX;
                    out.push(pixel(V2::new(x, y), PX, bit.color, (rest.life / 1.2).min(1.0)));
                }
                None => out.push(pixel(bit.position, PX, bit.color, 1.0)),
            }
        }

        for streak in &self.streaks {
            let fade = 1.0 - streak.age / 0.14;
            let line = streak.to - streak.from;
            let n = ((line.length() / 4.0).ceil() as usize).max(1);
            for k in 0..=n {
                out.push(pixel(streak.from + line * (k as f32 / n as f32), PX, FROST, fade));
            }
        }

        for text in &self.texts {
            if text.age > 0.0 {
                push_text(&mut out, text.text, text.position, text.color, (1.0 - text.age / 1.3).max(0.0));
            }
        }

        let (wood, snow) = self.score_seen;
        if wood + snow > 0 {
            let corner = |line: f32, s: &str| V2::new(self.width - 12.0 - s.len() as f32 * 4.0 * PX, self.height - 12.0 - line * 8.0 * PX);
            let (a, b) = (format!("Wood {wood}"), format!("Snow {snow}"));
            push_text(&mut out, &a, corner(0.0, &a), WOOD_TEXT, 1.0);
            push_text(&mut out, &b, corner(1.0, &b), SNOW_TEXT, 1.0);
        }
        self.out = out;
    }
}

/// The livelier of two paces.
pub fn busiest(a: Pace, b: Pace) -> Pace {
    let rank = |p: Pace| match p {
        Pace::Calm => 0,
        Pace::Lively => 1,
        Pace::Interactive => 2,
    };
    if rank(b) > rank(a) { b } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> WindowSurface {
        WindowSurface {
            id: 7,
            frame: Rect { x: 200.0, y: 300.0, width: 400.0, height: 300.0 },
            blockers: Vec::new(),
            corner_radius: 0.0,
            corner_exponent: 2.0,
        }
    }

    fn grown() -> Forest {
        let mut forest = Forest::new(1000.0, 800.0);
        let focus = Input { foreground: 7, typing: false };
        for _ in 0..800 {
            forest.step(0.05, &[window()], V2::new(-9999.0, -9999.0), &focus);
        }
        forest
    }

    #[test]
    fn every_stage_has_art_with_frost_and_a_trunk() {
        for stage in 0..3 {
            for art in &PRESETS.iter().map(|p| build_art(p, 5)).collect::<Vec<_>>()[stage..=stage] {
                assert!(art.cells.iter().any(|c| c.2 == FROST));
                assert!(art.cells.iter().any(|c| c.2 == TRUNK || c.2 == TRUNK2));
                assert!(art.cells.iter().all(|c| c.1 >= 0 && (c.1 as usize) < art.height));
            }
        }
    }

    #[test]
    fn trees_grow_only_where_the_user_works() {
        let mut idle = Forest::new(1000.0, 800.0);
        let away = Input { foreground: GROUND, typing: false };
        for _ in 0..200 {
            idle.step(0.05, &[window()], V2::new(-9999.0, -9999.0), &away);
        }
        assert!(idle.trees.is_empty());

        let forest = grown();
        assert!(!forest.trees.is_empty());
        assert!(forest.trees.iter().all(|t| t.slot == 7));
        assert!(forest.trees.iter().any(|t| t.stage == Stage::Mature));
    }

    #[test]
    fn a_fast_swipe_cuts_a_mature_tree_and_a_slow_one_does_not() {
        let mut forest = grown();
        let focus = Input { foreground: 7, typing: false };
        let tree = forest.trees.iter().find(|t| t.stage == Stage::Mature).unwrap();
        let (x, y) = (200.0 + tree.x, 600.0 + 30.0);
        let before = forest.trees.len();

        forest.step(0.016, &[window()], V2::new(x - 30.0, y), &focus);
        forest.step(0.016, &[window()], V2::new(x - 29.0, y), &focus);
        assert_eq!(forest.trees.len(), before, "slow movement must not cut");

        forest.step(0.016, &[window()], V2::new(x + 30.0, y), &focus);
        assert_eq!(forest.trees.len(), before - 1);
        assert!(!forest.bits.is_empty());
        assert!(forest.sprite_count() > 0);
    }

    #[test]
    fn falling_needles_land_on_the_window_below_and_fade() {
        let mut forest = Forest::new(1000.0, 800.0);
        forest.step(0.016, &[window()], V2::new(-9999.0, -9999.0), &Input { foreground: GROUND, typing: false });
        forest.spawn_bit(Kind::Flake, V2::new(300.0, 640.0), V2::new(0.0, -50.0));
        for _ in 0..200 {
            forest.step(0.016, &[window()], V2::new(-9999.0, -9999.0), &Input { foreground: GROUND, typing: false });
        }
        assert!(forest.bits.iter().any(|b| b.rest.as_ref().is_some_and(|r| r.slot == 7)));
        for _ in 0..1000 {
            forest.step(0.016, &[window()], V2::new(-9999.0, -9999.0), &Input { foreground: GROUND, typing: false });
        }
        assert!(forest.bits.is_empty());
    }
}
