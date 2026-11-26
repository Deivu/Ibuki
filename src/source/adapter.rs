use impero_source::runtime::logger::{LogLevel, PluginLogger};

pub struct IbukiPluginLogger;

impl PluginLogger for IbukiPluginLogger {
    fn log(&self, kind: &str, level: LogLevel, timestamp: u64, message: &[String]) {
        let message = message.join("\n");

        match level {
            LogLevel::Info => tracing::info!(kind, timestamp, "{}", message),
            LogLevel::Debug => tracing::debug!(kind, timestamp, "{}", message),
            LogLevel::Warn => tracing::warn!(kind, timestamp, "{}", message),
            LogLevel::Error => tracing::error!(kind, timestamp, "{}", message),
        }
    }
}
