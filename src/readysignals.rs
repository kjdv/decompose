extern crate futures;
extern crate log;
extern crate nix;
extern crate regex;
extern crate reqwest;
extern crate tokio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::output::Receiver;
use super::tokio_utils::make_err;

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

pub async fn output(mut rx: Receiver, re: &str) -> Result {
    let re = regex::Regex::new(re).map_err(make_err)?;

    loop {
        match rx.recv().await {
            Err(tokio::sync::broadcast::RecvError::Closed) => return Ok(false),
            Err(e) => return Err(make_err(e)),
            Ok(line) => {
                let rn: &[_] = &['\r', '\n'];
                let line = line.trim_end_matches(rn);

                if re.is_match(line) {
                    println!("match");
                    return Ok(true);
                }
            }
        }
    }
}

pub async fn healthcheck(endpoint: &str) -> Result {
    let interval = std::time::Duration::from_millis(1);
    loop {
        let response = reqwest::get(endpoint).await;
        if let Ok(r) = response {
            if r.status().is_success() {
                return Ok(true);
            }
        }
        tokio::time::delay_for(interval).await;
    }
}

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

    #[tokio::test]
    async fn test_output_good() {
        let (tx, rx) = tokio::sync::broadcast::channel(10);

        for line in &["aap\n", "program:123 running\n", "noot\n"] {
            tx.send(line.to_string()).unwrap();
        }
        drop(tx);

        let result = output(rx, "^program:[0-9]+.*$").await.expect("re");
        assert!(result);
    }

    #[tokio::test]
    async fn test_output_bad() {
        let (tx, rx) = tokio::sync::broadcast::channel(10);

        for line in &["aap\n", "noot\n", "mies\n"] {
            tx.send(line.to_string()).unwrap();
        }
        drop(tx);

        let result = output(rx, "^program:[0-9]+.*$").await.expect("re");
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
