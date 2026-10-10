use crate::errors::CliError;
use crate::remote::{cache_dir, cache_filename, CACHE_TTL_SECS};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

fn is_cache_entry(entry: &fs::DirEntry, extensions: &[&str]) -> bool {
    let path = entry.path();
    entry.file_type().is_ok_and(|kind| kind.is_file())
        && path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(crate) fn is_schema_entry(entry: &fs::DirEntry) -> bool {
    is_cache_entry(entry, &["json"])
}

fn clear_at(dir: &Path, url: Option<&str>) -> std::io::Result<usize> {
    let mut stems = std::collections::BTreeSet::new();
    if let Some(url) = url {
        stems.insert(cache_filename(url).trim_end_matches(".json").to_string());
    } else {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if is_cache_entry(&entry, &["json", "meta"]) {
                stems.insert(
                    entry
                        .path()
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut cleared = 0;
    for stem in stems {
        let mut removed = false;
        for extension in ["json", "meta"] {
            match fs::remove_file(dir.join(format!("{stem}.{extension}"))) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        cleared += usize::from(removed);
    }
    Ok(cleared)
}

/// List all cached remote schemas
#[doc(hidden)]
pub fn run_list() -> Result<(), CliError> {
    let dir = cache_dir()
        .ok_or_else(|| CliError::Input("Cache directory not available on this system".into()))?;

    if !dir.exists() {
        println!("Cache directory: {}", dir.display());
        println!("\nNo cached schemas.");
        return Ok(());
    }

    println!("Cache directory: {}", dir.display());
    println!();

    let entries: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| CliError::Input(format!("Failed to read cache directory: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(is_schema_entry)
        .collect();

    if entries.is_empty() {
        println!("No cached schemas.");
        return Ok(());
    }

    // Print header
    println!(
        "{:<20} {:>10} {:>12} {:>12}",
        "Filename", "Size", "Age", "Expires"
    );
    println!("{}", "-".repeat(56));

    let mut total_size: u64 = 0;

    for entry in &entries {
        let path = entry.path();
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let metadata = fs::metadata(&path).ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        total_size += size;

        let age_secs = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let expires_secs = CACHE_TTL_SECS.saturating_sub(age_secs);

        println!(
            "{:<20} {:>10} {:>12} {:>12}",
            truncate_str(&filename, 20),
            format_size(size),
            format_duration(age_secs),
            if expires_secs > 0 {
                format_duration(expires_secs)
            } else {
                "expired".to_string()
            }
        );
    }

    println!();
    println!(
        "Total: {} schema(s), {}",
        entries.len(),
        format_size(total_size)
    );

    Ok(())
}

/// Clear cached schemas
#[doc(hidden)]
pub fn run_clear(url: Option<&str>) -> Result<(), CliError> {
    let dir = cache_dir()
        .ok_or_else(|| CliError::Input("Cache directory not available on this system".into()))?;

    if !dir.exists() {
        println!("Cache is already empty.");
        return Ok(());
    }

    match url {
        Some(url) => {
            // Clear specific URL
            let filename = cache_filename(url);
            let count = clear_at(&dir, Some(url))
                .map_err(|e| CliError::Input(format!("Failed to clear cache: {}", e)))?;
            if count > 0 {
                println!("Cleared: {}", url);
                println!("  ({})", filename);
            } else {
                println!("No cached entry for: {}", url);
                println!("  (would be: {})", filename);
            }
        }
        None => {
            // Clear all
            let count = clear_at(&dir, None)
                .map_err(|e| CliError::Input(format!("Failed to clear cache: {}", e)))?;

            if count > 0 {
                println!(
                    "Cleared {} cache entry/entries (schemas and metadata).",
                    count
                );
            } else {
                println!("Cache is already empty.");
            }
        }
    }

    Ok(())
}

/// Print cache directory path
#[doc(hidden)]
pub fn run_path() -> Result<(), CliError> {
    let dir = cache_dir()
        .ok_or_else(|| CliError::Input("Cache directory not available on this system".into()))?;
    println!("{}", dir.display());
    Ok(())
}

/// Show cache statistics
#[doc(hidden)]
pub fn run_stats() -> Result<(), CliError> {
    let dir = cache_dir()
        .ok_or_else(|| CliError::Input("Cache directory not available on this system".into()))?;

    println!("Cache Statistics");
    println!("================");
    println!();
    println!("Directory: {}", dir.display());

    if !dir.exists() {
        println!("Status:    Not initialized (no cached schemas)");
        println!();
        println!("Schemas:   0");
        println!("Size:      0 B");
        return Ok(());
    }

    let entries: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| CliError::Input(format!("Failed to read cache directory: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(is_schema_entry)
        .collect();

    let mut total_size: u64 = 0;
    let mut expired_count: usize = 0;
    let mut oldest_age: u64 = 0;
    let mut newest_age: u64 = u64::MAX;

    for entry in &entries {
        let path = entry.path();
        let metadata = fs::metadata(&path).ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        total_size += size;

        let age_secs = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if age_secs > oldest_age {
            oldest_age = age_secs;
        }
        if age_secs < newest_age {
            newest_age = age_secs;
        }
        if age_secs > CACHE_TTL_SECS {
            expired_count += 1;
        }
    }

    println!("Status:    Active");
    println!();
    println!("Schemas:   {}", entries.len());
    println!("Size:      {}", format_size(total_size));
    println!("TTL:       {}", format_duration(CACHE_TTL_SECS));

    if !entries.is_empty() {
        println!();
        println!("Age range:");
        if newest_age < u64::MAX {
            println!("  Newest:  {}", format_duration(newest_age));
        }
        println!("  Oldest:  {}", format_duration(oldest_age));

        if expired_count > 0 {
            println!();
            println!("Expired:   {} (will refresh on next use)", expired_count);
        }
    }

    Ok(())
}

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Format seconds as human-readable duration
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Truncate string with ellipsis if too long. Char-boundary-safe: cuts on a
/// UTF-8 boundary so a non-ASCII cache filename never panics a byte slice.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        let end = s
            .char_indices()
            .nth(max_len - 3)
            .map_or(s.len(), |(i, _)| i);
        format!("{}...", &s[..end])
    } else {
        let end = s.char_indices().nth(max_len).map_or(s.len(), |(i, _)| i);
        s[..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_removes_schema_and_rate_limit_sidecars_without_touching_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.com/schema.json";
        let schema = dir.path().join(cache_filename(url));
        let meta = schema.with_extension("meta");
        fs::write(&schema, "{}").unwrap();
        fs::write(&meta, "rate limit metadata").unwrap();
        assert_eq!(clear_at(dir.path(), Some(url)).unwrap(), 1);
        assert!(!schema.exists() && !meta.exists());
        fs::write(&meta, "orphan metadata").unwrap();
        fs::write(dir.path().join("notes.json"), "preserve").unwrap();
        fs::write(dir.path().join(".tmp-write"), "in flight").unwrap();
        fs::create_dir(dir.path().join("1111111111111111.json")).unwrap();
        assert_eq!(clear_at(dir.path(), None).unwrap(), 1);
        assert!(!meta.exists());
        assert!(dir.path().join("notes.json").exists());
        assert!(dir.path().join(".tmp-write").exists());
        assert!(dir.path().join("1111111111111111.json").is_dir());
        assert_eq!(clear_at(dir.path(), None).unwrap(), 0);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2560), "2.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(3660), "1h 1m");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("verylongstring", 10), "verylon...");
        assert_eq!(truncate_str("ab", 2), "ab");
    }

    #[test]
    fn test_cache_path_returns_path() {
        // This test just verifies run_path doesn't panic
        // Actual path depends on system
        let result = run_path();
        assert!(result.is_ok());
    }

    #[test]
    fn test_clear_nonexistent_url() {
        // Clearing a URL that was never cached should not error
        let result = run_clear(Some("https://nonexistent.example.com/schema.json"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_size_zero() {
        assert_eq!(format_size(0), "0 B");
    }

    #[test]
    fn test_format_size_boundary_kb() {
        // Just under 1 KB
        assert_eq!(format_size(1023), "1023 B");
        // Exactly 1 KB
        assert_eq!(format_size(1024), "1.0 KB");
    }

    #[test]
    fn test_format_size_boundary_mb() {
        // Just under 1 MB
        let just_under_mb = 1024 * 1024 - 1;
        let result = format_size(just_under_mb);
        assert!(result.contains("KB"));
        // Exactly 1 MB
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_boundary_minute() {
        // Just under 1 minute
        assert_eq!(format_duration(59), "59s");
        // Exactly 1 minute
        assert_eq!(format_duration(60), "1m");
    }

    #[test]
    fn test_format_duration_boundary_hour() {
        // Just under 1 hour
        assert_eq!(format_duration(3599), "59m");
        // Exactly 1 hour
        assert_eq!(format_duration(3600), "1h 0m");
    }

    #[test]
    fn test_format_duration_large_value() {
        // 25 hours and 30 minutes
        let secs = 25 * 3600 + 30 * 60;
        assert_eq!(format_duration(secs), "25h 30m");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 10), "");
    }

    #[test]
    fn test_truncate_str_exact_limit() {
        assert_eq!(truncate_str("0123456789", 10), "0123456789");
    }

    #[test]
    fn test_truncate_str_small_max_len() {
        // When max_len <= 3, no room for ellipsis
        assert_eq!(truncate_str("abcdef", 3), "abc");
        assert_eq!(truncate_str("abcdef", 2), "ab");
        assert_eq!(truncate_str("abcdef", 1), "a");
    }

    #[test]
    fn test_truncate_str_with_ellipsis() {
        // When max_len > 3, uses ellipsis
        assert_eq!(truncate_str("abcdefghij", 7), "abcd...");
    }

    #[test]
    fn test_run_list_returns_ok() {
        // run_list should return Ok even if cache is empty
        let result = run_list();
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_clear_all_when_empty() {
        // Clearing all when nothing cached should return Ok
        let result = run_clear(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_stats_returns_ok() {
        // run_stats should return Ok even if cache is empty
        let result = run_stats();
        assert!(result.is_ok());
    }
}
