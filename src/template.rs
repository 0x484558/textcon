use crate::error::{Result, TextconError};
use crate::fs_utils::{generate_directory_tree, read_file_contents, resolve_reference_path};
use regex::Regex;
use std::fmt::Write;
use std::fs;
use std::path::Path;

/// Maximum file size (64KB) before requiring force syntax
pub const MAX_FILE_SIZE: u64 = 64 * 1024;

/// Configuration for template processing
#[derive(Debug, Clone)]
pub struct TemplateConfig {
    /// Base directory for resolving references (usually current working directory)
    pub base_dir: std::path::PathBuf,
    /// Maximum depth for directory tree generation
    pub max_tree_depth: Option<usize>,
    /// Whether to include file contents inline
    pub inline_contents: bool,
    /// Whether to add file path comments
    pub add_path_comments: bool,
    /// Maximum file size before requiring force syntax
    pub max_file_size: u64,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            base_dir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            max_tree_depth: Some(5),
            inline_contents: true,
            add_path_comments: true,
            max_file_size: MAX_FILE_SIZE,
        }
    }
}

/// Represents a reference found in a template
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateReference {
    /// The full match including {{ and }}
    pub full_match: String,
    /// The reference content (e.g., "@file.txt" or "@!large.txt")
    pub reference: String,
    /// Starting position in the template
    pub start: usize,
    /// Ending position in the template
    pub end: usize,
    /// Whether this is a forced inclusion (using @! prefix)
    pub force: bool,
}

