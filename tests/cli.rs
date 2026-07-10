use std::fs;
#[cfg(unix)]
use std::io::Read as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn textcon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_textcon"))
}

#[test]
fn no_input_is_usage_error() {
    let output = textcon().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn direct_markdown_is_labelled_and_adaptive() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("guide.md"), "# One\n## Two\n").unwrap();
    fs::write(temporary.path().join("code.rs"), "# not Markdown\n").unwrap();

    let output = textcon()
        .current_dir(temporary.path())
        .args(["guide.md", "code.rs"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"# `guide.md`\n\n## One\n### Two\n\n# `code.rs`\n\n# not Markdown\n\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn direct_raw_is_byte_exact() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("a"), b"a\0").unwrap();
    fs::write(temporary.path().join("b"), b"\xffb").unwrap();
    let output = textcon()
        .current_dir(temporary.path())
        .args(["--render", "raw", "a", "b"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a\0\xffb");
}

#[test]
fn template_processor_matrix_is_observable() {
    let temporary = TempDir::new().unwrap();
    fs::create_dir(temporary.path().join("docs")).unwrap();
    fs::write(temporary.path().join("docs/a.md"), "# A\n").unwrap();
    fs::write(temporary.path().join("docs/b.txt"), "B").unwrap();
    fs::write(
        temporary.path().join("template"),
        "bare={{ @docs/a.md }}\nlabelled:\n{{ @docs | markdown }}raw={{ @docs/a.md | raw }}",
    )
    .unwrap();

    let output = textcon()
        .current_dir(temporary.path())
        .args(["--template", "template"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"bare=## A\n\nlabelled:\n# `docs/a.md`\n\n## A\n\n# `docs/b.txt`\n\nB\n\nraw=# A\n"
    );
}

#[test]
fn directory_selection_honors_gitignore_and_ordered_overrides() {
    let temporary = TempDir::new().unwrap();
    fs::create_dir(temporary.path().join("src")).unwrap();
    fs::write(temporary.path().join(".gitignore"), "src/ignored.txt\n").unwrap();
    fs::write(temporary.path().join("src/z.txt"), "Z").unwrap();
    fs::write(temporary.path().join("src/a.txt"), "A").unwrap();
    fs::write(temporary.path().join("src/ignored.txt"), "I").unwrap();

    let output = textcon()
        .current_dir(temporary.path())
        .args(["--render", "raw", "src"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"AZ");

    let output = textcon()
        .current_dir(temporary.path())
        .args([
            "--render",
            "raw",
            "--exclude",
            "src/*.txt",
            "--exclude",
            "!src/ignored.txt",
            "src",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"I");
}

#[test]
fn parent_directory_spelling_uses_the_selected_gitignore_hierarchy() {
    let temporary = TempDir::new().unwrap();
    let base = temporary.path().join("base");
    let outside = temporary.path().join("outside");
    fs::create_dir(&base).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join(".gitignore"), "secret\n").unwrap();
    fs::write(outside.join("keep"), "KEEP").unwrap();
    fs::write(outside.join("secret"), "SECRET").unwrap();

    let direct = textcon()
        .current_dir(&base)
        .args(["--render", "raw", "../outside"])
        .output()
        .unwrap();
    assert!(
        direct.status.success(),
        "{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(direct.stdout, b"KEEP");

    fs::write(base.join("template"), "{{ @../outside }}").unwrap();
    let template = textcon()
        .current_dir(&base)
        .args(["--template", "template", "--render", "raw"])
        .output()
        .unwrap();
    assert!(
        template.status.success(),
        "{}",
        String::from_utf8_lossy(&template.stderr)
    );
    assert_eq!(template.stdout, b"KEEP");
}

#[cfg(unix)]
#[test]
fn explicitly_selected_directory_symlink_keeps_its_filter_namespace() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().unwrap();
    fs::create_dir(temporary.path().join("real")).unwrap();
    fs::write(temporary.path().join("real/keep"), "KEEP").unwrap();
    fs::write(temporary.path().join("real/secret"), "SECRET").unwrap();
    fs::create_dir(temporary.path().join("sub")).unwrap();
    symlink("real", temporary.path().join("alias")).unwrap();
    fs::write(temporary.path().join(".gitignore"), "alias/secret\n").unwrap();

    let ignored = textcon()
        .current_dir(temporary.path())
        .args(["--render", "raw", "alias"])
        .output()
        .unwrap();
    assert!(ignored.status.success());
    assert_eq!(ignored.stdout, b"KEEP");

    let excluded = textcon()
        .current_dir(temporary.path())
        .args([
            "--render",
            "raw",
            "--no-gitignore",
            "--exclude",
            "alias/secret",
            "alias",
        ])
        .output()
        .unwrap();
    assert!(excluded.status.success());
    assert_eq!(excluded.stdout, b"KEEP");

    let parent_before_alias = textcon()
        .current_dir(temporary.path())
        .args(["--render", "raw", "sub/../alias"])
        .output()
        .unwrap();
    assert!(parent_before_alias.status.success());
    assert_eq!(parent_before_alias.stdout, b"KEEP");

    fs::write(temporary.path().join("template"), "{{ @sub/../alias }}").unwrap();
    let template = textcon()
        .current_dir(temporary.path())
        .args(["--template", "template", "--render", "raw"])
        .output()
        .unwrap();
    assert!(template.status.success());
    assert_eq!(template.stdout, b"KEEP");
}

#[cfg(unix)]
#[test]
fn aliased_base_retains_gitignore_after_parent_component_resolution() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().unwrap();
    let real_root = temporary.path().join("real");
    let anchor = temporary.path().join("anchor");
    fs::create_dir(&real_root).unwrap();
    fs::create_dir(real_root.join("sub")).unwrap();
    fs::create_dir(real_root.join("content")).unwrap();
    symlink("real", &anchor).unwrap();
    symlink("content", real_root.join("alias")).unwrap();
    fs::write(real_root.join(".gitignore"), "ignored\n").unwrap();
    fs::write(real_root.join("content/ignored"), "IGNORED").unwrap();
    fs::write(real_root.join("content/keep"), "KEEP").unwrap();
    fs::write(temporary.path().join("template"), "{{ @sub/../alias }}").unwrap();

    let output = textcon()
        .current_dir(temporary.path())
        .args([
            "--base-dir",
            "anchor",
            "--template",
            "template",
            "--render",
            "raw",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"KEEP",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn reinclude_rules_must_reexclude_unwanted_siblings() {
    let temporary = TempDir::new().unwrap();
    fs::create_dir(temporary.path().join("target")).unwrap();
    fs::write(temporary.path().join("target/drop.txt"), "DROP").unwrap();
    fs::write(temporary.path().join("target/keep.txt"), "KEEP").unwrap();
    let output = textcon()
        .current_dir(temporary.path())
        .args([
            "--render",
            "raw",
            "--exclude",
            "target/",
            "--exclude",
            "!target/",
            "--exclude",
            "target/*",
            "--exclude",
            "!target/keep.txt",
            ".",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"KEEP");
}

#[test]
fn reference_syntax_trims_whitespace_after_at_sign() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("file"), "VALUE").unwrap();
    fs::write(
        temporary.path().join("template"),
        "{{ @  file  }}|{{ @\tfile\t | raw }}",
    )
    .unwrap();
    let output = textcon()
        .current_dir(temporary.path())
        .args(["--template", "template"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"VALUE|VALUE");
}

#[test]
fn hidden_and_depth_policies_are_shared() {
    let temporary = TempDir::new().unwrap();
    fs::create_dir_all(temporary.path().join("root/nested/deep")).unwrap();
    fs::create_dir(temporary.path().join("root/.hidden")).unwrap();
    fs::write(temporary.path().join("root/top"), "T").unwrap();
    fs::write(temporary.path().join("root/nested/one"), "N").unwrap();
    fs::write(temporary.path().join("root/nested/deep/two"), "D").unwrap();
    fs::write(temporary.path().join("root/.hidden/secret"), "H").unwrap();

    let output = textcon()
        .current_dir(temporary.path())
        .args(["root", "--render", "raw", "--max-depth", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"T");

    let output = textcon()
        .current_dir(temporary.path())
        .args(["root", "--render", "raw", "--hidden"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.contains(&b'H'));
}

#[test]
fn filename_cannot_inject_template_references() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("secret"), "SECRET").unwrap();
    let hostile = "x }} {{ @secret }}";
    fs::write(temporary.path().join(hostile), "SAFE").unwrap();
    let output = textcon()
        .current_dir(temporary.path())
        .arg(hostile)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.ends_with(b"SAFE\n\n"));
    assert_eq!(
        output
            .stdout
            .windows(6)
            .filter(|window| *window == b"SECRET")
            .count(),
        0
    );
}

#[test]
fn late_template_error_preserves_prefix_and_fails() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("template"), "prefix{{ @missing }}").unwrap();
    let output = textcon()
        .current_dir(temporary.path())
        .args(["--template", "template"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"prefix");
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("textcon: "));
}

#[test]
fn recursive_discovery_skips_redirected_stdout() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("input.txt"), "INPUT").unwrap();
    let output_path = temporary.path().join("CODE.md");
    let output_file = fs::File::create(&output_path).unwrap();
    let status = textcon()
        .current_dir(temporary.path())
        .arg(".")
        .stdout(Stdio::from(output_file))
        .status()
        .unwrap();
    assert!(status.success());
    let contents = fs::read(&output_path).unwrap();
    assert!(contents.windows(5).any(|window| window == b"INPUT"));
    assert!(!contents.windows(7).any(|window| window == b"CODE.md"));
}

#[cfg(unix)]
#[test]
fn sandbox_blocks_escaping_symlink() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(temporary.path().join("outside"), "OUTSIDE").unwrap();
    symlink(temporary.path().join("outside"), root.join("link")).unwrap();
    fs::write(root.join("template"), "{{ @link }}").unwrap();

    let output = textcon()
        .args([
            "--template",
            root.join("template").to_str().unwrap(),
            "--base-dir",
            root.to_str().unwrap(),
            "--sandbox",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stdout.windows(7).any(|window| window == b"OUTSIDE"));
}

#[cfg(unix)]
#[test]
fn sandbox_allows_absolute_inside_and_skips_discovered_symlinks() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("root");
    fs::create_dir(&root).unwrap();
    let inside = root.join("inside");
    fs::write(&inside, "INSIDE").unwrap();
    symlink(&inside, root.join("alias")).unwrap();
    let template = root.join("template");
    fs::write(&template, format!("{{{{ @{} }}}}", inside.display())).unwrap();

    let output = textcon()
        .args([
            "--template",
            template.to_str().unwrap(),
            "--base-dir",
            root.to_str().unwrap(),
            "--sandbox",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"INSIDE");

    fs::write(&template, "{{ @. | markdown }}").unwrap();
    let output = textcon()
        .args([
            "--template",
            template.to_str().unwrap(),
            "--base-dir",
            root.to_str().unwrap(),
            "--sandbox",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.windows(7).any(|window| window == b"`alias`"));
}

#[cfg(unix)]
#[test]
fn broken_pipe_is_quiet_success() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("large"), vec![b'x'; 4 * 1024 * 1024]).unwrap();
    let mut child = textcon()
        .current_dir(temporary.path())
        .args(["--render", "raw", "large"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut byte = [0_u8; 1];
    stdout.read_exact(&mut byte).unwrap();
    drop(stdout);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}

#[test]
fn duplicate_stdin_is_usage_error() {
    let output = textcon()
        .args(["-", "-"])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn template_stdin_streams() {
    let temporary = TempDir::new().unwrap();
    fs::write(temporary.path().join("file"), "VALUE").unwrap();
    let mut child = textcon()
        .current_dir(temporary.path())
        .args(["--template", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"x={{ @file }}")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"x=VALUE");
}
