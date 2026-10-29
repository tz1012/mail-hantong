#![forbid(unsafe_code)]

//! Pure review data and presentation state. No user-verification or authority issuer.

pub mod account_setup_review;
pub mod registration_review;
pub mod resources;
pub mod scope_refresh_review;
pub mod setup_resources;
pub use account_setup_review::{AccountSetupReview, setup_summary};
pub use registration_review::{
    EffectiveTheme, Palette, Presentation, RegistrationReview, ReviewIntent, ReviewModel,
    ReviewState, Rgb,
};
pub use resources::{TextKey, text};
pub use scope_refresh_review::ScopeRefreshReview;
pub use setup_resources::{SetupTextKey, setup_text};
