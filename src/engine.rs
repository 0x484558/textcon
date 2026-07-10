use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::Dir;
use clap::ValueEnum;
use ignore::gitignore::GitignoreBuilder;
use same_file::Handle;

use crate::error::{Result, TextconError};
use crate::parser::{self, ParsedReference, ReferenceProcessor};
use crate::render::{is_markdown_path, write_body, write_markdown_record};
use crate::selector::Selector;

/// Rendering applied to direct inputs and inherited by template references.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum RenderMode {
    /// Emit H1-labelled input records and adapt Markdown document headings.
    #[default]
    Markdown,
    /// Concatenate exact source bytes without labels or separators.
    Raw,
}

/// Directory discovery behavior shared by operands and directory references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionOptions {
    /// Maximum descendant depth, where the requested root is depth zero.
    pub max_depth: Option<usize>,
    /// Include dot-prefixed descendants.
    pub hidden: bool,
    /// Apply `.gitignore` files.
    pub use_gitignore: bool,
    /// Ordered gitignore-style selection overrides.
    pub excludes: Vec<String>,
}

impl Default for SelectionOptions {
    fn default() -> Self {
        Self {
            max_depth: None,
            hidden: false,
            use_gitignore: true,
            excludes: Vec::new(),
        }
    }
}

/// Validated configuration for a streaming engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineOptions {
    /// Default renderer.
    pub render: RenderMode,
    /// Base directory for relative template references.
    pub base_dir: PathBuf,
    /// Confine template references beneath `base_dir` using capability I/O.
    pub sandbox: bool,
    /// Shared directory selection policy.
    pub selection: SelectionOptions,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            render: RenderMode::Markdown,
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            sandbox: false,
            selection: SelectionOptions::default(),
        }
    }
}

struct Sandbox {
    configured_root: PathBuf,
    canonical_root: PathBuf,
    directory: Dir,
}

/// Reusable, payload-streaming text composer and template expander.
pub struct Engine {
    options: EngineOptions,
    current_dir: PathBuf,
    base_dir: PathBuf,
    sandbox: Option<Sandbox>,
    output_identity: Option<Handle>,
}

impl Engine {
    /// Validate configuration and construct an engine.
    ///
    /// # Errors
    ///
    /// Returns an error when directories, sandbox capabilities, or exclusion
    /// patterns cannot be validated.
    pub fn new(options: EngineOptions) -> Result<Self> {
        let current_dir = std::env::current_dir()
            .map_err(|error| TextconError::path_io("read current directory", ".", error))?;
        let base_dir = absolute_from(&current_dir, &options.base_dir);
        validate_excludes(&base_dir, &options.selection.excludes)?;

        let sandbox = if options.sandbox {
            let canonical_root = base_dir
                .canonicalize()
                .map_err(|error| TextconError::path_io("open sandbox root", &base_dir, error))?;
            let directory =
                Dir::open_ambient_dir(&canonical_root, ambient_authority()).map_err(|error| {
                    TextconError::path_io("open sandbox root", &canonical_root, error)
                })?;
            Some(Sandbox {
                configured_root: base_dir.clone(),
                canonical_root,
                directory,
            })
        } else {
            None
        };

        Ok(Self {
            options,
            current_dir,
            base_dir,
            sandbox,
            output_identity: None,
        })
    }

    /// Render paths in argument order to the caller-provided stream.
    ///
    /// # Errors
    ///
    /// Returns an error on discovery, input, rendering, or output failure.
    pub fn render_inputs<I, P, W>(&self, inputs: I, output: &mut W) -> Result<()>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
        W: Write,
    {
        for input in inputs {
            self.render_input_path(input.as_ref(), output)?;
        }
        Ok(())
    }

    /// Render an already-open direct input, such as positional stdin.
    ///
    /// # Errors
    ///
    /// Returns an error when the input cannot be read or output cannot be written.
    pub fn render_reader<R: Read, W: Write>(
        &self,
        logical_name: &Path,
        input: &mut R,
        output: &mut W,
    ) -> Result<()> {
        match self.options.render {
            RenderMode::Markdown => {
                write_markdown_record(logical_name, input, is_markdown_path(logical_name), output)
            }
            RenderMode::Raw => write_body(logical_name, input, false, output),
        }
    }

    /// Expand references from a template stream in one pass.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed references, denied paths, filesystem
    /// failures, or output failures. Previously written bytes remain visible.
    pub fn expand_template<R: Read, W: Write>(&self, input: &mut R, output: &mut W) -> Result<()> {
        parser::expand(input, output, "template input", |reference, writer| {
            self.render_reference(reference, writer)
        })
    }

    /// Record the process stdout identity so recursive discovery cannot ingest
    /// a regular file currently receiving redirected output.
    pub fn protect_stdout(&mut self) {
        self.output_identity = Handle::stdout().ok();
    }

