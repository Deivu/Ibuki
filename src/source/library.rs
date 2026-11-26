use super::adapter::IbukiPluginLogger;
use crate::CONFIG;
use crate::filter::library::FilterLibrary;
use impero_source::runtime::plugin::Plugin;
use std::io::ErrorKind;
use std::sync::{Arc, OnceLock};
use tokio::fs;

static LIBRARY: OnceLock<SourceLibrary> = OnceLock::new();

pub struct SourceLibrary {
    sources: Vec<Arc<Plugin>>,
}

impl SourceLibrary {
    pub async fn load_sources() {
        let mut current_dir = std::env::current_dir().unwrap();
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

        let mut paths = Vec::new();

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

            if !metadata.is_file() || FilterLibrary::is_filter_library(&path) {
                continue;
            }

            paths.push(path);
        }

        paths.sort();

        let mut sources: Vec<Arc<Plugin>> = Vec::with_capacity(paths.len());

        for path in paths {
            tracing::info!("Loading plugin at: {:?} ...", path);

            let plugin = match Plugin::spawn(&path, &CONFIG.plugins, IbukiPluginLogger).await {
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

            if sources.iter().any(|source| source.name == plugin.name) {
                tracing::warn!(
                    plugin = &plugin.name,
                    path = %path.display(),
                    "Plugin is already loaded"
                );

                drop(plugin);

                continue;
            }

            let name = plugin.name.clone();

            sources.push(Arc::new(plugin));

            tracing::info!(
                plugin = name,
                path = %path.display(),
                "Loaded plugin"
            );
        }

        LIBRARY.set(Self { sources }).ok();
    }

    pub fn sources() -> &'static [Arc<Plugin>] {
        LIBRARY
            .get()
            .map(|library| library.sources.as_slice())
            .unwrap_or_default()
    }
}
