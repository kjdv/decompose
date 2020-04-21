extern crate clap;
extern crate string_error;

use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    let args = clap::App::new("server")
        .author("Klaas de Vries")
        .about("simple server, for aide in automated tests of decompose")
        .arg(
            clap::Arg::with_name("address")
                .help("address to bind to")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:8080"),
        )
        .arg(
            clap::Arg::with_name("extra")
                .help("you can add extra argumens, they will be ignored")
                .multiple(true),
        )
        .get_matches();

    let address = args.value_of("address").unwrap();
    serve(address);
}

fn serve(address: &str) {
    println!("listening at {}", address);

    let listener = TcpListener::bind(address).expect("bind");

    for stream in listener.incoming() {
        match handle(stream.expect("stream")) {
            Ok(_) => println!("done"),
            Err(e) => println!("Error: {}", e),
        };
    }
}

fn handle(mut stream: TcpStream) -> Result<()> {
    let mut buf = [0; 512];

    let size = stream.read(&mut buf)?;
    let request = String::from_utf8_lossy(&buf[0..size]);

    println!("request='{}', size={}", request, size);

    if request.starts_with("hello") {
        let hello = "hello!\n";

        print!("{}", hello);
        stream.write_all(hello.as_bytes())?;
    } else if request.starts_with("args") {
        let idx = request.split_ascii_whitespace().nth(1).ok_or("bad index")?;
        let idx: usize = idx.parse()?;
        let arg = std::env::args()
            .nth(idx)
            .ok_or("bad arg index")?;

        stream.write_all(arg.as_bytes())?;
    } else if request.starts_with("cwd") {
        let cwd = std::env::current_dir()?;
        let cwd = cwd.into_os_string();
        let cwd = cwd.into_string().unwrap();

        stream.write_all(cwd.as_bytes())?;
    } else if request.starts_with("env") {
        let key = request.split_ascii_whitespace().nth(1).ok_or("bad index")?;
        let value = std::env::var(key)?;

        stream.write_all(value.as_bytes())?;
    } else {
        return Err(string_error::new_err("404"));
    }

    stream.flush()?;
    Ok(())
}
