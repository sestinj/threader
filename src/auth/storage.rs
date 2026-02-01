use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::debug;

use super::Credentials;

/// Store credentials in the encrypted file.
pub fn store(creds: &Credentials) -> Result<()> {
    let json = serde_json::to_string(creds)?;
    store_file(&json)?;
    debug!("stored credentials");
    Ok(())
}

/// Load credentials from the encrypted file.
pub fn load() -> Result<Option<Credentials>> {
    load_file()
}

/// Delete credentials.
pub fn delete() -> Result<()> {
    delete_file()
}

// --- Encrypted file backend ---

fn credentials_path() -> Result<PathBuf> {
    let proj =
        directories::ProjectDirs::from("com", "threader", "daemon")
            .context("could not determine data directory")?;
    let dir = proj.data_local_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("credentials.enc"))
}

fn derive_key() -> Result<[u8; 32]> {
    let machine_id = read_machine_id()?;
    let salt = b"threader-credential-store";
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(machine_id.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("argon2 error: {e}"))?;
    Ok(key)
}

fn read_machine_id() -> Result<String> {
    // macOS: use IOPlatformUUID via ioreg
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .context("failed to run ioreg")?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("IOPlatformUUID") {
                if let Some(uuid) = line.split('"').nth(3) {
                    return Ok(uuid.to_string());
                }
            }
        }
        anyhow::bail!("could not read IOPlatformUUID");
    }

    // Linux: /etc/machine-id
    #[cfg(target_os = "linux")]
    {
        let id = fs::read_to_string("/etc/machine-id")
            .or_else(|_| fs::read_to_string("/var/lib/dbus/machine-id"))
            .context("could not read machine-id")?;
        return Ok(id.trim().to_string());
    }

    // Windows: MachineGuid from registry
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKLM\SOFTWARE\Microsoft\Cryptography",
                "/v",
                "MachineGuid",
            ])
            .output()
            .context("failed to query MachineGuid")?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if line.contains("MachineGuid") {
                if let Some(guid) = line.split_whitespace().last() {
                    return Ok(guid.to_string());
                }
            }
        }
        anyhow::bail!("could not read MachineGuid");
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        anyhow::bail!("unsupported platform for machine-id");
    }
}

fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
    use chacha20poly1305::aead::rand_core::RngCore;
    use chacha20poly1305::XChaCha20Poly1305;

    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);

    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption error: {e}"))?;

    // Prepend nonce to ciphertext
    let mut out = Vec::with_capacity(24 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::XChaCha20Poly1305;

    if data.len() < 24 {
        anyhow::bail!("encrypted data too short");
    }

    let (nonce_bytes, ciphertext) = data.split_at(24);
    let nonce = chacha20poly1305::XNonce::from_slice(nonce_bytes);
    let cipher = XChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption error: {e}"))?;
    Ok(plaintext)
}

fn store_file(json: &str) -> Result<()> {
    let path = credentials_path()?;
    let key = derive_key()?;
    let encrypted = encrypt(json.as_bytes(), &key)?;

    fs::write(&path, &encrypted)?;

    // Set permissions to 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    debug!("stored credentials in encrypted file: {}", path.display());
    Ok(())
}

fn load_file() -> Result<Option<Credentials>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let data = fs::read(&path)?;
    let key = derive_key()?;
    let plaintext = decrypt(&data, &key)?;
    let json = String::from_utf8(plaintext)?;
    let creds: Credentials = serde_json::from_str(&json)?;
    Ok(Some(creds))
}

fn delete_file() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
