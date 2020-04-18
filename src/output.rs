extern crate chrono;
use std::fs;
use std::path::PathBuf;

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

        let mut latest = outdir_root_buf;
        latest.push("latest");

        let _ = fs::remove_file(&latest);
        std::os::unix::fs::symlink(&outdir, latest)?;

        Ok(OutputFileFactory { outdir })
    }

    pub fn open(&self, filename: &str) -> Result<(fs::File, std::path::PathBuf)> {
        let mut path = self.outdir.clone();
        path.push(filename);
        let p = path.clone();
        let f = fs::File::create(path)?;
        Ok((f, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate tempfile;

    use std::io::{Read, Write};
    use tempfile::Builder;

    fn root() -> tempfile::TempDir {
        Builder::new().tempdir().unwrap()
    }

    #[test]
    fn creates_dirs() {
        let r = root();
        let _ = OutputFileFactory::new(&r.path().to_str().unwrap());

        let mut latest = r.into_path();
        latest.push("latest");
        assert!(latest.is_dir());

        let symlink = fs::read_link(&latest).unwrap();
        let symlink = symlink.as_path();
        assert_eq!(latest.parent().unwrap(), symlink.parent().unwrap());

        let re =
            regex::Regex::new(r#"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.([0-9]+)"#)
                .unwrap();

        // below... rust gets crazy
        let pid = symlink
            .to_path_buf()
            .file_name()
            .and_then(|f| f.to_str())
            .and_then(|f| re.captures(f))
            .and_then(|cs| cs.get(1))
            .map(|p| p.as_str())
            .and_then(|p| p.parse::<u32>().ok())
            .unwrap();

        assert_eq!(std::process::id(), pid);
    }

    #[test]
    fn writes_content() {
        let r = root();
        let output = OutputFileFactory::new(&r.path().to_str().unwrap()).unwrap();

        {
            let mut f = output.open("test").unwrap();
            f.0.write_all(b"hello!\n").unwrap();
        }

        let mut p = r.into_path();
        p.push("latest");
        p.push("test");

        let mut f = fs::File::open(p).unwrap();
        let mut buf = String::new();
        f.read_to_string(&mut buf).unwrap();

        assert_eq!("hello!\n", buf.as_str());
    }

    #[test]
    fn open_returns_path_() {
        let r = root();
        let output = OutputFileFactory::new(&r.path().to_str().unwrap()).unwrap();

        let (_, path) = output.open("test").unwrap();

        let mut expect = output.outdir;
        expect.push("test");

        assert_eq!(expect, path);
    }
}
