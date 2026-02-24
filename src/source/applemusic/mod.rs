pub mod model;
pub mod source;

pub use source::AppleMusic;

static API_BASE: &str = "https://api.music.apple.com/v1";
static MAX_PAGE_ITEMS: u32 = 300;
static BATCH_SIZE_DEFAULT: u32 = 5;
