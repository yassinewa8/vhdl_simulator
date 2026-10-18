//! A small VHDL scanner that finds what a file defines and what it uses.
//!
//! This isn't a VHDL parser. It strips comments and literals, splits the
//! text into words and symbols, then looks for a few short patterns such
//! as `entity X is` or `lib.unit`. That's enough to work out compile order,
//! and it still works on files with syntax errors, so nvc gets to report
//! the real error instead of us failing first.
//!
//! | Pattern                               | Means                          |
//! |---------------------------------------|--------------------------------|
//! | `entity X is`                         | defines entity X               |
//! | `package X is` (also `is new G`)      | defines package X (uses G)     |
//! | `package body X is`                   | body of package X (uses X)     |
//! | `context X is`                        | defines context X              |
//! | `architecture A of E is`              | architecture of E (uses E)     |
//! | `configuration C of E is`             | defines configuration C (uses E) |
//! | `lib.unit` where `lib` is a library   | uses unit in lib               |
//! | `label : comp port/generic map`       | uses comp                      |
//! | `label : component comp`              | uses comp                      |
//!
//! A word counts as a library name in `lib.unit` if the file has a
//! `library lib;` clause, or if it is `work`. Names that don't match any
//! unit in the project (such as `ieee.std_logic_1164`) are simply ignored
//! later, when the compile order is worked out.

use std::collections::{BTreeSet, HashSet};

/// What a source file defines and what it refers to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScannedFile {
    /// Design units in the order they appear in the file.
    pub units: Vec<DesignUnit>,
    /// Names this file needs to be compiled after. Sorted, no duplicates.
    pub references: BTreeSet<Reference>,
    /// Instances inside this file's architectures, in order, for the
    /// design hierarchy.
    pub instances: Vec<Instance>,
}

/// One instance in an architecture, such as `u_add : adder port map ...`
/// or `dut : entity work.top`. Names are lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The entity whose architecture contains the instance.
    pub parent: String,
    /// The instance label, e.g. `u_add`.
    pub label: String,
    /// What it instantiates: `Selected` for `entity lib.name`,
    /// `Unqualified` for a component (matched to the entity of that name).
    pub target: Reference,
}

/// A design unit found in a file. Names are lower case, because VHDL
/// names are case-insensitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignUnit {
    pub kind: UnitKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitKind {
    Entity,
    Architecture { entity: String },
    Package,
    PackageBody,
    Context,
    Configuration { entity: String },
}

impl UnitKind {
    /// True for units other files can refer to by name. Architectures and
    /// package bodies aren't: other files refer to their entity or package.
    pub fn is_primary(&self) -> bool {
        !matches!(self, UnitKind::Architecture { .. } | UnitKind::PackageBody)
    }
}

/// A name a file refers to. Lower case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reference {
    /// `library.unit`, e.g. `work.uart_pkg` or `osvvm.alertlogpkg`. The
    /// library may be `work`, meaning the file's own library.
    Selected { library: String, unit: String },
    /// A bare unit name, e.g. a component or the entity an architecture
    /// belongs to. Looked up in the file's own library first.
    Unqualified(String),
}

/// Scans VHDL source text.
pub fn scan(source: &str) -> ScannedFile {
    let tokens = tokenize(source);
    let libraries = library_clause_names(&tokens);
    let mut result = ScannedFile::default();
    // The entity whose architecture we're in, if any.
    let mut inside: Option<String> = None;

    for i in 0..tokens.len() {
        let units_before = result.units.len();
        find_design_unit(&tokens[i..], &mut result);
        if result.units.len() > units_before {
            inside = match &result.units[units_before].kind {
                UnitKind::Architecture { entity } => Some(entity.clone()),
                _ => None,
            };
        }
        if let Some((label, target)) = find_instantiation(&tokens[i..], &mut result)
            && let Some(parent) = &inside
        {
            result.instances.push(Instance {
                parent: parent.clone(),
                label,
                target,
            });
        }
        let follows_dot = i > 0 && tokens[i - 1] == Token::Symbol('.');
        if !follows_dot {
            find_selected_name(&tokens[i..], &libraries, &mut result);
        }
    }
    result
}

