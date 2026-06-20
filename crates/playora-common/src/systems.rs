use crate::GameSystem;

#[derive(Debug, Clone, Copy)]
pub struct SystemSpec {
    pub system: GameSystem,
    pub folder: &'static str,
    pub display_name: &'static str,
    pub extensions: &'static [&'static str],
    pub default_emulator: &'static str,
    pub retroarch_core: Option<&'static str>,
}

pub const SYSTEMS: &[SystemSpec] = &[
    SystemSpec {
        system: GameSystem::Nes,
        folder: "nes",
        display_name: "Nintendo Entertainment System",
        extensions: &["nes", "unf", "fds", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("fceumm"),
    },
    SystemSpec {
        system: GameSystem::Snes,
        folder: "snes",
        display_name: "Super Nintendo",
        extensions: &["smc", "sfc", "fig", "swc", "bs", "st", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("snes9x"),
    },
    SystemSpec {
        system: GameSystem::Gb,
        folder: "gb",
        display_name: "Game Boy",
        extensions: &["gb", "gbc", "sgb", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("gambatte"),
    },
    SystemSpec {
        system: GameSystem::Gbc,
        folder: "gbc",
        display_name: "Game Boy Color",
        extensions: &["gbc", "gb", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("gambatte"),
    },
    SystemSpec {
        system: GameSystem::Gba,
        folder: "gba",
        display_name: "Game Boy Advance",
        extensions: &["gba", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("mgba"),
    },
    SystemSpec {
        system: GameSystem::Megadrive,
        folder: "megadrive",
        display_name: "Sega Mega Drive / Genesis",
        extensions: &["md", "smd", "gen", "bin", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("genesis_plus_gx"),
    },
    SystemSpec {
        system: GameSystem::MasterSystem,
        folder: "mastersystem",
        display_name: "Sega Master System",
        extensions: &["sms", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("genesis_plus_gx"),
    },
    SystemSpec {
        system: GameSystem::GameGear,
        folder: "gamegear",
        display_name: "Sega Game Gear",
        extensions: &["gg", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("genesis_plus_gx"),
    },
    SystemSpec {
        system: GameSystem::N64,
        folder: "n64",
        display_name: "Nintendo 64",
        extensions: &["n64", "v64", "z64", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("mupen64plus_next"),
    },
    SystemSpec {
        system: GameSystem::Psx,
        folder: "psx",
        display_name: "Sony PlayStation",
        extensions: &[
            "chd", "cue", "pbp", "m3u", "iso", "img", "bin", "cbn", "mdf", "toc", "pbp", "ccd",
            "sub",
        ],
        default_emulator: "retroarch",
        retroarch_core: Some("pcsx_rearmed"),
    },
    SystemSpec {
        system: GameSystem::Psp,
        folder: "psp",
        display_name: "Sony PlayStation Portable",
        extensions: &["iso", "cso", "pbp", "chd"],
        default_emulator: "ppsspp",
        retroarch_core: Some("ppsspp"),
    },
    SystemSpec {
        system: GameSystem::Dreamcast,
        folder: "dreamcast",
        display_name: "Sega Dreamcast",
        extensions: &["chd", "cdi", "gdi", "cue"],
        default_emulator: "retroarch",
        retroarch_core: Some("flycast"),
    },
    SystemSpec {
        system: GameSystem::Saturn,
        folder: "saturn",
        display_name: "Sega Saturn",
        extensions: &["chd", "cue", "iso", "mds", "ccd", "m3u"],
        default_emulator: "retroarch",
        retroarch_core: Some("yabasanshiro"),
    },
    SystemSpec {
        system: GameSystem::Atari2600,
        folder: "atari2600",
        display_name: "Atari 2600",
        extensions: &["a26", "bin", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("stella"),
    },
    SystemSpec {
        system: GameSystem::Atari7800,
        folder: "atari7800",
        display_name: "Atari 7800",
        extensions: &["a78", "bin", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("prosystem"),
    },
    SystemSpec {
        system: GameSystem::Arcade,
        folder: "arcade",
        display_name: "Arcade",
        extensions: &["zip", "7z", "chd"],
        default_emulator: "retroarch",
        retroarch_core: Some("fbneo"),
    },
    SystemSpec {
        system: GameSystem::Mame,
        folder: "mame",
        display_name: "MAME",
        extensions: &["zip", "7z", "chd"],
        default_emulator: "retroarch",
        retroarch_core: Some("mame2003_plus"),
    },
    SystemSpec {
        system: GameSystem::NeoGeo,
        folder: "neogeo",
        display_name: "Neo Geo",
        extensions: &["zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("fbneo"),
    },
    SystemSpec {
        system: GameSystem::PcEngine,
        folder: "pcengine",
        display_name: "PC Engine / TurboGrafx-16",
        extensions: &["pce", "sgx", "zip", "7z", "cue", "chd", "iso"],
        default_emulator: "retroarch",
        retroarch_core: Some("mednafen_pce"),
    },
    SystemSpec {
        system: GameSystem::Wonderswan,
        folder: "wonderswan",
        display_name: "Wonderswan / Color",
        extensions: &["ws", "wsc", "zip", "7z"],
        default_emulator: "retroarch",
        retroarch_core: Some("mednafen_wswan"),
    },
];

pub fn spec_for(system: GameSystem) -> Option<&'static SystemSpec> {
    SYSTEMS.iter().find(|s| s.system == system)
}

pub fn spec_by_folder(folder: &str) -> Option<&'static SystemSpec> {
    let f = folder.to_lowercase();
    SYSTEMS.iter().find(|s| s.folder == f)
}

pub fn ext_belongs_to_system(ext: &str, system: GameSystem) -> bool {
    let e = ext.trim_start_matches('.').to_lowercase();
    spec_for(system).is_some_and(|s| s.extensions.contains(&e.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_lookup_by_system() {
        assert!(spec_for(GameSystem::Snes).is_some());
        assert!(spec_for(GameSystem::N64).is_some());
    }

    #[test]
    fn spec_lookup_by_folder_caseinsensitive() {
        assert!(spec_by_folder("SNES").is_some());
        assert_eq!(
            spec_by_folder("snes").unwrap().retroarch_core,
            Some("snes9x")
        );
    }

    #[test]
    fn extension_check() {
        assert!(ext_belongs_to_system("smc", GameSystem::Snes));
        assert!(ext_belongs_to_system(".SFC", GameSystem::Snes));
        assert!(!ext_belongs_to_system("nes", GameSystem::Snes));
    }

    #[test]
    fn psp_uses_ppsspp_emulator() {
        let s = spec_for(GameSystem::Psp).unwrap();
        assert_eq!(s.default_emulator, "ppsspp");
    }
}
