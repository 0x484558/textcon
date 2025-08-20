use crate::error::{Result, TextconError};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

/// Reads the contents of a file at the given path
///
/// # Errors
///
/// - `TextconError::FileNotFound` if the path doesn't exist or isn't a file.
/// - `TextconError::Io` if there's an error reading the file.
pub fn read_file_contents(path: &Path) -> Result<String> {
    if !path.exists() {
        return Err(TextconError::FileNotFound {
            path: path.to_path_buf(),
        });
    }

    if !path.is_file() {
        return Err(TextconError::FileNotFound {
            path: path.to_path_buf(),
        });
    }

    fs::read_to_string(path).map_err(std::convert::Into::into)
}

fn remaining_depth_for_children(max_depth: Option<usize>) -> Option<usize> {
    max_depth.map(|d| d.saturating_sub(1))
}

fn walk_dir(dir: &Path, prefix: &str, remaining: Option<usize>, out: &mut String) -> Result<()> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .map_err(TextconError::Io)?
        .filter_map(std::result::Result::ok)
        .collect();

    // Sort by name for stable output
    entries.sort_by_key(std::fs::DirEntry::file_name);

    // Skip hidden files/dirs (name starts with '.')
    entries.retain(|e| e.file_name().to_str().is_some_and(|n| !n.starts_with('.')));

    let last_index = entries.len().saturating_sub(1);

    for (idx, entry) in entries.into_iter().enumerate() {
        let is_last = idx == last_index;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let is_dir = path.is_dir();

        let connector = if is_last { "└── " } else { "├── " };
        let suffix = if is_dir { "/" } else { "" };
        writeln!(out, "{prefix}{connector}{name}{suffix}").unwrap();

        if is_dir {
            // Depth control: remaining is the number of additional directory levels to traverse
            if let Some(rem) = remaining
                && rem == 0
            {
                continue;
            }

            let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            let next_remaining = remaining.map(|r| r.saturating_sub(1));
            walk_dir(&path, &next_prefix, next_remaining, out)?;
        }
    }

    Ok(())
}

/// Generates a tree-like representation of a directory structure
///
/// # Errors
///
/// - `TextconError::DirectoryNotFound` if the path doesn't exist or isn't a directory.
/// - `TextconError::WalkDir` if there's an error traversing the directory.
pub fn generate_directory_tree(path: &Path, max_depth: Option<usize>) -> Result<String> {
    if !path.exists() {
        return Err(TextconError::DirectoryNotFound {
            path: path.to_path_buf(),
        });
    }

    if !path.is_dir() {
        return Err(TextconError::DirectoryNotFound {
            path: path.to_path_buf(),
        });
    }

    let mut result = String::new();

    // Always print relative root
    writeln!(result, ".").unwrap();

    let remaining = remaining_depth_for_children(max_depth);
    walk_dir(path, "", remaining, &mut result)?;

    Ok(result)
}

/// Resolves a reference path relative to the current working directory
/// Ensures the path doesn't escape the working directory for security
///
/// # Errors
///
/// - `TextconError::PathTraversal` if the resolved path escapes the base directory.
/// - `TextconError::Io` if there's an error canonicalizing paths.
pub fn resolve_reference_path(reference: &str, base_dir: &Path) -> Result<PathBuf> {
    // Remove @ prefix and any leading slashes
    let cleaned = reference
        .trim_start_matches('@')
        .trim_start_matches('!')
        .trim_start_matches('/')
        .trim_start_matches('\\');

    // Handle special cases for current directory
    let path_str = if cleaned.is_empty() || cleaned == "." {
        "."
    } else {
        cleaned
    };

    // Create the full path relative to base directory
    let full_path = base_dir.join(path_str);

    // Canonicalize to resolve .. and . components
    let canonical = full_path.canonicalize().or_else(|_| {
        // If file doesn't exist yet, canonicalize the parent and append the filename
        if let Some(parent) = full_path.parent() {
            parent
                .canonicalize()
                .map(|p| p.join(full_path.file_name().unwrap_or_default()))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Invalid path",
            ))
        }
    })?;

    // ensure the resolved path is within the base directory
    let base_canonical = base_dir.canonicalize()?;
    if !canonical.starts_with(&base_canonical) {
        return Err(TextconError::PathTraversal { path: canonical });
    }

    Ok(canonical)
}

