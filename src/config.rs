extern crate serde;
extern crate serde_any;
extern crate shellexpand;

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

    #[serde(default = "default_start_timeout")]
    pub start_timeout: Option<f64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Program {
    pub name: String,
    pub exec: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default = "default_cwd")]
    pub cwd: String,

    #[serde(default = "default_ready_signal")]
    pub ready: ReadySignal,

    #[serde(default = "default_depends")]
    pub depends: Vec<String>,

    #[serde(default)]
    pub critical: bool,

    #[serde(default)]
    pub disabled: bool,
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ReadySignal {
    Nothing,
    Manual,
    Timer(f64),
    Port(u16),
    Stdout(String),
    Stderr(String),
    Completed,
    Healthcheck(Endpoint),
}

#[derive(Deserialize, Debug, PartialEq, Clone)]
pub struct Endpoint {
    pub port: u16,
    pub path: String,
    #[serde(default = "localhost")]
    pub host: String,
}

fn default_cwd() -> String {
    let cwd = std::env::current_dir().unwrap();
    let cwd = cwd.into_os_string();
    cwd.into_string().unwrap()
}

fn default_terminate_timeout() -> f64 {
    1.0
}

fn default_start_timeout() -> Option<f64> {
    None
}

fn default_ready_signal() -> ReadySignal {
    ReadySignal::Nothing
}

fn default_depends() -> Vec<String> {
    Vec::new()
}

fn localhost() -> String {
    "127.0.0.1".to_string()
}

impl System {
    pub fn from_file(filename: &str) -> Result<System> {
        let format = serde_any::guess_format(filename);
        let raw_data = std::fs::read_to_string(filename)?;
        Self::from_str(raw_data.as_str(), format)
    }

    #[allow(dead_code)] // surpress false warning, used in tests
    pub fn from_toml(toml: &str) -> Result<System> {
        Self::from_str(toml, Some(serde_any::Format::Toml))
    }

    fn from_str(raw_data: &str, format: Option<serde_any::Format>) -> Result<System> {
        let expanded = shellexpand::env(raw_data)?;
        let s = match format {
            Some(format) => serde_any::from_str(&expanded, format),
            None => serde_any::from_str_any(&expanded),
        };
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
            start_timeout = 10.2
            terminate_timeout = 0.5

            [[program]]
            name = "prog1"
            exec = "abc"
            args = ["def"]
            env = {ghi = "jkl", mno = "pqr"}
            cwd = "/tmp"
       
            [[program]]
            name = "prog2"
            exec = "exec"
            env = {}
            cwd = "."
            critical = true
            disabled = true
        "#;

        let system = System::from_toml(toml).unwrap();

        assert!((system.terminate_timeout - 0.5).abs() < 0.001);
        assert!((system.start_timeout.unwrap() - 10.2).abs() < 0.001);

        let prog1 = &system.program[0];

        assert_eq!("prog1", prog1.name);
        assert_eq!("abc", prog1.exec);
        assert_eq!(vec!["def"], prog1.args);
        assert_eq!("jkl", prog1.env.get("ghi").unwrap());
        assert_eq!("pqr", prog1.env.get("mno").unwrap());
        assert_eq!("/tmp", prog1.cwd);
        assert_eq!(false, prog1.critical);
        assert_eq!(false, prog1.disabled);

        let prog2 = &system.program[1];

        assert_eq!("prog2", prog2.name);
        assert_eq!("exec", prog2.exec);
        assert!(prog2.args.is_empty());
        assert_eq!(0, prog2.env.len());
        assert_eq!(".", prog2.cwd);
        assert_eq!(true, prog2.critical);
        assert_eq!(true, prog2.disabled);
    }

    #[test]
    fn test_optional_values_give_defaults() {
        let toml = r#"
            [[program]]
            name = "prog"
            exec = "abc"
        "#;

        let system = System::from_toml(toml).unwrap();

        assert!((system.terminate_timeout - 1.0).abs() < 0.001);
        assert_eq!(None, system.start_timeout);

        let prog = &system.program[0];

        assert_eq!(0, prog.env.len());
        assert_eq!(default_cwd(), prog.cwd);
        assert_eq!(ReadySignal::Nothing, prog.ready);
    }

    #[test]
    fn test_fail_if_mandatory_are_absent() {
        let toml = r#"
            [[program]]
            exec = "abc"
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
            args = []
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_fail_unless_there_is_a_starting_point() {
        let toml = r#"
            [[program]]
            name = "prog"
            exec = "foo"
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
            exec = "foo"

            [[program]]
            name = "prog"
            exec = "foo"
        "#;

        let res = System::from_toml(toml);
        res.unwrap_err();
    }

    #[test]
    fn test_ready_signals() {
        let toml = r#"
            [[program]]
            name = "default"
            exec = "foo"

            [[program]]
            name = "port"
            exec = "foo"
            ready = {port = 123}

            [[program]]
            name = "nothing"
            exec = "foo"
            ready = {nothing={}}

            [[program]]
            name = "manual"
            exec = "foo"
            ready = {manual={}}

            [[program]]
            name = "timer"
            exec = "foo"
            ready = {timer=0.5}

            [[program]]
            name = "stdout"
            exec = "foo"
            ready = {stdout="^ready$"}

            [[program]]
            name = "stderr"
            exec = "foo"
            ready = {stderr="^ready$"}

            [[program]]
            name = "completed"
            exec = "foo"
            ready = {completed={}}

            [[program]]
            name = "healthcheck"
            exec = "foo"
            ready = {healthcheck={port=123, path="/health", host="localhost"}}
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
        assert_eq!(
            ReadySignal::Stderr("^ready$".to_string()),
            res.program[6].ready
        );
        assert_eq!(ReadySignal::Completed, res.program[7].ready);

        assert_eq!(
            ReadySignal::Healthcheck(Endpoint {
                port: 123,
                path: "/health".to_string(),
                host: "localhost".to_string()
            }),
            res.program[8].ready
        );
    }

    #[test]
    fn test_depends() {
        let toml = r#"
            [[program]]
            name = "default"
            exec = "foo"

            [[program]]
            name = "port"
            exec = "foo"
            depends = ["default"]
            "#;

        let res = System::from_toml(toml).unwrap();

        assert!(res.program[0].depends.is_empty());
        assert_eq!(vec!["default"], res.program[1].depends);
    }

    #[test]
    fn test_env_vars_are_expanded() {
        use std::env::set_var;

        set_var("TEST_NAME", "testingtesting");
        set_var("TEST_FOO", "bar");

        let toml = r#"
            [[program]]
            name = "$TEST_NAME"
            exec = "${TEST_FOO}"
            args = ["${TEST_CWD:-here}"]
        "#;
        let sys = System::from_toml(toml).unwrap();

        assert_eq!(sys.program[0].name, "testingtesting");
        assert_eq!(sys.program[0].exec, "bar");
        assert_eq!(sys.program[0].args[0], "here");
    }
}
