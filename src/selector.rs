#![allow(
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::redundant_pub_crate,
    clippy::too_many_lines
)]

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use cap_std::fs::Dir;
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use same_file::Handle;

use crate::engine::SelectionOptions;
use crate::error::{Result, TextconError};

pub(crate) struct Selector<'a> {
    options: &'a SelectionOptions,
    output_identity: Option<&'a Handle>,
}

impl<'a> Selector<'a> {
    pub(crate) fn new(options: &'a SelectionOptions, output_identity: Option<&'a Handle>) -> Self {
        Self {
            options,
            output_identity,
        }
    }

    pub(crate) fn select_ambient<F>(
        &self,
        root: &Path,
        logical_root: &Path,
        policy_root: &Path,
        callback: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&Path, File) -> Result<()>,
    {
        let cli = build_cli_matcher(policy_root, &self.options.excludes)?;
        let mut ignores = Vec::new();
        self.load_ambient_ancestor_ignores(policy_root, root, &mut ignores)?;
        let mut ancestors = Vec::new();
        let root_handle = Handle::from_path(root)
            .map_err(|error| TextconError::path_io("identify directory", root, error))?;
        ancestors.push(root_handle);
        self.walk_ambient(
            root,
            logical_root,
            policy_root,
            0,
            &cli,
            &mut ignores,
            &mut ancestors,
            true,
            callback,
        )
    }

