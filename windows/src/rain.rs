use std::collections::hash_map::RandomState;
use std::f32::consts::PI;
use std::hash::{BuildHasher, Hasher};

use crate::geometry::{ V2, smoothstep };
use crate::simulation::{ Pace, Snowfall, Sprite, WindowId, WindowSurface };
use crate::weather::{ DrawOutput, Weather };

const MAX_DROPS: usize = 2500;
const MAX_SPLASHES: usize = 1200;
const MAX_TRICKLES: usize = 384;

const SIDE_MARGIN: f32 = 180.0;
const TOP_SPAWN_BAND: f32 = 48.0;
const WIND_RESPONSE: f32 = 6.0;
const SPLASH_GRAVITY: f32 = -1650.0;
const DRIP_GRAVITY: f32 = -1450.0;
const RAIN_STREAK_LUMP: f32 = -1.0;

const DROP_TINT: [f32; 3] = [0.76, 0.85, 0.96];
const SPLASH_TINT: [f32; 3] = [0.82, 0.89, 0.98];

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        Rng(RandomState::new().build_hasher().finish().max(1))
    }

    fn seeded(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, lower: f32, upper: f32) -> f32 {
        lower + (upper - lower) * self.next()
    }
}

#[derive(Clone, Copy)]
struct Drop {
    position: V2,
    velocity: V2,
    fall_speed: f32,
    depth: f32,
    size: f32,
    alpha: f32,
    wind_scale: f32,
}

#[derive(Clone, Copy)]
struct Splash {
    position: V2,
    velocity: V2,
    size: f32,
    alpha: f32,
    age: f32,
    life: f32,
}

#[derive(Clone, Copy)]
struct Trickle {
    window_id: WindowId,
    position: V2,
    velocity: V2,
    offset_x: f32,
    drift_bias: f32,
    size: f32,
    alpha: f32,
    age: f32,
    ledge_life: f32,
    falling: bool,
}

#[derive(Clone, Copy)]
struct Gust {
    start: f64,
    duration: f32,
    peak_wind: f32,
}

#[derive(Clone, Copy)]
struct Hit {
    t: f32,
    point: V2,
    window_index: Option<usize>,
}

pub struct Rain {
    width: f32,
    height: f32,
    intensity: Snowfall,
    density: f32,
    trickle: bool,
    clock: f64,
    strength: f32,
    strength_target: f32,
    next_shift: f64,
    gust: Option<Gust>,
    next_gust: f64,
    last_wind: f32,
    spawn_debt: f32,
    rng: Rng,
    drops: Vec<Drop>,
    splashes: Vec<Splash>,
    trickles: Vec<Trickle>,
}

impl Rain {
    pub fn new(width: f32, height: f32, intensity: Snowfall) -> Rain {
        Self::build(width, height, intensity, Rng::new())
    }

    pub fn with_seed(width: f32, height: f32, intensity: Snowfall, seed: u64) -> Rain {
        Self::build(width, height, intensity, Rng::seeded(seed))
    }

    fn build(width: f32, height: f32, intensity: Snowfall, mut rng: Rng) -> Rain {
        let next_gust = rng.range(4.0, 10.0) as f64;
        Rain {
            width,
            height,
            intensity,
            density: 1.0,
            trickle: false,
            clock: 0.0,
            strength: 1.0,
            strength_target: 1.0,
            next_shift: 0.0,
            gust: None,
            next_gust,
            last_wind: 0.0,
            spawn_debt: 0.0,
            rng,
            drops: Vec::with_capacity(MAX_DROPS),
            splashes: Vec::with_capacity(MAX_SPLASHES),
            trickles: Vec::with_capacity(MAX_TRICKLES),
        }
    }

    pub fn set_trickle(&mut self, enabled: bool) {
        self.trickle = enabled;
        if !enabled {
            self.trickles.clear();
        }
    }

    pub fn trickle(&self) -> bool {
        self.trickle
    }

    pub fn current_wind(&self) -> f32 {
        self.last_wind
    }

    pub fn drop_count(&self) -> usize {
        self.drops.len()
    }

    pub fn splash_count(&self) -> usize {
        self.splashes.len()
    }

    pub fn trickle_count(&self) -> usize {
        self.trickles.len()
    }

