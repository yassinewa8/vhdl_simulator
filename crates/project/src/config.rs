//! The `project.toml` file: what it contains, loading, saving and checking.
//!
//! The schema is described in `docs/phase2-design.md`. Example:
//!
//! ```toml
//! [project]
//! name = "uart"
//!
//! [[library]]
//! name = "uart_lib"
//! files = ["src/**/*.vhd"]
//!
//! [[testbench]]
//! top = "uart_tx_tb"
//! stop_time = "10 ms"
//! ```

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use nvc::VhdlStandard;
use serde::{Deserialize, Serialize};
use studio_core::SimTime;

/// Everything in a `project.toml` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Unknown keys are an error, so a typo such as `stoptime` is reported
// instead of silently ignored.
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub project: ProjectSettings,

    /// The user's own libraries, written as `[[library]]` in the file.
    #[serde(default, rename = "library", skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<LibraryConfig>,

    /// Pre-compiled libraries such as OSVVM, UVVM or vendor libraries.
    #[serde(default, skip_serializing_if = "ExternalLibraries::is_empty")]
    pub external: ExternalLibraries,

    /// Testbenches that can be run, written as `[[testbench]]`.
    #[serde(default, rename = "testbench", skip_serializing_if = "Vec::is_empty")]
    pub testbenches: Vec<TestbenchConfig>,
}

/// The `[project]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSettings {
    pub name: String,

    /// VHDL revision for every file. Written as `"2008"` (or `"08"`).
    #[serde(default, with = "standard_format")]
    pub standard: VhdlStandard,

    /// Where compiled libraries, build state and waveforms go. Relative to
    /// the project folder unless absolute.
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,

    /// Extra nvc options for every analysis, e.g. `["--relaxed"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub analyse_args: Vec<String>,
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("build")
}

/// One `[[library]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryConfig {
    pub name: String,
    /// File paths or glob patterns such as `src/**/*.vhd`, relative to the
    /// project folder. The order here doesn't matter for compiling.
    #[serde(default)]
    pub files: Vec<String>,
}

/// The `[external]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLibraries {
    /// Folders containing compiled libraries, passed as `nvc -L`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_paths: Vec<PathBuf>,
    /// Single libraries by name, passed as `nvc --map=NAME:PATH`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub map: BTreeMap<String, PathBuf>,
}

impl ExternalLibraries {
    pub fn is_empty(&self) -> bool {
        self.search_paths.is_empty() && self.map.is_empty()
    }
}

/// One `[[testbench]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestbenchConfig {
    /// The top-level entity. Its library is found automatically.
    pub top: String,

    /// Name shown in the GUI. Defaults to `top`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Top-level generic overrides. Values may be written as text or as
    /// TOML numbers/booleans (`BAUD = 115200`); they're passed to nvc as text.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "generics_format::deserialize"
    )]
    pub generics: BTreeMap<String, String>,

    /// Stop the simulation at this time, e.g. `"10 ms"`.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "stop_time_format")]
    pub stop_time: Option<SimTime>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elaborate_args: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_args: Vec<String>,
}

impl TestbenchConfig {
    /// A testbench with just a top entity and nothing else set.
    pub fn new(top: &str) -> TestbenchConfig {
        TestbenchConfig {
            top: top.to_string(),
            name: None,
            generics: BTreeMap::new(),
            stop_time: None,
            elaborate_args: Vec::new(),
            run_args: Vec::new(),
        }
    }

    /// The name to show the user: `name` if given, otherwise `top`.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.top)
    }
}

