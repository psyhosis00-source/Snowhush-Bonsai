#[cfg(windows)]
pub mod card;
pub mod geometry;
#[cfg(windows)]
pub mod install;
pub mod layer;
pub mod rain;
pub mod simulation;
pub mod trees;
pub mod version;
pub mod weather;

#[cfg(test)]
mod tests;
