use std::path::{Component, Path, PathBuf};

/// Layout IDs are BLAKE3 256-bit hashes formatted as 64 lowercase hex chars.
/// Validate before using one in filesystem paths — an unvalidated ID allows
/// path traversal (`..\\..\\evil`) via `cache_dir.join(format!("temp_{}.rs", id))`.
pub fn is_valid_layout_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Rejects cache directories that contain parent-directory components or are
/// otherwise unsafe to join attacker-influenced filenames onto.
pub fn validate_cache_dir(dir: &str) -> Result<PathBuf, String> {
    let trimmed = dir.trim();
    if trimmed.is_empty() {
        return Err("cache_dir is empty".to_string());
    }
    let path = Path::new(trimmed);
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("cache_dir contains '..': {}", dir));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_layout_id_accepts_hex64() {
        let id = "a".repeat(64);
        assert!(is_valid_layout_id(&id));
    }

    #[test]
    fn invalid_layout_id_rejected() {
        assert!(!is_valid_layout_id(""));
        assert!(!is_valid_layout_id("short"));
        assert!(!is_valid_layout_id(&"g".repeat(64)));
        assert!(!is_valid_layout_id("../etc/passwd"));
        assert!(!is_valid_layout_id(&format!("{}..", "a".repeat(62))));
    }

    #[test]
    fn cache_dir_rejects_parent_dir() {
        assert!(validate_cache_dir("../evil").is_err());
        assert!(validate_cache_dir("a/../../b").is_err());
        assert!(validate_cache_dir("").is_err());
        assert!(validate_cache_dir("optimus_cache").is_ok());
    }
}
