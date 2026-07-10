use clap::{CommandFactory as _, Parser as _};
use textcon::cli::Cli;

#[test]
fn man_page_covers_every_visible_cli_option() {
    let source = include_str!("../docs/man/textcon.1.scd");
    let command = Cli::command();
    for argument in command
        .get_arguments()
        .filter(|argument| !argument.is_hide_set())
    {
        if let Some(long) = argument.get_long() {
            assert!(
                source.contains(&format!("--{long}")),
                "man page does not mention --{long}"
            );
        }
        if let Some(short) = argument.get_short() {
            assert!(
                source.contains(&format!("-{short}")),
                "man page does not mention -{short}"
            );
        }
    }
}

#[test]
fn removed_surface_is_not_documented_as_options() {
    let source = include_str!("../docs/man/textcon.1.scd");
    for removed in [
        "--output",
        "--format",
        "--dry-run",
        "--list",
        "--verbose",
        "--quiet",
    ] {
        assert!(
            !source.contains(removed),
            "stale option {removed} in man page"
        );
    }
}

#[test]
fn cli_schema_parses_documented_examples() {
    for arguments in [
        vec!["textcon", "src/main.rs", "src/lib.rs"],
        vec!["textcon", "--render", "raw", "part1", "part2"],
        vec!["textcon", "--template", "-"],
        vec![
            "textcon",
            "--template",
            "context.md",
            "--base-dir",
            "./project",
            "--sandbox",
        ],
    ] {
        Cli::try_parse_from(arguments).unwrap();
    }
}
