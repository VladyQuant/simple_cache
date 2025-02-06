use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::interval;

#[derive(Clone)]
struct CacheItem<V> {
    ///A cache item with value and expiration period.
    value: V,
    expiration: Instant,
}

///Cache structure.
#[derive(Clone)]
pub struct Cache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    map: Arc<RwLock<HashMap<K, CacheItem<V>>>>,
    order: Arc<RwLock<VecDeque<K>>>,
    max_size: usize,
}

impl<K, V> Cache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    ///Cache intitialization.
    pub fn new(max_size: usize) -> Self {
        let cache = Cache {
            map: Arc::new(RwLock::new(HashMap::new())),
            order: Arc::new(RwLock::new(VecDeque::new())),
            max_size,
        };

        let cache_clone = cache.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(100));
            loop {
                interval.tick().await;
                cache_clone.remove_expired().await;
            }
        });

        cache
    }

    ///Get an item by key.
    pub async fn get(&self, key: K) -> Option<V> {
        let map = self.map.read().unwrap();
        if let Some(item) = map.get(&key) {
            if item.expiration > Instant::now() {
                return Some(item.value.clone());
            }
        }
        None
    }

    ///Insert an item.
    pub async fn insert(&self, key: K, value: V, ttl: Duration) {
        
        let expiration = Instant::now() + ttl;
        let mut map = self.map.write().unwrap();
        let mut order = self.order.write().unwrap();

        if map.len() >= self.max_size {
            if let Some(old_key) = order.pop_front() {
                map.remove(&old_key);
            }
        }

        map.insert(key.clone(), CacheItem { value, expiration });
        order.push_back(key);
    }

    ///Remove an item.
    pub async fn remove(&self, key: K) -> Option<V> {
        let mut map = self.map.write().unwrap();
        let mut order = self.order.write().unwrap();

        if let Some(item) = map.remove(&key) {
            order.retain(|k| k != &key);
            return Some(item.value);
        }
        None
    }

    ///Remove all expired items.
    async fn remove_expired(&self) {
        let mut map = self.map.write().unwrap();
        let mut order = self.order.write().unwrap();

        let now = Instant::now();
        order.retain(|key| {
            if let Some(item) = map.get(key) {
                if item.expiration > now {
                    return true;
                }
            }
            map.remove(key);
            false
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_get() {
        let cache: Cache<String, i32> = Cache::new(4);
        cache
            .insert("apple".to_string(), 2, Duration::from_secs(3))
            .await;

        let value = cache.get("apple".to_string()).await;
        assert_eq!(value, Some(2));
    }

    #[tokio::test]
    async fn test_expiration() {
        let cache: Cache<String, String> = Cache::new(10);
        cache
            .insert(
                "Bob".to_string(),
                "Marley".to_string(),
                Duration::from_secs(1),
            )
            .await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        let value = cache.get("Bob".to_string()).await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_remove() {
        let cache: Cache<String, String> = Cache::new(10);
        cache
            .insert(
                "Bob".to_string(),
                "Marley".to_string(),
                Duration::from_secs(10),
            )
            .await;

        let removed_value = cache.remove("Bob".to_string()).await;
        assert_eq!(removed_value, Some("Marley".to_string()));

        let value = cache.get("Bob".to_string()).await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_max_size() {
        let cache: Cache<String, String> = Cache::new(2);
        cache
            .insert(
                "Bob".to_string(),
                "Marley".to_string(),
                Duration::from_secs(10),
            )
            .await;
        cache
            .insert(
                "Decemer".to_string(),
                "Bueno".to_string(),
                Duration::from_secs(10),
            )
            .await;
        cache
            .insert(
                "Nicky".to_string(),
                "Jam".to_string(),
                Duration::from_secs(10),
            )
            .await;

        let value1 = cache.get("Bob".to_string()).await;
        assert_eq!(value1, None);

        let value2 = cache.get("Decemer".to_string()).await;
        assert_eq!(value2, Some("Bueno".to_string()));

        let value3 = cache.get("Nicky".to_string()).await;
        assert_eq!(value3, Some("Jam".to_string()));
    }

    #[tokio::test]
    async fn load_test_cache() {
        let cache: Cache<String, String> = Cache::new(100);
        let num_tasks = 1000;
        let num_iterations = 10;

        let tasks: Vec<_> = (0..num_tasks)
            .map(|i| {
                let cache = cache.clone();
                tokio::spawn(async move {
                    for j in 0..num_iterations {
                        let key = format!("key_{}_{}", i, j);
                        let value = format!("value_{}_{}", i, j);

                        // Insert
                        cache
                            .insert(key.clone(), value.clone(), Duration::from_secs(10))
                            .await;

                        // Get
                        let _ = cache.get(key.clone()).await;

                        // Remove
                        let _ = cache.remove(key.clone()).await;
                    }
                })
            })
            .collect();

        for task in tasks {
            let _ = task.await;
        }

        assert!(cache.map.read().unwrap().len() <= 100);
    }

    #[tokio::test]
    async fn high_concurrency_test() {
        let cache: Cache<String, String> = Cache::new(50);
        let num_tasks = 500;

        let tasks: Vec<_> = (0..num_tasks)
            .map(|i| {
                let cache = cache.clone();
                tokio::spawn(async move {
                    let key = format!("key_{}", i);
                    let value = format!("value_{}", i);

                    // Insert
                    cache
                        .insert(key.clone(), value.clone(), Duration::from_secs(10))
                        .await;

                    // Get
                    let _ = cache.get(key.clone()).await;

                    // Remove
                    let _ = cache.remove(key.clone()).await;
                })
            })
            .collect();

        for task in tasks {
            let _ = task.await;
        }

        assert!(cache.map.read().unwrap().len() <= 50);
    }
}
