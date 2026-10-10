//! Building the command-line arguments for each nvc step.
//!
//! Kept separate from actually running nvc so the exact arguments can be
//! checked in tests without nvc installed.

use std::fmt;
use std::path::{Path, PathBuf};

use studio_core::SimTime;

/// VHDL language revision, passed to nvc as `--std=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VhdlStandard {
    Vhdl1993,
    Vhdl2000,
    Vhdl2002,
    #[default]
    Vhdl2008,
    Vhdl2019,
}

impl VhdlStandard {
    pub const ALL: [VhdlStandard; 5] = [
        VhdlStandard::Vhdl1993,
        VhdlStandard::Vhdl2000,
        VhdlStandard::Vhdl2002,
        VhdlStandard::Vhdl2008,
        VhdlStandard::Vhdl2019,
    ];

    /// Reads `"2008"` or the short form `"08"`.
    pub fn parse(text: &str) -> Option<VhdlStandard> {
        let text = text.trim();
        VhdlStandard::ALL
            .into_iter()
            .find(|standard| standard.year() == text || &standard.year()[2..] == text)
    }

    /// The four-digit year, as nvc expects it.
    pub fn year(self) -> &'static str {
        match self {
            VhdlStandard::Vhdl1993 => "1993",
            VhdlStandard::Vhdl2000 => "2000",
            VhdlStandard::Vhdl2002 => "2002",
            VhdlStandard::Vhdl2008 => "2008",
            VhdlStandard::Vhdl2019 => "2019",
        }
    }
}

impl fmt::Display for VhdlStandard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VHDL-{}", self.year())
    }
}

/// A VHDL library: its name and the folder nvc stores it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub dir: PathBuf,
}

/// Settings shared by every step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvcOptions {
    pub standard: VhdlStandard,
    /// The library that design units are analysed into.
    pub work: Library,
    /// Folders containing compiled libraries (`-L`).
    pub search_paths: Vec<PathBuf>,
    /// Other libraries by name (`--map=NAME:PATH`).
    pub library_map: Vec<Library>,
    /// Extra options placed after `-a`, e.g. `--relaxed`.
    pub extra_analyse_args: Vec<String>,
    /// Extra options placed after `-e`, e.g. `--jit`.
    pub extra_elaborate_args: Vec<String>,
    /// Extra options placed after `-r`, e.g. `--exit-severity=error`.
    pub extra_run_args: Vec<String>,
}

/// Settings for the run step only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunOptions {
    /// Stop the simulation at this time. `None` runs until it stops itself.
    pub stop_time: Option<SimTime>,
    /// Write an FST waveform to this file. `None` writes no waveform.
    pub wave_file: Option<PathBuf>,
}

impl NvcOptions {
    /// Options with just a standard and work library, and nothing extra.
    pub fn new(standard: VhdlStandard, work: Library) -> NvcOptions {
        NvcOptions {
            standard,
            work,
            search_paths: Vec::new(),
            library_map: Vec::new(),
            extra_analyse_args: Vec::new(),
            extra_elaborate_args: Vec::new(),
            extra_run_args: Vec::new(),
        }
    }

    /// Arguments for analysing `files` into the work library.
    pub fn analyse_args(&self, files: &[PathBuf]) -> Vec<String> {
        let mut args = self.global_args("full");
        args.push("-a".to_string());
        args.extend(self.extra_analyse_args.iter().cloned());
        args.extend(files.iter().map(|file| path_arg(file)));
        args
    }

    /// Arguments for elaborating `top` with optional generic overrides.
    pub fn elaborate_args(&self, top: &str, generics: &[(String, String)]) -> Vec<String> {
        let mut args = self.global_args("full");
        args.push("-e".to_string());
        args.extend(self.extra_elaborate_args.iter().cloned());
        for (name, value) in generics {
            args.push(format!("-g{name}={value}"));
        }
        args.push(top.to_string());
        args
    }

