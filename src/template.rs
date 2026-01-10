use crate::error::{Result, TextconError};
use crate::fs_utils::{read_file_contents, resolve_reference_path};

use regex::Regex;

use std::path::Path;

/// Configuration for template processing
#[derive(Debug, Clone)]
pub struct TemplateConfig {
    /// Base directory for resolving references (usually current working directory)
    pub base_dir: std::path::PathBuf,
    /// Maximum depth for directory recursion
    pub max_tree_depth: Option<usize>,
    /// Whether to include file contents inline
    pub inline_contents: bool,
    /// Optional overrides for file exclusion (ripgrep semantics)
    pub overrides: Option<ignore::overrides::Override>,
    /// Whether to respect .gitignore files
    pub use_gitignore: bool,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            base_dir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            max_tree_depth: Some(5),
            inline_contents: true,
            overrides: None,
            use_gitignore: true,
        }
    }
}

/// Represents a reference found in a template
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateReference {
    /// The full match including {{ and }}
    pub full_match: String,
    /// The reference content (e.g., "@file.txt")
    pub reference: String,
    /// Starting position in the template
    pub start: usize,
    /// Ending position in the template
    pub end: usize,
}

/// Finds all template references in the given text
///
/// # Errors
///
/// Returns `TextconError::Regex` if there's an error compiling the regex pattern.
pub fn find_references(template: &str) -> Result<Vec<TemplateReference>> {
    // Modified regex: no longer looks for '!' specifically
    let pattern = Regex::new(r"\{\{\s*(@[^}]+?)\s*\}\}")?;
    let mut references = Vec::new();

    for capture in pattern.captures_iter(template) {
        if let Some(full_match) = capture.get(0)
            && let Some(ref_match) = capture.get(1)
        {
            let reference_str = ref_match.as_str();

            references.push(TemplateReference {
                full_match: full_match.as_str().to_string(),
                reference: reference_str.to_string(),
                start: full_match.start(),
                end: full_match.end(),
            });
        }
    }

    Ok(references)
}