/// Looks for a design unit starting at the first token.
fn find_design_unit(tokens: &[Token], result: &mut ScannedFile) {
    use Token::Word;
    let (kind, name) = match tokens {
        [Word(kw), Word(body), Word(name), Word(is), ..]
            if kw == "package" && body == "body" && is == "is" =>
        {
            result.references.insert(Reference::Unqualified(name.clone()));
            (UnitKind::PackageBody, name)
        }
        [Word(kw), Word(name), Word(is), rest @ ..] if kw == "package" && is == "is" => {
            // `package P is new G` instantiates generic package G. A
            // `new lib.G` is found by the selected-name rule instead.
            if let [Word(new), Word(generic), next, ..] = rest
                && new == "new"
                && *next != Token::Symbol('.')
            {
                result.references.insert(Reference::Unqualified(generic.clone()));
            }
            (UnitKind::Package, name)
        }
        [Word(kw), Word(name), Word(is), ..] if is == "is" => match kw.as_str() {
            "entity" => (UnitKind::Entity, name),
            "context" => (UnitKind::Context, name),
            _ => return,
        },
        [Word(kw), Word(name), Word(of), Word(entity), Word(is), ..] if of == "of" && is == "is" => {
            let kind = match kw.as_str() {
                "architecture" => UnitKind::Architecture {
                    entity: entity.clone(),
                },
                "configuration" => UnitKind::Configuration {
                    entity: entity.clone(),
                },
                _ => return,
            };
            result.references.insert(Reference::Unqualified(entity.clone()));
            (kind, name)
        }
        _ => return,
    };
    result.units.push(DesignUnit {
        kind,
        name: name.clone(),
    });
}

/// Looks for an instantiation starting at the first token:
/// `label : comp port map`, `label : comp generic map`,
/// `label : component comp` or `label : entity lib.name`. Returns the
/// label and what's instantiated.
fn find_instantiation(tokens: &[Token], result: &mut ScannedFile) -> Option<(String, Reference)> {
    use Token::{Symbol, Word};
    let (label, target) = match tokens {
        [Word(label), Symbol(':'), Word(kw), Word(library), Symbol('.'), Word(name), ..] if kw == "entity" => {
            // The selected-name rule adds the reference, if `library` is
            // visible; here it's only needed for the hierarchy.
            let target = Reference::Selected {
                library: library.clone(),
                unit: name.clone(),
            };
            return Some((label.clone(), target));
        }
        [Word(label), Symbol(':'), Word(kw), Word(name), ..] if kw == "component" => (label, name),
        [Word(label), Symbol(':'), Word(name), Word(port_or_generic), Word(map), ..]
            if (port_or_generic == "port" || port_or_generic == "generic") && map == "map" =>
        {
            (label, name)
        }
        _ => return None,
    };
    let target = Reference::Unqualified(target.clone());
    result.references.insert(target.clone());
    Some((label.clone(), target))
}

/// Looks for `lib.unit` starting at the first token, where `lib` is a
/// library this file can see.
fn find_selected_name(tokens: &[Token], libraries: &HashSet<String>, result: &mut ScannedFile) {
    use Token::{Symbol, Word};
    if let [Word(library), Symbol('.'), Word(unit), ..] = tokens
        && libraries.contains(library)
    {
        result.references.insert(Reference::Selected {
            library: library.clone(),
            unit: unit.clone(),
        });
    }
}

/// Names from every `library a, b;` clause, plus `work`, which is always
/// visible.
fn library_clause_names(tokens: &[Token]) -> HashSet<String> {
    let mut names = HashSet::from(["work".to_string()]);
    for (i, token) in tokens.iter().enumerate() {
        if *token != Token::Word("library".to_string()) {
            continue;
        }
        // Collect `a , b , c` up to the semicolon.
        for next in &tokens[i + 1..] {
            match next {
                Token::Word(name) => {
                    names.insert(name.clone());
                }
                Token::Symbol(',') => {}
                _ => break,
            }
        }
    }
    names
}

/// A piece of VHDL text, as far as the scanner cares.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An identifier or keyword, lower case. Extended identifiers such as
    /// `\My Name\` keep their exact text, backslashes included.
    Word(String),
    /// One of `. : ; , ( )`.
    Symbol(char),
    /// A number, string, bit string or character literal. Its value never
    /// matters; it's kept only so the words either side of it aren't seen
    /// as next to each other.
    Literal,
}

/// Splits VHDL text into tokens, dropping comments and anything else the
/// patterns above don't need (operators, `<=`, and so on).
fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        if c == '-' && next == Some('-') {
            // Line comment: skip to the end of the line.
            i = find_from(&chars, i, |c| c == '\n');
        } else if c == '/' && next == Some('*') {
            // Block comment (VHDL-2008): skip past the closing `*/`.
            i = match find_sequence(&chars, i + 2, &['*', '/']) {
                Some(end) => end + 2,
                None => chars.len(),
            };
        } else if c == '"' {
            i = skip_string(&chars, i);
            tokens.push(Token::Literal);
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            // Character literal such as 'a'. A tick on its own (as in
            // `clk'event`) is an attribute and is ignored below.
            i += 3;
            tokens.push(Token::Literal);
        } else if c == '\\' {
            // Extended identifier: \any text\ (a doubled \\ is a backslash).
            let end = find_from(&chars, i + 1, |c| c == '\\');
            let end = (end + 1).min(chars.len());
            tokens.push(Token::Word(chars[i..end].iter().collect()));
            i = end;
        } else if c.is_ascii_alphabetic() {
            let end = find_from(&chars, i, |c| !(c.is_ascii_alphanumeric() || c == '_'));
            let word: String = chars[i..end].iter().collect();
            tokens.push(Token::Word(word.to_lowercase()));
            i = end;
        } else if c.is_ascii_digit() {
            // Numbers, including 1.5, 1_000, 16#FF# and 1.0e-3.
            i = find_from(&chars, i, |c| !(c.is_ascii_alphanumeric() || "_#.".contains(c)));
            tokens.push(Token::Literal);
        } else {
            if ".:;,()".contains(c) {
                tokens.push(Token::Symbol(c));
            }
            i += 1;
        }
    }
    tokens
}