    /// Injects a single raindrop directly (used by unit tests for deterministic kinematic & collision checks).
    pub fn spawn_drop_at(&mut self, position: V2, velocity: V2, depth: f32) {
        if self.drops.len() >= MAX_DROPS {
            return;
        }
        let d = depth.clamp(0.0, 1.0);
        let fall_speed = (-velocity.y).max(200.0);
        self.drops.push(Drop {
            position,
            velocity: V2::new(velocity.x, -fall_speed),
            fall_speed,
            depth: d,
            size: 0.95 + d * 0.95,
            alpha: 0.24 + d * 0.32,
            wind_scale: 0.65 + d * 0.35,
        });
    }

    fn drops_per_megapoint(intensity: Snowfall) -> f32 {
        match intensity {
            Snowfall::Off => 0.0,
            Snowfall::Light => 110.0,
            Snowfall::Steady => 280.0,
            Snowfall::Heavy => 620.0,
            Snowfall::Variable => 280.0,
        }
    }

    fn target_drops(&self) -> f32 {
        let area = (self.width.max(1.0) * self.height.max(1.0)) / 1_000_000.0;
        let count = Self::drops_per_megapoint(self.intensity) * area * self.density * self.strength;
        count.min(MAX_DROPS as f32)
    }

    fn vary_intensity(&mut self, dt: f32) {
        if self.intensity != Snowfall::Variable {
            return;
        }
        if self.clock >= self.next_shift {
            self.strength_target = 0.25 + self.rng.next().powf(1.4) * 1.85;
            self.next_shift = self.clock + self.rng.range(10.0, 26.0) as f64;
        }
        let blend = 1.0 - (-dt / 6.0).exp();
        self.strength += (self.strength_target - self.strength) * blend;
    }

    fn sample_wind(&mut self) -> f32 {
        let t = self.clock as f32;
        let ambient = 58.0 * (t * 0.19).sin() + 34.0 * (t * 0.47 + 1.3).sin() + 18.0 * (t * 1.15 + 2.7).cos();

        if let Some(gust) = self.gust
            && self.clock - gust.start > gust.duration as f64
        {
            self.gust = None;
        }
        if self.gust.is_none() && self.clock >= self.next_gust {
            let duration = self.rng.range(2.2, 4.6);
            let sign = if ambient >= 0.0 {
                if self.rng.next() < 0.75 { 1.0 } else { -1.0 }
            } else if self.rng.next() < 0.75 {
                -1.0
            } else {
                1.0
            };
            let peak_wind = sign * self.rng.range(135.0, 290.0);
            self.gust = Some(Gust { start: self.clock, duration, peak_wind });
            self.next_gust = self.clock + (duration + self.rng.range(6.5, 15.0)) as f64;
        }

        let gust_wind = if let Some(gust) = self.gust {
            let u = ((self.clock - gust.start) as f32 / gust.duration).clamp(0.0, 1.0);
            // Smooth attack and gradual decay so the rain slant leans smoothly into the gust.
            let envelope = smoothstep(0.0, 0.24, u) * (1.0 - smoothstep(0.45, 1.0, u));
            gust.peak_wind * envelope
        } else {
            0.0
        };

        ambient + gust_wind
    }

