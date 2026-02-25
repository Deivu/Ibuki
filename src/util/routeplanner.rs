use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use dashmap::DashMap;

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
        match s.to_lowercase().as_str() {
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
    pub size: usize,
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
    banned_ips: Arc<DashMap<String, BannedIp>>,
    banned_ip_cooldown: Duration,
    current_index: AtomicUsize,
    rotate_index: AtomicUsize,
    nano_index: AtomicUsize,
}

struct IpBlock {
    base_ip: IpAddr,
    mask: u8,
    ips: Vec<IpAddr>,
}

impl IpBlock {
    fn from_cidr(cidr: &str) -> Result<Self, String> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid CIDR notation: {}", cidr));
        }

        let base_ip = IpAddr::from_str(parts[0])
            .map_err(|e| format!("Invalid IP address: {}", e))?;
        let mask: u8 = parts[1]
            .parse()
            .map_err(|e| format!("Invalid mask: {}", e))?;

        let ips = Self::generate_ips(&base_ip, mask)?;

        Ok(IpBlock {
            base_ip,
            mask,
            ips,
        })
    }

    fn generate_ips(base_ip: &IpAddr, mask: u8) -> Result<Vec<IpAddr>, String> {
        match base_ip {
            IpAddr::V4(ipv4) => {
                let base = u32::from(*ipv4);
                let host_bits = 32 - mask;
                let num_ips = 2_u32.pow(host_bits as u32);
                if num_ips > 65536 {
                    return Err(format!("IP block too large: {} IPs", num_ips));
                }

                let mut ips = Vec::with_capacity(num_ips as usize);
                for i in 0..num_ips {
                    let ip_u32 = base + i;
                    ips.push(IpAddr::V4(std::net::Ipv4Addr::from(ip_u32)));
                }
                Ok(ips)
            }
            IpAddr::V6(_) => {
                Ok(vec![*base_ip])
            }
        }
    }

    fn get_ip(&self, index: usize) -> Option<&IpAddr> {
        self.ips.get(index % self.ips.len())
    }

    fn size(&self) -> usize {
        self.ips.len()
    }

    fn to_cidr(&self) -> String {
        format!("{}/{}", self.base_ip, self.mask)
    }
}

impl RoutePlanner {
    pub fn new(config: &crate::util::config::RoutePlannerConfig) -> Result<Self, String> {
        let strategy = Strategy::from_str(&config.strategy)?;
        
        let mut ip_blocks = Vec::new();
        for block_str in &config.ip_blocks {
            let block = IpBlock::from_cidr(block_str)?;
            tracing::info!("Loaded IP block: {} ({} addresses)", block_str, block.size());
            ip_blocks.push(block);
        }

        if ip_blocks.is_empty() {
            return Err("No IP blocks configured for route planner".to_string());
        }

        let banned_ip_cooldown = Duration::from_millis(config.banned_ip_cooldown);

        Ok(RoutePlanner {
            strategy,
            ip_blocks,
            banned_ips: Arc::new(DashMap::new()),
            banned_ip_cooldown,
            current_index: AtomicUsize::new(0),
            rotate_index: AtomicUsize::new(0),
            nano_index: AtomicUsize::new(0),
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

    fn total_slots(&self) -> u64 {
        self.ip_blocks.iter().map(|b| b.size()).sum()
    }

    fn get_ip_from_global_index(&self, mut global_index: u64) -> Option<IpAddr> {
        for block in &self.ip_blocks {
            if global_index < block.size() {
                return block.get_ip(global_index);
            }
            global_index -= block.size();
        }
        None
    }

    fn rotate_on_ban(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 { return None; }
        let index = self.current_index.load(Ordering::Relaxed) % total;
        
        if let Some(ip) = self.get_ip_from_global_index(index) {
            if !self.is_banned(&ip) {
                return Some(ip);
            }
        }
        for offset in 1..total {
            let next_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(next_index) {
                if !self.is_banned(&ip) {
                    self.current_index.store(next_index, Ordering::Relaxed);
                    return Some(ip);
                }
            }
        }

        None
    }

    fn load_balance(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 { return None; }
        let index = self.current_index.fetch_add(1, Ordering::Relaxed) % total;
        for offset in 0..total {
            let try_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    fn nano_switch(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 { return None; }
        let index = self.nano_index.fetch_add(1, Ordering::Relaxed) % total;

        for offset in 0..total {
            let try_index = (index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    fn rotating_nano_switch(&self) -> Option<IpAddr> {
        let total = self.total_slots();
        if total == 0 { return None; }
        let rotate_idx = self.rotate_index.load(Ordering::Relaxed);
        let nano_idx = self.nano_index.fetch_add(1, Ordering::Relaxed);
        
        let combined_index = (rotate_idx + nano_idx) % total;

        for offset in 0..total {
            let try_index = (combined_index + offset) % total;
            if let Some(ip) = self.get_ip_from_global_index(try_index) {
                if !self.is_banned(&ip) {
                    return Some(ip);
                }
            }
        }

        None
    }

    pub fn ban_ip(&self, ip: IpAddr) {
        tracing::warn!("Marking IP as banned: {} (cooldown: {:?})", ip, self.banned_ip_cooldown);
        
        self.banned_ips.insert(
            ip.to_string(),
            BannedIp {
                ip,
                banned_at: Instant::now(),
            },
        );

        crate::api::metrics::record_route_planner_ban();
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
        self.banned_ips.retain(|_, banned| {
            now.duration_since(banned.banned_at) < self.banned_ip_cooldown
        });
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
        let class_name = match self.strategy {
            Strategy::RotateOnBan => "RotateOnBanIpRoutePlanner",
            Strategy::LoadBalance => "BalancingIpRoutePlanner",
            Strategy::NanoSwitch => "NanoIpRoutePlanner",
            Strategy::RotatingNanoSwitch => "RotatingNanoIpRoutePlanner",
        }.to_string();

        let failing_addresses: Vec<FailingAddress> = self.banned_ips
            .iter()
            .map(|entry| FailingAddress {
                failing_address: entry.key().clone(),
                failing_timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64 - entry.value().banned_at.elapsed().as_millis() as u64,
            })
            .collect();

        let ip_block_status = self.ip_blocks.first().map(|block| IpBlockStatus {
            ip_block: block.to_cidr(),
            size: block.size(),
        }).unwrap_or(IpBlockStatus {
            ip_block: "0.0.0.0/32".to_string(),
            size: 0,
        });

        let current_index = self.current_index.load(Ordering::Relaxed);
        let rotate_index = self.rotate_index.load(Ordering::Relaxed);
        let current_ip = self.ip_blocks.first()
            .and_then(|b| b.get_ip(current_index))
            .map(|ip| ip.to_string());

        RoutePlannerStatus {
            class_name,
            details: RoutePlannerDetails {
                ip_block: ip_block_status,
                failing_addresses,
                rotate_index: Some(rotate_index.to_string()),
                ip_index: Some(current_index.to_string()),
                current_address: current_ip.clone(),
                current_address_index: Some(current_index.to_string()),
                block_index: Some("0".to_string()),
            },
        }
    }

    pub fn available_ips(&self) -> usize {
        let total: usize = self.ip_blocks.iter().map(|b| b.size()).sum();
        let banned = self.banned_ips.len();
        total.saturating_sub(banned)
    }
}
