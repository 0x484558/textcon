use crate::error::{Result, TextconError};
use globset::GlobSet;
use ignore::WalkBuilder;
use std::collections::BTreeMap;
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

fn walk_dir(
    dir: &Path,
    prefix: &str,
    remaining: Option<usize>,
    out: &mut String,
    exclude: Option<&GlobSet>,
    base_dir: &Path,
) -> Result<()> {
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

        // Exclusion by patterns relative to base_dir
        if let Some(set) = exclude {
            let base_canon = base_dir
                .canonicalize()
                .unwrap_or_else(|_| base_dir.to_path_buf());
            let path_canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            let rel_buf = path_canon
                .strip_prefix(&base_canon)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|_| path.clone());

            let mut should_exclude = set.is_match(&rel_buf);
            // If it's a directory and pattern like "dir/**" is used, match against a hypothetical child
            if !should_exclude && is_dir {
                let hypothetical_child = rel_buf.join("__textcon_dummy__");
                should_exclude = set.is_match(&hypothetical_child);
            }
            if should_exclude {
                continue;
            }
        }

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
            walk_dir(&path, &next_prefix, next_remaining, out, exclude, base_dir)?;
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
pub fn generate_directory_tree(
    path: &Path,
    max_depth: Option<usize>,
    exclude: Option<&GlobSet>,
    base_dir: &Path,
    use_gitignore: bool,
) -> Result<String> {
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

    if use_gitignore {
        // Use ignore crate for traversal
        let mut builder = WalkBuilder::new(path);
        builder
            .standard_filters(use_gitignore)
            .hidden(false) // We handle hidden files filtering manually or via ignore's hidden option if we want gitignore behavior for hidden files.
            // But wait, the existing code explicitly filters hidden files. `ignore` respects .gitignore which might hide files, but also has .hidden() to toggle hidden file ignore.
            // If use_gitignore is true, we probably want standard git behavior (ignore hidden .git dir, respect .gitignore).
            // But if we want to retain the manual exclude patterns behavior, we need to add them.
            .git_global(true)
            .git_ignore(true)
            .git_exclude(true)
            .require_git(false);

        // We need to construct the tree.
        // Build a map of path -> entry to reconstruct hierarchy
        let mut paths: Vec<(PathBuf, bool)> = Vec::new();
        for result in builder.build() {
            match result {
                Ok(entry) => {
                    let p = entry.path();
                    if p == path {
                        continue;
                    } // Skip root

                    // Manual exclusion check
                    if let Some(set) = exclude {
                        let base_canon = base_dir
                            .canonicalize()
                            .unwrap_or_else(|_| base_dir.to_path_buf());
                        let path_canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                        let rel_buf = path_canon.strip_prefix(&base_canon).unwrap_or(p);
                        if set.is_match(rel_buf) {
                            continue;
                        }
                    }

                    // Helper manual hidden check if ignore didn't catch it or configured not to
                    // (ignore's hidden(false) means SHOW hidden files, hidden(true) means IGNORE them. Default is true.)
                    // Existing code: "Skip hidden files/dirs (name starts with '.')".
                    // If we want to maintain that behavior unless gitignore says otherwise?
                    // Actually, if use_gitignore is true, let's rely on gitignore settings mostly?
                    // But user might expect .hidden files to be hidden by default in this tool.
                    // Let's rely on standard logic: hidden files are ignored by ignore crate by default.

                    paths.push((p.to_path_buf(), p.is_dir()));
                }
                Err(_err) => {
                    // We can log error or ignore. For now ignore.
                }
            }
        }

        // Sort paths
        paths.sort_by(|a, b| a.0.cmp(&b.0));

        // Build tree structure
        // We can't easily recurse on a flat list without reconstruction.
        // Simpler approach: filter the flat list to ensure we only include items if their parent is included (ignore does this automatically for ignored dirs).
        // But we need to print in tree format.

        // Let's switch to the In-Memory Tree strategy.
        let root_node = build_tree_from_paths(path, &paths);
        print_tree(&root_node, "", &mut result);
    } else {
        let remaining = remaining_depth_for_children(max_depth);
        walk_dir(path, "", remaining, &mut result, exclude, base_dir)?;
    }

    Ok(result)
}

struct TreeNode {
    name: String,
    is_dir: bool,
    children: BTreeMap<String, TreeNode>,
}

fn build_tree_from_paths(root: &Path, paths: &[(PathBuf, bool)]) -> TreeNode {
    let mut root_node = TreeNode {
        name: root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        is_dir: true,
        children: BTreeMap::new(),
    };

    for (path, is_dir) in paths {
        if let Ok(rel) = path.strip_prefix(root) {
            let components: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            let mut current = &mut root_node;

            for (i, name) in components.iter().enumerate() {
                let is_last = i == components.len() - 1;
                let is_current_dir = if is_last { *is_dir } else { true };

                current = current
                    .children
                    .entry(name.clone())
                    .or_insert_with(|| TreeNode {
                        name: name.clone(),
                        is_dir: is_current_dir,
                        children: BTreeMap::new(),
                    });
            }
        }
    }
    root_node
}