/// Finds all template references in the given text
///
/// # Errors
///
/// Returns `TextconError::Regex` if there's an error compiling the regex pattern.
pub fn find_references(template: &str) -> Result<Vec<TemplateReference>> {
    let pattern = Regex::new(r"\{\{\s*(@!?[^}]+?)\s*\}\}")?;
    let mut references = Vec::new();

    for capture in pattern.captures_iter(template) {
        if let Some(full_match) = capture.get(0)
            && let Some(ref_match) = capture.get(1)
        {
            let reference_str = ref_match.as_str();
            let force = reference_str.starts_with("@!");

            references.push(TemplateReference {
                full_match: full_match.as_str().to_string(),
                reference: reference_str.to_string(),
                start: full_match.start(),
                end: full_match.end(),
                force,
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
/// - `TextconError::FileSizeExceeded` if a file exceeds size limits without force flag.
/// - Other errors from file system operations or path resolution.
pub fn process_reference(reference: &str, config: &TemplateConfig, force: bool) -> Result<String> {
    // Validate reference format
    if !reference.starts_with('@') {
        return Err(TextconError::InvalidReference {
            reference: reference.to_string(),
        });
    }

    // Remove @! or @ prefix
    let clean_ref = if let Some(stripped) = reference.strip_prefix("@!") {
        stripped
    } else {
        &reference[1..]
    };

    // Resolve the path (use clean reference without @ or @!)
    let path = resolve_reference_path(&format!("@{clean_ref}"), &config.base_dir)?;

    // Check if it's a directory or file
    if path.is_dir() {
        if force {
            // For @!dir/, include tree AND all file contents
            process_directory_deep(&path, config)
        } else {
            // For @dir/, just include tree
            process_directory_reference(&path, config)
        }
    } else if path.is_file() {
        process_file_reference(&path, config, force)
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

/// Processes a file reference with size checking
fn process_file_reference(path: &Path, config: &TemplateConfig, force: bool) -> Result<String> {
    // Check file size
    let metadata = fs::metadata(path)?;
    let size = metadata.len();

    if !force && size > config.max_file_size {
        return Err(TextconError::FileSizeExceeded {
            path: path.to_path_buf(),
            size,
            max_size: config.max_file_size,
        });
    }

    let contents = read_file_contents(path)?;

    if config.add_path_comments {
        let path_str = path
            .strip_prefix(&config.base_dir)
            .unwrap_or(path)
            .display()
            .to_string();

        Ok(format!("<!-- File: {path_str} -->\n{contents}"))
    } else {
        Ok(contents)
    }
}

/// Processes a directory reference (tree only)
fn process_directory_reference(path: &Path, config: &TemplateConfig) -> Result<String> {
    let tree = generate_directory_tree(path, config.max_tree_depth)?;

    if config.add_path_comments {
        let path_str = path
            .strip_prefix(&config.base_dir)
            .unwrap_or(path)
            .display()
            .to_string();

        Ok(format!(
            "<!-- Directory tree: {} -->\n{}",
            if path_str.is_empty() { "." } else { &path_str },
            tree
        ))
    } else {
        Ok(tree)
    }
}

/// Processes a deep directory reference (tree + all file contents)
fn process_directory_deep(path: &Path, config: &TemplateConfig) -> Result<String> {
    let mut result = String::new();

    // First add the tree
    result.push_str(&process_directory_reference(path, config)?);
    result.push('\n');

    // Then add all file contents
    let walker = walkdir::WalkDir::new(path).max_depth(config.max_tree_depth.unwrap_or(usize::MAX));

    let base_path = path
        .strip_prefix(&config.base_dir)
        .unwrap_or(path)
        .display();

    if config.add_path_comments {
        writeln!(result, "<!-- Files in {base_path} -->\n").unwrap();
    }

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

            // Get relative path for display
            let relative_path = entry_path
                .strip_prefix(&config.base_dir)
                .unwrap_or(entry_path)
                .display();

            // Read file contents (force=true to bypass size limits for deep directory inclusion)
            match process_file_reference(entry_path, config, true) {
                Ok(contents) => {
                    let cleaned_contents = contents
                        .lines()
                        .skip_while(|line| line.starts_with("<!--"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    writeln!(
                        result,
                        "### {relative_path}\n\n```\n{cleaned_contents}\n```\n"
                    )
                    .unwrap();
                }
                Err(e) => {
                    // Log error but continue with other files
                    writeln!(result, "### {relative_path}\n\nError reading file: {e}\n").unwrap();
                }
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
        let replacement = process_reference(&reference.reference, config, reference.force)?;
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
#[allow(unused)]
const _: () = {};

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
            add_path_comments: true,
            max_file_size: 100, // Small size for testing
        };
        (temp_dir, config)
    }

    #[test]
    fn test_find_references_basic() {
        let template = "Start {{ @file.txt }} middle {{ @/dir/ }} end";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@file.txt");
        assert!(!refs[0].force);
        assert_eq!(refs[1].reference, "@/dir/");
        assert!(!refs[1].force);
    }

    #[test]
    fn test_find_references_with_force() {
        let template = "{{ @!large.txt }} and {{ @!dir/ }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@!large.txt");
        assert!(refs[0].force);
        assert_eq!(refs[1].reference, "@!dir/");
        assert!(refs[1].force);
    }

    #[test]
    fn test_find_references_mixed() {
        let template = "{{ @normal.txt }} {{ @!forced.txt }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert!(!refs[0].force);
        assert!(refs[1].force);
    }

    #[test]
    fn test_find_references_with_spaces() {
        let template = "{{  @file.txt  }} {{   @!large.txt   }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].reference, "@file.txt");
        assert!(!refs[0].force);
        assert_eq!(refs[1].reference, "@!large.txt");
        assert!(refs[1].force);
    }

    #[test]
    fn test_find_references_special_paths() {
        let template = "{{ @. }} {{ @!. }} {{ @/ }} {{ @!/ }}";
        let refs = find_references(template).unwrap();
        assert_eq!(refs.len(), 4);
        assert_eq!(refs[0].reference, "@.");
        assert!(!refs[0].force);
        assert_eq!(refs[1].reference, "@!.");
        assert!(refs[1].force);
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
    fn test_file_size_check() {
        let (temp_dir, config) = create_test_env();

        // Create a small file
        let small_file = temp_dir.path().join("small.txt");
        fs::write(&small_file, "small content").unwrap();

        // Should succeed
        let result = process_file_reference(&small_file, &config, false);
        assert!(result.is_ok());

        // Create a large file
        let large_file = temp_dir.path().join("large.txt");
        fs::write(&large_file, "x".repeat(200)).unwrap();

        // Should fail without force
        let result = process_file_reference(&large_file, &config, false);
        assert!(matches!(result, Err(TextconError::FileSizeExceeded { .. })));

        // Should succeed with force
        let result = process_file_reference(&large_file, &config, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_process_reference_invalid() {
        let (_, config) = create_test_env();

        // Missing @ prefix
        let result = process_reference("file.txt", &config, false);
        assert!(matches!(result, Err(TextconError::InvalidReference { .. })));
    }

    #[test]
    fn test_process_reference_nonexistent() {
        let (_temp_dir, config) = create_test_env();

        // Create a reference to nonexistent file
        // The path resolution succeeds but the file check fails
        let result = process_reference("@nonexistent.txt", &config, false);
        // This should fail when trying to check if it's a file or directory
        assert!(result.is_err());

        // Nonexistent directory with trailing slash
        let result = process_reference("@nonexistent/", &config, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_template_integration() {
        let (temp_dir, mut config) = create_test_env();
        config.max_file_size = 100;

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

        // Test template with mixed references
        let template =
            "Start\n{{ @file1.txt }}\nMiddle\n{{ @!file2.txt }}\nDir:\n{{ @testdir/ }}\nEnd";
        let result = process_template(template, &config);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("content1"));
        assert!(output.contains("xxx")); // From large file
        assert!(output.contains("testdir"));

        // Test template that should fail (large file without force)
        let template_fail = "{{ @file2.txt }}";
        let result = process_template(template_fail, &config);
        assert!(matches!(result, Err(TextconError::FileSizeExceeded { .. })));
    }

    #[test]
    fn test_deep_directory_inclusion() {
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

        // Test normal directory reference (tree only)
        let result = process_reference("@project/", &config, false).unwrap();
        assert!(result.contains("project"));
        assert!(result.contains("file1.txt"));
        assert!(result.contains("subdir"));
        assert!(!result.contains("file1 content")); // Should NOT include contents

        // Test forced directory reference (tree + contents)
        let result = process_reference("@!project/", &config, true).unwrap();
        assert!(result.contains("project"));
        assert!(result.contains("file1.txt"));
        assert!(result.contains("file1 content")); // Should include contents
        assert!(result.contains("file2 content")); // Should include nested file contents
    }

    #[test]
    fn test_current_directory_references() {
        let (temp_dir, config) = create_test_env();

        // Create a file in temp dir
        let file = temp_dir.path().join("test.txt");
        fs::write(&file, "test content").unwrap();

        // Test @. reference
        let result = process_reference("@.", &config, false);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("test.txt"));

        // Test @!. reference (deep)
        let result = process_reference("@!.", &config, true);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("test.txt"));
        assert!(output.contains("test content"));

        // Test @/ reference (equivalent to @.)
        let result = process_reference("@/", &config, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_comments_config() {
        let (temp_dir, mut config) = create_test_env();

        let file = temp_dir.path().join("test.txt");
        fs::write(&file, "content").unwrap();

        // With comments
        config.add_path_comments = true;
        let result = process_file_reference(&file, &config, false).unwrap();
        assert!(result.contains("<!-- File:"));
        assert!(result.contains("content"));

        // Without comments
        config.add_path_comments = false;
        let result = process_file_reference(&file, &config, false).unwrap();
        assert!(!result.contains("<!--"));
        assert_eq!(result, "content");
    }

    #[test]
    fn test_template_reference_equality() {
        let ref1 = TemplateReference {
            full_match: "{{ @file.txt }}".to_string(),
            reference: "@file.txt".to_string(),
            start: 0,
            end: 15,
            force: false,
        };

        let ref2 = TemplateReference {
            full_match: "{{ @file.txt }}".to_string(),
            reference: "@file.txt".to_string(),
            start: 0,
            end: 15,
            force: false,
        };

        let ref3 = TemplateReference {
            full_match: "{{ @!file.txt }}".to_string(),
            reference: "@!file.txt".to_string(),
            start: 0,
            end: 16,
            force: true,
        };

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }

    #[test]
    fn test_config_default() {
        let config = TemplateConfig::default();
        assert_eq!(config.max_tree_depth, Some(5));
        assert!(config.inline_contents);
        assert!(config.add_path_comments);
        assert_eq!(config.max_file_size, MAX_FILE_SIZE);
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

        // Tree should not show hidden file
        let result = process_reference("@.", &config, false).unwrap();
        assert!(result.contains("normal.txt"));
        assert!(!result.contains(".hidden"));

        // Deep inclusion should also skip hidden files
        let result = process_reference("@!.", &config, true).unwrap();
        assert!(result.contains("normal content"));
        assert!(!result.contains("hidden content"));
    }

    #[test]
    fn test_process_template_empty() {
        let (_, config) = create_test_env();

        let result = process_template("", &config).unwrap();
        assert_eq!(result, "");

        let result = process_template("No references", &config).unwrap();
        assert_eq!(result, "No references");
    }

    #[test]
    fn test_process_template_position_preservation() {
        let (temp_dir, config) = create_test_env();

        let file = temp_dir.path().join("test.txt");
        fs::write(&file, "TEST").unwrap();

        let template = "A{{ @test.txt }}B{{ @test.txt }}C";
        let result = process_template(template, &config).unwrap();

        // Check that positions are preserved
        assert!(result.starts_with('A'));
        assert!(result.contains('B'));
        assert!(result.ends_with('C'));
        assert_eq!(result.matches("TEST").count(), 2);
    }
}
