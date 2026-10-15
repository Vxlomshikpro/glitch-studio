//! Режим командной строки: пакетная обработка без GUI.

use std::path::PathBuf;

use clap::Parser;

use crate::core::error::{GlitchError, Result};
use crate::core::media;
use crate::core::pipeline::Pipeline;
use crate::core::preset::Preset;
use crate::core::registry::Registry;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "glitch-studio",
    about = "Порча фото и видео в стиле glitchcore",
    version
)]
pub struct Cli {
    /// Исходный файл. Без него запускается GUI.
    #[arg(long, short)]
    pub input: Option<PathBuf>,

    /// Куда сохранить результат.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Имя пресета (встроенного или сохранённого), например "VHS Death".
    #[arg(long)]
    pub preset: Option<String>,

    /// Seed: одинаковый seed всегда даёт одинаковый результат.
    #[arg(long, default_value_t = 42)]
    pub seed: u64,

    /// Глобальная интенсивность 0..=100.
    #[arg(long, default_value_t = 100.0)]
    pub chaos: f32,

    /// Включить только один эффект по его id, например rgb_split.
    #[arg(long)]
    pub effect: Option<String>,

    /// Сила одиночного эффекта, 0..=100 (используется с --effect).
    #[arg(long, default_value_t = 100.0)]
    pub mix: f32,

    /// Качество JPEG/WebP 1..=100.
    #[arg(long, default_value_t = 92)]
    pub quality: u8,

    /// Показать список доступных эффектов и выйти.
    #[arg(long)]
    pub list_effects: bool,

    /// Показать список доступных пресетов и выйти.
    #[arg(long)]
    pub list_presets: bool,
}

/// Пайплайн, собранный из флагов CLI.
pub fn pipeline_from_cli(cli: &Cli, registry: &Registry) -> Result<Pipeline> {
    let mut pipeline = Pipeline::new(cli.seed);
    pipeline.chaos = cli.chaos.clamp(0.0, 100.0);

    let user_presets = load_user_presets();

    if let Some(effect_id) = &cli.effect {
        let eff = registry.get(effect_id)?;
        let mut entry = crate::core::StackEntry::new(eff.id(), eff.defaults());
        entry.mix = (cli.mix.clamp(0.0, 100.0)) / 100.0;
        pipeline.stack.push(entry);
        return Ok(pipeline);
    }

    match &cli.preset {
        Some(name) => {
            let preset: Preset = crate::core::builtin_presets::resolve(name, &user_presets)
                .ok_or_else(|| GlitchError::UnknownPreset(name.clone()))?;
            let mut p = preset.into_pipeline(registry);
            p.seed = cli.seed;
            p.chaos = cli.chaos.clamp(0.0, 100.0);
            Ok(p)
        }
        None => {
            if cli.input.is_some() {
                // Без пресета даём предсказуемый набор: порча должна быть видна.
                pipeline = crate::core::builtin_presets::find_builtin("VHS Death")
                    .map(|p| p.into_pipeline(registry))
                    .unwrap_or(pipeline);
                pipeline.seed = cli.seed;
                pipeline.chaos = cli.chaos.clamp(0.0, 100.0);
            }
            Ok(pipeline)
        }
    }
}

pub fn load_user_presets() -> Vec<Preset> {
    std::fs::read_to_string(crate::core::config::presets_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .map(|lib: crate::core::PresetLibrary| lib.presets)
        .unwrap_or_default()
}

/// Обработка одного файла в CLI-режиме.
pub fn run(cli: &Cli) -> Result<()> {
    let registry = Registry::new();

    if cli.list_effects {
        for e in registry.all() {
            println!("{:<22} {:<26} {}", e.id(), e.group().label(), e.name());
        }
        return Ok(());
    }
    if cli.list_presets {
        for p in crate::core::PresetLibrary::load(&load_user_presets()).presets {
            let tag = if p.builtin {
                "встроенный"
            } else {
                "пользовательский"
            };
            println!("{:<22} {tag}", p.name);
        }
        return Ok(());
    }

    let input = cli
        .input
        .as_ref()
        .ok_or_else(|| GlitchError::other("не указан --input"))?;
    let _ = &input;

    let output = cli
        .output
        .clone()
        .unwrap_or_else(|| input.with_extension("glitch.png"));

    let pipeline = pipeline_from_cli(cli, &registry)?;

    if media::is_video_ext(&media::ext_of(input).unwrap_or_default()) {
        return crate::video::process_file(input, &output, &pipeline, &registry, cli.quality);
    }

    let frame = media::load(input)?;
    let out = pipeline.apply(&frame, &registry, &[])?;
    media::save(&out, &output, cli.quality)?;
    println!(
        "готово: {} -> {} ({}x{}, seed {})",
        input.display(),
        output.display(),
        out.width,
        out.height,
        pipeline.seed
    );
    Ok(())
}
