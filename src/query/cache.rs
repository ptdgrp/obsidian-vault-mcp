use std::{
    collections::HashMap,
    fs,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use camino::Utf8Path;

use crate::parser::{NoteParser, ParsedNote};

#[derive(Debug)]
pub struct ParseCache {
    entries: RwLock<HashMap<String, CachedParsedNote>>,
    ttl: Duration,
    max_entries: usize,
}

impl ParseCache {
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    pub fn parse_note(
        &self,
        path: &Utf8Path,
        relative_path: String,
        max_note_bytes: usize,
    ) -> anyhow::Result<Arc<ParsedNote>> {
        let metadata = fs::metadata(path)?;
        let fingerprint = FileFingerprint {
            size_bytes: metadata.len(),
            modified_unix_ms: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64),
        };

        let now = Instant::now();
        if let Some(parsed) = self.get_fresh(&relative_path, fingerprint, now) {
            return Ok(parsed);
        }

        let content = fs::read_to_string(path)?;
        let parsed = Arc::new(NoteParser::parse(
            relative_path.clone(),
            &content,
            max_note_bytes,
        )?);
        self.store(relative_path, fingerprint, parsed.clone(), now);
        Ok(parsed)
    }

    pub fn invalidate(&self, relative_path: &str) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(relative_path);
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn expire_all_for_test(&self) {
        if let Ok(mut entries) = self.entries.write() {
            let expired_at = Instant::now()
                .checked_sub(self.ttl + Duration::from_secs(1))
                .unwrap_or_else(Instant::now);
            for entry in entries.values_mut() {
                entry.last_access = expired_at;
            }
        }
    }

    fn get_fresh(
        &self,
        key: &str,
        fingerprint: FileFingerprint,
        now: Instant,
    ) -> Option<Arc<ParsedNote>> {
        let mut entries = self.entries.write().ok()?;
        let entry = entries.get_mut(key)?;
        if entry.fingerprint != fingerprint || now.duration_since(entry.last_access) > self.ttl {
            return None;
        }
        entry.last_access = now;
        Some(entry.parsed.clone())
    }

    fn store(
        &self,
        key: String,
        fingerprint: FileFingerprint,
        parsed: Arc<ParsedNote>,
        now: Instant,
    ) {
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                key,
                CachedParsedNote {
                    fingerprint,
                    parsed,
                    last_access: now,
                },
            );
            prune_entries(&mut entries, self.ttl, self.max_entries, now);
        }
    }
}

impl Default for ParseCache {
    fn default() -> Self {
        Self::new(600, 1024)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileFingerprint {
    size_bytes: u64,
    modified_unix_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct CachedParsedNote {
    fingerprint: FileFingerprint,
    parsed: Arc<ParsedNote>,
    last_access: Instant,
}

fn prune_entries(
    entries: &mut HashMap<String, CachedParsedNote>,
    ttl: Duration,
    max_entries: usize,
    now: Instant,
) {
    entries.retain(|_, entry| now.duration_since(entry.last_access) <= ttl);
    if max_entries == 0 {
        entries.clear();
        return;
    }
    while entries.len() > max_entries {
        let Some(oldest_key) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        entries.remove(&oldest_key);
    }
}
