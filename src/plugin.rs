use crate::{CONFIG, SOURCES};
use impero_source::logger::{LogLevel, PluginLogKind, PluginLogger};
use impero_source::plugin::{Plugin, QueryMatcher};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

// todo: I did an oopsie here, will fix
struct AllowAllQueries;

impl QueryMatcher for AllowAllQueries {
    fn supports_query(&self, _: &str) -> bool {
        true
    }
}

struct IbukiPluginLogger;

impl PluginLogger for IbukiPluginLogger {
    fn log(
        &self,
        kind: PluginLogKind,
        name: &str,
        level: LogLevel,
        timestamp: u64,
        message: &[String],
    ) {
        let message = message.join("\n");
        let kind = kind.as_str();

        match level {
            LogLevel::Info => tracing::info!(plugin = name, kind, timestamp, "{}", message),
            LogLevel::Debug => tracing::debug!(plugin = name, kind, timestamp, "{}", message),
            LogLevel::Warn => tracing::warn!(plugin = name, kind, timestamp, "{}", message),
            LogLevel::Error => tracing::error!(plugin = name, kind, timestamp, "{}", message),
        }
    }
}

pub async fn load_plugins() {
    let mut current_dir = std::env::current_dir().unwrap();
    tracing::debug!("Current directory: {}", current_dir.display());

    current_dir.push("plugins");

    let mut directory = match fs::read_dir(&current_dir).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Err(error) = fs::create_dir_all(&current_dir).await {
                tracing::error!("Failed to create plugin directory: {}", error);
            }

            return;
        }
        Err(error) => {
            tracing::error!("Failed to read plugin directory: {}", error);
            return;
        }
    };

    loop {
        let entry = match directory.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(error) => {
                tracing::error!("Failed to read plugin directory entry: {}", error);
                continue;
            }
        };

        let path = entry.path();

        let metadata = match entry.metadata().await {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::error!(
                    path = %path.display(),
                    "Failed to read plugin metadata: {}",
                    error
                );
                continue;
            }
        };

        if !metadata.is_file() || !is_plugin_executable(&path, &metadata) {
            continue;
        }

        tracing::info!("Loading plugin at: {:?} ...", path);

        let plugin =
            match Plugin::spawn(&path, &CONFIG.plugins, AllowAllQueries, IbukiPluginLogger).await {
                Ok(plugin) => plugin,
                Err(error) => {
                    tracing::error!(
                        path = %path.display(),
                        "Failed to load plugin: {}",
                        error
                    );
                    continue;
                }
            };

        if SOURCES.contains_key(&plugin.name) {
            tracing::warn!(
                plugin = &plugin.name,
                path = %path.display(),
                "Plugin is already loaded"
            );

            drop(plugin);

            continue;
        }

        let name = plugin.name.clone();

        SOURCES.insert(name.clone(), Arc::new(plugin));

        tracing::info!(
            plugin = name,
            path = %path.display(),
            "Loaded plugin"
        );
    }
}

#[cfg(windows)]
fn is_plugin_executable(path: &Path, _: &std::fs::Metadata) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

#[cfg(unix)]
fn is_plugin_executable(_: &Path, metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}
