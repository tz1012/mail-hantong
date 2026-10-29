use hantong_domain::config::Language;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupTextKey {
    Title,
    Introduction,
    Password,
    VerifyConnect,
    InvalidInput,
    Unnamed,
    DefaultColor,
    DisplayName,
    Color,
    Server,
    Tls,
    Login,
    Authentication,
    AppPassword,
    ImplicitTls,
    Starttls,
}
impl SetupTextKey {
    pub const ALL: &'static [Self] = &[
        Self::Title,
        Self::Introduction,
        Self::Password,
        Self::VerifyConnect,
        Self::InvalidInput,
        Self::Unnamed,
        Self::DefaultColor,
        Self::DisplayName,
        Self::Color,
        Self::Server,
        Self::Tls,
        Self::Login,
        Self::Authentication,
        Self::AppPassword,
        Self::ImplicitTls,
        Self::Starttls,
    ];
    const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Introduction => "introduction",
            Self::Password => "password",
            Self::VerifyConnect => "verify_connect",
            Self::InvalidInput => "invalid_input",
            Self::Unnamed => "unnamed",
            Self::DefaultColor => "default_color",
            Self::DisplayName => "display_name",
            Self::Color => "color",
            Self::Server => "server",
            Self::Tls => "tls",
            Self::Login => "login",
            Self::Authentication => "authentication",
            Self::AppPassword => "app_password",
            Self::ImplicitTls => "implicit_tls",
            Self::Starttls => "starttls",
        }
    }
}
pub fn setup_text(language: Language, key: SetupTextKey) -> &'static str {
    let source = match language {
        Language::En => include_str!("../resources/setup-en.txt"),
        Language::Ko => include_str!("../resources/setup-ko.txt"),
    };
    source
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(name, _)| *name == key.as_str())
                .map(|(_, value)| value)
        })
        .expect("validated setup resource")
}
