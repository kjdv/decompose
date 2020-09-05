extern crate futures;
extern crate nix;
extern crate regex;
extern crate tokio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Result = std::result::Result<bool, tokio::io::Error>;

pub async fn nothing() -> Result {
    Ok(true)
}

pub async fn manual(name: &str) -> Result {
    let mut stdout = tokio::io::stdout();
    stdout
        .write(format!("Manually waiting for {}, press enter\n", name).as_bytes())
        .await?;
    stdout.flush().await?;

    let mut stdin = tokio::io::stdin();
    let mut buf = [0; 1];
    stdin.read(&mut buf).await?;
    Ok(true)
}

pub async fn timer(dur: std::time::Duration) -> Result {
    tokio::time::delay_for(dur).await;
    Ok(true)
}

pub async fn port(port: u16) -> Result {
    host_and_port("127.0.0.1", port).await
}

async fn host_and_port(host: &str, port: u16) -> Result {
    use tokio::net::TcpStream;

    let interval = std::time::Duration::from_millis(1);
    let address = format!("{}:{}", host, port);

    loop {
        if TcpStream::connect(&address).await.is_ok() {
            return Ok(true);
        }
        tokio::time::delay_for(interval).await;
    }
}

pub async fn completed(proc: tokio::process::Child) -> Result {
    proc.wait_with_output()
        .await
        .map(|output| output.status.success())
}

/*
pub struct Stdout {
    regex: regex::Regex,
    reader: Option<std::io::BufReader<std::fs::File>>,
}

impl Stdout {
    pub fn new(filename: std::path::PathBuf, re: String) -> Result<Stdout> {
        let re = regex::Regex::new(re.as_str())?;
        let file = std::fs::File::open(filename)?;
        let reader = std::io::BufReader::new(file);

        Ok(Stdout {
            regex: re,
            reader: Some(reader),
        })
    }
}

impl ReadySignal for Stdout {
    fn poll(&mut self) -> Result<bool> {
        if let Some(mut reader) = self.reader.take() {
            let mut line = String::new();
            reader.read_line(&mut line)?;

            let rn: &[_] = &['\r', '\n'];
            let line = line.trim_end_matches(rn);

            if !self.regex.is_match(line) {
                self.reader.replace(reader);
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub struct Completed {
    pid: nix::unistd::Pid,
    ready: bool,
}

impl Completed {
    pub fn new(pid: u32) -> Completed {
        Completed {
            pid: nix::unistd::Pid::from_raw(pid as i32),
            ready: false,
        }
    }
}

impl ReadySignal for Completed {
    fn poll(&mut self) -> Result<bool> {
        use nix::sys::wait;

        if self.ready {
            return Ok(true);
        }

        let status = wait::waitpid(self.pid, None)?;
        match status {
            wait::WaitStatus::Exited(_, 0) => {
                self.ready = true;
                Ok(true)
            }
            wait::WaitStatus::Exited(_, n) => {
                self.ready = true;
                let e = format!("non-zero exit status {}", n);
                Err(string_error::new_err(e.as_str()))
            }
            _ => Ok(false),
        }
    }
}

*/

#[cfg(test)]
mod tests {
    extern crate tokio;

    use super::*;

    #[tokio::test]
    async fn test_nothing() {
        let result = nothing().await.expect("nothing");
        assert!(result);
    }

    #[tokio::test]
    async fn test_port() {
        // cheating on unit test rules: is opening a port okay?
        let _listener = std::net::TcpListener::bind("127.0.0.1:9092").expect("open 9292");

        let result = port(9092).await.expect("port");
        assert!(result);
    }

    /*

    #[test]
    fn stdout() {
        let tempdir = tempfile::Builder::new().tempdir().expect("tempir");
        let filename = "test.file";

        let mut buf = tempdir.path().to_path_buf();
        buf.push(filename);
        let mut f = std::fs::File::create(buf).expect("open for write");

        let mut buf = tempdir.path().to_path_buf();
        buf.push(filename);
        let mut rs = Stdout::new(buf, "^ready [0-9]+$".to_string()).expect("valid regex");

        assert_is_false(&mut rs);

        f.write_all(b"something\n").unwrap();
        f.flush().unwrap();

        assert_is_false(&mut rs);

        f.write_all(b"ready 123\n").unwrap();
        f.flush().unwrap();

        assert_is_true(&mut rs);

        f.write_all(b"more stuff\n").unwrap();
        f.flush().unwrap();

        assert_is_true(&mut rs);
    }
    */

    #[tokio::test]
    async fn test_completed() {
        let proc = tokio::process::Command::new("/bin/ls")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("/bin/ls");

        let result = completed(proc).await.expect("completed");
        assert!(result);
    }

    #[tokio::test]
    async fn completed_failing_process() {
        let proc = tokio::process::Command::new("/bin/ls")
            .arg("no such file or directory")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("/bin/ls");

        let result = completed(proc).await.expect("completed");
        assert!(!result);
    }
}