    fn spawn_drops(&mut self, dt: f32, wind: f32, surfaces: &[WindowSurface]) {
        let target = self.target_drops();
        if target <= 0.0 || self.height <= 1.0 {
            self.spawn_debt = 0.0;
            return;
        }
        // Mean vertical speed across depth distribution is ~1010 DIP/s.
        // Flux = target_count * mean_vertical_speed / (height + TOP_SPAWN_BAND).
        let mean_speed = 1010.0;
        let flux = target * mean_speed / (self.height + TOP_SPAWN_BAND);
        self.spawn_debt = (self.spawn_debt + flux * dt).min((MAX_DROPS - self.drops.len()) as f32);

        let count = self.spawn_debt.floor() as usize;
        self.spawn_debt -= count as f32;

        let span = self.width + 2.0 * SIDE_MARGIN;
        for _ in 0..count {
            if self.drops.len() >= MAX_DROPS {
                break;
            }
            let depth = self.rng.next().powf(0.8);
            let fall_speed = 700.0 + depth * 620.0 + self.rng.range(-65.0, 65.0);
            let wind_scale = 0.62 + depth * 0.40;
            let vx = wind * wind_scale + self.rng.range(-26.0, 26.0);
            let velocity = V2::new(vx, -fall_speed);

            let start_x = -SIDE_MARGIN + self.rng.next() * span;
            let start_y = self.height + self.rng.range(2.0, TOP_SPAWN_BAND);
            // Sub-frame birth offset tau in [0, dt] prevents horizontal sheet banding at lower frame rates.
            let tau = self.rng.range(0.0, dt);
            let prev = V2::new(start_x, start_y);
            let next = prev + velocity * tau;

            if let Some(hit) = first_hit(prev, next, self.width, surfaces) {
                let rem_dt = (1.0 - hit.t).max(0.0) * tau;
                self.on_drop_hit(hit, velocity, depth, rem_dt, surfaces);
            } else {
                let wrapped_x = wrap_x(next.x, self.width);
                self.drops.push(Drop {
                    position: V2::new(wrapped_x, next.y),
                    velocity,
                    fall_speed,
                    depth,
                    size: 0.96 + depth * 1.02 + self.rng.range(-0.10, 0.14),
                    alpha: 0.25 + depth * 0.36,
                    wind_scale,
                });
            }
        }
    }

    fn advance_drops(&mut self, dt: f32, wind: f32, surfaces: &[WindowSurface]) {
        let decay = (-WIND_RESPONSE * dt).exp();
        let integral = if WIND_RESPONSE * dt > 1e-4 {
            (1.0 - decay) / WIND_RESPONSE
        } else {
            dt
        };

        let mut i = 0;
        while i < self.drops.len() {
            let drop = self.drops[i];
            let prev = drop.position;
            let target_vx = wind * drop.wind_scale;
            let dvx = drop.velocity.x - target_vx;
            let next_vx = target_vx + dvx * decay;
            let next = V2::new(prev.x + target_vx * dt + dvx * integral, prev.y - drop.fall_speed * dt);
            let impact_vel = V2::new(next_vx, -drop.fall_speed);

            if let Some(hit) = first_hit(prev, next, self.width, surfaces) {
                self.drops.swap_remove(i);
                let rem_dt = (1.0 - hit.t).max(0.0) * dt;
                self.on_drop_hit(hit, impact_vel, drop.depth, rem_dt, surfaces);
                continue;
            }

            if next.y < -32.0 {
                self.drops.swap_remove(i);
                continue;
            }

            let slot = &mut self.drops[i];
            slot.position = V2::new(wrap_x(next.x, self.width), next.y);
            slot.velocity = impact_vel;
            i += 1;
        }
    }

    fn on_drop_hit(&mut self, hit: Hit, velocity: V2, depth: f32, rem_dt: f32, surfaces: &[WindowSurface]) {
        if hit.point.x >= -8.0 && hit.point.x <= self.width + 8.0 {
            let speed_scale = ((-velocity.y) / 980.0).clamp(0.7, 1.35);
            let splash_count = 3 + (self.rng.next() * 3.0) as usize; // 3..=5 particles
            for k in 0..splash_count {
                if self.splashes.len() >= MAX_SPLASHES {
                    break;
                }
                // Spread splash angles across an upward fan, slightly biased by incoming horizontal wind.
                let frac = (k as f32 + self.rng.range(0.15, 0.85)) / splash_count as f32;
                let angle = (frac - 0.5) * 2.1; // ~ -60 deg .. +60 deg around vertical
                let burst_speed = self.rng.range(65.0, 175.0) * speed_scale;
                let vx = angle.sin() * burst_speed + velocity.x * 0.10;
                let vy = angle.cos() * burst_speed;
                // Strictly 100-250 ms lifetime per specification.
                let life = self.rng.range(0.10, 0.25);

                if rem_dt < life {
                    let pos = V2::new(
                        hit.point.x + vx * rem_dt,
                        (hit.point.y + vy * rem_dt + 0.5 * SPLASH_GRAVITY * rem_dt * rem_dt).max(hit.point.y),
                    );
                    let vy_after = vy + SPLASH_GRAVITY * rem_dt;
                    self.splashes.push(Splash {
                        position: pos,
                        velocity: V2::new(vx, vy_after),
                        size: 1.05 + depth * 0.85 + self.rng.range(0.0, 0.35),
                        alpha: 0.36 + depth * 0.28,
                        age: rem_dt,
                        life,
                    });
                }
            }
        }

        if self.trickle
            && let Some(wi) = hit.window_index
            && let Some(surface) = surfaces.get(wi)
            && self.trickles.len() < MAX_TRICKLES
            && self.rng.next() < 0.28
        {
            let offset_x = (hit.point.x - surface.frame.min_x()).clamp(1.0, (surface.frame.width - 1.0).max(1.0));
            let near_left = offset_x < surface.frame.width * 0.5;
            let bias_sign = if near_left { -1.0 } else { 1.0 };
            self.trickles.push(Trickle {
                window_id: surface.id,
                position: V2::new(hit.point.x, hit.point.y + 0.6),
                velocity: V2::ZERO,
                offset_x,
                drift_bias: bias_sign * self.rng.range(6.0, 22.0),
                size: 1.55 + depth * 0.70,
                alpha: 0.38 + depth * 0.22,
                age: 0.0,
                ledge_life: self.rng.range(0.85, 2.10),
                falling: false,
            });
        }
    }

