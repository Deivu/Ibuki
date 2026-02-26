use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone)]
pub struct Variant {
    pub url: String,
    pub bandwidth: u64,
    pub codecs: Option<String>,
    pub audio: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AudioGroup {
    pub uri: Option<String>,
    pub group_id: String,
    pub language: Option<String>,
    pub name: Option<String>,
    pub default: bool,
    pub autoselect: bool,
}

#[derive(Debug, Clone)]
pub struct Key {
    pub method: String,
    pub uri: String,
    pub iv: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub uri: String,
    pub byte_range: Option<ByteRange>,
}

#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub length: u64,
    pub offset: u64,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub url: String,
    pub duration: f64,
    pub sequence: u64,
    pub key: Option<Key>,
    pub map: Option<Map>,
    pub byte_range: Option<ByteRange>,
    pub discontinuity: bool,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub is_master: bool,
    pub variants: Vec<Variant>,
    pub audio_groups: HashMap<String, Vec<AudioGroup>>,
    pub segments: Vec<Segment>,
    pub target_duration: f64,
    pub media_sequence: u64,
    pub is_live: bool,
}

pub struct PlaylistParser;

impl PlaylistParser {
    pub fn parse(content: &str, base_url: &str) -> Option<Playlist> {
        if !content.contains("#EXT") {
            return None;
        }

        let lines: Vec<&str> = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();

        if lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF")) {
            let (variants, audio_groups) = Self::parse_master(&lines, base_url);
            return Some(Playlist {
                is_master: true,
                variants,
                audio_groups,
                segments: Vec::new(),
                target_duration: 0.0,
                media_sequence: 0,
                is_live: false,
            });
        }

        let mut segments = Vec::new();
        let mut target_duration = 5.0;
        let mut media_sequence = 0;
        let is_live = !content.contains("#EXT-X-ENDLIST");

        let mut current_key: Option<Key> = None;
        let mut current_map: Option<Map> = None;
        let mut last_byte_range: Option<ByteRange> = None;
        let mut pending_discontinuity = false;

