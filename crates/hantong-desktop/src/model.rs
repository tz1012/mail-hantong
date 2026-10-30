use hantong_contracts::ErrorCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
    Korean,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveTheme {
    Light,
    Dark,
    HighContrast,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellOptions {
    pub language: Option<Language>,
    pub theme: Theme,
}
impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            language: None,
            theme: Theme::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreStatus {
    Checking,
    Available { mail_ready: bool },
    Unavailable(ErrorCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Navigation {
    Inbox,
    Sent,
    Drafts,
    Scheduled,
    Starred,
    Spam,
    Trash,
    Promotions,
    Folders,
    Tags,
    Contacts,
    Rules,
    Settings,
}
impl Navigation {
    pub const ALL: &'static [Self] = &[
        Self::Inbox,
        Self::Sent,
        Self::Drafts,
        Self::Scheduled,
        Self::Starred,
        Self::Spam,
        Self::Trash,
        Self::Promotions,
        Self::Folders,
        Self::Tags,
        Self::Contacts,
        Self::Rules,
        Self::Settings,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Inbox => "nav_inbox",
            Self::Sent => "nav_sent",
            Self::Drafts => "nav_drafts",
            Self::Scheduled => "nav_scheduled",
            Self::Starred => "nav_starred",
            Self::Spam => "nav_spam",
            Self::Trash => "nav_trash",
            Self::Promotions => "nav_promotions",
            Self::Folders => "nav_folders",
            Self::Tags => "nav_tags",
            Self::Contacts => "nav_contacts",
            Self::Rules => "nav_rules",
            Self::Settings => "nav_settings",
        }
    }
}

/// Current-window display choices only. This model does not persist settings or grant access.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Model {
    language: Language,
    theme: Theme,
    os_dark: bool,
    high_contrast: bool,
    selected: Navigation,
    status: CoreStatus,
}
impl Model {
    pub fn new(options: ShellOptions, os_korean: bool, os_dark: bool, high_contrast: bool) -> Self {
        Self {
            language: options.language.unwrap_or(if os_korean {
                Language::Korean
            } else {
                Language::English
            }),
            theme: options.theme,
            os_dark,
            high_contrast,
            selected: Navigation::Inbox,
            status: CoreStatus::Checking,
        }
    }
    pub fn language(&self) -> Language {
        self.language
    }
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
    }
    pub fn theme(&self) -> Theme {
        self.theme
    }
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }
    pub fn update_os(&mut self, os_dark: bool, high_contrast: bool) {
        self.os_dark = os_dark;
        self.high_contrast = high_contrast;
    }
    pub fn effective_theme(&self) -> EffectiveTheme {
        if self.high_contrast {
            return EffectiveTheme::HighContrast;
        }
        match self.theme {
            Theme::Light => EffectiveTheme::Light,
            Theme::Dark => EffectiveTheme::Dark,
            Theme::System if self.os_dark => EffectiveTheme::Dark,
            Theme::System => EffectiveTheme::Light,
        }
    }
    pub fn selected(&self) -> Navigation {
        self.selected
    }
    pub fn set_selected(&mut self, selected: Navigation) {
        self.selected = selected;
    }
    pub fn status(&self) -> CoreStatus {
        self.status
    }
    pub fn set_status(&mut self, status: CoreStatus) {
        self.status = status;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
const fn rgb(value: u32) -> Rgb {
    Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    pub background: Rgb,
    pub surface: Rgb,
    pub raised: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub border: Rgb,
    pub accent: Rgb,
    pub selected: Rgb,
    pub focus: Rgb,
}

/// High contrast uses current Windows system colors, never a fixed palette.
pub fn palette(theme: EffectiveTheme) -> Option<Palette> {
    match theme {
        EffectiveTheme::Light => Some(Palette {
            background: rgb(0xf4f6f9),
            surface: rgb(0xffffff),
            raised: rgb(0xe9edf3),
            text: rgb(0x18202b),
            muted: rgb(0x475569),
            border: rgb(0x78879a),
            accent: rgb(0x1d4ed8),
            selected: rgb(0xdbeafe),
            focus: rgb(0x1d4ed8),
        }),
        EffectiveTheme::Dark => Some(Palette {
            background: rgb(0x111827),
            surface: rgb(0x1f2937),
            raised: rgb(0x273448),
            text: rgb(0xf3f4f6),
            muted: rgb(0xcbd5e1),
            border: rgb(0x94a3b8),
            accent: rgb(0x93c5fd),
            selected: rgb(0x263d60),
            focus: rgb(0xbfdbfe),
        }),
        EffectiveTheme::HighContrast => None,
    }
}

const ENGLISH: &str = include_str!("../resources/en.txt");
const KOREAN: &str = include_str!("../resources/ko.txt");
const MISSING_TEXT: &str = "[missing text]";

fn lookup(resource: &'static str, key: &str) -> Option<&'static str> {
    resource
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(name, value)| (name == key && !value.is_empty()).then_some(value))
}
pub fn text(language: Language, key: &str) -> &'static str {
    let resource = match language {
        Language::English => ENGLISH,
        Language::Korean => KOREAN,
    };
    lookup(resource, key)
        .or_else(|| lookup(ENGLISH, key))
        .unwrap_or(MISSING_TEXT)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