    fn advance_splashes(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.splashes.len() {
            let s = &mut self.splashes[i];
            s.age += dt;
            if s.age >= s.life {
                self.splashes.swap_remove(i);
                continue;
            }
            s.position.x += s.velocity.x * dt;
            s.position.y += s.velocity.y * dt + 0.5 * SPLASH_GRAVITY * dt * dt;
            s.velocity.y += SPLASH_GRAVITY * dt;
            if s.position.y < 0.0 {
                self.splashes.swap_remove(i);
                continue;
            }
            i += 1;
        }
    }

    fn advance_trickles(&mut self, dt: f32, wind: f32, surfaces: &[WindowSurface]) {
        if !self.trickle {
            self.trickles.clear();
            return;
        }
        let mut i = 0;
        while i < self.trickles.len() {
            let mut tr = self.trickles[i];
            tr.age += dt;

            if !tr.falling {
                if let Some(surface) = surfaces.iter().find(|s| s.id == tr.window_id) {
                    let span = surface.frame.width;
                    // Near rounded corners, gravity pulls the bead faster toward the corner to drip off.
                    let corner_pull = if surface.corner_radius > 0.0 {
                        if tr.offset_x < surface.corner_radius {
                            -42.0 * (1.0 - tr.offset_x / surface.corner_radius)
                        } else if span - tr.offset_x < surface.corner_radius {
                            42.0 * (1.0 - (span - tr.offset_x) / surface.corner_radius)
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };
                    let slide = wind * 0.11 + tr.drift_bias + corner_pull;
                    tr.offset_x += slide * dt;
                    let world_x = surface.frame.min_x() + tr.offset_x;
                    let corner_y = corner_drop(tr.offset_x, span, surface.corner_radius, surface.corner_exponent);
                    let world_y = surface.frame.max_y() + corner_y;

                    if tr.offset_x <= 0.5 || tr.offset_x >= span - 0.5 {
                        // Reached the top corner of the window: detach and drip downward along the side!
                        tr.falling = true;
                        let edge_x = surface.frame.min_x() + tr.offset_x.clamp(0.0, span);
                        tr.position = V2::new(edge_x, world_y - 1.0);
                        tr.velocity = V2::new(slide * 0.25, -45.0);
                    } else if occluded(world_x, world_y + 1.0, surface) || tr.age >= tr.ledge_life {
                        // Faded out on the ledge or covered by a front window.
                        self.trickles.swap_remove(i);
                        continue;
                    } else {
                        tr.position = V2::new(world_x, world_y + 0.5);
                        tr.velocity = V2::new(slide, 0.0);
                    }
                } else {
                    // Window closed or minimized: bead detaches into freefall.
                    tr.falling = true;
                    tr.velocity = V2::new(wind * 0.12, -35.0);
                }
            } else {
                let prev = tr.position;
                let next_vx = tr.velocity.x + (wind * 0.45 - tr.velocity.x) * (1.0 - (-2.5 * dt).exp());
                let next_vy = (tr.velocity.y + DRIP_GRAVITY * dt).max(-920.0);
                let next = V2::new(
                    prev.x + 0.5 * (tr.velocity.x + next_vx) * dt,
                    prev.y + tr.velocity.y * dt + 0.5 * DRIP_GRAVITY * dt * dt,
                );
                let vel = V2::new(next_vx, next_vy);
                if let Some(hit) = first_hit(prev, next, self.width, surfaces) {
                    self.trickles.swap_remove(i);
                    let rem_dt = (1.0 - hit.t).max(0.0) * dt;
                    self.on_drop_hit(hit, vel, 0.55, rem_dt, surfaces);
                    continue;
                }
                if next.y < -16.0 {
                    self.trickles.swap_remove(i);
                    continue;
                }
                tr.position = next;
                tr.velocity = vel;
            }

            self.trickles[i] = tr;
            i += 1;
        }
    }
}

impl Weather for Rain {
    fn step(&mut self, dt: f32, surfaces: &[WindowSurface], wind: Option<f32>, _cursor: V2) -> bool {
        let dt = dt.max(0.0);
        let was_active = self.sprite_count() > 0;
        if dt == 0.0 {
            return was_active;
        }

        self.clock += dt as f64;
        self.vary_intensity(dt);
        let natural_wind = self.sample_wind();
        let active_wind = wind.unwrap_or(natural_wind);
        self.last_wind = active_wind;

        self.advance_splashes(dt);
        self.advance_trickles(dt, active_wind, surfaces);
        self.advance_drops(dt, active_wind, surfaces);
        self.spawn_drops(dt, active_wind, surfaces);

        was_active || self.sprite_count() > 0
    }

