//! Out-of-band admin password reset.
//!
//! Reachable only from the server's own command line — never over HTTP.
//! NetSentinel is a single-admin tool, so a self-service "forgot password"
//! button on the login page would hand the instance to anyone who can reach
//! that page. Running a command on the host proves ownership of the box,
//! which is the property the reset actually needs to establish.
//!
//! The issued password is temporary: `users.must_change_password` is set, and
//! the router refuses every authenticated route except identity and password
//! change until the operator picks a real one. A temporary credential that
//! leaks (shell history, a pasted terminal log) therefore cannot be used to
//! operate the instance.

use argon2::password_hash::rand_core::{OsRng, RngCore};

use crate::db::DbPool;
use crate::repositories::users_repo;
use crate::services::{refresh_token, user_auth};

/// Character classes chosen so the generated password satisfies the same
/// policy `validate_password` enforces on user-chosen ones, and so the result
/// survives being read aloud or retyped: no `l`/`1`/`O`/`0` lookalikes.
const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const LOWER: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
const DIGIT: &[u8] = b"23456789";
const SPECIAL: &[u8] = b"!@#%^&*-_=+";
const LENGTH: usize = 20;

/// Uniform index into `set`, rejecting the biased tail of the u32 range.
fn pick(set: &[u8]) -> u8 {
    let n = set.len() as u32;
    let limit = u32::MAX - (u32::MAX % n);
    loop {
        let v = OsRng.next_u32();
        if v < limit {
            return set[(v % n) as usize];
        }
    }
}

fn generate_temp_password() -> String {
    let all: Vec<u8> = [UPPER, LOWER, DIGIT, SPECIAL].concat();

    // Seed one of each class so the policy is satisfied by construction, then
    // fill the rest, then shuffle so the classes are not in fixed positions.
    let mut bytes: Vec<u8> = vec![pick(UPPER), pick(LOWER), pick(DIGIT), pick(SPECIAL)];
    while bytes.len() < LENGTH {
        bytes.push(pick(&all));
    }
    for i in (1..bytes.len()).rev() {
        let j = (OsRng.next_u32() as usize) % (i + 1);
        bytes.swap(i, j);
    }

    String::from_utf8(bytes).expect("generated password is ASCII by construction")
}

/// Issue a temporary password for `username`, or for the sole admin when none
/// is given. Returns `(username, temporary password)`.
///
/// Every existing session is destroyed: refresh tokens are revoked and
/// `tokens_revoked_at` is stamped, so an attacker who already holds a live
/// JWT does not keep access across the reset.
pub async fn reset_admin_password(
    pool: &DbPool,
    username: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let target = match username {
        Some(name) => name.to_string(),
        None => {
            let admins = users_repo::list_admin_usernames(pool).await?;
            match admins.len() {
                0 => anyhow::bail!(
                    "No admin account exists yet.\n\n\
                     Open the dashboard and complete the initial setup first."
                ),
                1 => admins.into_iter().next().expect("length checked above"),
                _ => anyhow::bail!(
                    "More than one admin account exists — name the one to reset:\n\n    \
                     netsentinel-server reset-admin-password <username>\n\n\
                     Accounts: {}",
                    admins.join(", ")
                ),
            }
        }
    };

    let user = users_repo::find_by_username(pool, &target)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No account named '{target}'."))?;

    let temp = generate_temp_password();
    let to_hash = temp.clone();
    let hash = tokio::task::spawn_blocking(move || user_auth::hash_password(&to_hash))
        .await
        .map_err(|e| anyhow::anyhow!("Password hashing task failed: {e:#}"))?
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {e:#}"))?;

    users_repo::set_temporary_password(pool, user.id, &hash).await?;
    users_repo::revoke_user_tokens(pool, user.id).await?;
    refresh_token::revoke_all_for_user(pool, user.id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to revoke sessions: {e:#}"))?;

    Ok((user.username, temp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_ok(p: &str) -> bool {
        p.len() >= 8
            && p.len() <= 128
            && p.chars().any(|c| c.is_uppercase())
            && p.chars().any(|c| c.is_lowercase())
            && p.chars().any(|c| c.is_ascii_digit())
            && p.chars().any(|c| !c.is_alphanumeric())
    }

    #[test]
    fn generated_password_satisfies_the_login_policy() {
        for _ in 0..200 {
            let p = generate_temp_password();
            assert_eq!(p.len(), LENGTH);
            assert!(policy_ok(&p), "policy violation: {p}");
        }
    }

    #[test]
    fn generated_password_avoids_lookalike_characters() {
        for _ in 0..200 {
            let p = generate_temp_password();
            assert!(
                !p.contains(['l', '1', 'I', 'O', '0', 'o']),
                "lookalike character in: {p}"
            );
        }
    }

    #[test]
    fn generated_passwords_are_distinct() {
        let a = generate_temp_password();
        let b = generate_temp_password();
        assert_ne!(a, b);
    }
}
