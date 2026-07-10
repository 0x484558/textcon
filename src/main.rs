use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory as _, Parser as _, error::ErrorKind};
use textcon::cli::Cli;
use textcon::{Engine, EngineOptions, Result, SelectionOptions, TextconError};

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli
        .inputs
        .iter()
        .filter(|path| path.as_path() == Path::new("-"))
        .count()
        > 1
    {
        Cli::command()
            .error(
                ErrorKind::ArgumentConflict,
                "stdin operand '-' may be specified only once",
            )
            .exit();
    }

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.is_output_broken_pipe() => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("textcon: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let options = EngineOptions {
        render: cli.render,
        base_dir: cli.base_dir.unwrap_or_else(|| PathBuf::from(".")),
        sandbox: cli.sandbox,
        selection: SelectionOptions {
            max_depth: cli.max_depth,
            hidden: cli.hidden,
            use_gitignore: !cli.no_gitignore,
            excludes: cli.excludes,
        },
    };
    let mut engine = Engine::new(options)?;
    engine.protect_stdout();

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    if let Some(template) = cli.template {
        if template == Path::new("-") {
            let stdin = io::stdin();
            engine.expand_template(&mut stdin.lock(), &mut output)?;
        } else {
            let file = File::open(&template).map_err(|source| TextconError::Input {
                name: template.display().to_string(),
                source,
            })?;
            engine.expand_template(&mut BufReader::new(file), &mut output)?;
        }
    } else {
        for input in cli.inputs {
            if input == Path::new("-") {
                let stdin = io::stdin();
                engine.render_reader(Path::new("-"), &mut stdin.lock(), &mut output)?;
            } else {
                engine.render_inputs(std::iter::once(input), &mut output)?;
            }
        }
    }
    output.flush().map_err(TextconError::Output)
}
