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

impl Drop for OutputFileFactory {
    fn drop(&mut self) {

    }
}