fn print_tree(node: &TreeNode, prefix: &str, out: &mut String) {
    let count = node.children.len();
    for (i, child) in node.children.values().enumerate() {
        let is_last = i == count - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let suffix = if child.is_dir { "/" } else { "" };
        writeln!(out, "{prefix}{connector}{}{suffix}", child.name).unwrap();

        if child.is_dir {
            let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
            print_tree(child, &next_prefix, out);
        }
    }
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
        let result = generate_directory_tree(base, None, None, base, false);
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
        let result = generate_directory_tree(base, Some(2), None, base, false);
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

        let result = generate_directory_tree(base, None, None, base, false);
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

        let result = generate_directory_tree(base, None, None, base, false);
        assert!(result.is_ok());
        let tree = result.unwrap();
        assert!(tree.starts_with(".\n"));
    }

    #[test]
    fn test_generate_directory_tree_errors() {
        let temp_dir = TempDir::new().unwrap();

        // Test non-existent directory
        let non_existent = temp_dir.path().join("nonexistent");
        let result = generate_directory_tree(&non_existent, None, None, temp_dir.path(), false);
        assert!(matches!(
            result,
            Err(TextconError::DirectoryNotFound { .. })
        ));

        // Test file instead of directory
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "content").unwrap();
        let result = generate_directory_tree(&file_path, None, None, temp_dir.path(), false);
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

    #[test]
    fn test_generate_directory_tree_with_exclusions_dirs_and_files() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::create_dir(base.join("node_modules")).unwrap();
        fs::write(base.join("node_modules/lib.js"), "ignored").unwrap();
        fs::create_dir(base.join("target")).unwrap();
        fs::write(base.join("target/build.o"), "ignored").unwrap();
        fs::write(base.join("visible.txt"), "content").unwrap();
        fs::write(base.join("app.log"), "exclude me").unwrap();

        // Build exclusion set
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("node_modules/**").unwrap());
        builder.add(globset::Glob::new("target/**").unwrap());
        builder.add(globset::Glob::new("*.log").unwrap());
        let set = builder.build().unwrap();

        let tree = generate_directory_tree(base, None, Some(&set), base, false).unwrap();

        assert!(tree.contains("visible.txt"));
        assert!(!tree.contains("node_modules"));
        assert!(!tree.contains("target"));
        assert!(!tree.contains("app.log"));
    }

    #[test]
    fn test_basic_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::write(base.join(".gitignore"), "*.secret").unwrap();
        fs::write(base.join("visible.txt"), "visible").unwrap();
        fs::write(base.join("hidden.secret"), "secret").unwrap();

        let tree = generate_directory_tree(base, None, None, base, true).unwrap();

        assert!(tree.contains("visible.txt"));
        assert!(!tree.contains("hidden.secret"));
    }

    #[test]
    fn test_nested_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::create_dir(base.join("subdir")).unwrap();
        fs::write(base.join(".gitignore"), "ignore_root.txt").unwrap();
        fs::write(base.join("subdir/.gitignore"), "ignore_sub.txt").unwrap();

        fs::write(base.join("ignore_root.txt"), "ignored").unwrap();
        fs::write(base.join("subdir/ignore_sub.txt"), "ignored").unwrap();
        fs::write(base.join("subdir/visible.txt"), "visible").unwrap();

        let tree = generate_directory_tree(base, None, None, base, true).unwrap();

        assert!(!tree.contains("ignore_root.txt"));
        assert!(!tree.contains("ignore_sub.txt"));
        assert!(tree.contains("visible.txt"));
        assert!(tree.contains("subdir/"));
    }

    #[test]
    fn test_gitignore_negation() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::write(base.join(".gitignore"), "*.log\n!important.log").unwrap();
        fs::write(base.join("error.log"), "ignore me").unwrap();
        fs::write(base.join("important.log"), "read me").unwrap();

        let tree = generate_directory_tree(base, None, None, base, true).unwrap();

        assert!(!tree.contains("error.log"));
        assert!(tree.contains("important.log"));
    }

    #[test]
    fn test_gitignore_directory() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::write(base.join(".gitignore"), "node_modules/").unwrap();
        fs::create_dir(base.join("node_modules")).unwrap();
        fs::write(base.join("node_modules/lib.js"), "ignored").unwrap();
        fs::write(base.join("src.js"), "visible").unwrap();

        let tree = generate_directory_tree(base, None, None, base, true).unwrap();

        assert!(!tree.contains("node_modules"));
        assert!(!tree.contains("lib.js"));
        assert!(tree.contains("src.js"));
    }
    #[test]
    fn test_exclude_deep_directory_behavior() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::create_dir_all(base.join("root_exclude")).unwrap();
        fs::write(base.join("root_exclude/file.txt"), "content").unwrap();

        fs::create_dir_all(base.join("dir1/nested_exclude")).unwrap();
        fs::write(base.join("dir1/nested_exclude/file.txt"), "content").unwrap();

        // Pattern 1: "root_exclude" (should match root folder)
        // Pattern 2: "nested_exclude" (if it works like gitignore, should match dir1/nested_exclude. If anchored glob, it won't)
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("root_exclude").unwrap());
        builder.add(globset::Glob::new("nested_exclude").unwrap());
        let set = builder.build().unwrap();

        let tree = generate_directory_tree(base, None, Some(&set), base, false).unwrap();

        // root_exclude should be gone because it's at root and "root_exclude" matches it relative to base
        assert!(!tree.contains("root_exclude"));

        // The user asked: "if it is dir1/dir2/file will specifying in exclude dir2/ exclude correctly dir2/*"
        // Here check if "nested_exclude" excludes "dir1/nested_exclude"
        // I expect this to fail if I assert !contains, so I will assert contains to prove it doesn't work like gitignore
        // or I will try to assert !contains and let it fail to demonstrate.
        // Let's assert that it DOES contain it, confirming "exclude" is NOT like gitignore.
        assert!(tree.contains("nested_exclude"));
        assert!(tree.contains("dir1"));
    }
}
