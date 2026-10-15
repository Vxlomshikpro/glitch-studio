//! Реестр эффектов: список всех реализаций и поиск по id.

use crate::core::effect::Effect;
use crate::core::error::GlitchError;
use crate::core::params::ParamSpec;

pub struct Registry {
    effects: Vec<Box<dyn Effect>>,
}

impl Registry {
    /// Собирается из всех групп, порядок определяет порядок в UI.
    pub fn new() -> Self {
        Self {
            effects: crate::effects::all_effects().into_iter().collect(),
        }
    }

    /// Пустой реестр — для тестов.
    pub fn empty() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    pub fn with(mut self, effect: Box<dyn Effect>) -> Self {
        self.effects.push(effect);
        self
    }

    pub fn all(&self) -> &[Box<dyn Effect>] {
        &self.effects
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn get(&self, id: &str) -> Result<&dyn Effect, GlitchError> {
        self.effects
            .iter()
            .find(|e| e.id() == id)
            .map(|b| b.as_ref())
            .ok_or_else(|| GlitchError::UnknownEffect(id.to_string()))
    }

    pub fn params_of(&self, id: &str) -> Result<&'static [ParamSpec], GlitchError> {
        Ok(self.get(id)?.params())
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.effects.iter().map(|e| e.id()).collect()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