#[cfg(test)]
#[allow(unused)]
const _: () = {};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_read_file_contents() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Test reading existing file
        fs::write(&file_path, "test content").unwrap();
        let result = read_file_contents(&file_path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test content");

        // Test reading non-existent file
        let non_existent = temp_dir.path().join("nonexistent.txt");
        let result = read_file_contents(&non_existent);
        assert!(matches!(result, Err(TextconError::FileNotFound { .. })));

        // Test reading directory as file
        let dir_path = temp_dir.path().join("dir");
        fs::create_dir(&dir_path).unwrap();
        let result = read_file_contents(&dir_path);
        assert!(matches!(result, Err(TextconError::FileNotFound { .. })));
    }

    #[test]
    fn test_read_file_contents_empty() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");

        fs::write(&file_path, "").unwrap();
        let result = read_file_contents(&file_path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_read_file_contents_unicode() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("unicode.txt");

        let content = "Hello 世界 🌍 Здравствуй";
        fs::write(&file_path, content).unwrap();
        let result = read_file_contents(&file_path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), content);
    }

    #[test]
    fn test_generate_directory_tree() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create directory structure
        fs::create_dir(base.join("dir1")).unwrap();
        fs::create_dir(base.join("dir2")).unwrap();
        fs::write(base.join("file1.txt"), "content").unwrap();
        fs::write(base.join("dir1/file2.txt"), "content").unwrap();
        fs::create_dir(base.join("dir1/subdir")).unwrap();
        fs::write(base.join("dir1/subdir/file3.txt"), "content").unwrap();

        // Test tree generation
        let result = generate_directory_tree(base, None);
        assert!(result.is_ok());
        let tree = result.unwrap();

        // Check that expected items are present
        assert!(tree.contains("dir1/"));
        assert!(tree.contains("dir2/"));
        assert!(tree.contains("file1.txt"));
        assert!(tree.contains("file2.txt"));
        assert!(tree.contains("subdir/"));
        assert!(tree.contains("file3.txt"));
    }

    #[test]
    fn test_generate_directory_tree_max_depth() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create deep directory structure
        fs::create_dir(base.join("level1")).unwrap();
        fs::create_dir(base.join("level1/level2")).unwrap();
        fs::create_dir(base.join("level1/level2/level3")).unwrap();
        fs::write(base.join("level1/level2/level3/deep.txt"), "content").unwrap();

        // Test with max_depth = 2
        let result = generate_directory_tree(base, Some(2));
        assert!(result.is_ok());
        let tree = result.unwrap();

        assert!(tree.contains("level1/"));
        assert!(tree.contains("level2/"));
        assert!(!tree.contains("level3/")); // Should not be included due to depth limit
        assert!(!tree.contains("deep.txt"));
    }

    #[test]
    fn test_generate_directory_tree_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create files including hidden ones
        fs::write(base.join("visible.txt"), "content").unwrap();
        fs::write(base.join(".hidden"), "content").unwrap();
        fs::create_dir(base.join(".hidden_dir")).unwrap();

        let result = generate_directory_tree(base, None);
        assert!(result.is_ok());
        let tree = result.unwrap();

        assert!(tree.contains("visible.txt"));
        assert!(!tree.contains(".hidden"));
        assert!(!tree.contains(".hidden_dir"));
    }

    #[test]
    fn test_generate_directory_tree_empty() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let result = generate_directory_tree(base, None);
        assert!(result.is_ok());
        let tree = result.unwrap();
        assert!(tree.starts_with(".\n"));
    }

    #[test]
    fn test_generate_directory_tree_errors() {
        let temp_dir = TempDir::new().unwrap();

        // Test non-existent directory
        let non_existent = temp_dir.path().join("nonexistent");
        let result = generate_directory_tree(&non_existent, None);
        assert!(matches!(
            result,
            Err(TextconError::DirectoryNotFound { .. })
        ));

        // Test file instead of directory
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();
        let result = generate_directory_tree(&file_path, None);
        assert!(matches!(
            result,
            Err(TextconError::DirectoryNotFound { .. })
        ));
    }

    #[test]
    fn test_resolve_reference_path_basic() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create test file
        let file_path = base.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        // Test basic reference
        let result = resolve_reference_path("@test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());

        // Test with leading slash
        let result = resolve_reference_path("@/test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_reference_path_force_syntax() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let file_path = base.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        // Test @! reference
        let result = resolve_reference_path("@!test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());

        // Test @!/ reference
        let result = resolve_reference_path("@!/test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_reference_path_current_dir() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Test @.
        let result = resolve_reference_path("@.", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), base.canonicalize().unwrap());

        // Test @/
        let result = resolve_reference_path("@/", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), base.canonicalize().unwrap());

        // Test @ (empty after prefix)
        let result = resolve_reference_path("@", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), base.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_reference_path_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create nested structure
        fs::create_dir(base.join("dir1")).unwrap();
        fs::create_dir(base.join("dir1/dir2")).unwrap();
        let file_path = base.join("dir1/dir2/file.txt");
        fs::write(&file_path, "content").unwrap();

        let result = resolve_reference_path("@dir1/dir2/file.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_reference_path_traversal_prevention() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create a nested directory structure to ensure we have something to escape from
        let nested_dir = base.join("subdir").join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        // Test from the nested directory - try to escape to parent of base
        let result = resolve_reference_path("@../../../", &nested_dir);
        assert!(matches!(result, Err(TextconError::PathTraversal { .. })));

        // Test from base directory - try to escape to parent
        let result = resolve_reference_path("@../", base);
        assert!(matches!(result, Err(TextconError::PathTraversal { .. })));

        // Test deeply nested escape attempt from base
        let result = resolve_reference_path(
            "@../../../../../../../../../../../../../../../../../../../../../../../../../../../../",
            base,
        );
        assert!(matches!(result, Err(TextconError::PathTraversal { .. })));
    }

    #[test]
    fn test_resolve_reference_path_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Non-existent file (should still resolve path)
        let result = resolve_reference_path("@nonexistent.txt", base);
        assert!(result.is_ok());
        let resolved = result.unwrap();

        // Compare with the expected canonicalized path
        let expected = base.canonicalize().unwrap().join("nonexistent.txt");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn test_resolve_reference_path_windows_paths() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let file_path = base.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        // Test with Windows-style path separator
        let result = resolve_reference_path("@\\test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_resolve_reference_path_complex_prefixes() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let file_path = base.join("test.txt");
        fs::write(&file_path, "content").unwrap();

        // Multiple slashes
        let result = resolve_reference_path("@///test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());

        // Mixed separators
        let result = resolve_reference_path("@!/\\test.txt", base);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file_path.canonicalize().unwrap());
    }

    #[test]
    fn test_special_characters_in_names() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create files with special characters
        let special_file = base.join("file with spaces.txt");
        fs::write(&special_file, "content").unwrap();

        let result = resolve_reference_path("@file with spaces.txt", base);
        assert!(result.is_ok());

        // Create file with unicode
        let unicode_file = base.join("文件.txt");
        fs::write(&unicode_file, "content").unwrap();

        let result = resolve_reference_path("@文件.txt", base);
        assert!(result.is_ok());
    }
}
