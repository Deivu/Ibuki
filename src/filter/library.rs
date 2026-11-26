use super::adapter::PluginFilter;
use abi_stable::RRef;
use abi_stable::library::lib_header_from_path;
use abi_stable::std_types::{RBox, RErr, ROk};
use impero_filter::filter::AudioFilter_TO;
use impero_filter::module::FilterModuleRef;
use impero_filter::object::FilterValue;
use songbird::Config;
use songbird::driver::{AudioFilter, FilterError};
use std::env::consts::DLL_EXTENSION;
use std::env::current_dir;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static LIBRARY: OnceLock<FilterLibrary> = OnceLock::new();

pub struct FilterLibrary {
    module: FilterModuleRef,
    names: Vec<String>,
}

impl FilterLibrary {
    pub fn load_filters() {
        let directory = match current_dir() {
            Ok(directory) => directory.join("plugins"),
            Err(error) => {
                tracing::warn!(
                    "Failed to read the current directory, filters are unavailable: {}",
                    error
                );
                return;
            }
        };

        let entries = match read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    "Failed to read plugin directory, filters are unavailable: {}",
                    error
                );
                return;
            }
        };

        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_file() && Self::is_filter_library(path))
            .collect();

        paths.sort();

        for path in paths {
            if LIBRARY.get().is_some() {
                tracing::warn!(
                    path = %path.display(),
                    "A filter library is already loaded, skipping"
                );
                continue;
            }

            let module = match lib_header_from_path(&path)
                .and_then(|header| header.init_root_module::<FilterModuleRef>())
            {
                Ok(module) => module,
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        "Failed to load filter library, skipping: {}",
                        error
                    );
                    continue;
                }
            };

            let names = (module.filters())()
                .into_iter()
                .map(|name| name.into_string())
                .collect();

            LIBRARY.set(Self { module, names }).ok();
            tracing::info!(path = %path.display(), "Loaded filter library");
        }

        if LIBRARY.get().is_none() {
            tracing::warn!("No filter library loaded, filters are unavailable");
        }
    }

    pub fn is_filter_library(path: &Path) -> bool {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(DLL_EXTENSION))
    }

    pub fn names() -> &'static [String] {
        LIBRARY
            .get()
            .map(|library| library.names.as_slice())
            .unwrap_or_default()
    }

    pub fn register_filters(config: Config) -> Config {
        Self::names().iter().fold(config, |config, name| {
            let filter = name.clone();

            config.add_filter(name.as_str(), move || Self::plugin_filter(&filter))
        })
    }

    pub fn validate(name: &str, value: &FilterValue) -> Result<(), String> {
        let mut filter = Self::new_filter(name)?;

        match filter.configure(RRef::new(value)) {
            ROk(()) => Ok(()),
            RErr(error) => Err(error.to_string()),
        }
    }

    fn plugin_filter(name: &str) -> Result<Box<dyn AudioFilter>, FilterError> {
        Ok(Box::new(PluginFilter(Self::new_filter(name)?)))
    }

    fn new_filter(name: &str) -> Result<AudioFilter_TO<'static, RBox<()>>, String> {
        let library = LIBRARY
            .get()
            .ok_or_else(|| "Filter library is not loaded".to_string())?;

        (library.module.new_filter())(name.into())
            .into_option()
            .ok_or_else(|| format!("Filter library has no filter named {}", name))
    }
}
