//! Version-based cache invalidation system.
//!
//! This module provides `CachedValue<T>` which automatically tracks
//! when a cached value was computed and can determine if it's stale
//! based on a version number.

/// A cached value that tracks when it was computed relative to a version.
///
/// Use this for expensive computations that should be recomputed only
/// when the underlying data changes. The version is typically incremented
/// by `mark_modified()` on AppState.
#[derive(Debug)]
pub struct CachedValue<T> {
    /// The cached value, if computed
    value: Option<T>,
    /// The data version when this value was computed
    computed_at_version: u64,
}

impl<T> Default for CachedValue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> CachedValue<T> {
    /// Create a new empty cache
    pub const fn new() -> Self {
        Self {
            value: None,
            computed_at_version: 0,
        }
    }

    /// Get the cached value if it's still valid for the given version.
    /// Returns None if the cache is empty or stale.
    pub fn get(&self, current_version: u64) -> Option<&T> {
        if self.computed_at_version == current_version {
            self.value.as_ref()
        } else {
            None
        }
    }

    /// Get a mutable reference to the cached value if it's still valid.
    pub fn get_mut(&mut self, current_version: u64) -> Option<&mut T> {
        if self.computed_at_version == current_version {
            self.value.as_mut()
        } else {
            None
        }
    }

    /// Store a new value, recording the version at which it was computed.
    pub fn set(&mut self, value: T, version: u64) {
        self.value = Some(value);
        self.computed_at_version = version;
    }

    /// Check if the cache has a valid value for the given version.
    pub fn is_valid(&self, current_version: u64) -> bool {
        self.value.is_some() && self.computed_at_version == current_version
    }

    /// Check if the cache is stale (computed at an older version).
    pub fn is_stale(&self, current_version: u64) -> bool {
        self.computed_at_version != current_version
    }

    /// Explicitly invalidate the cache.
    pub fn invalidate(&mut self) {
        self.value = None;
    }

    /// Get the version at which this value was computed.
    pub fn version(&self) -> u64 {
        self.computed_at_version
    }

    /// Take ownership of the cached value, leaving the cache empty.
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    /// Get or compute the value, storing it in the cache.
    ///
    /// If the cache is valid for the current version, returns a reference
    /// to the cached value. Otherwise, computes a new value using the
    /// provided closure, stores it, and returns a reference.
    pub fn get_or_compute<F, E>(&mut self, version: u64, compute: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if self.is_stale(version) {
            let value = compute()?;
            self.set(value, version);
        }
        // Safe because we just set it if it was stale
        Ok(self.value.as_ref().unwrap())
    }

    /// Get or compute the value, with mutable access to the result.
    pub fn get_or_compute_mut<F, E>(&mut self, version: u64, compute: F) -> Result<&mut T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if self.is_stale(version) {
            let value = compute()?;
            self.set(value, version);
        }
        // Safe because we just set it if it was stale
        Ok(self.value.as_mut().unwrap())
    }
}

impl<T: Clone> CachedValue<T> {
    /// Get a clone of the cached value if valid.
    pub fn get_cloned(&self, current_version: u64) -> Option<T> {
        self.get(current_version).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic() {
        let mut cache: CachedValue<i32> = CachedValue::new();

        // Initially empty
        assert!(cache.get(1).is_none());
        assert!(cache.is_stale(1));

        // Set a value
        cache.set(42, 1);
        assert_eq!(cache.get(1), Some(&42));
        assert!(cache.is_valid(1));
        assert!(!cache.is_stale(1));

        // Stale at newer version
        assert!(cache.get(2).is_none());
        assert!(cache.is_stale(2));
    }

    #[test]
    fn test_cache_get_or_compute() {
        let mut cache: CachedValue<String> = CachedValue::new();
        let mut compute_count = 0;

        // First call computes
        let result = cache.get_or_compute(1, || {
            compute_count += 1;
            Ok::<_, ()>("hello".to_string())
        });
        assert_eq!(result.unwrap(), "hello");
        assert_eq!(compute_count, 1);

        // Second call at same version uses cache
        let result = cache.get_or_compute(1, || {
            compute_count += 1;
            Ok::<_, ()>("world".to_string())
        });
        assert_eq!(result.unwrap(), "hello"); // Still "hello"
        assert_eq!(compute_count, 1); // Not recomputed

        // New version triggers recompute
        let result = cache.get_or_compute(2, || {
            compute_count += 1;
            Ok::<_, ()>("world".to_string())
        });
        assert_eq!(result.unwrap(), "world");
        assert_eq!(compute_count, 2);
    }

    #[test]
    fn test_cache_invalidate() {
        let mut cache: CachedValue<i32> = CachedValue::new();
        cache.set(42, 1);
        assert!(cache.is_valid(1));

        cache.invalidate();
        assert!(cache.get(1).is_none());
    }
}