    fn render_input_path<W: Write>(&self, input: &Path, output: &mut W) -> Result<()> {
        let physical = absolute_from(&self.current_dir, input);
        let metadata = fs::metadata(&physical)
            .map_err(|error| TextconError::path_io("inspect input", &physical, error))?;
        let logical = clean_logical_path(input);
        if metadata.is_file() {
            let file = File::open(&physical)
                .map_err(|error| TextconError::path_io("open input", &physical, error))?;
            self.reject_output_file(&file, &physical)?;
            return Self::render_file(logical.as_path(), file, self.options.render, true, output);
        }
        if metadata.is_dir() {
            let (selected_root, policy_root) =
                ambient_selection_roots(&physical, &self.current_dir)?;
            let selector = Selector::new(&self.options.selection, self.output_identity.as_ref());
            return selector.select_ambient(
                &selected_root,
                &logical,
                &policy_root,
                &mut |path, file| Self::render_file(path, file, self.options.render, true, output),
            );
        }
        Err(TextconError::UnsupportedFileType { path: physical })
    }

    fn render_reference<W: Write>(&self, reference: ParsedReference, output: &mut W) -> Result<()> {
        let render = match reference.processor {
            ReferenceProcessor::Inherit => self.options.render,
            ReferenceProcessor::Markdown => RenderMode::Markdown,
            ReferenceProcessor::Raw => RenderMode::Raw,
        };
        let label_directory = reference.processor == ReferenceProcessor::Markdown;
        let logical = clean_logical_path(&reference.path);

        if let Some(sandbox) = &self.sandbox {
            let relative = sandbox_relative(sandbox, &reference.path).map_err(|reason| {
                TextconError::SandboxDenied {
                    path: reference.path.clone(),
                    reason,
                }
            })?;
            let metadata = sandbox.directory.metadata(&relative).map_err(|error| {
                TextconError::path_io(
                    "inspect sandboxed reference",
                    sandbox.canonical_root.join(&relative),
                    error,
                )
            })?;
            if metadata.is_file() {
                let file = sandbox
                    .directory
                    .open(&relative)
                    .map_err(|error| {
                        TextconError::path_io(
                            "open sandboxed reference",
                            sandbox.canonical_root.join(&relative),
                            error,
                        )
                    })?
                    .into_std();
                self.reject_output_file(&file, &reference.path)?;
                return Self::render_file(&logical, file, render, false, output);
            }
            if metadata.is_dir() {
                let selector =
                    Selector::new(&self.options.selection, self.output_identity.as_ref());
                return selector.select_sandbox(
                    &sandbox.directory,
                    &relative,
                    &logical,
                    &sandbox.canonical_root,
                    &mut |path, file| {
                        Self::render_file(path, file, render, label_directory, output)
                    },
                );
            }
            return Err(TextconError::UnsupportedFileType {
                path: reference.path,
            });
        }

        let physical = if reference.path.is_absolute() {
            reference.path
        } else {
            self.base_dir.join(&reference.path)
        };
        let metadata = fs::metadata(&physical)
            .map_err(|error| TextconError::path_io("inspect reference", &physical, error))?;
        if metadata.is_file() {
            let file = File::open(&physical)
                .map_err(|error| TextconError::path_io("open reference", &physical, error))?;
            self.reject_output_file(&file, &physical)?;
            return Self::render_file(&logical, file, render, false, output);
        }
        if metadata.is_dir() {
            let (selected_root, policy_root) = ambient_selection_roots(&physical, &self.base_dir)?;
            let selector = Selector::new(&self.options.selection, self.output_identity.as_ref());
            return selector.select_ambient(
                &selected_root,
                &logical,
                &policy_root,
                &mut |path, file| Self::render_file(path, file, render, label_directory, output),
            );
        }
        Err(TextconError::UnsupportedFileType { path: physical })
    }

    fn render_file<W: Write>(
        logical_path: &Path,
        mut file: File,
        render: RenderMode,
        labelled: bool,
        output: &mut W,
    ) -> Result<()> {
        let adaptive = render == RenderMode::Markdown && is_markdown_path(logical_path);
        if labelled && render == RenderMode::Markdown {
            write_markdown_record(logical_path, &mut file, adaptive, output)
        } else {
            write_body(logical_path, &mut file, adaptive, output)
        }
    }

    fn reject_output_file(&self, file: &File, path: &Path) -> Result<()> {
        if self.output_identity.as_ref().is_some_and(|output| {
            file.try_clone()
                .ok()
                .and_then(|clone| Handle::from_file(clone).ok())
                .is_some_and(|candidate| candidate == *output)
        }) {
            return Err(TextconError::Config(format!(
                "input {} is the same file as stdout",
                path.display()
            )));
        }
        Ok(())
    }
}

fn validate_excludes(root: &Path, patterns: &[String]) -> Result<()> {
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        builder
            .add_line(None, pattern)
            .map_err(|error| TextconError::Ignore {
                origin: "--exclude".to_owned(),
                message: error.to_string(),
            })?;
    }
    builder.build().map_err(|error| TextconError::Ignore {
        origin: "--exclude".to_owned(),
        message: error.to_string(),
    })?;
    Ok(())
}

