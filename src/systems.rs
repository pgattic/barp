use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
    sync::Arc,
};

use anyhow::{anyhow, bail};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub(crate) struct System {
    pub(crate) core: String,
    extensions: Arc<HashSet<String>>,
    pub(crate) threads: bool,
}

#[derive(Debug)]
pub(crate) struct SystemRegistry {
    by_folder: HashMap<String, System>,
}

#[derive(Debug, Deserialize)]
struct CoreMetadata {
    name: String,
    extensions: Vec<String>,
    #[serde(default)]
    options: CoreOptions,
}

#[derive(Debug, Default, Deserialize)]
struct CoreOptions {
    #[serde(default, rename = "requireThreads")]
    require_threads: bool,
}

struct BuiltinSystem {
    target: &'static str,
    default_core: &'static str,
    folders: &'static [&'static str],
}

impl SystemRegistry {
    pub(crate) fn new(
        emulatorjs_path: &Path,
        additional_mappings: &HashMap<String, String>,
    ) -> anyhow::Result<Self> {
        let cores_path = emulatorjs_path.join("cores/cores.json");
        let cores_text = std::fs::read_to_string(&cores_path).map_err(|err| {
            anyhow!(
                "failed to read EmulatorJS cores metadata at {}: {err}",
                cores_path.display()
            )
        })?;
        let cores: Vec<CoreMetadata> = serde_json::from_str(&cores_text)?;
        let mut concrete = HashMap::new();
        for core in cores {
            concrete.insert(
                core.name.clone(),
                System {
                    core: core.name,
                    extensions: Arc::new(
                        core.extensions
                            .into_iter()
                            .map(|extension| extension.to_lowercase())
                            .collect(),
                    ),
                    threads: core.options.require_threads,
                },
            );
        }

        let mut by_target = concrete.clone();
        let mut by_folder = HashMap::new();
        for builtin in builtin_systems() {
            let metadata = concrete.get(builtin.default_core).ok_or_else(|| {
                anyhow!(
                    "EmulatorJS core metadata is missing {}",
                    builtin.default_core
                )
            })?;
            let system = System {
                core: builtin.target.to_string(),
                extensions: metadata.extensions.clone(),
                threads: metadata.threads,
            };
            by_target.insert(builtin.target.to_string(), system.clone());
            for folder in builtin.folders {
                by_folder.insert(normalize_folder(folder), system.clone());
            }
        }

        for (folder, target) in additional_mappings {
            validate_folder_name(folder)?;
            let system = by_target
                .get(target)
                .ok_or_else(|| anyhow!("unknown EmulatorJS system or core: {target}"))?
                .clone();
            by_folder.insert(normalize_folder(folder), system);
        }

        Ok(Self { by_folder })
    }

    pub(crate) fn for_folder(&self, folder: &str) -> Option<&System> {
        self.by_folder.get(&normalize_folder(folder))
    }

    pub(crate) fn for_path(&self, path: &str) -> Option<&System> {
        path.split('/')
            .next()
            .and_then(|folder| self.for_folder(folder))
    }

    pub(crate) fn len(&self) -> usize {
        self.by_folder.len()
    }

    pub(crate) fn supports_file(&self, system: &System, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| system.extensions.contains(&extension.to_lowercase()))
    }
}

fn normalize_folder(folder: &str) -> String {
    folder.to_lowercase()
}

fn validate_folder_name(folder: &str) -> anyhow::Result<()> {
    let mut components = Path::new(folder).components();
    let valid = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !folder.starts_with('.');
    if valid {
        Ok(())
    } else {
        bail!("invalid system folder mapping: {folder}")
    }
}

