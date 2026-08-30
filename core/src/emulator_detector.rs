// core/src/emulator_detector.rs
//
// Detección multi-capa de emuladores instalados en sistemas Linux.
// Detecta emuladores nativos (PATH, paquetes), Flatpak, Snap y .desktop files,
// devolviendo las opciones disponibles para la interfaz TUI.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InstallSource {
    Path,
    Dpkg,
    Rpm,
    Pacman,
    Flatpak,
    Snap,
    DesktopFile,
    DownloadedAppImage,
}

impl InstallSource {
    pub fn display_label(&self) -> &'static str {
        match self {
            InstallSource::Path => "System (PATH)",
            InstallSource::Dpkg => "Debian/Ubuntu (dpkg)",
            InstallSource::Rpm => "Fedora/RPM",
            InstallSource::Pacman => "Arch/Pacman",
            InstallSource::Flatpak => "Flatpak",
            InstallSource::Snap => "Snap",
            InstallSource::DesktopFile => "Desktop File",
            InstallSource::DownloadedAppImage => "Downloaded AppImage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DetectedEmulator {
    pub name: String,
    pub sources: Vec<InstallSource>,
    pub exec_path: Option<String>,
    pub flatpak_app_id: Option<String>,
}

impl DetectedEmulator {
    /// Devuelve el comando ejecutable recomendado.
    /// Para Flatpak: `flatpak run <app_id>`
    /// Para nativo/PATH: la ruta ejecutable directa.
    pub fn launch_command(&self) -> String {
        if let Some(ref app_id) = self.flatpak_app_id {
            format!("flatpak run {}", app_id)
        } else if let Some(ref path) = self.exec_path {
            path.clone()
        } else {
            self.name.to_lowercase()
        }
    }
}

pub struct KnownEmulator {
    pub name: &'static str,
    pub binaries: &'static [&'static str],
    pub flatpak_ids: &'static [&'static str],
    pub snap_names: &'static [&'static str],
}