/// Why a project file couldn't be used.
#[derive(Debug)]
pub enum ProjectError {
    /// Reading or writing the file failed.
    Io { path: PathBuf, error: std::io::Error },
    /// The file isn't valid TOML, or doesn't match the schema. The message
    /// from the `toml` crate includes the line and column.
    Parse { path: PathBuf, message: String },
    /// The file was read, but some settings don't make sense.
    Invalid { path: PathBuf, problems: Vec<String> },
    /// Source files need each other in a loop, so they can't be ordered.
    Cycle(crate::order::CycleError),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            ProjectError::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            ProjectError::Invalid { path, problems } => {
                write!(f, "{}: problems in the project file:", path.display())?;
                for problem in problems {
                    write!(f, "\n  - {problem}")?;
                }
                Ok(())
            }
            ProjectError::Cycle(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ProjectError {}

/// Library names that nvc already provides. A project library with one of
/// these names would hide the real one.
const RESERVED_LIBRARY_NAMES: [&str; 2] = ["std", "ieee"];

impl ProjectConfig {
    /// An empty project with default settings.
    pub fn new(name: &str) -> ProjectConfig {
        ProjectConfig {
            project: ProjectSettings {
                name: name.to_string(),
                standard: VhdlStandard::default(),
                output_dir: default_output_dir(),
                analyse_args: Vec::new(),
            },
            libraries: Vec::new(),
            external: ExternalLibraries::default(),
            testbenches: Vec::new(),
        }
    }

    /// Reads and checks a project file.
    pub fn load(path: &Path) -> Result<ProjectConfig, ProjectError> {
        let text = std::fs::read_to_string(path).map_err(|error| ProjectError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        ProjectConfig::from_toml(&text, path)
    }

    /// Parses and checks project file text. `path` is only used in messages.
    pub fn from_toml(text: &str, path: &Path) -> Result<ProjectConfig, ProjectError> {
        let config: ProjectConfig = toml::from_str(text).map_err(|error| ProjectError::Parse {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let problems = config.problems();
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(ProjectError::Invalid {
                path: path.to_path_buf(),
                problems,
            })
        }
    }

    /// The project as TOML text.
    pub fn to_toml(&self) -> String {
        // Can only fail for types TOML can't represent; ours are all fine.
        toml::to_string_pretty(self).expect("project settings always convert to TOML")
    }

    /// Writes the project file. Note that comments in an existing file are
    /// not kept.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        std::fs::write(path, self.to_toml()).map_err(|error| ProjectError::Io {
            path: path.to_path_buf(),
            error,
        })
    }

    /// Everything wrong with the settings, as messages for the user. Empty
    /// if all is well.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.project.name.trim().is_empty() {
            problems.push("[project] name must not be empty".to_string());
        }

        // Library names are case-insensitive in VHDL, so compare lower case.
        let mut seen_libraries = HashSet::new();
        let external_names = self.external.map.keys();
        let all_libraries = self
            .libraries
            .iter()
            .map(|library| (&library.name, "[[library]]"))
            .chain(external_names.map(|name| (name, "[external] map")));
        for (name, section) in all_libraries {
            let lower = name.to_lowercase();
            if !is_vhdl_identifier(name) {
                problems.push(format!(
                    "{section}: '{name}' is not a valid VHDL library name \
                     (letters, digits and single underscores, starting with a letter)"
                ));
            } else if RESERVED_LIBRARY_NAMES.contains(&lower.as_str()) {
                problems.push(format!(
                    "{section}: '{name}' is a built-in nvc library and can't be redefined"
                ));
            } else if !seen_libraries.insert(lower) {
                problems.push(format!("library '{name}' is defined more than once"));
            }
        }

        let mut seen_testbenches = HashSet::new();
        for testbench in &self.testbenches {
            if !is_vhdl_identifier(&testbench.top) {
                problems.push(format!(
                    "[[testbench]]: top = '{}' is not a valid entity name",
                    testbench.top
                ));
            }
            let name = testbench.display_name().to_lowercase();
            if !seen_testbenches.insert(name) {
                problems.push(format!(
                    "testbench '{}' appears more than once; give one of them a different `name`",
                    testbench.display_name()
                ));
            }
        }
        problems
    }
}

/// True for a VHDL basic identifier such as `uart_lib`: starts with a
/// letter, then letters, digits and underscores, with no two underscores
/// in a row and no underscore at the end.
pub fn is_vhdl_identifier(name: &str) -> bool {
    let starts_with_letter = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let valid_characters = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    starts_with_letter && valid_characters && !name.contains("__") && !name.ends_with('_')
}

