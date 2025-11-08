use std::num::NonZeroU64;
use songbird::id::{GuildId, ChannelId, UserId};

pub trait FromU64 {
    fn from_u64(id: u64) -> Self;
}

macro_rules! impl_from_u64 {
    ($($t:ty),*) => {
        $(impl FromU64 for $t {
            fn from_u64(id: u64) -> Self {
                Self::from(NonZeroU64::new(id).expect("ID cannot be zero"))
            }
        })*
    };
}

impl_from_u64!(GuildId, UserId, ChannelId);