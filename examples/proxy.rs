extern crate clap;
extern crate string_error;

use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
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
    serve(address, forward.to_string());
}

fn serve(address: &str, forward: String) {
    println!("listening at {}, forwarding to {}", address, forward);

    let listener = TcpListener::bind(address).expect("bind");

    for stream in listener.incoming() {
        println!("new connection");

        match handle(stream.expect("stream"), &forward) {
            Ok(_) => println!("done handling"),
            Err(e) => println!("Error: {}", e),
        };
    }
}

fn handle(mut stream: TcpStream, forward: &String) -> Result<()> {
    let mut forward = TcpStream::connect(forward.as_str())?;

    loop {
        handle_single(&mut stream, &mut forward)?;
    }
}

fn handle_single(stream: &mut TcpStream, forward: &mut TcpStream) -> Result<()> {
    let mut buf = [0; 512];

    let size = stream.read(&mut buf)?;

    if size == 0 {
        return Err(string_error::new_err("0 read"));
    }

    println!("proxying '{}'", String::from_utf8_lossy(&buf[0..size]));

    forward.write_all(&buf[0..size])?;
    forward.flush()?;

    let size = forward.read(&mut buf)?;
    if size == 0 {
        return Err(string_error::new_err("0 read"));
    }
    stream.write_all(&buf[0..size])?;
    stream.flush()?;

    Ok(())
}
