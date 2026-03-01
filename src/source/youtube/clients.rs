use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientType {
    Android,
    AndroidMusic,
    AndroidVr,
    Ios,
    Tv,
    TvEmbedded,
    Web,
    WebEmbedded,
    WebParentTools,
    WebRemix,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeContext {
    pub client: InnertubeClientInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub third_party: Option<InnertubeThirdParty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<InnertubeUser>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeClientInfo {
    pub client_name: String,
    pub client_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visitor_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_form_factor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_info: Option<ConfigInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub android_sdk_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_density_float: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_height_points: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_pixel_density: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen_width_points: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInfo {
    pub app_install_data: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeThirdParty {
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnertubeUser {
    pub locked_safety_mode: bool,
}

pub trait InnertubeClient: Send + Sync {
    fn name(&self) -> &'static str;
    fn context(&self) -> InnertubeContext;
    fn needs_cipher(&self) -> bool {
        false
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn extra_payload(&self) -> Option<serde_json::Value> {
        None
    }
    fn player_params(&self) -> Option<&'static str> {
        None
    }
}

// ---------------------------
// ANDROID
// ---------------------------
pub struct AndroidClient;
impl InnertubeClient for AndroidClient {
    fn name(&self) -> &'static str {
        "Android"
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "ANDROID".to_string(),
                client_version: "19.44.38".to_string(),
                user_agent: Some(
                    "com.google.android.youtube/19.44.38 (Linux; U; Android 11) gzip"
                        .to_string(),
                ),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Android".to_string()),
                os_version: Some("11".to_string()),
                platform: Some("MOBILE".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: Some(30),
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            (
                "User-Agent".to_string(),
                "com.google.android.youtube/19.44.38 (Linux; U; Android 11) gzip".to_string(),
            ),
            ("X-Goog-Api-Format-Version".to_string(), "2".to_string()),
        ]
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("CgIIAdgDAQ%3D%3D")
    }
}

// ---------------------------
// ANDROID MUSIC
// ---------------------------
pub struct AndroidMusicClient;
impl InnertubeClient for AndroidMusicClient {
    fn name(&self) -> &'static str {
        "AndroidMusic"
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "ANDROID_MUSIC".to_string(),
                client_version: "7.30.51".to_string(),
                user_agent: Some(
                    "com.google.android.apps.youtube.music/7.30.51 (Linux; U; Android 14) gzip"
                        .to_string(),
                ),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Android".to_string()),
                os_version: Some("14".to_string()),
                platform: Some("MOBILE".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: Some(34),
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            (
                "User-Agent".to_string(),
                "com.google.android.apps.youtube.music/7.30.51 (Linux; U; Android 14) gzip"
                    .to_string(),
            ),
            ("X-Goog-Api-Format-Version".to_string(), "2".to_string()),
        ]
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("CgIIAdgDAQ%3D%3D")
    }
}

// ---------------------------
// ANDROID VR
// ---------------------------
pub struct AndroidVrClient;
impl InnertubeClient for AndroidVrClient {
    fn name(&self) -> &'static str {
        "AndroidVR"
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "ANDROID_VR".to_string(),
                client_version: "1.60.19".to_string(),
                user_agent: Some("com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12; eureka-user Build/SQ3A.220605.009.A1) gzip".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Android".to_string()),
                os_version: Some("12".to_string()),
                platform: Some("MOBILE".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12; eureka-user Build/SQ3A.220605.009.A1) gzip".to_string()),
            ("X-Goog-Api-Format-Version".to_string(), "2".to_string()),
            ("X-YouTube-Client-Name".to_string(), "84".to_string()),
            ("X-YouTube-Client-Version".to_string(), "1.60.19".to_string()),
        ]
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("CgIIAdgDAQ%3D%3D")
    }
}

// ---------------------------
// IOS
// ---------------------------
pub struct IosClient;
impl InnertubeClient for IosClient {
    fn name(&self) -> &'static str {
        "IOS"
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "IOS".to_string(),
                client_version: "20.03.02".to_string(),
                user_agent: Some(
                    "com.google.ios.youtube/20.03.02 (iPhone16,2; U; CPU iOS 18_2_1 like Mac OS X)"
                        .to_string(),
                ),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("iOS".to_string()),
                os_version: Some("18.2.1".to_string()),
                platform: Some("MOBILE".to_string()),
                client_form_factor: Some("SMALL_FORM_FACTOR".to_string()),
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            (
                "User-Agent".to_string(),
                "com.google.ios.youtube/20.03.02 (iPhone16,2; U; CPU iOS 18_2_1 like Mac OS X)"
                    .to_string(),
            ),
            ("X-Goog-Api-Format-Version".to_string(), "2".to_string()),
            ("X-YouTube-Client-Name".to_string(), "5".to_string()),
            (
                "X-YouTube-Client-Version".to_string(),
                "20.03.02".to_string(),
            ),
        ]
    }
}

// ---------------------------
// TV
// ---------------------------
pub struct TvClient;
impl InnertubeClient for TvClient {
    fn name(&self) -> &'static str {
        "TV"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "TVHTML5".to_string(),
                client_version: "7.20241223.10.00".to_string(),
                user_agent: Some("Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.6.1039866-gold (unlike Gecko) v8/8.8.278.14-jit gyp/25.lts.6.1039866-gold Starboard/16, like TV Safari/537.36".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: None,
                os_version: None,
                platform: Some("TV".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.6.1039866-gold (unlike Gecko) v8/8.8.278.14-jit gyp/25.lts.6.1039866-gold Starboard/16, like TV Safari/537.36".to_string()),
            ("Origin".to_string(), "https://www.youtube.com".to_string()),
            ("Referer".to_string(), "https://www.youtube.com/tv".to_string()),
        ]
    }
}

