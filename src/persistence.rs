// src/persistence.rs

use sled::Db;
use std::path::Path;

/// Wrapper around the Sled database instance for structured access.
pub struct Persistence {
    pub db: Db,
}

impl Persistence {
    /// Opens the Sled database at the specified path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, sled::Error> {
        let db = sled::open(path)?;
        Ok(Persistence { db })
    }

    /// Opens a temporary in-memory Sled database for testing.
    /// This avoids filesystem access and ensures test isolation.
    #[cfg(test)]
    pub fn open_test_db() -> Result<Self, sled::Error> {
        let db = sled::Config::new().temporary(true).open()?;
        Ok(Persistence { db })
    }

    /// Stores a key-value pair in the database.
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let key_bytes = key.as_bytes();
        let value_bytes = value.as_bytes();
        self.db.insert(key_bytes, value_bytes)
            .map_err(|e| format!("Sled SET error for key '{}': {}", key, e))?;
        Ok(())
    }

    /// Retrieves a value by key.
    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        match self.db.get(key.as_bytes()) {
            Ok(Some(ivec)) => Ok(Some(String::from_utf8_lossy(&ivec).into_owned())),
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Sled GET error for key '{}': {}", key, e)),
        }
    }

    /// Executes any pending writes and closes the database.
    pub fn close(self) -> Result<(), sled::Error> {
        self.db.flush()?;
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistence_set_and_get() -> Result<(), String> {
        // Use the in-memory database wrapper
        let persistence = Persistence::open_test_db().map_err(|e| format!("{}", e))?;

        let key = "challenge_id_D01";
        let value = "0000FFFF";

        // Test SET
        persistence.set(key, value)?;

        // Test GET success
        let retrieved_value = persistence.get(key)?.expect("Value should have been set");
        assert_eq!(retrieved_value, value);

        // Test GET failure (key not found)
        let not_found_value = persistence.get("non_existent_key")?;
        assert!(not_found_value.is_none());

        Ok(())
    }

    #[test]
    fn test_persistence_overwrite() -> Result<(), String> {
        let persistence = Persistence::open_test_db().map_err(|e| format!("{}", e))?;

        let key = "last_index";
        persistence.set(key, "100")?;

        // Overwrite
        persistence.set(key, "200")?;

        let retrieved_value = persistence.get(key)?.expect("Value should be 200");
        assert_eq!(retrieved_value, "200");

        Ok(())
    }

    #[test]
    fn test_persistence_close() -> Result<(), String> {
        let persistence = Persistence::open_test_db().map_err(|e| format!("{}", e))?;
        persistence.set("test_key", "test_value")?;

        // Closing the in-memory DB doesn't panic and returns Ok
        persistence.close().map_err(|e| format!("Close failed: {}", e))?;

        Ok(())
    }
}
