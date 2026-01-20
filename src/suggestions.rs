//! Suggestions module for "Did You Mean?" functionality
//!
//! Uses Levenshtein distance to suggest similar variable names and enum values

/// Calculate the Levenshtein distance between two strings
/// Returns the minimum number of single-character edits (insertions, deletions, substitutions)
/// needed to transform one string into the other
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    // Create a 2D vector for dynamic programming
    let mut matrix: Vec<Vec<usize>> = vec![vec![0; b_len + 1]; a_len + 1];

    // Initialize first column
    for (i, row) in matrix.iter_mut().enumerate().take(a_len + 1) {
        row[0] = i;
    }

    // Initialize first row
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    // Fill in the rest of the matrix
    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };

            matrix[i][j] = (matrix[i - 1][j] + 1) // deletion
                .min(matrix[i][j - 1] + 1) // insertion
                .min(matrix[i - 1][j - 1] + cost); // substitution
        }
    }

    matrix[a_len][b_len]
}

/// Find the closest match from a list of candidates
/// Returns the closest match and its edit distance if within max_distance
pub fn find_closest_match<'a>(
    input: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    max_distance: usize,
) -> Option<(&'a str, usize)> {
    let input_lower = input.to_lowercase();
    let mut best_match: Option<(&str, usize)> = None;

    for candidate in candidates {
        let candidate_lower = candidate.to_lowercase();
        let distance = levenshtein_distance(&input_lower, &candidate_lower);

        if distance <= max_distance {
            match best_match {
                None => best_match = Some((candidate, distance)),
                Some((_, best_distance)) if distance < best_distance => {
                    best_match = Some((candidate, distance))
                }
                _ => {}
            }
        }
    }

    best_match
}

/// Suggest a variable name from schema keys for an unknown variable
pub fn suggest_variable_name<'a>(
    unknown_key: &str,
    schema_keys: impl IntoIterator<Item = &'a String>,
) -> Option<String> {
    // Max distance of 3 for variable names (allows for common typos)
    let max_distance = 3;

    let candidates: Vec<&str> = schema_keys.into_iter().map(|s| s.as_str()).collect();
    find_closest_match(unknown_key, candidates, max_distance)
        .map(|(candidate, distance)| format!("Did you mean {}? (edit distance: {})", candidate, distance))
}

/// Suggest an enum value from allowed values
pub fn suggest_enum_value<'a>(
    invalid_value: &str,
    allowed_values: impl IntoIterator<Item = &'a String>,
) -> Option<String> {
    // Max distance of 3 for enum values
    let max_distance = 3;

    let candidates: Vec<&str> = allowed_values.into_iter().map(|s| s.as_str()).collect();

    // First check for prefix match (e.g., "dev" -> "development")
    for candidate in &candidates {
        if candidate.to_lowercase().starts_with(&invalid_value.to_lowercase()) && candidate != &invalid_value {
            return Some(format!("Did you mean \"{}\"? (prefix match)", candidate));
        }
    }

    // Then check for edit distance
    find_closest_match(invalid_value, candidates, max_distance)
        .map(|(candidate, distance)| format!("Did you mean \"{}\"? (edit distance: {})", candidate, distance))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Levenshtein distance tests
    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein_distance("test", "test"), 0);
    }

    #[test]
    fn test_levenshtein_empty_strings() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("abc", ""), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
    }

    #[test]
    fn test_levenshtein_one_char_diff() {
        assert_eq!(levenshtein_distance("cat", "bat"), 1);
        assert_eq!(levenshtein_distance("cat", "car"), 1);
        assert_eq!(levenshtein_distance("cat", "cats"), 1);
    }

    #[test]
    fn test_levenshtein_database_typo() {
        assert_eq!(levenshtein_distance("DATABSE_URL", "DATABASE_URL"), 1);
    }

    #[test]
    fn test_levenshtein_multiple_edits() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("saturday", "sunday"), 3);
    }

    // Find closest match tests
    #[test]
    fn test_find_closest_match_exact() {
        let candidates = vec!["DATABASE_URL", "PORT", "NODE_ENV"];
        let result = find_closest_match("DATABASE_URL", candidates, 3);
        assert_eq!(result, Some(("DATABASE_URL", 0)));
    }

    #[test]
    fn test_find_closest_match_typo() {
        let candidates = vec!["DATABASE_URL", "PORT", "NODE_ENV"];
        let result = find_closest_match("DATABSE_URL", candidates, 3);
        assert_eq!(result, Some(("DATABASE_URL", 1)));
    }

    #[test]
    fn test_find_closest_match_no_match() {
        let candidates = vec!["DATABASE_URL", "PORT", "NODE_ENV"];
        let result = find_closest_match("COMPLETELY_DIFFERENT", candidates, 3);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_closest_match_case_insensitive() {
        let candidates = vec!["DATABASE_URL", "PORT", "NODE_ENV"];
        let result = find_closest_match("database_url", candidates, 3);
        assert_eq!(result, Some(("DATABASE_URL", 0)));
    }

    // Suggest variable name tests
    #[test]
    fn test_suggest_variable_name_typo() {
        let schema_keys = [
            "DATABASE_URL".to_string(),
            "PORT".to_string(),
            "NODE_ENV".to_string(),
        ];
        let suggestion = suggest_variable_name("DATABSE_URL", schema_keys.iter());
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("DATABASE_URL"));
    }

    #[test]
    fn test_suggest_variable_name_no_match() {
        let schema_keys = [
            "DATABASE_URL".to_string(),
            "PORT".to_string(),
            "NODE_ENV".to_string(),
        ];
        let suggestion = suggest_variable_name("COMPLETELY_UNKNOWN", schema_keys.iter());
        assert!(suggestion.is_none());
    }

    // Suggest enum value tests
    #[test]
    fn test_suggest_enum_value_prefix() {
        let allowed = [
            "development".to_string(),
            "staging".to_string(),
            "production".to_string(),
        ];
        let suggestion = suggest_enum_value("dev", allowed.iter());
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("development"));
    }

    #[test]
    fn test_suggest_enum_value_typo() {
        let allowed = [
            "development".to_string(),
            "staging".to_string(),
            "production".to_string(),
        ];
        let suggestion = suggest_enum_value("producton", allowed.iter());
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("production"));
    }

    #[test]
    fn test_suggest_enum_value_no_match() {
        let allowed = [
            "development".to_string(),
            "staging".to_string(),
            "production".to_string(),
        ];
        let suggestion = suggest_enum_value("completely_wrong", allowed.iter());
        assert!(suggestion.is_none());
    }

    #[test]
    fn test_suggest_enum_value_case_insensitive_prefix() {
        let allowed = [
            "development".to_string(),
            "staging".to_string(),
            "production".to_string(),
        ];
        let suggestion = suggest_enum_value("DEV", allowed.iter());
        assert!(suggestion.is_some());
        assert!(suggestion.unwrap().contains("development"));
    }
}
