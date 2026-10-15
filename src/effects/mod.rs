//! Реестр всех эффектов. Каждая группа — свой файл, здесь только сборка.

pub mod artifacts;
pub mod color;
pub mod geometry;
pub mod pixels;
pub mod temporal;

use crate::core::effect::Effect;

pub fn all_effects() -> Vec<Box<dyn Effect>> {
    let mut v: Vec<Box<dyn Effect>> = Vec::new();
    v.extend(color::effects());
    v.extend(geometry::effects());
    v.extend(pixels::effects());
    v.extend(artifacts::effects());
    v.extend(temporal::effects());
    v
}