    pub(crate) fn select_sandbox<F>(
        &self,
        capability_root: &Dir,
        root_relative: &Path,
        logical_root: &Path,
        display_root: &Path,
        callback: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&Path, File) -> Result<()>,
    {
        let cli = build_cli_matcher(display_root, &self.options.excludes)?;
        let mut ignores = Vec::new();
        self.load_sandbox_ancestor_ignores(
            capability_root,
            root_relative,
            display_root,
            &mut ignores,
        )?;
        let root_dir = capability_root.open_dir(root_relative).map_err(|error| {
            TextconError::path_io(
                "open sandboxed directory",
                display_root.join(root_relative),
                error,
            )
        })?;
        let root_handle = Handle::from_file(
            root_dir
                .try_clone()
                .map_err(|error| {
                    TextconError::path_io(
                        "clone sandboxed directory",
                        display_root.join(root_relative),
                        error,
                    )
                })?
                .into_std_file(),
        )
        .map_err(|error| {
            TextconError::path_io(
                "identify sandboxed directory",
                display_root.join(root_relative),
                error,
            )
        })?;
        let mut ancestors = vec![root_handle];
        self.walk_sandbox(
            root_dir,
            root_relative,
            logical_root,
            display_root,
            0,
            &cli,
            &mut ignores,
            &mut ancestors,
            true,
            callback,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_ambient<F>(
        &self,
        physical_dir: &Path,
        logical_dir: &Path,
        policy_root: &Path,
        depth: usize,
        cli: &Gitignore,
        ignores: &mut Vec<Gitignore>,
        ancestors: &mut Vec<Handle>,
        ignore_already_loaded: bool,
        callback: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&Path, File) -> Result<()>,
    {
        let pushed = if ignore_already_loaded {
            false
        } else {
            self.load_ambient_ignore(physical_dir, ignores)?
        };
        let mut entries = fs::read_dir(physical_dir)
            .map_err(|error| TextconError::path_io("read directory", physical_dir, error))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| TextconError::path_io("read directory entry", physical_dir, error))?;
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            let name = entry.file_name();
            let physical = entry.path();
            let logical = logical_dir.join(&name);
            let metadata = fs::symlink_metadata(&physical)
                .map_err(|error| TextconError::path_io("inspect", &physical, error))?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() || self.is_hidden(&name) {
                continue;
            }
            let relative = physical.strip_prefix(policy_root).unwrap_or(&physical);
            let is_dir = file_type.is_dir();
            if Self::is_ignored(&policy_root.join(relative), is_dir, cli, ignores) {
                continue;
            }
            let child_depth = depth.saturating_add(1);
            if is_dir {
                if self
                    .options
                    .max_depth
                    .is_some_and(|maximum| child_depth >= maximum)
                {
                    continue;
                }
                let handle = Handle::from_path(&physical).map_err(|error| {
                    TextconError::path_io("identify directory", &physical, error)
                })?;
                if ancestors.contains(&handle) {
                    return Err(TextconError::Config(format!(
                        "directory cycle detected at {}",
                        physical.display()
                    )));
                }
                ancestors.push(handle);
                self.walk_ambient(
                    &physical,
                    &logical,
                    policy_root,
                    child_depth,
                    cli,
                    ignores,
                    ancestors,
                    false,
                    callback,
                )?;
                ancestors.pop();
            } else if file_type.is_file()
                && self
                    .options
                    .max_depth
                    .is_none_or(|maximum| child_depth <= maximum)
            {
                let file = File::open(&physical)
                    .map_err(|error| TextconError::path_io("open", &physical, error))?;
                if self.file_is_output(&file) {
                    continue;
                }
                callback(&logical, file)?;
            }
        }

        if pushed {
            ignores.pop();
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_sandbox<F>(
        &self,
        directory: Dir,
        relative_dir: &Path,
        logical_dir: &Path,
        display_root: &Path,
        depth: usize,
        cli: &Gitignore,
        ignores: &mut Vec<Gitignore>,
        ancestors: &mut Vec<Handle>,
        ignore_already_loaded: bool,
        callback: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&Path, File) -> Result<()>,
    {
        let pushed = if ignore_already_loaded {
            false
        } else {
            self.load_sandbox_ignore(&directory, relative_dir, display_root, ignores)?
        };
        let mut entries = directory
            .entries()
            .map_err(|error| {
                TextconError::path_io(
                    "read sandboxed directory",
                    display_root.join(relative_dir),
                    error,
                )
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| {
                TextconError::path_io(
                    "read sandboxed directory entry",
                    display_root.join(relative_dir),
                    error,
                )
            })?;
        entries.sort_by_key(cap_std::fs::DirEntry::file_name);

        for entry in entries {
            let name = entry.file_name();
            let relative = relative_dir.join(&name);
            let logical = logical_dir.join(&name);
            let file_type = entry.file_type().map_err(|error| {
                TextconError::path_io(
                    "inspect sandboxed entry",
                    display_root.join(&relative),
                    error,
                )
            })?;
            if file_type.is_symlink() || self.is_hidden(&name) {
                continue;
            }
            let is_dir = file_type.is_dir();
            if Self::is_ignored(&display_root.join(&relative), is_dir, cli, ignores) {
                continue;
            }
            let child_depth = depth.saturating_add(1);
            if is_dir {
                if self
                    .options
                    .max_depth
                    .is_some_and(|maximum| child_depth >= maximum)
                {
                    continue;
                }
                let child = entry.open_dir().map_err(|error| {
                    TextconError::path_io(
                        "open sandboxed directory",
                        display_root.join(&relative),
                        error,
                    )
                })?;
                let handle = Handle::from_file(
                    child
                        .try_clone()
                        .map_err(|error| {
                            TextconError::path_io(
                                "clone sandboxed directory",
                                display_root.join(&relative),
                                error,
                            )
                        })?
                        .into_std_file(),
                )
                .map_err(|error| {
                    TextconError::path_io(
                        "identify sandboxed directory",
                        display_root.join(&relative),
                        error,
                    )
                })?;
                if ancestors.contains(&handle) {
                    return Err(TextconError::Config(format!(
                        "directory cycle detected at {}",
                        display_root.join(&relative).display()
                    )));
                }
                ancestors.push(handle);
                self.walk_sandbox(
                    child,
                    &relative,
                    &logical,
                    display_root,
                    child_depth,
                    cli,
                    ignores,
                    ancestors,
                    false,
                    callback,
                )?;
                ancestors.pop();
            } else if file_type.is_file()
                && self
                    .options
                    .max_depth
                    .is_none_or(|maximum| child_depth <= maximum)
            {
                let file = entry.open().map_err(|error| {
                    TextconError::path_io(
                        "open sandboxed file",
                        display_root.join(&relative),
                        error,
                    )
                })?;
                let std_file = file.into_std();
                if self.file_is_output(&std_file) {
                    continue;
                }
                callback(&logical, std_file)?;
            }
        }

        if pushed {
            ignores.pop();
        }
        Ok(())
    }

    fn is_ignored(path: &Path, is_dir: bool, cli: &Gitignore, ignores: &[Gitignore]) -> bool {
        match cli.matched_path_or_any_parents(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
        for matcher in ignores.iter().rev() {
            match matcher.matched_path_or_any_parents(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }

    fn is_hidden(&self, name: &std::ffi::OsStr) -> bool {
        if self.options.hidden {
            return false;
        }
        name.as_encoded_bytes().first() == Some(&b'.')
    }

    fn file_is_output(&self, file: &File) -> bool {
        self.output_identity.is_some_and(|output| {
            file.try_clone()
                .ok()
                .and_then(|clone| Handle::from_file(clone).ok())
                .is_some_and(|candidate| candidate == *output)
        })
    }

    fn load_ambient_ancestor_ignores(
        &self,
        policy_root: &Path,
        selected_root: &Path,
        stack: &mut Vec<Gitignore>,
    ) -> Result<()> {
        if !self.options.use_gitignore {
            return Ok(());
        }
        let mut current = policy_root.to_path_buf();
        self.load_ambient_ignore(&current, stack)?;
        if let Ok(relative) = selected_root.strip_prefix(policy_root) {
            for component in relative.components() {
                if let Component::Normal(part) = component {
                    current.push(part);
                    self.load_ambient_ignore(&current, stack)?;
                }
            }
        }
        Ok(())
    }

    fn load_ambient_ignore(&self, directory: &Path, stack: &mut Vec<Gitignore>) -> Result<bool> {
        if !self.options.use_gitignore {
            return Ok(false);
        }
        let path = directory.join(".gitignore");
        match File::open(&path) {
            Ok(file) => {
                stack.push(build_ignore_file(directory, &path, file)?);
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(TextconError::path_io("open", path, error)),
        }
    }

    fn load_sandbox_ancestor_ignores(
        &self,
        root: &Dir,
        selected: &Path,
        display_root: &Path,
        stack: &mut Vec<Gitignore>,
    ) -> Result<()> {
        if !self.options.use_gitignore {
            return Ok(());
        }
        let mut relative = PathBuf::new();
        self.load_sandbox_ignore(root, &relative, display_root, stack)?;
        for component in selected.components() {
            if let Component::Normal(part) = component {
                relative.push(part);
                let directory = root.open_dir(&relative).map_err(|error| {
                    TextconError::path_io(
                        "open sandboxed directory",
                        display_root.join(&relative),
                        error,
                    )
                })?;
                self.load_sandbox_ignore(&directory, &relative, display_root, stack)?;
            }
        }
        Ok(())
    }

    fn load_sandbox_ignore(
        &self,
        directory: &Dir,
        relative: &Path,
        display_root: &Path,
        stack: &mut Vec<Gitignore>,
    ) -> Result<bool> {
        if !self.options.use_gitignore {
            return Ok(false);
        }
        match directory.open(".gitignore") {
            Ok(file) => {
                let path = display_root.join(relative).join(".gitignore");
                stack.push(build_ignore_file(
                    &display_root.join(relative),
                    &path,
                    file.into_std(),
                )?);
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(TextconError::path_io(
                "open sandboxed ignore file",
                display_root.join(relative).join(".gitignore"),
                error,
            )),
        }
    }
}

fn build_cli_matcher(root: &Path, patterns: &[String]) -> Result<Gitignore> {
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
    })
}

fn build_ignore_file<R: Read>(root: &Path, path: &Path, reader: R) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(root);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut line_number = 0_u64;
    loop {
        line.clear();
        let count = reader
            .read_line(&mut line)
            .map_err(|error| TextconError::path_io("read ignore file", path, error))?;
        if count == 0 {
            break;
        }
        line_number += 1;
        if line_number == 1 {
            line = line.trim_start_matches('\u{feff}').to_owned();
        }
        while line.ends_with(['\n', '\r']) {
            line.pop();
        }
        builder
            .add_line(Some(path.to_path_buf()), &line)
            .map_err(|error| TextconError::Ignore {
                origin: format!("{}:{line_number}", path.display()),
                message: error.to_string(),
            })?;
    }
    builder.build().map_err(|error| TextconError::Ignore {
        origin: path.display().to_string(),
        message: error.to_string(),
    })
}