        for line in &lines {
            if line.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
                if let Ok(seq) = line.split(':').nth(1).unwrap_or("0").parse::<u64>() {
                    media_sequence = seq;
                }
            } else if line.starts_with("#EXT-X-TARGETDURATION:") {
                if let Ok(dur) = line.split(':').nth(1).unwrap_or("5").parse::<f64>() {
                    target_duration = dur;
                }
            }
        }

        let mut segment_index = 0;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];

            if line.starts_with("#EXT-X-DISCONTINUITY") {
                pending_discontinuity = true;
            } else if line.starts_with("#EXT-X-KEY:") {
                current_key = Self::parse_key_attributes(line, base_url);
            } else if line.starts_with("#EXT-X-MAP:") {
                current_map = Self::parse_map_attributes(line, base_url);
            } else if line.starts_with("#EXTINF:") {
                let duration_str = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("0")
                    .split(',')
                    .next()
                    .unwrap_or("0");
                let duration = duration_str.parse::<f64>().unwrap_or(0.0);

                let mut j = i + 1;
                while j < lines.len() && lines[j].starts_with('#') {
                    if lines[j].starts_with("#EXT-X-BYTERANGE:") {
                        last_byte_range = Self::parse_byte_range(lines[j], last_byte_range);
                    }
                    j += 1;
                }

                if j < lines.len() {
                    let segment_url = lines[j];
                    let url = if let Ok(u) = Url::parse(base_url).and_then(|b| b.join(segment_url))
                    {
                        u.to_string()
                    } else {
                        segment_url.to_string()
                    };

                    segments.push(Segment {
                        url,
                        duration,
                        sequence: media_sequence + segment_index,
                        key: current_key.clone(),
                        map: current_map.clone(),
                        byte_range: last_byte_range,
                        discontinuity: pending_discontinuity,
                    });

                    segment_index += 1;
                    last_byte_range = None;
                    pending_discontinuity = false;
                    i = j;
                }
            }
            i += 1;
        }

        Some(Playlist {
            is_master: false,
            variants: Vec::new(),
            audio_groups: HashMap::new(),
            segments,
            target_duration,
            media_sequence,
            is_live,
        })
    }

    fn parse_master(
        lines: &[&str],
        base_url: &str,
    ) -> (Vec<Variant>, HashMap<String, Vec<AudioGroup>>) {
        let mut variants = Vec::new();
        let mut audio_groups: HashMap<String, Vec<AudioGroup>> = HashMap::new();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("#EXT-X-MEDIA:") {
                let attrs = Self::parse_attributes(line);
                if attrs.get("TYPE").map(|s| s.as_str()) == Some("AUDIO") {
                    if let Some(group_id) = attrs.get("GROUP-ID").map(|s| s.to_string()) {
                        let group = AudioGroup {
                            uri: attrs.get("URI").map(|s| {
                                Url::parse(base_url)
                                    .and_then(|b| b.join(s))
                                    .map(|u| u.to_string())
                                    .unwrap_or(s.to_string())
                            }),
                            group_id: group_id.clone(),
                            language: attrs.get("LANGUAGE").map(|s| s.to_string()),
                            name: attrs.get("NAME").map(|s| s.to_string()),
                            default: attrs.get("DEFAULT").map(|s| s == "YES").unwrap_or(false),
                            autoselect: attrs
                                .get("AUTOSELECT")
                                .map(|s| s == "YES")
                                .unwrap_or(false),
                        };
                        audio_groups.entry(group_id).or_default().push(group);
                    }
                }
            } else if line.starts_with("#EXT-X-STREAM-INF:") {
                let attrs = Self::parse_attributes(line);
                i += 1;
                if i < lines.len() {
                    let url_line = lines[i];
                    let url = Url::parse(base_url)
                        .and_then(|b| b.join(url_line))
                        .map(|u| u.to_string())
                        .unwrap_or(url_line.to_string());

                    variants.push(Variant {
                        url,
                        bandwidth: attrs
                            .get("BANDWIDTH")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        codecs: attrs.get("CODECS").map(|s| s.to_string()),
                        audio: attrs.get("AUDIO").map(|s| s.to_string()),
                    });
                }
            }
            i += 1;
        }

        // Sort by bandwidth descending
        variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

        (variants, audio_groups)
    }

    fn parse_attributes(line: &str) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        let content = line.split_once(':').map(|(_, c)| c).unwrap_or("");

        // Regex-like parsing for KEY=VALUE or KEY="VALUE" patterns
        let mut chars = content.chars().peekable();
        let mut key = String::new();
        let mut value = String::new();
        let mut in_quotes = false;
        let mut reading_key = true;

        while let Some(c) = chars.next() {
            if reading_key {
                if c == '=' {
                    reading_key = false;
                } else if c != ',' && !c.is_whitespace() {
                    key.push(c);
                }
            } else {
                if c == '"' {
                    in_quotes = !in_quotes;
                } else if c == ',' && !in_quotes {
                    if !key.is_empty() {
                        attrs.insert(key.trim().to_uppercase(), value.trim().to_string());
                    }
                    key.clear();
                    value.clear();
                    reading_key = true;
                } else {
                    value.push(c);
                }
            }
        }

        if !key.is_empty() {
            attrs.insert(key.trim().to_uppercase(), value.trim().to_string());
        }

        attrs
    }

    fn parse_key_attributes(line: &str, base_url: &str) -> Option<Key> {
        let attrs = Self::parse_attributes(line);
        let method = attrs.get("METHOD")?.clone();

        if method == "NONE" {
            return Some(Key {
                method,
                uri: String::new(),
                iv: None,
            });
        }

        let uri = attrs.get("URI").map(|s| {
            Url::parse(base_url)
                .and_then(|b| b.join(s))
                .map(|u| u.to_string())
                .unwrap_or_else(|_| s.to_string())
        })?;

        let iv = attrs.get("IV").and_then(|iv_str| {
            if iv_str.starts_with("0x") || iv_str.starts_with("0X") {
                hex::decode(&iv_str[2..]).ok()
            } else {
                None
            }
        });

        Some(Key { method, uri, iv })
    }

    fn parse_map_attributes(line: &str, base_url: &str) -> Option<Map> {
        let attrs = Self::parse_attributes(line);
        let uri = attrs.get("URI").map(|s| {
            Url::parse(base_url)
                .and_then(|b| b.join(s))
                .map(|u| u.to_string())
                .unwrap_or_else(|_| s.to_string())
        })?;

        let byte_range = attrs
            .get("BYTERANGE")
            .and_then(|s| Self::parse_byte_range_value(s, None));

        Some(Map { uri, byte_range })
    }

    fn parse_byte_range(line: &str, last_range: Option<ByteRange>) -> Option<ByteRange> {
        let range_str = line.split(':').nth(1)?;
        Self::parse_byte_range_value(range_str, last_range)
    }

    fn parse_byte_range_value(range_str: &str, last_range: Option<ByteRange>) -> Option<ByteRange> {
        let parts: Vec<&str> = range_str.trim().split('@').collect();
        let length = parts.get(0)?.parse::<u64>().ok()?;

        let offset = if let Some(offset_str) = parts.get(1) {
            offset_str.parse::<u64>().ok()?
        } else if let Some(last) = last_range {
            last.offset + last.length
        } else {
            0
        };

        Some(ByteRange { length, offset })
    }
}
