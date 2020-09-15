extern crate chrono;
extern crate tokio;

use super::config;
use log;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::broadcast;

pub type Sender = broadcast::Sender<String>;
pub type Receiver = broadcast::Receiver<String>;

pub trait OutputFactory {
    fn stdout(&self, prog: &config::Program) -> Sender;
    fn stderr(&self, prog: &config::Program) -> Sender {
        self.stdout(prog)
    }
}

fn make_channel() -> (Sender, Receiver) {
    broadcast::channel(16)
}

pub async fn consume<W, F>(mut rx: Receiver, mut writer: W, formatter: F)
where
    W: AsyncWrite + std::marker::Unpin,
    F: Fn(String) -> String,
{
    use tokio::io::AsyncWriteExt;

    while let Ok(line) = rx.recv().await.map_err(|e| {
        log::debug!("{}", e);
        e
    }) {
        let line = formatter(line);
        if let Err(e) = writer.write(line.as_bytes()).await {
            log::error!("{}", e);
            return;
        }
    }
}

pub async fn produce<R>(tx: Sender, reader: Option<R>)
where
    R: AsyncRead + std::marker::Unpin,
{
    use tokio::io::AsyncBufReadExt;

    if let Some(reader) = reader {
        let mut reader = tokio::io::BufReader::new(reader).lines();

        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| {
                log::error!("{}", e);
                e
            })
            .ok()
            .flatten()
        {
            if let Err(e) = tx.send(line) {
                log::debug!("{:?}", e);
            }
        }
    }
}

pub struct NullOutputFactory();

impl OutputFactory for NullOutputFactory {
    fn stdout(&self, _: &config::Program) -> Sender {
        let (tx, rx) = make_channel();

        tokio::spawn(consume(rx, tokio::io::sink(), |s| s));
        tx
    }
}

pub struct InheritOutputFactory();

impl InheritOutputFactory {
    fn formatter(&self, prog: &config::Program) -> impl Fn(String) -> String {
        let tag = prog.name.clone();
        move |s| format!("[{}] => {}\n", tag.clone(), s)
    }
}

impl OutputFactory for InheritOutputFactory {
    fn stdout(&self, prog: &config::Program) -> Sender {
        let (tx, rx) = make_channel();
        let fmt = self.formatter(prog);

        tokio::spawn(consume(rx, tokio::io::stdout(), fmt));
        tx
    }

    fn stderr(&self, prog: &config::Program) -> Sender {
        let (tx, rx) = make_channel();
        let fmt = self.formatter(prog);

        tokio::spawn(consume(rx, tokio::io::stderr(), fmt));
        tx
    }
}

pub struct OutputFileFactory {
    outdir: PathBuf,
}

impl OutputFileFactory {
    pub fn new(outdir_root: &Path) -> std::result::Result<OutputFileFactory, std::io::Error> {
        let outdir_root_buf = outdir_root.to_path_buf();

        let now = chrono::Local::now();
        let dirname = format!("{}.{}", now.format("%Y-%m-%dT%H:%M:%S"), std::process::id());

        let mut outdir = outdir_root_buf.clone();
        outdir.push(dirname.clone());

        std::fs::create_dir_all(&outdir)?;

        let _guard = ChdirGuard::new(outdir_root_buf.as_path())?;

        if let Err(e) = std::fs::remove_file("latest") {
            log::debug!("can't remove latest: {:?}", e);
        }
        std::os::unix::fs::symlink(dirname, "latest")?;

        Ok(OutputFileFactory { outdir })
    }

    fn stream(&self, name: String) -> Sender {
        let path = self.outdir.clone();
        let (tx, rx) = make_channel();

        tokio::spawn(async move {
            match open(path, name.as_str()).await {
                Ok((file, path)) => {
                    log::debug!("opend log file {:?} for {}", path, name);

                    consume(rx, file, |s| format!("{}\n", s)).await;
                    log::debug!("closing log file {:?} for {}", path, name);
                }
                Err(e) => {
                    log::error!("{}", e);
                }
            }
        });
        tx
    }
}

impl OutputFactory for OutputFileFactory {
    fn stdout(&self, prog: &config::Program) -> Sender {
        self.stream(format!("{}.out", prog.name))
    }

    fn stderr(&self, prog: &config::Program) -> Sender {
        self.stream(format!("{}.err", prog.name))
    }
}

async fn open(mut path: PathBuf, filename: &str) -> tokio::io::Result<(tokio::fs::File, PathBuf)> {
    path.push(filename);
    let p = path.clone();
    let f = tokio::fs::File::create(path).await?;
    Ok((f, p))
}

struct ChdirGuard {
    orig: PathBuf,
}

impl ChdirGuard {
    fn new(path: &Path) -> std::io::Result<ChdirGuard> {
        let orig = std::env::current_dir()?;
        std::env::set_current_dir(path)?;
        Ok(ChdirGuard { orig })
    }
}

impl Drop for ChdirGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(self.orig.as_path()).expect("set current dir");
    }
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
        let _ = OutputFileFactory::new(Path::new(r.path().to_str().unwrap()));

        let mut latest = r.into_path();
        latest.push("latest");
        assert!(latest.is_dir());

        let symlink = std::fs::read_link(&latest).unwrap();
        let symlink = symlink.as_path();
        assert_eq!("", symlink.parent().unwrap().as_os_str());

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

    fn make_prog(name: &str) -> config::Program {
        let cfg = format!(
            "
            [[program]]
            name = \"{}\"
            argv = [\"blah\"]
            ",
            name
        );

        let sys = config::System::from_toml(cfg.as_str()).expect("sys");
        sys.program[0].clone()
    }

    fn produce_data<F: OutputFactory>(data: String, output: F) {
        let prog = make_prog("blah");

        tokio_utils::run(async move {
            let reader = StringReader::new(data);
            let output = output.stdout(&prog);

            produce(output, Some(reader)).await;

            // todo: why is this needed?
            tokio::time::delay_for(std::time::Duration::from_millis(100)).await
        });
    }

    #[test]
    fn writes_content() {
        let r = root();
        let output = OutputFileFactory::new(r.path()).expect("output factory");

        produce_data("hello!\n".to_string(), output);

        let mut p = r.into_path();
        p.push("latest");
        p.push("blah.out");

        let mut f = std::fs::File::open(p).unwrap();
        let mut buf = String::new();
        f.read_to_string(&mut buf).unwrap();

        assert_eq!("hello!\n", buf.as_str());
    }

    #[tokio::test]
    async fn test_produce() {
        let reader = StringReader::new("aap\nnoot\nmies\n".to_string());
        let (tx, mut rx) = make_channel();

        tokio::spawn(produce(tx, Some(reader)));

        assert_eq!("aap", rx.recv().await.unwrap());
        assert_eq!("noot", rx.recv().await.unwrap());
        assert_eq!("mies", rx.recv().await.unwrap());
        assert!(rx.recv().await.is_err());
    }

    #[tokio::test]
    async fn test_produce_does_nothing_on_empty_reader() {
        let reader: Option<StringReader> = None;
        let (tx, mut rx) = make_channel();

        tokio::spawn(produce(tx, reader));

        assert!(rx.recv().await.is_err());
    }
}