fn builtin_systems() -> &'static [BuiltinSystem] {
    // Keep this list aligned with README "Supported Platforms": single-file
    // cartridge / handheld systems that work without BIOS or multi-track CD
    // setup. Other EmulatorJS cores remain reachable via `system_mappings`
    // (folder → concrete core name).
    //
    // Nintendo DS is deliberately absent. Its cores bind battery saves to
    // the cartridge at load time, and EmulatorJS only exposes the save
    // path after boot, so BARP cannot restore a save before the game
    // starts. Map a folder to `melonds` explicitly to opt in anyway.
    //
    // PSP is deliberately absent. EmulatorJS marks `ppsspp` as
    // `save: false` (memory-stick saves, not a single .srm), so BARP
    // cannot persist progress the way it does for other systems. Map a
    // folder to `ppsspp` explicitly to opt in anyway.
    &[
        BuiltinSystem {
            target: "vb",
            default_core: "beetle_vb",
            folders: &["vb", "virtualboy", "virtual-boy"],
        },
        BuiltinSystem {
            target: "nes",
            default_core: "fceumm",
            folders: &["nes", "famicom"],
        },
        BuiltinSystem {
            target: "gb",
            default_core: "gambatte",
            folders: &[
                "gb",
                "gbc",
                "gameboy",
                "game-boy",
                "gameboy-color",
                "game-boy-color",
            ],
        },
        BuiltinSystem {
            target: "segaMS",
            default_core: "smsplus",
            folders: &[
                "sms",
                "mastersystem",
                "master-system",
                "sega-master-system",
                "sg1000",
                "sg-1000",
            ],
        },
        BuiltinSystem {
            target: "segaMD",
            default_core: "genesis_plus_gx",
            folders: &[
                "genesis",
                "megadrive",
                "mega-drive",
                "sega-genesis",
                "sega-mega-drive",
            ],
        },
        BuiltinSystem {
            target: "segaGG",
            default_core: "genesis_plus_gx",
            folders: &["gg", "gamegear", "game-gear", "sega-game-gear"],
        },
        BuiltinSystem {
            target: "sega32x",
            default_core: "picodrive",
            folders: &["32x", "sega32x", "sega-32x"],
        },
        BuiltinSystem {
            target: "ngp",
            default_core: "mednafen_ngp",
            folders: &["ngp", "ngpc", "neo-geo-pocket", "neogeo-pocket"],
        },
        BuiltinSystem {
            target: "pce",
            default_core: "mednafen_pce",
            folders: &[
                "pce",
                "pcengine",
                "pc-engine",
                "turbografx16",
                "turbografx-16",
                "tg16",
            ],
        },
        BuiltinSystem {
            target: "ws",
            default_core: "mednafen_wswan",
            folders: &[
                "ws",
                "wsc",
                "wonderswan",
                "wonderswancolor",
                "wonderswan-color",
            ],
        },
        BuiltinSystem {
            target: "gba",
            default_core: "mgba",
            folders: &["gba", "gameboy-advance", "game-boy-advance"],
        },
        BuiltinSystem {
            target: "n64",
            default_core: "mupen64plus_next",
            folders: &["n64", "nintendo64", "nintendo-64"],
        },
        BuiltinSystem {
            target: "atari7800",
            default_core: "prosystem",
            folders: &["atari7800", "atari-7800", "a7800"],
        },
        BuiltinSystem {
            target: "snes",
            default_core: "snes9x",
            folders: &["snes", "super-nintendo", "super-famicom"],
        },
        BuiltinSystem {
            target: "atari2600",
            default_core: "stella2014",
            folders: &["atari2600", "atari-2600", "a2600"],
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn test_emulatorjs_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/emulatorjs/data")
    }

    #[test]
    fn resolves_common_aliases_case_insensitively() {
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &HashMap::new()).unwrap();
        assert_eq!(registry.for_folder("NES").unwrap().core, "nes");
        assert_eq!(registry.for_folder("megadrive").unwrap().core, "segaMD");
        assert_eq!(registry.for_folder("game-boy-color").unwrap().core, "gb");
        assert_eq!(registry.for_folder("tg16").unwrap().core, "pce");
        assert!(registry.for_folder("playstation").is_none());
        assert!(registry.for_folder("fds").is_none());
    }

    #[test]
    fn supports_every_fixture_core_as_a_config_target() {
        let emulatorjs_path = test_emulatorjs_path();
        let cores_text = std::fs::read_to_string(emulatorjs_path.join("cores/cores.json")).unwrap();
        let cores: Vec<CoreMetadata> = serde_json::from_str(&cores_text).unwrap();
        for core in cores {
            let mappings = HashMap::from([("test".to_string(), core.name.clone())]);
            let registry = SystemRegistry::new(&emulatorjs_path, &mappings).unwrap();
            assert_eq!(registry.for_folder("test").unwrap().core, core.name);
        }
    }

    #[test]
    fn config_mappings_can_select_systems_or_concrete_cores() {
        let mappings = HashMap::from([
            ("homebrew".to_string(), "nes".to_string()),
            ("accurate-gba".to_string(), "mgba".to_string()),
        ]);
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &mappings).unwrap();
        assert_eq!(registry.for_folder("homebrew").unwrap().core, "nes");
        assert_eq!(registry.for_folder("accurate-gba").unwrap().core, "mgba");
    }

    #[test]
    fn uses_extensions_for_the_selected_system() {
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &HashMap::new()).unwrap();
        let nes = registry.for_folder("nes").unwrap();
        assert!(registry.supports_file(nes, Path::new("game.nes")));
        assert!(!registry.supports_file(nes, Path::new("game.gba")));
    }

    #[test]
    fn carries_thread_requirements_from_core_metadata() {
        let mappings = HashMap::from([("dos".to_string(), "dosbox_pure".to_string())]);
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &mappings).unwrap();
        assert!(registry.for_folder("dos").unwrap().threads);
        assert!(!registry.for_folder("nes").unwrap().threads);
    }
}
