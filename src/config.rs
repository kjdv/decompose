extern crate serde;
extern crate serde_any;

use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::vec::Vec;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize, Debug)]
pub struct System {
    pub program: Vec<Program>,

    #[serde(default = "default_terminate_timeout")]
    pub terminate_timeout: f64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Program {
    pub name: String,
    pub argv: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default = "default_cwd")]
    pub cwd: String,

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_ready_signal")]
    pub ready: ReadySignal,

    #[serde(default = "default_depends")]
    pub depends: Vec<String>,
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ReadySignal {
    Nothing,
    Manual,
    Timer(f64),
    Port(u16),
    Stdout(String),
    Completed,
}

fn default_cwd() -> String {
    let cwd = std::env::current_dir().unwrap();
    let cwd = cwd.into_os_string();
    cwd.into_string().unwrap()
}

fn default_enabled() -> bool {
    true
}

fn default_terminate_timeout() -> f64 {
    1.0
}

fn default_ready_signal() -> ReadySignal {
    ReadySignal::Nothing
}

fn default_depends() -> Vec<String> {
    Vec::new()
}

impl System {
    pub fn from_file(filename: &str) -> Result<System> {
        let s = serde_any::from_file(filename);
        System::validate(s)
    }

    #[allow(dead_code)] // surpress false warning, used in tests
    pub fn from_toml(toml: &str) -> Result<System> {
        let s = serde_any::from_str(toml, serde_any::Format::Toml);
        System::validate(s)
    }

    fn validate(sys: std::result::Result<System, serde_any::Error>) -> Result<System> {
        if let Err(e) = sys {
            let e = format!("{:?}", e);
            return Err(e.into());
        }
        let sys = sys.unwrap();

        let mut found_starting_point = false;
        let mut names = HashSet::new();
        for prog in &sys.program {
            if prog.argv.is_empty() {
                let msg = format!("need at least one argv argument for {:?}", prog.name);
                return Err(msg.into());
            }
            if prog.depends.is_empty() {
                found_starting_point = true;
            }
            if !names.insert(prog.name.clone()) {
                let msg = format!("duplicate program name {:?}", prog.name);
                return Err(msg.into());
            }
        }

        if !found_starting_point {
            return Err(string_error::new_err(
                "No valid entry point (with empty dependency list) found",
            ));
        }

        Ok(sys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read() {
        let toml = r#"
            terminate_timeout = 0.5

            [[program]]
            name = "prog1"
            argv = ["abc", "def"]
            env = {ghi = "jkl", mno = "pqr"}
            cwd = "/tmp"
            enabled = true

            [[program]]
            name = "prog2"
            argv = ["exec"]
            env = {}
            cwd = "."
            enabled = false
        "#;

        let system = System::from_toml(toml).unwrap();

        assert!((system.terminate_timeout - 0.5).abs() < 0.001);

        let prog1 = &system.program[0];

        assert_eq!("prog1", prog1.name);
        assert_eq!(vec!["abc", "def"], prog1.argv);
        assert_eq!("jkl", prog1.env.get("ghi").unwrap());
        assert_eq!("pqr", prog1.env.get("mno").unwrap());
        assert_eq!("/tmp", prog1.cwd);
        assert_eq!(true, prog1.enabled);

        let prog2 = &system.program[1];

        assert_eq!("prog2", prog2.name);
        assert_eq!(vec!["exec"], prog2.argv);
        assert_eq!(0, prog2.env.len());
        assert_eq!(".", prog2.cwd);
        assert_eq!(false, prog2.enabled);
    }

    #[test]
    fn test_optional_values_give_defaults() {
        let toml = r#"
            [[program]]
            name = "prog"
            argv = ["abc"]
        "#;

        let system = System::from_toml(toml).unwrap();

        assert!((system.terminate_timeout - 1.0).abs() < 0.001);

        let prog = &system.program[0];

        assert_eq!(0, prog.env.len());
        assert_eq!(default_cwd(), prog.cwd);
        assert_eq!(true, prog.enabled);
        assert_eq!(ReadySignal::Nothing, prog.ready);
    }

    #[test]
    fn test_fail_if_mandatory_are_absent() {
        let toml = r#"
            [[program]]
            argv = ["abc"]
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();

        let toml = r#"
            [[program]]
            name = "prog"
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_fail_unless_exec_is_given() {
        let toml = r#"
            [[program]]
            name = "prog"
            argv = []
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_fail_unless_there_is_a_starting_point() {
        let toml = r#"
            [[program]]
            name = "prog"
            argv = ["foo"]
            depends = ["prog"]
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_fail_on_duplicate_names() {
        let toml = r#"
            [[program]]
            name = "prog"
            argv = ["foo"]

            [[program]]
            name = "prog"
            argv = ["foo"]
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_ready_signals() {
        let toml = r#"
            [[program]]
            name = "default"
            argv = ["foo"]

            [[program]]
            name = "port"
            argv = ["foo"]
            ready = {port = 123}

            [[program]]
            name = "nothing"
            argv = ["foo"]
            ready = {nothing={}}

            [[program]]
            name = "manual"
            argv = ["foo"]
            ready = {manual={}}

            [[program]]
            name = "timer"
            argv = ["foo"]
            ready = {timer=0.5}

            [[program]]
            name = "stdout"
            argv = ["foo"]
            ready = {stdout="^ready$"}

            [[program]]
            name = "completed"
            argv = ["foo"]
            ready = {completed={}}
            "#;

        let res = System::from_toml(toml).unwrap();

        assert_eq!(ReadySignal::Nothing, res.program[0].ready);
        assert_eq!(ReadySignal::Port(123), res.program[1].ready);
        assert_eq!(ReadySignal::Nothing, res.program[2].ready);
        assert_eq!(ReadySignal::Manual, res.program[3].ready);
        assert_eq!(ReadySignal::Timer(0.5), res.program[4].ready);
        assert_eq!(
            ReadySignal::Stdout("^ready$".to_string()),
            res.program[5].ready
        );
        assert_eq!(ReadySignal::Completed, res.program[6].ready);
    }

    #[test]
    fn test_depends() {
        let toml = r#"
            [[program]]
            name = "default"
            argv = ["foo"]

            [[program]]
            name = "port"
            argv = ["foo"]
            depends = ["default"]
            "#;

        let res = System::from_toml(toml).unwrap();

        assert!(res.program[0].depends.is_empty());
        assert_eq!(vec!["default"], res.program[1].depends);
    }
}
