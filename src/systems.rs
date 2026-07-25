use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
    sync::Arc,
};

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
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let cores_path = emulatorjs_path.join("cores/cores.json");
        let cores_text = std::fs::read_to_string(&cores_path).map_err(|err| {
            format!(
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
                format!(
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
                .ok_or_else(|| format!("unknown EmulatorJS system or core: {target}"))?
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

    pub(crate) fn supports_file(&self, system: &System, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| system.extensions.contains(&extension.to_lowercase()))
    }
}

fn normalize_folder(folder: &str) -> String {
    folder.to_lowercase()
}

fn validate_folder_name(folder: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut components = Path::new(folder).components();
    let valid = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !folder.starts_with('.');
    if valid {
        Ok(())
    } else {
        Err(format!("invalid system folder mapping: {folder}").into())
    }
}

fn builtin_systems() -> &'static [BuiltinSystem] {
    &[
        BuiltinSystem {
            target: "atari5200",
            default_core: "a5200",
            folders: &["atari5200", "atari-5200", "a5200"],
        },
        BuiltinSystem {
            target: "vb",
            default_core: "beetle_vb",
            folders: &["vb", "virtualboy", "virtual-boy"],
        },
        BuiltinSystem {
            target: "nds",
            default_core: "melonds",
            folders: &["nds", "ds", "nintendo-ds"],
        },
        BuiltinSystem {
            target: "arcade",
            default_core: "fbneo",
            folders: &[
                "arcade",
                "fbneo",
                "fba",
                "finalburnneo",
                "neo-geo",
                "neogeo",
            ],
        },
        BuiltinSystem {
            target: "nes",
            default_core: "fceumm",
            folders: &["nes", "famicom", "fds"],
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
            target: "coleco",
            default_core: "gearcoleco",
            folders: &["coleco", "colecovision"],
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
            target: "segaCD",
            default_core: "genesis_plus_gx",
            folders: &["segacd", "sega-cd", "megacd", "mega-cd"],
        },
        BuiltinSystem {
            target: "sega32x",
            default_core: "picodrive",
            folders: &["32x", "sega32x", "sega-32x"],
        },
        BuiltinSystem {
            target: "sega",
            default_core: "genesis_plus_gx",
            folders: &["sega"],
        },
        BuiltinSystem {
            target: "lynx",
            default_core: "handy",
            folders: &["lynx", "atari-lynx"],
        },
        BuiltinSystem {
            target: "mame",
            default_core: "mame2003_plus",
            folders: &["mame"],
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
                "pcenginecd",
                "pc-engine-cd",
                "turbografx-cd",
                "tg-cd",
            ],
        },
        BuiltinSystem {
            target: "pcfx",
            default_core: "mednafen_pcfx",
            folders: &["pcfx", "pc-fx"],
        },
        BuiltinSystem {
            target: "psx",
            default_core: "pcsx_rearmed",
            folders: &["psx", "ps1", "playstation", "playstation-1"],
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
            target: "3do",
            default_core: "opera",
            folders: &["3do"],
        },
        BuiltinSystem {
            target: "psp",
            default_core: "ppsspp",
            folders: &["psp"],
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
        BuiltinSystem {
            target: "jaguar",
            default_core: "virtualjaguar",
            folders: &["jaguar", "atari-jaguar"],
        },
        BuiltinSystem {
            target: "segaSaturn",
            default_core: "yabause",
            folders: &["saturn", "sega-saturn"],
        },
        BuiltinSystem {
            target: "amiga",
            default_core: "puae",
            folders: &["amiga", "amigacd32", "amiga-cd32"],
        },
        BuiltinSystem {
            target: "c64",
            default_core: "vice_x64sc",
            folders: &["c64", "commodore64", "commodore-64"],
        },
        BuiltinSystem {
            target: "c128",
            default_core: "vice_x128",
            folders: &["c128", "commodore128", "commodore-128"],
        },
        BuiltinSystem {
            target: "pet",
            default_core: "vice_xpet",
            folders: &["pet", "commodore-pet"],
        },
        BuiltinSystem {
            target: "plus4",
            default_core: "vice_xplus4",
            folders: &["plus4", "plus-4", "commodore-plus4", "commodore-plus-4"],
        },
        BuiltinSystem {
            target: "vic20",
            default_core: "vice_xvic",
            folders: &["vic20", "vic-20", "commodore-vic20"],
        },
        BuiltinSystem {
            target: "dos",
            default_core: "dosbox_pure",
            folders: &["dos", "msdos", "ms-dos"],
        },
        BuiltinSystem {
            target: "same_cdi",
            default_core: "same_cdi",
            folders: &["cdi", "cd-i", "philips-cdi"],
        },
        BuiltinSystem {
            target: "81",
            default_core: "81",
            folders: &["zx81", "zx-81"],
        },
        BuiltinSystem {
            target: "fuse",
            default_core: "fuse",
            folders: &["zxspectrum", "zx-spectrum", "spectrum"],
        },
        BuiltinSystem {
            target: "cap32",
            default_core: "cap32",
            folders: &["amstradcpc", "amstrad-cpc", "cpc"],
        },
        BuiltinSystem {
            target: "prboom",
            default_core: "prboom",
            folders: &["doom"],
        },
        BuiltinSystem {
            target: "fbalpha2012_cps1",
            default_core: "fbalpha2012_cps1",
            folders: &["cps1", "cps-1"],
        },
        BuiltinSystem {
            target: "fbalpha2012_cps2",
            default_core: "fbalpha2012_cps2",
            folders: &["cps2", "cps-2"],
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
        assert_eq!(registry.for_folder("playstation").unwrap().core, "psx");
        assert_eq!(registry.for_folder("zx-spectrum").unwrap().core, "fuse");
    }

    #[test]
    fn supports_every_fixture_core_as_a_config_target() {
        let emulatorjs_path = test_emulatorjs_path();
        let cores_text =
            std::fs::read_to_string(emulatorjs_path.join("cores/cores.json")).unwrap();
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
            ("my-ps1".to_string(), "psx".to_string()),
            ("accurate-ps1".to_string(), "mednafen_psx_hw".to_string()),
        ]);
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &mappings).unwrap();
        assert_eq!(registry.for_folder("my-ps1").unwrap().core, "psx");
        assert_eq!(
            registry.for_folder("accurate-ps1").unwrap().core,
            "mednafen_psx_hw"
        );
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
        let registry = SystemRegistry::new(&test_emulatorjs_path(), &HashMap::new()).unwrap();
        assert!(registry.for_folder("psp").unwrap().threads);
        assert!(registry.for_folder("dos").unwrap().threads);
        assert!(!registry.for_folder("nes").unwrap().threads);
    }
}
