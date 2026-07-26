use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) roms_path: PathBuf,
    pub(crate) saves_path: PathBuf,
    /// Path to an EmulatorJS `data/` directory (contains `loader.js`).
    pub(crate) emulatorjs_path: PathBuf,
    #[serde(default = "default_port")]
    pub(crate) port: u16,
    #[serde(default)]
    pub(crate) default_options: Options,
    #[serde(default)]
    pub(crate) users: HashMap<String, UserConfig>,
    #[serde(default)]
    pub(crate) system_mappings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct Options {
    /// EmulatorJS shader setting. Use `"disabled"` or a built-in shader name
    /// such as `"crt-mattias.glslp"` / `"2xScaleHQ.glslp"` / `"bicubic"`.
    #[serde(default)]
    pub(crate) shader: Option<String>,
    /// Browser upscale filtering for the game canvas. `false` keeps pixels
    /// crisp; `true` allows smooth bilinear scaling.
    #[serde(default)]
    pub(crate) smooth: Option<bool>,
    /// Integer-scale the player canvas (RetroArch `video_scale_integer` + CSS).
    #[serde(default)]
    pub(crate) integer_scale: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserConfig {
    pub(crate) display_name: String,
    pub(crate) password_hash_file: PathBuf,
    #[serde(default)]
    pub(crate) option_overrides: Options,
}

#[derive(Debug, Serialize)]
pub(crate) struct EffectiveOptions {
    pub(crate) shader: String,
    pub(crate) smooth: bool,
    pub(crate) integer_scale: bool,
}

pub(crate) fn merge_options(defaults: &Options, overrides: &Options) -> Options {
    Options {
        shader: overrides.shader.clone().or_else(|| defaults.shader.clone()),
        smooth: overrides.smooth.or(defaults.smooth),
        integer_scale: overrides.integer_scale.or(defaults.integer_scale),
    }
}

pub(crate) fn effective_options(options: &Options) -> EffectiveOptions {
    EffectiveOptions {
        // EmulatorJS defaults to shaders disabled; its RetroArch cfg also
        // forces video_smooth=false, so crisp pixels are the natural default.
        shader: options
            .shader
            .clone()
            .unwrap_or_else(|| "disabled".to_string()),
        smooth: options.smooth.unwrap_or(false),
        integer_scale: options.integer_scale.unwrap_or(false),
    }
}

fn default_port() -> u16 {
    3000
}