/// Index of the first character at or after `start` that matches `stop`,
/// or the end of the text.
fn find_from(chars: &[char], start: usize, stop: impl Fn(char) -> bool) -> usize {
    (start..chars.len()).find(|&i| stop(chars[i])).unwrap_or(chars.len())
}

/// Index where `sequence` next starts at or after `start`.
fn find_sequence(chars: &[char], start: usize, sequence: &[char]) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i..].starts_with(sequence))
}

/// Skips a string literal starting at `start` (the opening quote). Two
/// quotes in a row inside the string stand for one quote character. A
/// string can't span lines, so an unclosed one ends at the line end.
fn skip_string(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '"' if chars.get(i + 1) == Some(&'"') => i += 2,
            '"' => return i + 1,
            '\n' => return i,
            _ => i += 1,
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(library: &str, unit: &str) -> Reference {
        Reference::Selected {
            library: library.to_string(),
            unit: unit.to_string(),
        }
    }

    fn unqualified(name: &str) -> Reference {
        Reference::Unqualified(name.to_string())
    }

    fn unit(kind: UnitKind, name: &str) -> DesignUnit {
        DesignUnit {
            kind,
            name: name.to_string(),
        }
    }

    fn references(source: &str) -> Vec<Reference> {
        scan(source).references.into_iter().collect()
    }

    #[test]
    fn finds_entity_and_architecture() {
        let scanned = scan(
            "library ieee;
             use ieee.std_logic_1164.all;
             Entity Counter is
               port (clk : in std_logic);
             end entity counter;
             architecture RTL of counter is
             begin
             end architecture;",
        );
        assert_eq!(
            scanned.units,
            [
                unit(UnitKind::Entity, "counter"),
                unit(
                    UnitKind::Architecture {
                        entity: "counter".to_string()
                    },
                    "rtl"
                ),
            ]
        );
        // The architecture needs its entity; ieee is filtered out later.
        assert_eq!(
            scanned.references.into_iter().collect::<Vec<_>>(),
            [selected("ieee", "std_logic_1164"), unqualified("counter")]
        );
    }

    #[test]
    fn finds_packages_bodies_and_contexts() {
        let scanned = scan(
            "package uart_pkg is end package;
             package body uart_pkg is end package body uart_pkg;
             context uart_ctx is
               library uart_lib;
               use uart_lib.uart_pkg.all;
             end context;",
        );
        let kinds: Vec<_> = scanned.units.iter().map(|u| (&u.kind, u.name.as_str())).collect();
        assert_eq!(
            kinds,
            [
                (&UnitKind::Package, "uart_pkg"),
                (&UnitKind::PackageBody, "uart_pkg"),
                (&UnitKind::Context, "uart_ctx"),
            ]
        );
        assert_eq!(
            scanned.references.into_iter().collect::<Vec<_>>(),
            [selected("uart_lib", "uart_pkg"), unqualified("uart_pkg")]
        );
    }

    #[test]
    fn package_instantiation_uses_the_generic_package() {
        let scanned = scan(
            "package fifo8 is new fifo_generic generic map (WIDTH => 8);
             package fifo16 is new work.fifo_generic generic map (WIDTH => 16);",
        );
        assert_eq!(scanned.units.len(), 2);
        assert!(scanned.units.iter().all(|u| u.kind == UnitKind::Package));
        assert_eq!(
            scanned.references.into_iter().collect::<Vec<_>>(),
            [selected("work", "fifo_generic"), unqualified("fifo_generic")]
        );
    }

    #[test]
    fn finds_configuration() {
        let scanned = scan(
            "configuration fast_cfg of top_tb is
               for sim
                 for dut : uart use entity work.uart(rtl); end for;
               end for;
             end configuration;",
        );
        assert_eq!(
            scanned.units,
            [unit(
                UnitKind::Configuration {
                    entity: "top_tb".to_string()
                },
                "fast_cfg"
            )]
        );
        assert_eq!(
            scanned.references.into_iter().collect::<Vec<_>>(),
            [selected("work", "uart"), unqualified("top_tb")]
        );
    }

    #[test]
    fn empty_file_has_nothing() {
        assert_eq!(scan(""), ScannedFile::default());
    }

    #[test]
    fn finds_all_kinds_of_instantiation() {
        let found = references(
            "architecture tb of top_tb is
             begin
               u1 : uart_tx port map (clk => clk);
               u2 : uart_rx
                 generic map (BAUD => 9600)
                 port map (clk => clk);
               u3 : component fifo port map (clk);
               u4 : entity work.counter(rtl) port map (clk);
               p1 : process (clk) begin end process;
               b1 : block begin end block;
               g1 : for i in 0 to 3 generate end generate;
             end architecture;",
        );
        assert_eq!(
            found,
            [
                selected("work", "counter"),
                unqualified("fifo"),
                unqualified("top_tb"),
                unqualified("uart_rx"),
                unqualified("uart_tx"),
            ]
        );
    }

    #[test]
    fn instances_know_their_label_and_parent() {
        let found = scan(
            "entity top_tb is end;
             architecture tb of top_tb is begin
               dut : entity work.top(rtl) port map (x => x);
               u_mon : monitor port map (clk);
             end;
             entity top is end;
             architecture rtl of top is begin
               g : for i in 0 to 3 generate
                 u_add : component adder port map (a);
               end generate;
             end;
             package p is end;
             -- not in an architecture: ignored
             x : thing port map (y);",
        );
        let instances: Vec<(&str, &str, Reference)> = found
            .instances
            .iter()
            .map(|i| (i.parent.as_str(), i.label.as_str(), i.target.clone()))
            .collect();
        assert_eq!(
            instances,
            [
                ("top_tb", "dut", selected("work", "top")),
                ("top_tb", "u_mon", unqualified("monitor")),
                ("top", "u_add", unqualified("adder")),
            ]
        );
    }

    #[test]
    fn selected_names_need_a_library_clause() {
        let found = references(
            "library osvvm;
             context osvvm.OsvvmContext;
             use work.helpers.all;
             architecture a of e is
             begin
               x <= rec.field;              -- a record field, not a unit
               y <= work.consts.WIDTH;      -- a direct reference
               z <= osvvm.a.b.c;            -- only the first pair counts
             end;",
        );
        assert_eq!(
            found,
            [
                selected("osvvm", "a"),
                selected("osvvm", "osvvmcontext"),
                selected("work", "consts"),
                selected("work", "helpers"),
                unqualified("e"),
            ]
        );
    }

    #[test]
    fn library_clause_with_several_names() {
        let found = references("library a, b; use a.p1.all; use b.p2.all; use c.p3.all;");
        assert_eq!(found, [selected("a", "p1"), selected("b", "p2")]);
    }

    #[test]
    fn ignores_comments_and_literals() {
        let scanned = scan(
            "-- entity commented_out is
             /* package hidden is
                end package; */
             entity real_one is end;
             architecture a of real_one is
             begin
               report \"entity in_a_string is\";
               s <= x\"FF\" when c = 'a' else (others => '0');
               t <= clk'event;  -- an attribute tick, not a character
             end;",
        );
        let names: Vec<_> = scanned.units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["real_one", "a"]);
    }

    #[test]
    fn still_finds_units_in_a_file_with_syntax_errors() {
        let scanned = scan(
            "entity broken is
               port (a : in bit  -- missing closing bracket and semicolon
             end entity;
             architecture rtl of broken is begin a <= ; end;",
        );
        assert_eq!(scanned.units.len(), 2);
    }

    #[test]
    fn end_lines_are_not_new_units() {
        let scanned = scan(
            "entity e is end entity e;
             package p is end package p;
             package body p is end package body p;",
        );
        assert_eq!(scanned.units.len(), 3);
    }

    #[test]
    fn extended_identifiers_keep_their_case() {
        let scanned = scan("entity \\My Entity\\ is end;");
        assert_eq!(scanned.units[0].name, "\\My Entity\\");
    }

    #[test]
    fn primary_units() {
        assert!(UnitKind::Entity.is_primary());
        assert!(UnitKind::Package.is_primary());
        assert!(UnitKind::Context.is_primary());
        assert!(!UnitKind::PackageBody.is_primary());
        let architecture = UnitKind::Architecture {
            entity: "e".to_string(),
        };
        assert!(!architecture.is_primary());
    }

    #[test]
    fn tokenizer_handles_awkward_literals() {
        let tokens = tokenize("a := 16#FF#; b := 1.5e-3; s := \"say \"\"hi\"\"\"; c := ''';");
        let words: Vec<_> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Word(w) => Some(w.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(words, ["a", "b", "s", "c"]);
    }
}
