use std::path::Path;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SystemInfo {
    pub(crate) folder: &'static str,
    pub(crate) label: &'static str,
    pub(crate) core: &'static str,
}

pub(crate) fn system_for_folder(folder: &str) -> Option<&'static SystemInfo> {
    systems().iter().find(|system| system.folder == folder)
}

pub(crate) fn systems() -> &'static [SystemInfo] {
    &[
        SystemInfo {
            folder: "nes",
            label: "NES",
            core: "nes",
        },
        SystemInfo {
            folder: "snes",
            label: "SNES",
            core: "snes",
        },
        SystemInfo {
            folder: "gb",
            label: "Game Boy",
            core: "gb",
        },
        SystemInfo {
            folder: "gbc",
            label: "Game Boy Color",
            core: "gb",
        },
        SystemInfo {
            folder: "gba",
            label: "Game Boy Advance",
            core: "gba",
        },
        SystemInfo {
            folder: "n64",
            label: "Nintendo 64",
            core: "n64",
        },
    ]
}

pub(crate) fn is_rom_file(path: &Path) -> bool {
    let Some(ext) = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase())
    else {
        return false;
    };
    matches!(
        ext.as_str(),
        "nes"
            | "unif"
            | "sfc"
            | "smc"
            | "fig"
            | "swc"
            | "gb"
            | "gbc"
            | "gba"
            | "n64"
            | "z64"
            | "v64"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_nintendo_rom_extensions() {
        assert!(is_rom_file(Path::new("game.nes")));
        assert!(is_rom_file(Path::new("game.SFC")));
        assert!(is_rom_file(Path::new("game.z64")));
        assert!(!is_rom_file(Path::new("readme.txt")));
    }
}