// ---------------------------
// TV EMBEDDED (TVHTML5_SIMPLY)
// ---------------------------
pub struct TvEmbeddedClient;
impl InnertubeClient for TvEmbeddedClient {
    fn name(&self) -> &'static str {
        "TvEmbedded"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER".to_string(),
                client_version: "2.0".to_string(),
                user_agent: Some("Mozilla/5.0 (SmartHub; SMART-TV; U; Linux/SmartTV; QM15A; Tizen 5.5) AppleWebKit/537.3 (KHTML, like Gecko) TV Safari/537.3".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: None,
                os_version: None,
                platform: Some("TV".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: Some(InnertubeThirdParty {
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("embedUrl".to_string(), serde_json::json!("https://www.youtube.com/tv_embed"));
                    m
                }
            }),
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (SmartHub; SMART-TV; U; Linux/SmartTV; QM15A; Tizen 5.5) AppleWebKit/537.3 (KHTML, like Gecko) TV Safari/537.3".to_string()),
            ("Origin".to_string(), "https://www.youtube.com".to_string()),
            ("Referer".to_string(), "https://www.youtube.com/tv_embed".to_string()),
        ]
    }
    fn extra_payload(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "attestationRequest": { "omitBotguardData": true }
        }))
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("CgIIAQ%3D%3D")
    }
}

// ---------------------------
// WEB
// ---------------------------
pub struct WebClient;
impl InnertubeClient for WebClient {
    fn name(&self) -> &'static str {
        "Web"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "WEB".to_string(),
                client_version: "2.20241223.01.00".to_string(),
                user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Windows".to_string()),
                os_version: Some("10.0".to_string()),
                platform: Some("DESKTOP".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
            ("Origin".to_string(), "https://www.youtube.com".to_string()),
            ("Referer".to_string(), "https://www.youtube.com/".to_string()),
        ]
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("2AMB")
    }
}

// ---------------------------
// WEB REMIX
// ---------------------------
pub struct WebRemixClient;
impl InnertubeClient for WebRemixClient {
    fn name(&self) -> &'static str {
        "WebRemix"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "WEB_REMIX".to_string(),
                client_version: "1.20241223.01.00".to_string(),
                user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Windows".to_string()),
                os_version: Some("10.0".to_string()),
                platform: Some("DESKTOP".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: None,
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
            ("Origin".to_string(), "https://music.youtube.com".to_string()),
            ("Referer".to_string(), "https://music.youtube.com/".to_string()),
        ]
    }
}

// ---------------------------
// WEB EMBEDDED
// ---------------------------
pub struct WebEmbeddedClient;
impl InnertubeClient for WebEmbeddedClient {
    fn name(&self) -> &'static str {
        "WebEmbedded"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "WEB_EMBEDDED_PLAYER".to_string(),
                client_version: "1.20241223.01.00".to_string(),
                user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Windows".to_string()),
                os_version: Some("10.0".to_string()),
                platform: Some("DESKTOP".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: Some(InnertubeThirdParty {
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("embedUrl".to_string(), serde_json::json!("https://www.youtube.com/embed"));
                    m
                }
            }),
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
            ("Origin".to_string(), "https://www.youtube.com".to_string()),
            ("Referer".to_string(), "https://www.youtube.com/embed".to_string()),
        ]
    }
    fn player_params(&self) -> Option<&'static str> {
        Some("2AMB")
    }
}

// ---------------------------
// WEB PARENT TOOLS
// ---------------------------
pub struct WebParentToolsClient;
impl InnertubeClient for WebParentToolsClient {
    fn name(&self) -> &'static str {
        "WebParentTools"
    }
    fn needs_cipher(&self) -> bool {
        true
    }
    fn context(&self) -> InnertubeContext {
        InnertubeContext {
            client: InnertubeClientInfo {
                client_name: "WEB_PARENT_TOOLS".to_string(),
                client_version: "1.20241223.01.00".to_string(),
                user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
                gl: Some("US".to_string()),
                hl: Some("en".to_string()),
                visitor_data: None,
                os_name: Some("Windows".to_string()),
                os_version: Some("10.0".to_string()),
                platform: Some("DESKTOP".to_string()),
                client_form_factor: None,
                config_info: None,
                client_screen: None,
                android_sdk_version: None,
                screen_density_float: None,
                screen_height_points: None,
                screen_pixel_density: None,
                screen_width_points: None,
            },
            third_party: Some(InnertubeThirdParty {
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("embedUrl".to_string(), serde_json::json!("https://www.youtube.com/embed"));
                    m
                }
            }),
            user: Some(InnertubeUser { locked_safety_mode: false }),
        }
    }
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36".to_string()),
            ("Origin".to_string(), "https://www.youtube.com".to_string()),
            ("Referer".to_string(), "https://www.youtube.com/mod".to_string()),
        ]
    }
}

pub fn get_client_by_name(name: &str) -> Option<Box<dyn InnertubeClient>> {
    match name.to_lowercase().as_str() {
        "androidvr" | "android_vr" => Some(Box::new(AndroidVrClient)),
        "android" => Some(Box::new(AndroidClient)),
        "androidmusic" | "android_music" => Some(Box::new(AndroidMusicClient)),
        "tvembedded" | "tv_embedded" => Some(Box::new(TvEmbeddedClient)),
        "tv" | "tvhtml5" => Some(Box::new(TvClient)),
        "web" => Some(Box::new(WebClient)),
        "webembedded" | "web_embedded" => Some(Box::new(WebEmbeddedClient)),
        "webremix" | "web_remix" => Some(Box::new(WebRemixClient)),
        "webparenttools" | "web_parent_tools" => Some(Box::new(WebParentToolsClient)),
        "ios" => Some(Box::new(IosClient)),
        _ => None,
    }
}
