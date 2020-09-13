extern crate chrono;
extern crate tokio;

use super::config;
use super::executor::ProcessInfo;
use log;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

pub struct LogItem {
    _info: Arc<ProcessInfo>,
    line: String,
}

type Sender = Option<mpsc::Sender<LogItem>>;
type Receiver = Option<mpsc::Receiver<LogItem>>;

pub struct Output {
    pub cfg: Stdio,
    pub tx: Sender,
}

pub trait OutputFactory {
    fn stdout(&self, prog: &config::Program) -> Output;
    fn stderr(&self, prog: &config::Program) -> Output {
        self.stdout(prog)
    }
}

pub async fn consume<W>(rx: Receiver, mut writer: W)
where
    W: AsyncWrite + std::marker::Unpin,
{
    use tokio::io::AsyncWriteExt;

    if let Some(mut rx) = rx {
        while let Some(item) = rx.recv().await {
            if let Err(e) = writer.write(item.line.as_bytes()).await {
                log::error!("{}", e);
                return;
            }
        }
    }
}

pub async fn produce<R>(tx: Sender, reader: Option<R>, info: &ProcessInfo)
where
    R: AsyncRead + std::marker::Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let h = tx.and_then(|tx| reader.map(|reader| (tx, reader)));

    if let Some((mut tx, reader)) = h {
        let info = Arc::new(info.clone());
        let mut reader = tokio::io::BufReader::new(reader);

        let mut buf = String::new();
        while let Ok(true) = reader
            .read_line(&mut buf)
            .await
            .map_err(|e| {
                log::error!("{}", e);
                e
            })
            .map(|s| s > 0)
        {
            let item = LogItem {
                _info: info.clone(),
                line: buf.clone(),
            };
            if let Err(e) = tx.send(item).await {
                log::error!("{}", e);
                return;
            }
        }
    }
}

pub struct NullOutputFactory();

impl OutputFactory for NullOutputFactory {
    fn stdout(&self, _: &config::Program) -> Output {
        Output {
            cfg: Stdio::null(),
            tx: None,
        }
    }
}

pub struct InheritOutputFactory();

impl OutputFactory for InheritOutputFactory {
    fn stdout(&self, _: &config::Program) -> Output {
        Output {
            cfg: Stdio::inherit(),
            tx: None,
        }
    }
}

pub struct OutputFileFactory {
    outdir: PathBuf,
}

impl OutputFileFactory {
    pub fn new(outdir_root: &str) -> std::result::Result<OutputFileFactory, std::io::Error> {
        let mut outdir_root_buf = PathBuf::new();
        outdir_root_buf.push(outdir_root);

        let now = chrono::Local::now();
        let dirname = format!("{}.{}", now.format("%Y-%m-%dT%H:%M:%S"), std::process::id());

        let mut outdir = outdir_root_buf.clone();
        outdir.push(dirname);

        std::fs::create_dir_all(&outdir)?;

        let mut latest = outdir_root_buf;
        latest.push("latest");

        if let Err(e) = std::fs::remove_file(&latest) {
            log::debug!("can't remove {:?}: {:?}", latest, e);
        }
        std::os::unix::fs::symlink(&outdir, latest)?;

        Ok(OutputFileFactory { outdir })
    }
}

impl OutputFactory for OutputFileFactory {
    fn stdout(&self, prog: &config::Program) -> Output {
        let path = self.outdir.clone();
        let name = prog.name.clone();
        let (tx, rx) = mpsc::channel(10);

        tokio::spawn(async move {
            match open(path, name.as_str()).await {
                Ok((file, path)) => {
                    log::debug!("opend log file {:?} for {}", path, name);

                    consume(Some(rx), file).await;
                    log::debug!("closing log file {:?} for {}", path, name);
                }
                Err(e) => {
                    log::error!("{}", e);
                }
            }
        });

        Output {
            cfg: Stdio::piped(),
            tx: Some(tx),
        }
    }
}

async fn open(
    mut path: PathBuf,
    filename: &str,
) -> tokio::io::Result<(tokio::fs::File, std::path::PathBuf)> {
    path.push(filename);
    let p = path.clone();
    let f = tokio::fs::File::create(path).await?;
    Ok((f, p))
}

#[cfg(test)]
mod tests {
    use super::super::tokio_utils;
    use super::*;
    use tokio_utils::tests::StringReader;
    extern crate tempfile;

    use std::io::Read;
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

        let symlink = std::fs::read_link(&latest).unwrap();
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

    fn produce_data<F: OutputFactory>(data: String, output: F) {
        let cfg = r#"
            [[program]]
            name = "blah"
            argv = ["blah"]
            "#;

        let sys = config::System::from_toml(cfg).expect("sys");
        let prog = sys.program[0].clone();

        tokio_utils::run(async move {
            let info = Arc::new(ProcessInfo {
                name: "blah".to_string(),
                pid: 123,
            });

            let reader = StringReader::new(data);
            let output = output.stdout(&prog);

            produce(output.tx, Some(reader), &info).await;

            // todo: why is this needed?
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await
        });
    }

    #[test]
    fn writes_content() {
        let r = root();
        let output = OutputFileFactory::new(&r.path().to_str().unwrap()).unwrap();

        produce_data("hello!\n".to_string(), output);

        let mut p = r.into_path();
        p.push("latest");
        p.push("blah");

        let mut f = std::fs::File::open(p).unwrap();
        let mut buf = String::new();
        f.read_to_string(&mut buf).unwrap();

        assert_eq!("hello!\n", buf.as_str());
    }
}
