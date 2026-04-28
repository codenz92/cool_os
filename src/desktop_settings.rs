use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DesktopSortMode {
    Default = 0,
    Name = 1,
    Type = 2,
}

impl DesktopSortMode {
    fn from_byte(value: u8) -> Self {
        match value {
            1 => DesktopSortMode::Name,
            2 => DesktopSortMode::Type,
            _ => DesktopSortMode::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DesktopSortMode::Default => "Default",
            DesktopSortMode::Name => "Name",
            DesktopSortMode::Type => "Type",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WallpaperPreset {
    Phosphor = 0,
    Aurora = 1,
    Midnight = 2,
}

impl WallpaperPreset {
    pub const ALL: [WallpaperPreset; 3] = [
        WallpaperPreset::Phosphor,
        WallpaperPreset::Aurora,
        WallpaperPreset::Midnight,
    ];

    fn from_byte(value: u8) -> Self {
        match value {
            1 => WallpaperPreset::Aurora,
            2 => WallpaperPreset::Midnight,
            _ => WallpaperPreset::Phosphor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WallpaperPreset::Phosphor => "Phosphor Blue",
            WallpaperPreset::Aurora => "Aurora Grid",
            WallpaperPreset::Midnight => "Midnight Core",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            WallpaperPreset::Phosphor => "classic coolOS bloom",
            WallpaperPreset::Aurora => "cool cyan-green sweep",
            WallpaperPreset::Midnight => "darker navy-violet shell",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DesktopSettings {
    pub show_icons: bool,
    pub compact_spacing: bool,
    pub sort_mode: DesktopSortMode,
    pub wallpaper: WallpaperPreset,
}

static SHOW_ICONS: AtomicBool = AtomicBool::new(true);
static COMPACT_SPACING: AtomicBool = AtomicBool::new(false);
static SORT_MODE: AtomicU8 = AtomicU8::new(DesktopSortMode::Default as u8);
static WALLPAPER: AtomicU8 = AtomicU8::new(WallpaperPreset::Phosphor as u8);

pub fn snapshot() -> DesktopSettings {
    DesktopSettings {
        show_icons: SHOW_ICONS.load(Ordering::Relaxed),
        compact_spacing: COMPACT_SPACING.load(Ordering::Relaxed),
        sort_mode: DesktopSortMode::from_byte(SORT_MODE.load(Ordering::Relaxed)),
        wallpaper: WallpaperPreset::from_byte(WALLPAPER.load(Ordering::Relaxed)),
    }
}

pub fn set_show_icons(value: bool) {
    SHOW_ICONS.store(value, Ordering::Relaxed);
}

pub fn set_compact_spacing(value: bool) {
    COMPACT_SPACING.store(value, Ordering::Relaxed);
}

pub fn set_sort_mode(value: DesktopSortMode) {
    SORT_MODE.store(value as u8, Ordering::Relaxed);
}

pub fn set_wallpaper(value: WallpaperPreset) {
    WALLPAPER.store(value as u8, Ordering::Relaxed);
}
