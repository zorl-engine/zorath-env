use crate::remote::{cache_dir, cache_filename, CACHE_TTL_SECS};
use std::fs;
use std::time::SystemTime;

/// List all cached remote schemas
pub fn run_list() -> Result<(), String> {
    let dir = cache_dir().ok_or("Cache directory not available on this system")?;

    if !dir.exists() {
        println!("Cache directory: {}", dir.display());
        println!("\nNo cached schemas.");
        return Ok(());
    }

    println!("Cache directory: {}", dir.display());
    println!();

    let entries: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read cache directory: {}", e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
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
pub fn run_clear(url: Option<&str>) -> Result<(), String> {
    let dir = cache_dir().ok_or("Cache directory not available on this system")?;

    if !dir.exists() {
        println!("Cache is already empty.");
        return Ok(());
    }

    match url {
        Some(url) => {
            // Clear specific URL
            let filename = cache_filename(url);
            let cache_path = dir.join(&filename);

            if cache_path.exists() {
                fs::remove_file(&cache_path)
                    .map_err(|e| format!("Failed to remove cached schema: {}", e))?;
                println!("Cleared: {}", url);
                println!("  ({})", filename);
            } else {
                println!("No cached entry for: {}", url);
                println!("  (would be: {})", filename);
            }
        }
        None => {
            // Clear all
            let mut count = 0;
            for entry in fs::read_dir(&dir)
                .map_err(|e| format!("Failed to read cache directory: {}", e))?
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.extension().map(|ext| ext == "json").unwrap_or(false)
                    && fs::remove_file(&path).is_ok()
                {
                    count += 1;
                }
            }

            if count > 0 {
                println!("Cleared {} cached schema(s).", count);
            } else {
                println!("Cache is already empty.");
            }
        }
    }

    Ok(())
}

/// Print cache directory path
pub fn run_path() -> Result<(), String> {
    let dir = cache_dir().ok_or("Cache directory not available on this system")?;
    println!("{}", dir.display());
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

/// Truncate string with ellipsis if too long
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 3 {
        format!("{}...", &s[..max_len - 3])
    } else {
        s[..max_len].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
