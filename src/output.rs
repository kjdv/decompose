extern crate chrono;
use std::path::PathBuf;
use std::fs;

use super::*;

type Result<T> = std::result::Result<T, Box<dyn Error>>;


pub struct OutputFileFactory {
    outdir: PathBuf,
}

impl OutputFileFactory {
    pub fn new(outdir_root: &str) -> Result<OutputFileFactory> {
        let mut outdir_root_buf = PathBuf::new();
        outdir_root_buf.push(outdir_root);

        let now = chrono::Local::now();
        let dirname = format!("{}.{}", now.format("%Y-%m-%dT%H:%M:%S"), std::process::id());

        let mut outdir = outdir_root_buf.clone();
        outdir.push(dirname);

        fs::create_dir_all(&outdir)?;
    
        let mut latest = outdir_root_buf.clone();
        latest.push("latest");

        let _ = fs::remove_file(&latest);
        std::os::unix::fs::symlink(&outdir, latest)?;

        Ok(OutputFileFactory{outdir})
    }

    pub fn open(&self, filename: &str) -> Result<fs::File> {
        let mut path = self.outdir.clone();
        path.push(filename);
        let f = fs::File::create(path)?;
        Ok(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate tempfile;

    use std::io::{Read, Write};
    use tempfile::Builder;
    use std::path;

    fn root() -> tempfile::TempDir {
        Builder::new()
            .tempdir()
            .unwrap()
    }

    #[test]
    fn creates_dirs() {
        let r = root();
        let _ = OutputFileFactory::new(&r.path().to_str().unwrap());

        let mut latest = r.into_path().to_path_buf();
        latest.push("latest");
        assert!(latest.is_dir());

        let symlink = fs::read_link(&latest).unwrap();
        let symlink = symlink.as_path();
        assert_eq!(latest.parent().unwrap(), symlink.parent().unwrap());

        let re = regex::Regex::new(r#"[0-9]+-[0-9]+-[0-9]+T[0-9]+:[0-9]+:[0-9]+\.[0-9]+"#)
            .unwrap();
        assert!(re.is_match(
            symlink.to_path_buf().file_name().unwrap().to_str().unwrap()
        ));
    }

    fn writes_content() {
        let r = root();
        let output = OutputFileFactory::new(&r.path().to_str().unwrap()).unwrap();

        {
            let mut f = output.open("test").unwrap();
            f.write_all(b"hello!\n").unwrap();
        }

        let mut p = r.into_path().to_path_buf();
        p.push("latest");
        p.push("test");

        let mut f = fs::File::open(p).unwrap();
        let mut buf = String::new();
        f.read_to_string(&mut buf).unwrap();

        assert_eq!("hello\n!", buf.as_str());
    }
}