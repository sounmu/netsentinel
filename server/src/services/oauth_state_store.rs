use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

const STATE_TTL: Duration = Duration::from_secs(5 * 60);
const TOKEN_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct PendingOAuthState {
    pub code_verifier: String,
    pub nonce: String,
    created_at: Instant,
}

pub struct OAuthStateStore {
    pending: RwLock<HashMap<String, PendingOAuthState>>,
    ttl: Duration,
}

impl OAuthStateStore {
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
            ttl: STATE_TTL,
        }
    }

    pub fn issue(&self, code_verifier: String, nonce: String) -> String {
        let state = random_url_token();
        let pending = PendingOAuthState {
            code_verifier,
            nonce,
            created_at: Instant::now(),
        };
        let mut guard = self.pending.write().unwrap_or_else(|e| e.into_inner());
        guard.insert(state.clone(), pending);
        state
    }

    pub fn consume(&self, state: &str) -> Option<PendingOAuthState> {
        let mut guard = self.pending.write().unwrap_or_else(|e| e.into_inner());
        let pending = guard.remove(state)?;
        if pending.created_at.elapsed() > self.ttl {
            return None;
        }
        Some(pending)
    }

    pub fn evict_expired(&self) {
        let mut guard = self.pending.write().unwrap_or_else(|e| e.into_inner());
        let ttl = self.ttl;
        guard.retain(|_, pending| pending.created_at.elapsed() <= ttl);
    }
}

pub fn random_url_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_is_single_use() {
        let store = OAuthStateStore::new();
        let state = store.issue("verifier".to_string(), "nonce".to_string());

        assert!(store.consume(&state).is_some());
        assert!(store.consume(&state).is_none());
    }
}
