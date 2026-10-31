use std::ops::{Deref, DerefMut};

use crate::geometry::V2;
use crate::simulation::{Pace, PileLayer, SnowSimulation, Snowfall, Sprite, WindowSurface};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    #[default]
    Snow,
    Rain,
}

impl Effect {
    pub const ALL: [Effect; 2] = [Effect::Snow, Effect::Rain];

    pub fn title(self) -> &'static str {
        match self {
            Effect::Snow => "Snow",
            Effect::Rain => "Rain",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Effect::Snow => "snow",
            Effect::Rain => "rain",
        }
    }

    pub fn from_key(value: &str) -> Option<Effect> {
        match value.trim().to_ascii_lowercase().as_str() {
            "snow" => Some(Effect::Snow),
            "rain" => Some(Effect::Rain),
            _ => None,
        }
    }
}

pub struct DrawOutput<'a> {
    pub sprites: usize,
    pub piles: Vec<PileLayer<'a>>,
    pub view_size: [f32; 2],
}

/// Common simulation and rendering interface shared by overlay weather effects.
pub trait Weather {
    fn step(&mut self, dt: f32, surfaces: &[WindowSurface], wind: Option<f32>, cursor: V2) -> bool;
    fn draw<'a>(&'a self, sprites: &mut [Sprite], heights: &mut [f32]) -> DrawOutput<'a>;
    fn set_intensity(&mut self, intensity: Snowfall);
    fn set_density(&mut self, density: f32);
    fn resize(&mut self, width: f32, height: f32);
    fn pace(&self) -> Pace;
    fn sprite_count(&self) -> usize;
    fn column_count(&self) -> usize;
    fn has_pile(&self) -> bool;
}

/// Wraps the existing snow simulation behind [`Weather`] without altering its physics or rendering.
pub struct Snow(pub SnowSimulation);

impl Snow {
    pub fn new(width: f32, height: f32, intensity: Snowfall) -> Snow {
        Snow(SnowSimulation::new(width, height, intensity))
    }
}

impl Deref for Snow {
    type Target = SnowSimulation;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Snow {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Weather for Snow {
    fn step(&mut self, dt: f32, _surfaces: &[WindowSurface], _wind: Option<f32>, cursor: V2) -> bool {
        self.0.step(dt, cursor)
    }

    fn draw<'a>(&'a self, sprites: &mut [Sprite], heights: &mut [f32]) -> DrawOutput<'a> {
        let written = self.0.write_sprites(sprites);
        let piles = self.0.write_layers(heights);
        DrawOutput {
            sprites: written,
            piles,
            view_size: [self.0.width(), self.0.height()],
        }
    }

    fn set_intensity(&mut self, intensity: Snowfall) {
        self.0.set_snowfall(intensity);
    }

    fn set_density(&mut self, density: f32) {
        self.0.set_density(density);
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.0.resize(width, height);
    }

    fn pace(&self) -> Pace {
        self.0.pace()
    }

    fn sprite_count(&self) -> usize {
        self.0.sprite_count()
    }

    fn column_count(&self) -> usize {
        self.0.column_count()
    }

    fn has_pile(&self) -> bool {
        self.0.has_snow()
    }
}