/// Reads and writes the standard as `"2008"` rather than an enum name.
mod standard_format {
    use nvc::VhdlStandard;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(standard: &VhdlStandard, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(standard.year())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<VhdlStandard, D::Error> {
        let text = String::deserialize(d)?;
        VhdlStandard::parse(&text).ok_or_else(|| {
            D::Error::custom(format!(
                "unknown VHDL standard '{text}'; use 1993, 2000, 2002, 2008 or 2019"
            ))
        })
    }
}

/// Reads and writes stop times as text such as `"10 ms"`.
mod stop_time_format {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use studio_core::SimTime;

    pub fn serialize<S: Serializer>(time: &Option<SimTime>, s: S) -> Result<S::Ok, S::Error> {
        match time {
            Some(time) => s.serialize_str(&time.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<SimTime>, D::Error> {
        let text = String::deserialize(d)?;
        SimTime::parse(&text).map(Some).ok_or_else(|| {
            D::Error::custom(format!(
                "'{text}' is not a time; use a whole number and unit, e.g. \"10 ms\""
            ))
        })
    }
}

/// Lets generic values be written as TOML text, numbers or booleans.
mod generics_format {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer};

    /// Any of the value types that make sense for a VHDL generic.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum GenericValue {
        Text(String),
        Integer(i64),
        Real(f64),
        Boolean(bool),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<String, String>, D::Error> {
        let values = BTreeMap::<String, GenericValue>::deserialize(d)?;
        Ok(values
            .into_iter()
            .map(|(name, value)| {
                let text = match value {
                    GenericValue::Text(text) => text,
                    GenericValue::Integer(number) => number.to_string(),
                    GenericValue::Real(number) => format!("{number:?}"),
                    GenericValue::Boolean(flag) => flag.to_string(),
                };
                (name, text)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full example from docs/phase2-design.md.
    const EXAMPLE: &str = r#"
        [project]
        name = "uart"
        standard = "2008"
        output_dir = "build"
        analyse_args = ["--relaxed"]

        [[library]]
        name = "uart_lib"
        files = ["src/**/*.vhd"]

        [[library]]
        name = "tb_lib"
        files = ["tb/*.vhd", "models/uart_model.vhd"]

        [external]
        search_paths = ["C:/vendor/nvc-libs"]
        map = { unisim = "C:/xilinx/nvc/unisim" }

        [[testbench]]
        top = "uart_tx_tb"
        generics = { BAUD = "115200", DATA_BITS = "8" }
        stop_time = "10 ms"
        elaborate_args = ["--jit"]
        run_args = ["--exit-severity=error"]

        [[testbench]]
        top = "uart_rx_tb"
        name = "rx, fast baud"
        generics = { BAUD = "921600" }
    "#;

    fn parse(text: &str) -> Result<ProjectConfig, ProjectError> {
        ProjectConfig::from_toml(text, Path::new("project.toml"))
    }

    fn problems_in(text: &str) -> Vec<String> {
        match parse(text) {
            Err(ProjectError::Invalid { problems, .. }) => problems,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn reads_the_design_example() {
        let config = parse(EXAMPLE).unwrap();
        assert_eq!(config.project.name, "uart");
        assert_eq!(config.project.standard, VhdlStandard::Vhdl2008);
        assert_eq!(config.project.analyse_args, ["--relaxed"]);
        assert_eq!(config.libraries.len(), 2);
        assert_eq!(config.libraries[1].files, ["tb/*.vhd", "models/uart_model.vhd"]);
        assert_eq!(config.external.map["unisim"], PathBuf::from("C:/xilinx/nvc/unisim"));

        let tx = &config.testbenches[0];
        assert_eq!(tx.display_name(), "uart_tx_tb");
        assert_eq!(tx.generics["BAUD"], "115200");
        assert_eq!(tx.stop_time, SimTime::parse("10ms"));
        assert_eq!(config.testbenches[1].display_name(), "rx, fast baud");
    }

    #[test]
    fn minimal_file_uses_defaults() {
        let config = parse("[project]\nname = \"x\"\n").unwrap();
        assert_eq!(config, ProjectConfig::new("x"));
        assert_eq!(config.project.output_dir, PathBuf::from("build"));
        assert_eq!(config.project.standard, VhdlStandard::Vhdl2008);
    }

    #[test]
    fn saving_and_loading_gives_the_same_project() {
        let config = parse(EXAMPLE).unwrap();
        let reloaded = parse(&config.to_toml()).unwrap();
        assert_eq!(reloaded, config);
    }

    #[test]
    fn saved_file_is_readable() {
        let mut config = ProjectConfig::new("demo");
        config.libraries.push(LibraryConfig {
            name: "work".to_string(),
            files: vec!["src/*.vhd".to_string()],
        });
        let mut testbench = TestbenchConfig::new("demo_tb");
        testbench.stop_time = SimTime::parse("1us");
        config.testbenches.push(testbench);

        let text = config.to_toml();
        assert!(text.contains("standard = \"2008\""), "{text}");
        assert!(text.contains("[[library]]"), "{text}");
        assert!(text.contains("stop_time = \"1 us\""), "{text}");
        // Empty optional settings are left out rather than written as [].
        assert!(!text.contains("analyse_args"), "{text}");
        assert!(!text.contains("[external]"), "{text}");
    }

    #[test]
    fn generics_can_be_numbers_or_booleans() {
        let config = parse(
            r#"
            [project]
            name = "x"
            [[testbench]]
            top = "tb"
            generics = { WIDTH = 8, FAST = true, RATIO = 1.5, MODE = "fast" }
            "#,
        )
        .unwrap();
        let generics = &config.testbenches[0].generics;
        assert_eq!(generics["WIDTH"], "8");
        assert_eq!(generics["FAST"], "true");
        assert_eq!(generics["RATIO"], "1.5");
        assert_eq!(generics["MODE"], "fast");
    }

    #[test]
    fn typo_in_a_key_is_an_error_with_a_line_number() {
        let error = parse("[project]\nname = \"x\"\nstoptime = \"1 ns\"\n").unwrap_err();
        let message = error.to_string();
        assert!(matches!(error, ProjectError::Parse { .. }));
        assert!(message.contains("stoptime"), "{message}");
        assert!(message.contains("line 3"), "{message}");
    }

    #[test]
    fn bad_standard_and_stop_time_are_explained() {
        let message = parse("[project]\nname = \"x\"\nstandard = \"2010\"\n")
            .unwrap_err()
            .to_string();
        assert!(message.contains("unknown VHDL standard '2010'"), "{message}");

        let message = parse("[project]\nname = \"x\"\n[[testbench]]\ntop = \"tb\"\nstop_time = \"soon\"\n")
            .unwrap_err()
            .to_string();
        assert!(message.contains("'soon' is not a time"), "{message}");
    }

    #[test]
    fn library_name_problems() {
        let problems = problems_in(
            r#"
            [project]
            name = "x"
            [[library]]
            name = "ieee"
            [[library]]
            name = "my lib"
            [[library]]
            name = "Common"
            [[library]]
            name = "common"
            [external]
            map = { COMMON = "elsewhere" }
            "#,
        );
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems[0].contains("'ieee' is a built-in"));
        assert!(problems[1].contains("'my lib' is not a valid"));
        assert!(problems[2].contains("'common' is defined more than once"));
        assert!(problems[3].contains("'COMMON' is defined more than once"));
    }

    #[test]
    fn testbench_problems() {
        let problems = problems_in(
            r#"
            [project]
            name = " "
            [[testbench]]
            top = "tb"
            [[testbench]]
            top = "tb"
            [[testbench]]
            top = "work.tb2"
            "#,
        );
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems[0].contains("name must not be empty"));
        assert!(problems[1].contains("testbench 'tb' appears more than once"));
        assert!(problems[2].contains("'work.tb2' is not a valid entity name"));
    }

    #[test]
    fn vhdl_identifiers() {
        assert!(is_vhdl_identifier("uart_lib"));
        assert!(is_vhdl_identifier("L2"));
        assert!(!is_vhdl_identifier(""));
        assert!(!is_vhdl_identifier("2lib"));
        assert!(!is_vhdl_identifier("_lib"));
        assert!(!is_vhdl_identifier("lib_"));
        assert!(!is_vhdl_identifier("my__lib"));
        assert!(!is_vhdl_identifier("my-lib"));
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let error = ProjectConfig::load(Path::new("no/such/project.toml")).unwrap_err();
        assert!(matches!(error, ProjectError::Io { .. }));
    }
}
