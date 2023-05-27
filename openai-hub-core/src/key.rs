use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::{fmt, mem};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct KeyPool {
    total: usize,
    keys: Mutex<VecDeque<String>>,
    semaphore: Arc<Semaphore>,
}

#[clippy::has_significant_drop]
pub struct KeyGuard {
    key: String,
    pool: Arc<KeyPool>,
    _permit: OwnedSemaphorePermit,
}

impl KeyPool {
    pub fn new(iter: impl IntoIterator<Item = String>) -> Self {
        let keys = VecDeque::from_iter(iter);
        let semaphore = Semaphore::new(keys.len());

        Self {
            total: keys.len(),
            keys: Mutex::new(keys),
            semaphore: Arc::new(semaphore),
        }
    }

    pub async fn get(self: Arc<Self>) -> KeyGuard {
        let permit = self.semaphore.clone().acquire_owned().await.unwrap();
        let key = self.keys.lock().pop_front().unwrap();

        KeyGuard {
            key,
            pool: self.clone(),
            _permit: permit,
        }
    }
}

impl fmt::Debug for KeyPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPool")
            .field("available", &self.semaphore.available_permits())
            .field("total", &self.total)
            .finish()
    }
}

impl KeyGuard {
    pub fn as_str(&self) -> &str {
        &self.key
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        let key = mem::take(&mut self.key);
        self.pool.keys.lock().push_back(key);
    }
}
