//! Time consuming task management.

use std::{
    future::Future,
    sync::{Arc, Mutex, MutexGuard},
};

pub struct Task<T: Send + 'static>(Arc<Mutex<Option<T>>>);

impl<T: Send + 'static> Clone for Task<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Send + 'static> Task<T> {
    pub fn new(future: impl Future<Output = T> + Send + 'static) -> Self {
        let arc = Arc::new(Mutex::new(None));
        {
            let arc = Arc::clone(&arc);
            tokio::spawn(async move {
                let result = future.await;
                *arc.lock().unwrap() = Some(result);
            });
        }
        Self(arc)
    }

    pub fn pending() -> Self {
        Self::new(std::future::pending())
    }

    pub fn ok(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }

    pub fn take(&mut self) -> Option<T> {
        self.0.lock().unwrap().take()
    }

    pub fn get(&self) -> MutexGuard<'_, Option<T>> {
        self.0.lock().unwrap()
    }
}

impl<T: Send + Clone + 'static> Task<T> {
    pub fn clone_result(&self) -> Option<T> {
        self.0.lock().unwrap().clone()
    }
}
