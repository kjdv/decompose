extern crate clap;
extern crate string_error;
extern crate tokio;

use std::marker::Unpin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

type Result<T> = std::result::Result<T, tokio::io::Error>;

#[tokio::main]
async fn main() {
    let args = clap::App::new("proxy")
        .author("Klaas de Vries")
        .about("simple forwding, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("address")
                .help("address to bind to")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:8081"),
        )
        .arg(
            clap::Arg::with_name("forward")
                .help("address to forwad to")
                .short("f")
                .long("forward")
                .default_value("127.0.0.1:8080"),
        )
        .get_matches();

    let address = args.value_of("address").unwrap();
    let forward = args.value_of("forward").unwrap();
    serve(address, forward).await;
}

async fn serve(address: &str, forward: &str) {
    use std::str::FromStr;

    println!("listening at {}, forwarding to {}", address, forward);

    let mut listener = TcpListener::bind(address).await.expect("bind");

    loop {
        let (stream, remote_address) = listener.accept().await.expect("accept");
        println!("new connection from {}", remote_address);

        let fs = String::from_str(forward).expect("from_str");
        tokio::spawn(handle_wrap(stream, fs));
    }
}

async fn handle_wrap(from_stream: TcpStream, forward: String) {
    if let Err(e) = handle(from_stream, forward).await {
        println!("Error: {}", e);
    }
}

async fn handle(from_stream: TcpStream, forward: String) -> Result<()> {
    let to_stream = TcpStream::connect(forward).await?;

    let from_stream = tokio::io::split(from_stream);
    let to_stream = tokio::io::split(to_stream);

    proxy(from_stream, to_stream).await
}

async fn proxy<T, U, V, W>(stream1: (T, U), stream2: (V, W)) -> Result<()>
where
    T: AsyncReadExt + Unpin,
    U: AsyncWriteExt + Unpin,
    V: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let (rx1, tx1) = stream1;
    let (rx2, tx2) = stream2;

    // Q: select or join?
    tokio::select! {
        x = copy(rx1, tx2) => {
            match x {
                Ok(_) => {
                    println!("rx1->tx2 completed");
                    Ok(())
                },
                Err(e) => {
                    println!("rx1->tx2 errored: {}", e);
                    Err(e)
                }
            }
        },
        x = copy(rx2, tx1) => {
            match x {
                Ok(_) => {
                    println!("rx2->tx1 completed");
                    Ok(())
                },
                Err(e) => {
                    println!("rx2->tx1 errored: {}", e);
                    Err(e)
                }
            }
        }
    }
}

async fn copy<T, U>(mut from: T, mut to: U) -> std::io::Result<()>
where
    T: AsyncReadExt + Unpin,
    U: AsyncWriteExt + Unpin,
{
    const BUFSIZE: usize = 512;

    let mut buf = [0; BUFSIZE];
    loop {
        let n = match from.read(&mut buf).await {
            Err(e) => {
                return Err(e);
            }
            Ok(0) => {
                return Ok(());
            }
            Ok(n) => n,
        };

        if let Err(e) = to.write_all(&buf[0..n]).await {
            return Err(e);
        }
    }
}