    fn draw<'a>(&'a self, sprites: &mut [Sprite], _heights: &mut [f32]) -> DrawOutput<'a> {
        let mut written = 0;
        let limit = sprites.len();

        for drop in &self.drops {
            if written >= limit {
                break;
            }
            sprites[written] = Sprite {
                position: drop.position,
                velocity: drop.velocity,
                size: drop.size,
                alpha: drop.alpha,
                // Foreground drops have a crisper, glassier edge; distant rain stays soft.
                soft: 0.38 - drop.depth * 0.20,
                tint_amount: 0.78,
                tint: DROP_TINT,
                lump: RAIN_STREAK_LUMP,
            };
            written += 1;
        }

        for splash in &self.splashes {
            if written >= limit {
                break;
            }
            let u = (splash.age / splash.life).clamp(0.0, 1.0);
            let fade = (1.0 - u) * (1.0 - u * 0.4);
            sprites[written] = Sprite {
                position: splash.position,
                velocity: splash.velocity * 0.35,
                size: splash.size * (1.0 - u * 0.25),
                alpha: splash.alpha * fade,
                soft: 0.55,
                tint_amount: 0.78,
                tint: SPLASH_TINT,
                lump: 0.0,
            };
            written += 1;
        }

        for tr in &self.trickles {
            if written >= limit {
                break;
            }
            let ledge_fade = if tr.falling {
                1.0
            } else {
                let u = (tr.age / tr.ledge_life).clamp(0.0, 1.0);
                (tr.age / 0.08).min(1.0) * (1.0 - smoothstep(0.72, 1.0, u))
            };
            let as_streak = tr.falling && tr.velocity.y < -180.0;
            sprites[written] = Sprite {
                position: tr.position,
                velocity: tr.velocity,
                size: if as_streak { tr.size * 0.72 } else { tr.size },
                alpha: tr.alpha * ledge_fade,
                soft: 0.35,
                tint_amount: 0.82,
                tint: SPLASH_TINT,
                lump: if as_streak { RAIN_STREAK_LUMP } else { 0.0 },
            };
            written += 1;
        }

