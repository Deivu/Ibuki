use dashmap::DashMap;
use std::fmt::Display;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;

pub struct AddTask<T, F, Fut>
where
    T: Eq + Clone + Display + Hash,
    F: (FnMut() -> Fut) + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    pub key: T,
    pub duration: Duration,
    pub handler: F,
}

/// Taken from @Kashima rust src
/// Global Task Scheduler that handles every periodic events
pub struct TasksManager<T> {
    runners: Arc<DashMap<T, JoinHandle<()>>>,
}

impl<T: Eq + Hash> Default for TasksManager<T> {
    fn default() -> Self {
        Self {
            runners: Arc::new(DashMap::new()),
        }
    }
}

impl<T> TasksManager<T>
where
    T: Eq + Clone + Display + Hash,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has(&self, key: T) -> bool {
        self.runners.contains_key(&key)
    }

    pub fn add<F, Fut>(&self, mut options: AddTask<T, F, Fut>)
    where
        F: (FnMut() -> Fut) + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.runners.contains_key(&options.key) {
            return tracing::warn!(
                "Tried to set interval [{}] when it already exists",
                &options.key
            );
        }
        let runner = tokio::spawn(async move {
            let mut interval = time::interval(options.duration);
            interval.tick().await;
            loop {
                interval.tick().await;
                (options.handler)().await;
            }
        });
        self.runners.insert(options.key.clone(), runner);
        tracing::info!(
            "Started interval [{}] that will run every {} second(s)",
            &options.key,
            options.duration.as_secs()
        );
    }

    #[allow(dead_code)]
    pub fn remove(&self, key: T) {
        if !self.runners.contains_key(&key) {
            tracing::warn!("Tried to remove interval [{}] when it doesn't exists", &key);
            return;
        }
        let runner = self.runners.get(&key).unwrap();
        runner.abort();
        self.runners.remove(&key);
        tracing::info!("Deleted interval {}", &key);
    }
}
