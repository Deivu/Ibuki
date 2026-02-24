pub mod model;
pub mod source;

pub use source::SoundCloud;

pub const BASE_URL: &str = "https://api-v2.soundcloud.com";
pub const SOUNDCLOUD_URL: &str = "https://soundcloud.com";
pub const BATCH_SIZE: usize = 50;
