extern crate serde;
extern crate serde_any;

use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::vec::Vec;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize, Debug)]
pub struct System {
    pub program: Vec<Program>,

    #[serde(default = "default_terminate_timeout")]
    pub terminate_timeout: f64,
}

#[derive(Deserialize, Debug)]
pub struct Program {
    pub name: String,
    pub argv: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default = "default_cwd")]
    pub cwd: String,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
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

impl System {
    pub fn from_file(filename: &str) -> Result<System> {
        serde_any::from_file(filename)
            .map_err(|e| {
                let e = format!("{:?}", e);
                e.into()
            })
            .and_then(System::validate)
    }

    fn validate(sys: System) -> Result<System> {
        for prog in &sys.program {
            if prog.argv.is_empty() {
                let msg = format!("need at least one argv argument for {:?}", prog.name);
                return Err(msg.into());
            }
        }
        Ok(sys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate tempfile;

    use std::io::{Seek, SeekFrom, Write};
    use tempfile::Builder;

    fn write_file(content: &str) -> tempfile::NamedTempFile {
        let mut file = Builder::new().suffix(".toml").tempfile().unwrap();
        file.as_file_mut().write_all(content.as_bytes()).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file
    }

    #[test]
    fn test_read() {
        let file = write_file(
            r#"
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
        "#,
        );
        let system = System::from_file(file.path().to_str().unwrap()).unwrap();

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
        let file = write_file(
            r#"
            [[program]]
            name = "prog"
            argv = ["abc"]
        "#,
        );

        let system = System::from_file(file.path().to_str().unwrap()).unwrap();

        assert!((system.terminate_timeout - 1.0).abs() < 0.001);

        let prog = &system.program[0];

        assert_eq!(0, prog.env.len());
        assert_eq!(default_cwd(), prog.cwd);
        assert_eq!(true, prog.enabled);
    }

    #[test]
    fn test_fail_if_mandatory_are_absent() {
        let file = write_file(
            r#"
            [[program]]
            argv = ["abc"]
        "#,
        );

        let res = System::from_file(file.path().to_str().unwrap());
        res.unwrap_err();

        let file = write_file(
            r#"
            [[program]]
            name = "prog"
        "#,
        );

        let res = System::from_file(file.path().to_str().unwrap());
        res.unwrap_err();
    }

    #[test]
    fn test_fail_unless_exec_is_given() {
        let file = write_file(
            r#"
            [[program]]
            name = "prog"
            argv = []
        "#,
        );

        let res = System::from_file(file.path().to_str().unwrap());
        res.unwrap_err();
    }
}