pub const KNOWN_EMULATORS: &[KnownEmulator] = &[
    KnownEmulator {
        name: "Azahar",
        binaries: &["azahar", "azaharplus"],
        flatpak_ids: &["org.azahar_emu.Azahar"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Cemu",
        binaries: &["cemu", "Cemu"],
        flatpak_ids: &["info.cemu.Cemu"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Dolphin",
        binaries: &["dolphin-emu"],
        flatpak_ids: &["org.DolphinEmu.dolphin-emu"],
        snap_names: &["dolphin-emulator"],
    },
    KnownEmulator {
        name: "MAME",
        binaries: &["mame"],
        flatpak_ids: &["org.mamedev.MAME"],
        snap_names: &["mame"],
    },
    KnownEmulator {
        name: "PCSX2",
        binaries: &["pcsx2", "PCSX2", "pcsx2-qt"],
        flatpak_ids: &["net.pcsx2.PCSX2"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "PPSSPP",
        binaries: &["ppsspp", "PPSSPPSDL", "PPSSPPQt"],
        flatpak_ids: &["org.ppsspp.PPSSPP"],
        snap_names: &["ppsspp"],
    },
    KnownEmulator {
        name: "RetroArch",
        binaries: &["retroarch"],
        flatpak_ids: &["org.libretro.RetroArch"],
        snap_names: &["retroarch"],
    },
    KnownEmulator {
        name: "Ryujinx",
        binaries: &["ryujinx", "Ryujinx"],
        flatpak_ids: &["io.github.ryubing.Ryujinx"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "melonDS",
        binaries: &["melonds", "melonDS"],
        flatpak_ids: &["net.kuribo64.melonDS"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "DuckStation",
        binaries: &["duckstation-qt", "duckstation-nogui"],
        flatpak_ids: &["org.duckstation.DuckStation"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "RPCS3",
        binaries: &["rpcs3"],
        flatpak_ids: &["net.rpcs3.RPCS3"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Redream",
        binaries: &["redream"],
        flatpak_ids: &[],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Vita3K",
        binaries: &["vita3k", "Vita3K"],
        flatpak_ids: &["org.vita3k.Vita3K"],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Eden",
        binaries: &["eden", "eden-emu"],
        flatpak_ids: &[],
        snap_names: &[],
    },
    KnownEmulator {
        name: "Citron",
        binaries: &["citron", "citron-emu"],
        flatpak_ids: &[],
        snap_names: &[],
    },
];

pub struct EmulatorDetector;

impl EmulatorDetector {
    pub fn new() -> Self {
        EmulatorDetector
    }

    /// Detecta todas las variantes/instalaciones posibles para un emulador por su nombre.
    pub fn detect_for_emulator(&self, emulator_name: &str) -> Vec<DetectedEmulator> {
        let known = KNOWN_EMULATORS.iter().find(|k| {
            k.name.eq_ignore_ascii_case(emulator_name)
                || canonical_key(k.name) == canonical_key(emulator_name)
        });

        let mut candidates = Vec::new();

        if let Some(k) = known {
            // 1. Detectar en PATH
            let path_hits = self.scan_path_for_known(k);
            for (_bin, path) in path_hits {
                candidates.push(DetectedEmulator {
                    name: format!("{} ({})", k.name, path),
                    sources: vec![InstallSource::Path],
                    exec_path: Some(path),
                    flatpak_app_id: None,
                });
            }

            // 2. Detectar Flatpaks
            let flatpak_hits = self.scan_flatpak();
            for app_id in k.flatpak_ids {
                if flatpak_hits.contains(*app_id) {
                    candidates.push(DetectedEmulator {
                        name: format!("{} (Flatpak: {})", k.name, app_id),
                        sources: vec![InstallSource::Flatpak],
                        exec_path: Some(format!("flatpak run {}", app_id)),
                        flatpak_app_id: Some(app_id.to_string()),
                    });
                }
            }

            // 3. Detectar Snaps
            let snap_hits = self.scan_snap();
            for snap_name in k.snap_names {
                if snap_hits.contains(*snap_name) {
                    let exec = format!("/snap/bin/{}", snap_name);
                    candidates.push(DetectedEmulator {
                        name: format!("{} (Snap: {})", k.name, snap_name),
                        sources: vec![InstallSource::Snap],
                        exec_path: Some(if Path::new(&exec).exists() {
                            exec
                        } else {
                            snap_name.to_string()
                        }),
                        flatpak_app_id: None,
                    });
                }
            }
        } else {
            // Caso genérico/desconocido: probar `which` con el nombre en minúsculas
            let bin = emulator_name.to_lowercase();
            if let Ok(output) = Command::new("which").arg(&bin).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        candidates.push(DetectedEmulator {
                            name: format!("{} ({})", emulator_name, path),
                            sources: vec![InstallSource::Path],
                            exec_path: Some(path),
                            flatpak_app_id: None,
                        });
                    }
                }
            }
        }

        // 4. Última opción: Detectar AppImage descargado en las rutas gestionadas por TUI Game Station
        if let Some(appimage_path) = self.scan_downloaded_appimage(emulator_name) {
            candidates.push(DetectedEmulator {
                name: format!("{} (Downloaded AppImage: {})", emulator_name, appimage_path),
                sources: vec![InstallSource::DownloadedAppImage],
                exec_path: Some(appimage_path),
                flatpak_app_id: None,
            });
        }

        candidates
    }

    /// Escaneo completo deduplicado (para listados generales).
    pub fn scan_all(&self) -> Vec<DetectedEmulator> {
        let mut results = Vec::new();
        for known in KNOWN_EMULATORS {
            let hits = self.detect_for_emulator(known.name);
            results.extend(hits);
        }
        results
    }

    fn scan_path_for_known(&self, known: &KnownEmulator) -> HashMap<String, String> {
        let mut found = HashMap::new();
        for bin in known.binaries {
            if let Ok(output) = Command::new("which").arg(bin).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() && Path::new(&path).exists() {
                        found.insert(bin.to_string(), path);
                    }
                }
            }
        }
        found
    }

    fn scan_flatpak(&self) -> HashSet<String> {
        let mut found = HashSet::new();
        if let Ok(output) = Command::new("flatpak")
            .args(["list", "--app", "--columns=application"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    found.insert(line.trim().to_string());
                }
            }
        }
        found
    }

    fn scan_snap(&self) -> HashSet<String> {
        let mut found = HashSet::new();
        if let Ok(output) = Command::new("snap").arg("list").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().skip(1) {
                    if let Some(name) = line.split_whitespace().next() {
                        found.insert(name.to_string());
                    }
                }
            }
        }
        found
    }

    fn scan_downloaded_appimage(&self, emulator_name: &str) -> Option<String> {
        let home = dirs::home_dir()?;
        let runners_dir = home.join(".local/share/tui_game_station/runners");
        let key = canonical_key(emulator_name);

        if key == "retroarch" {
            let managed = runners_dir.join("emulators/retroarch-data");
            if let Some(appimage) = crate::retroarch_manager::find_downloaded_appimage(&managed) {
                if appimage.exists() {
                    return Some(appimage.to_string_lossy().to_string());
                }
            }
        } else {
            let emu_dir = runners_dir.join("emulators");
            if let Ok(entries) = std::fs::read_dir(&emu_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let fname = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase();
                        if fname.contains(&key)
                            && (fname.ends_with(".appimage") || fname.contains(".appimage"))
                        {
                            return Some(path.to_string_lossy().to_string());
                        }
                    } else if path.is_dir() {
                        if let Ok(sub_entries) = std::fs::read_dir(&path) {
                            for sub in sub_entries.flatten() {
                                let sub_path = sub.path();
                                let fname = sub_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_lowercase();
                                if (fname.contains(&key)
                                    || sub_path.to_string_lossy().to_lowercase().contains(&key))
                                    && (fname.ends_with(".appimage") || fname.contains(".appimage"))
                                {
                                    return Some(sub_path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

fn canonical_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
