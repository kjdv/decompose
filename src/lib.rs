extern crate serde;
extern crate serde_any;

use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::vec::Vec;

#[derive(Deserialize, Debug)]
pub struct System {
    pub program: Vec<Program>
}

#[derive(Deserialize, Debug)]
pub struct Program {
    pub name: String,
    pub argv: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: String,
    pub enabled: bool,
}

impl System {
    pub fn from_file(filename: &str) -> Result<System, Box<dyn Error>> {
        serde_any::from_file(filename).map_err(|e| {
            let e = format!("{:?}", e);
            e.into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate tempfile;

    use std::io::{Read, Seek, SeekFrom, Write};
    use tempfile::Builder;

    fn write_file(content: &str) -> tempfile::NamedTempFile {
        let mut file = Builder::new()
            .suffix(".toml")
            .tempfile()
            .unwrap();
        file.as_file_mut()
            .write_all(content.as_bytes()).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file
    }

    #[test]
    fn test_read() {
        let file = write_file(r#"
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

        let mut contents = String::new();
        file.as_file().read_to_string(&mut contents).unwrap();

        let system = System::from_file(file.path().to_str().unwrap()).unwrap();

        let prog1 = &system.program[0];

        assert_eq!(vec!["abc", "def"], prog1.argv);
        assert_eq!("jkl", prog1.env.get("ghi").unwrap());
        assert_eq!("pqr", prog1.env.get("mno").unwrap());
        assert_eq!("/tmp", prog1.cwd);
        assert_eq!(true, prog1.enabled);

        let prog2 = &system.program[1];

        assert_eq!(vec!["exec"], prog2.argv);
        assert_eq!(0, prog2.env.len());
        assert_eq!(".", prog2.cwd);
        assert_eq!(false, prog2.enabled);
    }
}
