use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};
use lark_package_format::{check_package, pack_directory, unpack_archive, write_failure_log};

const LARK_EXTENSION: &str = "lark";

#[derive(Clone, Copy)]
enum Operation {
    Pack,
    Unpack,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Pack => "Pack",
            Self::Unpack => "Unpack",
        }
    }
}

struct Arguments {
    path: PathBuf,
    check_only: bool,
    ignore_uncovered: bool,
}

fn main() -> ExitCode {
    let args = match parse_arguments(env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("Error: {error}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let operation = if args.check_only {
        None
    } else if args.path.is_dir() {
        Some(Operation::Pack)
    } else if is_lark_file(&args.path) {
        Some(Operation::Unpack)
    } else {
        None
    };

    let result = execute(&args);
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if let Some(operation) = operation
                && let Err(log_error) =
                    write_failure_log(operation.name(), &args.path, &error.to_string())
            {
                eprintln!("Failed to write failure log: {log_error}");
            }
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments<I>(args: I) -> Result<Arguments>
where
    I: IntoIterator,
    I::Item: Into<std::ffi::OsString>,
{
    let mut path = None;
    let mut check_only = false;
    let mut ignore_uncovered = false;

    for arg in args {
        let arg = arg.into();
        if arg == "--check" {
            check_only = true;
        } else if arg == "--ignore-uncovered" {
            ignore_uncovered = true;
        } else if arg.to_string_lossy().starts_with('-') || path.is_some() {
            bail!("invalid arguments");
        } else {
            path = Some(PathBuf::from(arg));
        }
    }

    let path = path.ok_or_else(|| anyhow::anyhow!("input path is required"))?;
    Ok(Arguments {
        path: path.canonicalize().unwrap_or(path),
        check_only,
        ignore_uncovered,
    })
}

fn execute(args: &Arguments) -> Result<String> {
    if args.check_only {
        return check_package(&args.path, args.ignore_uncovered);
    }
    if args.path.is_dir() {
        return pack_directory(&args.path, args.ignore_uncovered);
    }
    if is_lark_file(&args.path) {
        return unpack_archive(&args.path, args.ignore_uncovered);
    }

    bail!(
        "input must be an existing directory or .lark file: {}",
        args.path.display()
    )
}

fn is_lark_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(LARK_EXTENSION))
}

fn print_usage() {
    println!("Usage:");
    println!("  lark-pack-tool <package-directory>");
    println!("  lark-pack-tool <package.lark>");
    println!("  lark-pack-tool --check <package-directory|package.lark>");
    println!("  Add --ignore-uncovered to skip rules coverage validation for extra files.");
}
