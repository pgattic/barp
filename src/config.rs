use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize, Default)]
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
    /// Inline Argon2 PHC string. Mutually exclusive with `password_hash_file`.
    #[serde(default)]
    pub(crate) password_hash: Option<String>,
    /// Path to a file containing an Argon2 PHC string. Mutually exclusive with
    /// `password_hash`. Prefer this for deployments (agenix/sops).
    #[serde(default)]
    pub(crate) password_hash_file: Option<PathBuf>,
    #[serde(default)]
    pub(crate) option_overrides: Options,
}

/// Where a user's password hash comes from after validating exclusivity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PasswordHashSource {
    Inline(String),
    File(PathBuf),
}

impl UserConfig {
    pub(crate) fn password_hash_source(
        &self,
        username: &str,
    ) -> anyhow::Result<PasswordHashSource> {
        match (&self.password_hash, &self.password_hash_file) {
            (Some(hash), None) => Ok(PasswordHashSource::Inline(hash.trim().to_owned())),
            (None, Some(path)) => Ok(PasswordHashSource::File(path.clone())),
            (Some(_), Some(_)) => anyhow::bail!(
                "user {username} must set only one of password_hash or password_hash_file"
            ),
            (None, None) => {
                anyhow::bail!("user {username} must set password_hash or password_hash_file")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EffectiveOptions {
    pub(crate) shader: String,
    pub(crate) smooth: bool,
    pub(crate) integer_scale: bool,
}

pub(crate) fn effective_options(defaults: &Options, overrides: &Options) -> EffectiveOptions {
    EffectiveOptions {
        // EmulatorJS defaults to shaders disabled; its RetroArch cfg also
        // forces video_smooth=false, so crisp pixels are the natural default.
        shader: overrides
            .shader
            .clone()
            .or_else(|| defaults.shader.clone())
            .unwrap_or_else(|| "disabled".to_string()),
        smooth: overrides.smooth.or(defaults.smooth).unwrap_or(false),
        integer_scale: overrides
            .integer_scale
            .or(defaults.integer_scale)
            .unwrap_or(false),
    }
}

fn default_port() -> u16 {
    3000
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolves_partial_option_overrides() {
        let defaults = Options {
            shader: Some("disabled".into()),
            smooth: Some(false),
            integer_scale: Some(false),
        };
        let overrides = Options {
            shader: None,
            smooth: Some(true),
            integer_scale: None,
        };
        let effective = effective_options(&defaults, &overrides);
        assert_eq!(effective.shader, "disabled");
        assert!(effective.smooth);
        assert!(!effective.integer_scale);
    }

    #[test]
    fn password_hash_source_requires_exactly_one() {
        let inline = UserConfig {
            password_hash: Some(" $argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$hash ".into()),
            password_hash_file: None,
            option_overrides: Options::default(),
        };
        assert_eq!(
            inline.password_hash_source("player").unwrap(),
            PasswordHashSource::Inline("$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$hash".into())
        );

        let file = UserConfig {
            password_hash: None,
            password_hash_file: Some(PathBuf::from("./secrets/player.hash")),
            option_overrides: Options::default(),
        };
        assert_eq!(
            file.password_hash_source("player").unwrap(),
            PasswordHashSource::File(PathBuf::from("./secrets/player.hash"))
        );

        let both = UserConfig {
            password_hash: Some("$argon2id$…".into()),
            password_hash_file: Some(PathBuf::from("./secrets/player.hash")),
            option_overrides: Options::default(),
        };
        assert!(both.password_hash_source("player").is_err());

        let neither = UserConfig {
            password_hash: None,
            password_hash_file: None,
            option_overrides: Options::default(),
        };
        assert!(neither.password_hash_source("player").is_err());
    }
}