        DrawOutput {
            sprites: written,
            piles: Vec::new(),
            view_size: [self.width, self.height],
        }
    }

    fn set_intensity(&mut self, intensity: Snowfall) {
        if intensity == Snowfall::Variable && self.intensity != Snowfall::Variable {
            self.next_shift = self.clock;
        }
        if intensity != Snowfall::Variable {
            self.strength = 1.0;
            self.strength_target = 1.0;
        }
        self.intensity = intensity;
    }

    fn set_density(&mut self, density: f32) {
        self.density = density.max(0.0);
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    fn pace(&self) -> Pace {
        if self.sprite_count() > 0 || self.intensity != Snowfall::Off {
            Pace::Lively
        } else {
            Pace::Calm
        }
    }

    fn sprite_count(&self) -> usize {
        self.drops.len() + self.splashes.len() + self.trickles.len()
    }

    fn column_count(&self) -> usize {
        0
    }

    fn has_pile(&self) -> bool {
        false
    }
}

fn wrap_x(x: f32, width: f32) -> f32 {
    let span = width.max(1.0) + 2.0 * SIDE_MARGIN;
    (x + SIDE_MARGIN).rem_euclid(span) - SIDE_MARGIN
}

/// Vertical drop of a rounded window top corner at `offset_x` in `0..width`.
fn corner_drop(offset_x: f32, width: f32, radius: f32, exponent: f32) -> f32 {
    if radius <= 0.0 || width <= 0.0 {
        return 0.0;
    }
    let inset = offset_x.min(width - offset_x);
    if inset >= radius {
        return 0.0;
    }
    if inset <= 0.0 {
        return -radius;
    }
    let exp = exponent.max(1.0);
    let u = 1.0 - inset / radius;
    -radius * (1.0 - (1.0 - u.powf(exp)).max(0.0).powf(1.0 / exp))
}

fn occluded(x: f32, y: f32, surface: &WindowSurface) -> bool {
    surface
        .blockers
        .iter()
        .any(|b| x >= b.min_x() && x < b.max_x() && y >= b.min_y() && y < b.max_y())
}

/// Finds the earliest unoccluded intersection of a falling drop segment `prev -> next`
/// with any window top edge or the bottom of the work area (`y = 0`).
fn first_hit(prev: V2, next: V2, screen_width: f32, surfaces: &[WindowSurface]) -> Option<Hit> {
    let dy = prev.y - next.y;
    if dy <= 1e-6 {
        return None;
    }

    let mut best: Option<Hit> = None;

    for (idx, surface) in surfaces.iter().enumerate() {
        let frame = &surface.frame;
        if frame.width < 16.0 {
            continue;
        }
        let flat_top = frame.max_y();
        let radius = surface.corner_radius.max(0.0);
        if prev.y < flat_top - radius || next.y > flat_top {
            continue;
        }

        let t_flat = ((prev.y - flat_top) / dy).clamp(0.0, 1.0);
        let x_approx = prev.x + (next.x - prev.x) * t_flat;
        if x_approx < frame.min_x() || x_approx > frame.max_x() {
            continue;
        }

        let edge_y = flat_top + corner_drop(x_approx - frame.min_x(), frame.width, radius, surface.corner_exponent);
        if prev.y < edge_y || next.y > edge_y {
            continue;
        }

        let t = ((prev.y - edge_y) / dy).clamp(0.0, 1.0);
        if best.is_some_and(|current| t >= current.t) {
            continue;
        }

        let hit_x = prev.x + (next.x - prev.x) * t;
        if hit_x < frame.min_x() || hit_x > frame.max_x() || hit_x < -4.0 || hit_x > screen_width + 4.0 {
            continue;
        }

        // Ignore portions of a background window's top edge hidden behind a foreground window.
        if occluded(hit_x, edge_y + 1.0, surface) {
            continue;
        }

        best = Some(Hit {
            t,
            point: V2::new(hit_x, edge_y),
            window_index: Some(idx),
        });
    }

    if best.is_none() && prev.y >= 0.0 && next.y <= 0.0 {
        let t = (prev.y / dy).clamp(0.0, 1.0);
        let hit_x = prev.x + (next.x - prev.x) * t;
        if hit_x >= -8.0 && hit_x <= screen_width + 8.0 {
            best = Some(Hit {
                t,
                point: V2::new(hit_x, 0.0),
                window_index: None,
            });
        }
    }

    best
}

/// Angle of rainfall in degrees from vertical (`0` = straight down, positive = leaning right).
pub fn fall_angle_degrees(velocity: V2) -> f32 {
    velocity.x.atan2((-velocity.y).max(1e-4)) * (180.0 / PI)
}
