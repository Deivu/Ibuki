use super::library::FilterLibrary;
use impero_filter::object::{FilterObject, FilterValue};
use std::any::Any;

const PLUGIN_FILTERS: &str = "pluginFilters";

pub type DriverFilters = Vec<(String, Box<dyn Any + Send>)>;

pub fn driver_filters(filters: &FilterObject) -> DriverFilters {
    FilterLibrary::names()
        .iter()
        .map(|name| {
            let value = filter_value(filters, name).cloned().unwrap_or_default();

            (name.clone(), Box::new(value) as Box<dyn Any + Send>)
        })
        .collect()
}

pub fn check_filters(filters: &FilterObject) -> Result<(), String> {
    let known = FilterLibrary::names();
    let plugin_filters = filters.get_object(PLUGIN_FILTERS);

    let requested = filters
        .iter()
        .filter(|(name, _)| *name != PLUGIN_FILTERS)
        .chain(plugin_filters.into_iter().flat_map(|plugin| plugin.iter()));

    for (name, _) in requested {
        if !known.iter().any(|known| known == name) {
            tracing::debug!(
                "Ignored filter:{}, known filters: [{}]",
                name,
                known.join(", ")
            );
        }
    }

    if let Some(plugin_filters) = plugin_filters {
        for (name, _) in plugin_filters.iter() {
            if filters.get(name).is_some() {
                // no duplicate filters, only one is used, which is the first one that appeared.
                continue;
            }
        }
    }

    for name in known {
        if let Some(value) = filter_value(filters, name) {
            FilterLibrary::validate(name, value)?;
        }
    }

    Ok(())
}

fn filter_value<'a>(filters: &'a FilterObject, name: &str) -> Option<&'a FilterValue> {
    filters
        .get(name)
        .or_else(|| filters.get_object(PLUGIN_FILTERS)?.get(name))
}
