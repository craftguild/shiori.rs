use super::{Error, Format, Variant};
use lru::LruCache;
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub hash: [u8; 32],
    pub width: u32,
    pub format: Format,
    pub policy: u8,
}
pub(super) type Result = std::result::Result<Arc<Variant>, Error>;
pub(super) struct Flight {
    result: Mutex<Option<Result>>,
    ready: Condvar,
}
impl Flight {
    pub fn wait(&self) -> Result {
        let mut result = self.result.lock().unwrap_or_else(|e| e.into_inner());
        while result.is_none() {
            result = self.ready.wait(result).unwrap_or_else(|e| e.into_inner());
        }
        result.clone().expect("finished flight")
    }
}
pub(super) struct Cache {
    entries: LruCache<Key, (Arc<Variant>, usize)>,
    pub flights: HashMap<Key, Arc<Flight>>,
    pub bytes: usize,
    pub budget: usize,
}
pub(super) enum Lookup {
    Cached(Arc<Variant>),
    Wait(Arc<Flight>),
    Own(Owner),
}
impl Cache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: LruCache::unbounded(),
            flights: HashMap::new(),
            bytes: 0,
            budget,
        }
    }
    pub fn lookup(cache: Arc<Mutex<Self>>, key: Key) -> Lookup {
        let mut state = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((value, _)) = state.entries.get(&key) {
            return Lookup::Cached(value.clone());
        }
        if let Some(flight) = state.flights.get(&key) {
            return Lookup::Wait(flight.clone());
        }
        let flight = Arc::new(Flight {
            result: Mutex::new(None),
            ready: Condvar::new(),
        });
        state.flights.insert(key.clone(), flight.clone());
        drop(state);
        Lookup::Own(Owner {
            cache,
            key,
            flight,
            completed: false,
        })
    }
    fn insert(&mut self, key: Key, value: Arc<Variant>) {
        let bytes = value.bytes.len()
            + value.etag.capacity()
            + std::mem::size_of::<Variant>()
            + std::mem::size_of::<Key>()
            + 128;
        if bytes > self.budget {
            return;
        }
        if let Some((_, old)) = self.entries.pop(&key) {
            self.bytes -= old;
        }
        while self.bytes + bytes > self.budget {
            if let Some((_, (_, old))) = self.entries.pop_lru() {
                self.bytes -= old;
            }
        }
        self.entries.put(key, (value, bytes));
        self.bytes += bytes;
    }
}
// The guard completes failed flights during a codec panic as well as errors.
pub(super) struct Owner {
    cache: Arc<Mutex<Cache>>,
    key: Key,
    flight: Arc<Flight>,
    completed: bool,
}
impl Owner {
    pub fn finish(mut self, result: Result) -> Result {
        self.publish(result.clone());
        self.completed = true;
        result
    }
    fn publish(&self, result: Result) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(value) = &result {
            cache.insert(self.key.clone(), value.clone());
        }
        *self.flight.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        cache.flights.remove(&self.key);
        self.flight.ready.notify_all();
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !self.completed {
            self.publish(Err(Error::Internal));
        }
    }
}
