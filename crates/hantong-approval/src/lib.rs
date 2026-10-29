#![forbid(unsafe_code)]

//! Pure review data and presentation state. No user-verification or authority issuer.

pub mod registration_review;
pub mod resources;
pub use registration_review::{
    EffectiveTheme, Palette, Presentation, RegistrationReview, ReviewIntent, ReviewModel,
    ReviewState, Rgb,
};
pub use resources::{TextKey, text};
