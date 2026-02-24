pub mod model;
pub mod source;

pub use source::AmazonMusic;

static API_BASE: &str = "https://na.mesk.skill.music.a2z.com/api";
static SEARCH_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36";
static BOT_USER_AGENT: &str = "Mozilla/5.0 (compatible; Ibuki/0.1; +https://github.com/Deivu/Ibuki)";
static MUSIC_BASE: &str = "https://music.amazon.com";
static CONFIG_URL: &str = "https://music.amazon.com/config.json";
static FALLBACK_DEVICE_ID: &str = "13580682033287541";
static FALLBACK_SESSION_ID: &str = "142-4001091-4160417";