/// Processes a single reference and returns its replacement content
///
/// # Errors
///
/// - `TextconError::InvalidReference` if the reference format is invalid.
/// - `TextconError::FileNotFound` or `TextconError::DirectoryNotFound` if the referenced path doesn't exist.
/// - Other errors from file system operations or path resolution.
pub fn process_reference(reference: &str, config: &TemplateConfig) -> Result<String> {
    // Validate reference format
    if !reference.starts_with('@') {
        return Err(TextconError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    // Remove @ prefix
    let clean_ref = &reference[1..];

    // Resolve the path
    let path = resolve_reference_path(reference, &config.base_dir)?;

    // Check if it's a directory or file
    if path.is_dir() {
        process_directory(&path, config)
    } else if path.is_file() {
        read_file_contents(&path)
    } else {
        // Try to determine if user meant a directory by checking for trailing slash or special refs
        if clean_ref.ends_with('/') || clean_ref == "." || clean_ref == "/" || clean_ref.is_empty()
        {
            Err(TextconError::DirectoryNotFound { path })
        } else {
            Err(TextconError::FileNotFound { path })
        }
    }
}

/// Processes a directory reference (concatenates files)
fn process_directory(path: &Path, config: &TemplateConfig) -> Result<String> {
    let mut result = String::new();

    let walker = walkdir::WalkDir::new(path).max_depth(config.max_tree_depth.unwrap_or(usize::MAX));

    for entry in walker {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_file() {
            // Skip hidden files
            if let Some(name) = entry_path.file_name()
                && let Some(name_str) = name.to_str()
                && name_str.starts_with('.')
            {
                continue;
            }

            // Read file contents
            match read_file_contents(entry_path) {
                Ok(contents) => {
                    result.push_str(&contents);
                    result.push('\n');
                }
                Err(e) => return Err(e),
            }
        }
    }

    Ok(result)
}

/// Main function to process a template with all its references
///
/// # Errors
///
/// Returns errors from `find_references` or `process_reference` for any issues with
/// template parsing, file operations, or reference resolution.
pub fn process_template(template: &str, config: &TemplateConfig) -> Result<String> {
    let references = find_references(template)?;

    // Process from end to beginning to maintain correct positions
    let mut result = template.to_string();
    for reference in references.iter().rev() {
        let replacement = process_reference(&reference.reference, config)?;
        result.replace_range(reference.start..reference.end, &replacement);
    }

    Ok(result)
}

/// Process a template from a file
///
/// # Errors
///
/// - `TextconError::FileNotFound` if the template file doesn't exist.
/// - Other errors from `read_file_contents` or `process_template`.
pub fn process_template_file(template_path: &Path, config: &TemplateConfig) -> Result<String> {
    let template = read_file_contents(template_path)?;
    process_template(&template, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_env() -> (TempDir, TemplateConfig) {
        let temp_dir = TempDir::new().unwrap();
        let config = TemplateConfig {
            base_dir: temp_dir.path().to_path_buf(),
            max_tree_depth: Some(3),
            inline_contents: true,
            overrides: None,
            use_gitignore: false,
        };
        (temp_dir, config)
    }

    #[test]
    fn test_find_references_basic() {
        let template = "Start {{ @file.txt }} middle {{ @/dir/ }} end";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@file.txt");
        assert_eq!(refs[1].reference, "@/dir/");
    }

    #[test]
    fn test_find_references_with_spaces() {
        let template = "{{  @file.txt  }} {{   @large.txt   }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@file.txt");
        assert_eq!(refs[1].reference, "@large.txt");
    }

    #[test]
    fn test_find_references_special_paths() {
        let template = "{{ @. }} {{ @/ }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@.");
        assert_eq!(refs[1].reference, "@/");
    }

    #[test]
    fn test_find_references_empty_template() {
        let template = "";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 0);

        let template = "No references here";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_find_references_malformed() {
        // Unclosed braces
        let template = "{{ @file.txt";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 0);

        // Missing @
        let template = "{{ file.txt }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 0);

        // Extra braces
        let template = "{{{ @file.txt }}}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 1); // Should still match the inner pattern
    }

    #[test]
    fn test_file_processing() {
        let (temp_dir, _config) = create_test_env();

        // Create a file
        let small_file = temp_dir.path().join("small.txt");
        fs::write(&small_file, "small content").unwrap();

        // Should succeed
        let result = read_file_contents(&small_file);
        assert!(result.is_ok());

        // Create a large file
        let large_file = temp_dir.path().join("large.txt");
        fs::write(&large_file, "x".repeat(200)).unwrap();

        // Should resolve happily no matter the size
        let result = read_file_contents(&large_file);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_reference_invalid() {
        let (_, config) = create_test_env();

        // Missing @ prefix
        let result = process_reference("file.txt", &config);
        assert!(matches!(result, Err(TextconError::InvalidReference { .. })));
    }

    #[test]
    fn test_process_reference_nonexistent() {
        let (_temp_dir, config) = create_test_env();

        // Create a reference to nonexistent file
        // The path resolution succeeds but the file check fails
        let result = process_reference("@nonexistent.txt", &config);
        // This should fail when trying to check if it's a file or directory
        assert!(result.is_err());
    }

    #[test]
    fn test_process_template_integration() {
        let (temp_dir, config) = create_test_env();

        // Create test files
        let file1 = temp_dir.path().join("file1.txt");
        fs::write(&file1, "content1").unwrap();

        let file2 = temp_dir.path().join("file2.txt");
        fs::write(&file2, "x".repeat(200)).unwrap(); // Large file

        // Create test directory with files
        let dir = temp_dir.path().join("testdir");
        fs::create_dir(&dir).unwrap();
        let subfile = dir.join("subfile.txt");
        fs::write(&subfile, "subcontent").unwrap();

        // Test template with references
        let template =
            "Start\n{{ @file1.txt }}\nMiddle\n{{ @file2.txt }}\nDir:\n{{ @testdir/ }}\nEnd";
        let result = process_template(template, &config);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("content1"));
        assert!(output.contains("xxx")); // From large file
        assert!(output.contains("subcontent")); // From directory stitching
    }

    #[test]
    fn test_directory_inclusion() {
        let (temp_dir, config) = create_test_env();

        // Create test directory structure
        let dir = temp_dir.path().join("project");
        fs::create_dir(&dir).unwrap();

        let file1 = dir.join("file1.txt");
        fs::write(&file1, "file1 content").unwrap();

        let subdir = dir.join("subdir");
        fs::create_dir(&subdir).unwrap();

        let file2 = subdir.join("file2.txt");
        fs::write(&file2, "file2 content").unwrap();

        // Test directory reference (file stitching only, no tree)
        let result = process_reference("@project/", &config).unwrap();
        // Should contain file contents
        assert!(result.contains("file1 content"));
        assert!(result.contains("file2 content"));

        // Should likely NOT contain ASCII tree characters
        assert!(!result.contains("├──"));
    }

    #[test]
    fn test_current_directory_references() {
        let (temp_dir, config) = create_test_env();

        // Create a file in temp dir
        let file = temp_dir.path().join("test.txt");
        fs::write(&file, "test content").unwrap();

        // Test @. reference
        let result = process_reference("@.", &config);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test content"));
    }

    /*
    #[test]
    fn test_path_comments_config() {
        // Removed as functionality is removed
    }
    */

    #[test]
    fn test_template_reference_equality() {
        let ref1 = TemplateReference {
            full_match: "{{ @file.txt }}".to_string(),
            reference: "@file.txt".to_string(),
            start: 0,
            end: 15,
        };

        let ref2 = TemplateReference {
            full_match: "{{ @file.txt }}".to_string(),
            reference: "@file.txt".to_string(),
            start: 0,
            end: 15,
        };

        assert_eq!(ref1, ref2);
    }

    #[test]
    fn test_config_default() {
        let config = TemplateConfig::default();
        assert_eq!(config.max_tree_depth, Some(5));
        assert!(config.inline_contents);
        // assert!(config.add_path_comments); // Field removed
        assert!(config.use_gitignore);
    }

    #[test]
    fn test_hidden_files_ignored() {
        let (temp_dir, config) = create_test_env();

        // Create hidden file
        let hidden = temp_dir.path().join(".hidden");
        fs::write(&hidden, "hidden content").unwrap();

        // Create normal file
        let normal = temp_dir.path().join("normal.txt");
        fs::write(&normal, "normal content").unwrap();

        // Should skip hidden files
        let result = process_reference("@.", &config).unwrap();
        assert!(result.contains("normal content"));
        assert!(!result.contains("hidden content"));
    }
}
