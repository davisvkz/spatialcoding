//! Command-line options: which folder and files to open at startup.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: spatialcoding [DIR] [--open FILE]... [--open-all] [--layout]

  DIR            folder to load as the workspace
  --open FILE    open FILE (relative to DIR) on the canvas; repeatable
  --open-all     open every text file in DIR (same as Ctrl+Shift+O)
  --layout       apply the hierarchical layout after opening files
  -h, --help     show this help";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct StartupOptions {
    pub dir: Option<PathBuf>,
    /// Files to open, relative to `dir` (or absolute).
    pub open: Vec<PathBuf>,
    pub open_all: bool,
    pub layout: bool,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Run(StartupOptions),
    Help,
}

/// Parses the arguments after the program name. Does not touch the filesystem.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut opts = StartupOptions::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--open" => {
                let file = args.next().ok_or("--open needs a FILE")?;
                opts.open.push(PathBuf::from(file));
            }
            "--open-all" => opts.open_all = true,
            "--layout" => opts.layout = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option: {flag}")),
            dir => {
                if opts.dir.is_some() {
                    return Err(format!("unexpected argument: {dir}"));
                }
                opts.dir = Some(PathBuf::from(dir));
            }
        }
    }
    if opts.dir.is_none() && (!opts.open.is_empty() || opts.open_all || opts.layout) {
        return Err("--open, --open-all and --layout need a DIR".into());
    }
    Ok(Command::Run(opts))
}

impl StartupOptions {
    /// Makes `dir` absolute and checks that it and every `--open` file exist.
    pub fn resolve(mut self) -> Result<Self, String> {
        let Some(dir) = self.dir.take() else {
            return Ok(self);
        };
        let dir = std::fs::canonicalize(&dir)
            .map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
        if !dir.is_dir() {
            return Err(format!("not a directory: {}", dir.display()));
        }
        for file in &self.open {
            if !dir.join(file).is_file() {
                return Err(format!("no such file in {}: {}", dir.display(), file.display()));
            }
        }
        self.dir = Some(dir);
        Ok(self)
    }

    /// Absolute paths of the `--open` files.
    pub fn open_paths(&self) -> Vec<PathBuf> {
        match &self.dir {
            Some(dir) => self.open.iter().map(|f| dir.join(f)).collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_start_empty() {
        assert_eq!(run(&[]), Ok(Command::Run(StartupOptions::default())));
    }

    #[test]
    fn parses_dir_files_and_flags_in_any_order() {
        let cmd = run(&["--open", "src/a.rs", "proj", "--layout", "--open", "b.rs", "--open-all"]);
        assert_eq!(
            cmd,
            Ok(Command::Run(StartupOptions {
                dir: Some("proj".into()),
                open: vec!["src/a.rs".into(), "b.rs".into()],
                open_all: true,
                layout: true,
            }))
        );
    }

    #[test]
    fn help_wins() {
        assert_eq!(run(&["proj", "--help"]), Ok(Command::Help));
        assert_eq!(run(&["-h"]), Ok(Command::Help));
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(run(&["--bogus"]).unwrap_err().contains("--bogus"));
        assert!(run(&["proj", "--open"]).is_err());
        assert!(run(&["a", "b"]).unwrap_err().contains("b"));
        assert!(run(&["--open", "x.rs"]).is_err(), "--open without DIR");
        assert!(run(&["--layout"]).is_err(), "--layout without DIR");
    }

    #[test]
    fn resolve_makes_dir_absolute_and_checks_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();

        let ok = StartupOptions {
            dir: Some(dir.path().join("src/..")),
            open: vec!["src/a.rs".into()],
            ..Default::default()
        }
        .resolve()
        .unwrap();
        assert_eq!(ok.dir.as_deref(), Some(canonical.as_path()));
        assert_eq!(ok.open_paths(), [canonical.join("src/a.rs")]);

        let missing_file = StartupOptions {
            dir: Some(dir.path().into()),
            open: vec!["nope.rs".into()],
            ..Default::default()
        };
        assert!(missing_file.resolve().unwrap_err().contains("nope.rs"));

        let missing_dir = StartupOptions {
            dir: Some(dir.path().join("nope")),
            ..Default::default()
        };
        assert!(missing_dir.resolve().is_err());

        let file_as_dir = StartupOptions {
            dir: Some(dir.path().join("src/a.rs")),
            ..Default::default()
        };
        assert!(file_as_dir.resolve().unwrap_err().contains("not a directory"));
    }
}
