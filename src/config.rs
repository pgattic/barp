use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) roms_path: PathBuf,
    pub(crate) saves_path: PathBuf,
    #[serde(default)]
    pub(crate) state_path: Option<PathBuf>,
    #[serde(default = "default_port")]
    pub(crate) port: u16,
    #[serde(default)]
    pub(crate) default_options: Options,
    #[serde(default)]
    pub(crate) users: Vec<UserConfig>,
    #[serde(default)]
    pub(crate) system_mappings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct Options {
    #[serde(default)]
    pub(crate) display_filter: Option<DisplayFilter>,
    #[serde(default)]
    pub(crate) integer_scaling: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DisplayFilter {
    Smooth,
    Pixelated,
    None,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserConfig {
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) password_hash_file: PathBuf,
    #[serde(default)]
    pub(crate) option_overrides: Options,
}

#[derive(Debug, Serialize)]
pub(crate) struct EffectiveOptions {
    pub(crate) display_filter: DisplayFilter,
    pub(crate) integer_scaling: bool,
}

pub(crate) fn merge_options(defaults: &Options, overrides: &Options) -> Options {
    Options {
        display_filter: overrides
            .display_filter
            .clone()
            .or_else(|| defaults.display_filter.clone()),
        integer_scaling: overrides.integer_scaling.or(defaults.integer_scaling),
    }
}

pub(crate) fn effective_options(options: &Options) -> EffectiveOptions {
    EffectiveOptions {
        display_filter: options
            .display_filter
            .clone()
            .unwrap_or(DisplayFilter::Smooth),
        integer_scaling: options.integer_scaling.unwrap_or(false),
    }
}

fn default_port() -> u16 {
    3000
}
