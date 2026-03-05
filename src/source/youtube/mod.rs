pub mod api;
pub mod chat;
pub mod cipher;
pub mod clients;
pub mod manager;
pub mod oauth;
pub mod sabr;
pub mod source;
pub mod stream;

pub const INNERTUBE_API_BASE: &str = "https://www.youtube.com";
pub const YOUTUBE_MUSIC_API_BASE: &str = "https://music.youtube.com";

use std::sync::{Arc, OnceLock};

pub static YOUTUBE_MANAGER: OnceLock<Arc<manager::YouTubeManager>> = OnceLock::new();
