//! Пресеты: именованный стек эффектов, сохраняемый в JSON.

use serde::{Deserialize, Serialize};

use crate::core::error::{GlitchError, Result};
use crate::core::pipeline::Pipeline;
use crate::core::registry::Registry;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub pipeline: Pipeline,
    /// Встроенные пресеты нельзя перезаписать пользовательскими.
    #[serde(default)]
    pub builtin: bool,
}

impl Preset {
    pub fn new(name: impl Into<String>, pipeline: Pipeline) -> Self {
        Self {
            name: name.into(),
            pipeline,
            builtin: false,
        }
    }

    /// Загружает пресет и подгоняет значения под текущий набор эффектов:
    /// неизвестные id пропускаются, недостающие параметры берутся из defaults.
    pub fn into_pipeline(self, registry: &Registry) -> Pipeline {
        let mut pipeline = self.pipeline;
        pipeline.stack.retain(|e| registry.get(&e.id).is_ok());
        pipeline.reconcile(registry);
        pipeline
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| GlitchError::other(e.to_string()))
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| GlitchError::other(format!("битый JSON: {e}")))
    }

    pub fn builtin(name: impl Into<String>, pipeline: Pipeline) -> Self {
        Self {
            name: name.into(),
            pipeline,
            builtin: true,
        }
    }
}

/// Файл с набором пользовательских пресетов.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresetLibrary {
    pub presets: Vec<Preset>,
}

impl PresetLibrary {
    /// Встроенные + пользовательские. Пользовательские с тем же именем перекрывают встроенные.
    pub fn load(user: &[Preset]) -> Self {
        let mut all = crate::core::builtin_presets::builtin_presets();
        for p in user {
            if let Some(slot) = all.iter_mut().find(|b| b.name == p.name) {
                *slot = p.clone();
            } else {
                all.push(p.clone());
            }
        }
        Self { presets: all }
    }

    pub fn find(&self, name: &str) -> Result<&Preset> {
        self.presets
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| GlitchError::UnknownPreset(name.to_string()))
    }

    pub fn user_presets(&self) -> Vec<&Preset> {
        self.presets.iter().filter(|p| !p.builtin).collect()
    }

    /// Сохраняет или перезаписывает пользовательский пресет.
    pub fn save(&mut self, preset: Preset) {
        let mut preset = preset;
        preset.builtin = false;
        match self.presets.iter_mut().find(|p| p.name == preset.name) {
            Some(slot) if !slot.builtin => *slot = preset,
            Some(_) => self.presets.push(preset),
            None => self.presets.push(preset),
        }
    }

    pub fn delete(&mut self, name: &str) {
        self.presets.retain(|p| p.builtin || p.name != name);
    }
}