    /// Arguments for running an already elaborated `top`.
    ///
    /// Uses compact messages: they point at the actual report/assert line,
    /// where the full style only gives the line the process starts on.
    pub fn run_args(&self, top: &str, run: &RunOptions) -> Vec<String> {
        let mut args = self.global_args("compact");
        args.push("-r".to_string());
        args.extend(self.extra_run_args.iter().cloned());
        if let Some(stop_time) = run.stop_time {
            args.push(format!("--stop-time={}", stop_time.to_nvc_string()));
        }
        if let Some(wave_file) = &run.wave_file {
            args.push(format!("--wave={}", path_arg(wave_file)));
        }
        args.push(top.to_string());
        args
    }

    /// Options that go before the command (`-a`, `-e`, `-r`).
    fn global_args(&self, message_style: &str) -> Vec<String> {
        let mut args = vec![
            format!("--std={}", self.standard.year()),
            format!("--work={}:{}", self.work.name, path_arg(&self.work.dir)),
            format!("--messages={message_style}"),
        ];
        for path in &self.search_paths {
            args.push(format!("-L{}", path_arg(path)));
        }
        for library in &self.library_map {
            args.push(format!("--map={}:{}", library.name, path_arg(&library.dir)));
        }
        args
    }
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> NvcOptions {
        NvcOptions::new(
            VhdlStandard::Vhdl2008,
            Library {
                name: "work".to_string(),
                dir: PathBuf::from("build/work"),
            },
        )
    }

    #[test]
    fn parses_standards() {
        assert_eq!(VhdlStandard::parse("2008"), Some(VhdlStandard::Vhdl2008));
        assert_eq!(VhdlStandard::parse("93"), Some(VhdlStandard::Vhdl1993));
        assert_eq!(VhdlStandard::parse("2019"), Some(VhdlStandard::Vhdl2019));
        assert_eq!(VhdlStandard::parse("2010"), None);
        assert_eq!(VhdlStandard::default(), VhdlStandard::Vhdl2008);
    }

    #[test]
    fn analyse_puts_global_options_before_the_command() {
        let args = options().analyse_args(&[PathBuf::from("a.vhd"), PathBuf::from("b.vhd")]);
        assert_eq!(
            args,
            [
                "--std=2008",
                "--work=work:build/work",
                "--messages=full",
                "-a",
                "a.vhd",
                "b.vhd"
            ]
        );
    }

    #[test]
    fn elaborate_passes_generics() {
        let generics = [("WIDTH".to_string(), "16".to_string())];
        let args = options().elaborate_args("counter_tb", &generics);
        assert_eq!(&args[3..], ["-e", "-gWIDTH=16", "counter_tb"]);
    }

    #[test]
    fn run_uses_compact_messages() {
        let args = options().run_args("tb", &RunOptions::default());
        assert!(args.contains(&"--messages=compact".to_string()));
        assert_eq!(&args[3..], ["-r", "tb"]);
    }

    #[test]
    fn libraries_and_extra_args_go_in_the_right_places() {
        let mut options = options();
        options.search_paths = vec![PathBuf::from("C:/libs")];
        options.library_map = vec![Library {
            name: "util".to_string(),
            dir: PathBuf::from("build/util"),
        }];
        options.extra_analyse_args = vec!["--relaxed".to_string()];
        options.extra_elaborate_args = vec!["--jit".to_string()];
        options.extra_run_args = vec!["--exit-severity=error".to_string()];

        let analyse = options.analyse_args(&[PathBuf::from("a.vhd")]);
        assert_eq!(
            &analyse[3..],
            ["-LC:/libs", "--map=util:build/util", "-a", "--relaxed", "a.vhd"]
        );
        let elaborate = options.elaborate_args("tb", &[("N".to_string(), "1".to_string())]);
        assert_eq!(&elaborate[5..], ["-e", "--jit", "-gN=1", "tb"]);
        let run = options.run_args("tb", &RunOptions::default());
        assert_eq!(&run[5..], ["-r", "--exit-severity=error", "tb"]);
    }

    #[test]
    fn run_with_stop_time_and_wave_file() {
        let run = RunOptions {
            stop_time: SimTime::parse("1500ns"),
            wave_file: Some(PathBuf::from("out/tb.fst")),
        };
        let args = options().run_args("tb", &run);
        assert_eq!(&args[3..], ["-r", "--stop-time=1500ns", "--wave=out/tb.fst", "tb"]);
    }
}