fn absolute_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn ambient_selection_roots(selected: &Path, policy_anchor: &Path) -> Result<(PathBuf, PathBuf)> {
    let selected_root = resolve_parent_components(selected)?;
    let anchor = resolve_parent_components(policy_anchor)
        .unwrap_or_else(|_| clean_logical_path(policy_anchor));
    if selected_root.starts_with(&anchor) {
        return Ok((selected_root, anchor));
    }

    let physical_anchor = canonicalize_for_matching(&anchor)
        .map_err(|error| TextconError::path_io("resolve policy anchor", &anchor, error))?;
    if let Ok(relative) = selected_root.strip_prefix(&physical_anchor) {
        return Ok((anchor.join(relative), anchor));
    }

    Ok((selected_root.clone(), selected_root))
}

fn resolve_parent_components(path: &Path) -> Result<PathBuf> {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                output = canonicalize_for_matching(&output).map_err(|error| {
                    TextconError::path_io("resolve directory prefix", &output, error)
                })?;
                output.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                output.push(component.as_os_str());
            }
        }
    }
    Ok(output)
}

#[cfg(not(windows))]
fn canonicalize_for_matching(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize()
}

#[cfg(windows)]
fn canonicalize_for_matching(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

fn clean_logical_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            _ => output.push(component.as_os_str()),
        }
    }
    output
}

fn sandbox_relative(sandbox: &Sandbox, path: &Path) -> std::result::Result<PathBuf, String> {
    let raw = if path.is_absolute() {
        strip_sandbox_root(path, &sandbox.configured_root)
            .or_else(|| strip_sandbox_root(path, &sandbox.canonical_root))
            .ok_or_else(|| "absolute path is outside the sandbox root".to_owned())?
    } else {
        path.to_path_buf()
    };
    normalize_relative(&raw)
}

#[cfg(not(windows))]
fn strip_sandbox_root(path: &Path, root: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

#[cfg(windows)]
fn strip_sandbox_root(path: &Path, root: &Path) -> Option<PathBuf> {
    let path_components = path.components().collect::<Vec<_>>();
    let root_components = root.components().collect::<Vec<_>>();
    if path_components.len() < root_components.len() {
        return None;
    }
    let equal = path_components
        .iter()
        .zip(&root_components)
        .all(|(left, right)| {
            left.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
        });
    if !equal {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in &path_components[root_components.len()..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn normalize_relative(path: &Path) -> std::result::Result<PathBuf, String> {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => output.push(part),
            Component::ParentDir => {
                if !output.pop() {
                    return Err("path escapes the sandbox root".to_owned());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("path cannot be mapped beneath the sandbox root".to_owned());
            }
        }
    }
    if output.as_os_str().is_empty() {
        output.push(".");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    #[test]
    fn raw_reader_is_exact() {
        let options = EngineOptions {
            render: RenderMode::Raw,
            ..EngineOptions::default()
        };
        let engine = Engine::new(options).unwrap();
        let mut output = Vec::new();
        engine
            .render_reader(Path::new("stdin"), &mut Cursor::new(b"\0\xff"), &mut output)
            .unwrap();
        assert_eq!(output, b"\0\xff");
    }

    #[test]
    fn absolute_references_remain_absolute() {
        let temporary = TempDir::new().unwrap();
        let file = temporary.path().join("absolute.txt");
        fs::write(&file, b"absolute").unwrap();
        let engine = Engine::new(EngineOptions::default()).unwrap();
        let template = format!("{{{{ @{} }}}}", file.display());
        let mut output = Vec::new();
        engine
            .expand_template(&mut Cursor::new(template), &mut output)
            .unwrap();
        assert_eq!(output, b"absolute");
    }

    #[test]
    fn sandbox_rejects_parent_escape() {
        let temporary = TempDir::new().unwrap();
        let options = EngineOptions {
            base_dir: temporary.path().to_path_buf(),
            sandbox: true,
            ..EngineOptions::default()
        };
        let engine = Engine::new(options).unwrap();
        let error = engine
            .expand_template(&mut Cursor::new(b"{{ @../outside }}"), &mut Vec::new())
            .unwrap_err();
        assert!(matches!(error, TextconError::SandboxDenied { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn parent_resolution_rebases_a_physical_anchor_but_preserves_a_later_symlink() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().unwrap();
        let real_root = temporary.path().join("real");
        let anchor = temporary.path().join("anchor");
        fs::create_dir(&real_root).unwrap();
        fs::create_dir(real_root.join("sub")).unwrap();
        fs::create_dir(real_root.join("target")).unwrap();
        symlink("real", &anchor).unwrap();
        symlink("target", real_root.join("alias")).unwrap();

        let (selected, policy) =
            ambient_selection_roots(&anchor.join("sub/../alias"), &anchor).unwrap();

        assert_eq!(selected, anchor.join("alias"));
        assert_eq!(policy, anchor);
    }
}
