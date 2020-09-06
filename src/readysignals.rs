extern crate futures;
extern crate nix;
extern crate regex;
extern crate tokio;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

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

pub async fn output<R: AsyncRead + std::marker::Unpin>(reader: R, re: String) -> Result {
    use tokio::io;
    use tokio::io::AsyncBufReadExt;

    let re = regex::Regex::new(re.as_str())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{}", e)))?;

    let mut reader = io::BufReader::new(reader).lines();

    while let Some(line) = reader.next_line().await? {
        if re.is_match(line.as_str()) {
            return Ok(true);
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    extern crate tokio;

    use super::*;
    use futures::task::Poll;

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

    struct StringReader {
        cursor: std::io::Cursor<String>,
    }

    impl StringReader {
        fn new(buf: String) -> StringReader {
            StringReader {
                cursor: std::io::Cursor::new(buf),
            }
        }
    }

    impl AsyncRead for StringReader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut futures::task::Context,
            mut buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            let r = std::io::Read::read(&mut self.cursor, &mut buf);
            Poll::Ready(r)
        }
    }

    #[tokio::test]
    async fn test_output_good() {
        let reader = StringReader::new("aap\nprogram:123 running\nnoot\n".to_string());

        let result = output(reader, "^program:[0-9]+.*$".to_string())
            .await
            .expect("re");
        assert!(result);
    }

    #[tokio::test]
    async fn test_output_bad() {
        let reader = StringReader::new("aap\nnoot\nmies".to_string());

        let result = output(reader, "^program:[0-9]+.*$".to_string())
            .await
            .expect("re");
        assert!(!result);
    }

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
