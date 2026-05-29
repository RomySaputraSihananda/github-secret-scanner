use rusqlite::{Connection, Result};
use sha2::{Digest, Sha256};

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scanned_files (
                hash TEXT PRIMARY KEY,
                scanned_at INTEGER NOT NULL
            )",
        )?;
        Ok(Self { conn })
    }

    pub fn is_seen(&self, key: &str) -> bool {
        let hash = hash_key(key);
        self.conn
            .query_row(
                "SELECT 1 FROM scanned_files WHERE hash = ?1",
                [&hash],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn mark_seen(&self, key: &str) -> Result<()> {
        let hash = hash_key(key);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.conn.execute(
            "INSERT OR IGNORE INTO scanned_files (hash, scanned_at) VALUES (?1, ?2)",
            rusqlite::params![hash, now],
        )?;
        Ok(())
    }
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_seen_returns_false_for_new_key() {
        let cache = Cache::new(":memory:").unwrap();
        assert!(!cache.is_seen("owner/repo/path/abc123"));
    }

    #[test]
    fn test_mark_seen_then_is_seen_returns_true() {
        let cache = Cache::new(":memory:").unwrap();
        let key = "owner/repo/path/abc123";
        cache.mark_seen(key).unwrap();
        assert!(cache.is_seen(key));
    }

    #[test]
    fn test_mark_seen_is_idempotent() {
        let cache = Cache::new(":memory:").unwrap();
        let key = "owner/repo/path/abc123";
        cache.mark_seen(key).unwrap();
        cache.mark_seen(key).unwrap();
        assert!(cache.is_seen(key));
    }

    #[test]
    fn test_different_keys_are_independent() {
        let cache = Cache::new(":memory:").unwrap();
        cache.mark_seen("key1").unwrap();
        assert!(cache.is_seen("key1"));
        assert!(!cache.is_seen("key2"));
    }
}
