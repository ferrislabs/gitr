use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffViewMode {
    #[default]
    Unified,
    Split,
}

impl DiffViewMode {
    pub const ALL: [DiffViewMode; 2] = [Self::Unified, Self::Split];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|mode| *mode == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unified => "Unified",
            Self::Split => "Split",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_unified() {
        assert_eq!(DiffViewMode::default(), DiffViewMode::Unified);
    }

    #[test]
    fn every_mode_round_trips_through_its_index() {
        for mode in DiffViewMode::ALL {
            assert_eq!(DiffViewMode::from_index(mode.index()), mode);
        }
    }

    #[test]
    fn an_out_of_range_index_falls_back_to_the_default() {
        assert_eq!(DiffViewMode::from_index(99), DiffViewMode::default());
    }
}
