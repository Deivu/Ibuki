use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::util::config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    RotateOnBan,
    LoadBalance,
    NanoSwitch,
    RotatingNanoSwitch,
}

impl FromStr for Strategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().replace('-', "").replace('_', "").as_str() {
            "rotateonban" => Ok(Strategy::RotateOnBan),
            "loadbalance" => Ok(Strategy::LoadBalance),
            "nanoswitch" => Ok(Strategy::NanoSwitch),
            "rotatingnanoswitch" => Ok(Strategy::RotatingNanoSwitch),
            _ => Err(format!("Unknown strategy: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpBlockStatus {
    pub ip_block: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailingAddress {
    pub failing_address: String,
    pub failing_timestamp: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlannerStatus {
    #[serde(rename = "class")]
    pub class_name: String,
    pub details: RoutePlannerDetails,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlannerDetails {
    pub ip_block: IpBlockStatus,
    pub failing_addresses: Vec<FailingAddress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotate_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_address_index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_index: Option<String>,
}

struct BannedIp {
    ip: IpAddr,
    banned_at: Instant,
}

pub struct RoutePlanner {
    strategy: Strategy,
    ip_blocks: Vec<IpBlock>,
    excluded_ips: HashSet<IpAddr>,
    banned_ips: Arc<DashMap<String, BannedIp>>,
    banned_ip_cooldown: Duration,
    current_index: AtomicU64,
    rotate_index: AtomicU64,
}

struct IpBlock {
    base_ip: IpAddr,
    mask: u8,
}

impl IpBlock {
    fn from_cidr(cidr: &str) -> Result<Self, String> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid CIDR notation: {}", cidr));
        }

        let base_ip =
            IpAddr::from_str(parts[0]).map_err(|e| format!("Invalid IP address: {}", e))?;
        let mask: u8 = parts[1]
            .parse()
            .map_err(|e| format!("Invalid mask: {}", e))?;

        let base_ip = match base_ip {
            IpAddr::V4(v4) => {
                if mask > 32 {
                    return Err(format!("Invalid IPv4 mask: {}", mask));
                }
                let mask_val = if mask == 0 {
                    0
                } else {
                    u32::MAX << (32 - mask)
                };
                IpAddr::V4(Ipv4Addr::from(u32::from(v4) & mask_val))
            }
            IpAddr::V6(v6) => {
                if mask > 128 {
                    return Err(format!("Invalid IPv6 mask: {}", mask));
                }
                let mask_val = if mask == 0 {
                    0
                } else {
                    u128::MAX << (128 - mask)
                };
                IpAddr::V6(Ipv6Addr::from(u128::from(v6) & mask_val))
            }
        };

        Ok(IpBlock { base_ip, mask })
    }

    fn get_ip(&self, index: u128) -> Option<IpAddr> {
        let size = self.size();
        if size == 0 {
            return None;
        }
        let idx = index % size;

        match self.base_ip {
            IpAddr::V4(v4) => {
                let base = u32::from(v4);
                let ip_u32 = base.checked_add(idx as u32)?;
                Some(IpAddr::V4(Ipv4Addr::from(ip_u32)))
            }
            IpAddr::V6(v6) => {
                let base = u128::from(v6);
                let ip_u128 = base.checked_add(idx)?;
                Some(IpAddr::V6(Ipv6Addr::from(ip_u128)))
            }
        }
    }

    fn size(&self) -> u128 {
        match self.base_ip {
            IpAddr::V4(_) => 1u128 << (32u8.saturating_sub(self.mask)),
            IpAddr::V6(_) => {
                let host_bits = 128u8.saturating_sub(self.mask);
                if host_bits >= 128 {
                    u128::MAX
                } else {
                    1u128 << host_bits
                }
            }
        }
    }

    fn to_cidr(&self) -> String {
        format!("{}/{}", self.base_ip, self.mask)
    }

    fn sub_block_count(&self) -> u128 {
        match self.base_ip {
            IpAddr::V4(_) => 1,
            IpAddr::V6(_) => {
                if self.mask >= 64 {
                    1
                } else {
                    1u128 << (64 - self.mask)
                }
            }
        }
    }
}

impl RoutePlanner {
    pub fn new(config: &config::RoutePlannerConfig) -> Result<Self, String> {
        let strategy = Strategy::from_str(&config.strategy)?;

        let mut ip_blocks = Vec::new();
        for block_str in &config.ip_blocks {
            let block = IpBlock::from_cidr(block_str)?;
            tracing::info!(
                "Loaded IP block: {} ({} addresses)",
                block_str,
                block.size()
            );
            ip_blocks.push(block);
        }

        if ip_blocks.is_empty() {
            return Err("No IP blocks configured for route planner".to_string());
        }

        let mut excluded_ips = HashSet::new();
        if let Some(excluded) = &config.excluded_ips {
            for ip_str in excluded {
                match IpAddr::from_str(ip_str) {
                    Ok(ip) => {
                        excluded_ips.insert(ip);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse excluded IP '{}': {}", ip_str, e);
                    }
                }
            }
        }

        let banned_ip_cooldown = Duration::from_millis(config.banned_ip_cooldown);

        Ok(RoutePlanner {
            strategy,
            ip_blocks,
            excluded_ips,
            banned_ips: Arc::new(DashMap::new()),
            banned_ip_cooldown,
            current_index: AtomicU64::new(0),
            rotate_index: AtomicU64::new(0),
        })
    }

    pub fn get_next_ip(&self) -> Option<IpAddr> {
        self.cleanup_expired_bans();

        match self.strategy {
            Strategy::RotateOnBan => self.rotate_on_ban(),
            Strategy::LoadBalance => self.load_balance(),
            Strategy::NanoSwitch => self.nano_switch(),
            Strategy::RotatingNanoSwitch => self.rotating_nano_switch(),
        }
    }

    fn total_slots(&self) -> u128 {
        self.ip_blocks.iter().map(|b| b.size()).sum()
    }

    fn get_ip_from_global_index(&self, mut global_index: u128) -> Option<IpAddr> {
        for block in &self.ip_blocks {
            let size = block.size();
            if global_index < size {
                return block.get_ip(global_index);
            }
            global_index -= size;
        }
        None
    }

    fn rotate_on_ban(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 {
            return None;
        }
        let index = (self.current_index.load(Ordering::Relaxed) as u128) % total;

        if let Some(ip) = self.get_ip_from_global_index(index) {
            if !self.is_banned(&ip) && !self.excluded_ips.contains(&ip) {
                return Some(ip);
            }
        }

        let search_limit = std::cmp::min(total, 65536);
        for offset in 1..search_limit {
            let next_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(next_index) {
                if !self.is_banned(&ip) && !self.excluded_ips.contains(&ip) {
                    self.current_index
                        .store(next_index as u64, Ordering::Relaxed);
                    return Some(ip);
                }
            }
        }

        None
    }

    fn load_balance(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 {
            return None;
        }
        let index = (self.current_index.fetch_add(1, Ordering::Relaxed) as u128) % total;

        let search_limit = std::cmp::min(total, 65536);
        for offset in 0..search_limit {
            let try_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) && !self.excluded_ips.contains(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    fn nano_switch(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 {
            return None;
        }

        let nano = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let index = nano % total;

        let search_limit = std::cmp::min(total, 1024);
        for offset in 0..search_limit {
            let try_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) && !self.excluded_ips.contains(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    fn rotating_nano_switch(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 {
            return None;
        }

        let nano = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let rotate_idx = self.rotate_index.load(Ordering::Relaxed) as u128;
        let combined_index = nano.wrapping_add(rotate_idx) % total;

        let search_limit = std::cmp::min(total, 1024);
        for offset in 0..search_limit {
            let try_index = (combined_index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) && !self.excluded_ips.contains(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    pub fn ban_ip(&self, ip: IpAddr) {
        tracing::warn!(
            "Marking IP as banned: {} (cooldown: {:?})",
            ip,
            self.banned_ip_cooldown
        );

        self.banned_ips.insert(
            ip.to_string(),
            BannedIp {
                ip,
                banned_at: Instant::now(),
            },
        );

        match self.strategy {
            Strategy::RotateOnBan => {
                self.current_index.fetch_add(1, Ordering::Relaxed);
            }
            Strategy::RotatingNanoSwitch => {
                self.rotate_index.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    fn is_banned(&self, ip: &IpAddr) -> bool {
        if let Some(entry) = self.banned_ips.get(&ip.to_string()) {
            let elapsed = entry.banned_at.elapsed();
            if elapsed < self.banned_ip_cooldown {
                return true;
            }
        }
        false
    }

    fn cleanup_expired_bans(&self) {
        let now = Instant::now();
        self.banned_ips
            .retain(|_, banned| now.duration_since(banned.banned_at) < self.banned_ip_cooldown);
    }

    pub fn unban_ip(&self, ip: IpAddr) -> bool {
        self.banned_ips.remove(&ip.to_string()).is_some()
    }

    pub fn unban_all(&self) {
        let count = self.banned_ips.len();
        self.banned_ips.clear();
        tracing::info!("Unbanned all {} IP addresses", count);
    }

    pub fn get_status(&self) -> RoutePlannerStatus {
        self.cleanup_expired_bans();
        let class_name = match self.strategy {
            Strategy::RotateOnBan => "RotateOnBanIpRoutePlanner",
            Strategy::LoadBalance => "BalancingIpRoutePlanner",
            Strategy::NanoSwitch => "NanoIpRoutePlanner",
            Strategy::RotatingNanoSwitch => "RotatingNanoIpRoutePlanner",
        }
        .to_string();

        let failing_addresses: Vec<FailingAddress> = self
            .banned_ips
            .iter()
            .map(|entry| FailingAddress {
                failing_address: entry.key().clone(),
                failing_timestamp: std::time::SystemTime::now()
                    .checked_sub(entry.value().banned_at.elapsed())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis().try_into().unwrap_or(u64::MAX))
                    .unwrap_or(0),
            })
            .collect();

        let ip_block_status = self
            .ip_blocks
            .first()
            .map(|block| IpBlockStatus {
                ip_block: block.to_cidr(),
                size: block.size().to_string(),
            })
            .unwrap_or_else(|| IpBlockStatus {
                ip_block: "0.0.0.0/32".to_string(),
                size: "0".to_string(),
            });

        let current_index = self.current_index.load(Ordering::Relaxed);
        let rotate_index = self.rotate_index.load(Ordering::Relaxed);
        let current_ip = self
            .ip_blocks
            .first()
            .and_then(|b| b.get_ip(current_index as u128))
            .map(|ip| ip.to_string());

        RoutePlannerStatus {
            class_name,
            details: RoutePlannerDetails {
                ip_block: ip_block_status,
                failing_addresses,
                rotate_index: Some(rotate_index.to_string()),
                ip_index: Some(current_index.to_string()),
                current_address: current_ip,
                current_address_index: Some(current_index.to_string()),
                block_index: Some("0".to_string()),
            },
        }
    }

    pub fn available_ips(&self) -> u128 {
        self.cleanup_expired_bans();
        let total = self.total_slots();
        let banned = self.banned_ips.len() as u128;
        let excluded = self.excluded_ips.len() as u128;
        total.saturating_sub(banned).saturating_sub(excluded)
    }
}
