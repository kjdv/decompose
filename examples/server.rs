extern crate clap;
extern crate string_error;
#[macro_use]
extern crate rouille;

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
    println!("listening at {}", address);
    rouille::start_server(address, move |request| {
        router!(request,
            (GET) (/health) => {
                try_respond(|_| Ok("healthy\n".to_string()), request)
            },
            (GET) (/hello) => {
                try_respond(|_| Ok("hello!\n".to_string()), request)
            },
            (GET) (/args) => {
                try_respond(|r| {
                    let idx = match r.get_param("idx") {
                        None => return Err(string_error::static_err("no index")),
                        Some(idx) => idx
                    };
                    let idx: usize = idx.parse()?;
                    let arg = std::env::args().nth(idx).ok_or("bad arg index")?;
                    Ok(arg.to_string())
                }, request)
            },
            (GET) (/cwd) => {
                try_respond(|_| {
                    let cwd = std::env::current_dir()?;
                    let cwd = cwd.into_os_string();
                    let cwd = cwd.into_string().unwrap();
                    Ok(cwd)
                }, request)
            },
            (GET) (/env) => {
                try_respond(|r| {
                    let key = match r.get_param("key") {
                        None => return Err(string_error::static_err("no key")),
                        Some(idx) => idx
                    };
                    let value = std::env::var(key)?;
                    Ok(value)
                }, request)
            },
            _ => {
                println!("404 not found");
                rouille::Response::empty_404()
            }
        )
    });
}

fn try_respond<F>(f: F, r: &rouille::Request) -> rouille::Response
where
    F: Fn(&rouille::Request) -> std::result::Result<String, Box<dyn std::error::Error>>,
{
    match f(r) {
        Ok(text) => {
            print!("{}", text);
            rouille::Response::text(text)
        }
        Err(e) => {
            println!("Error: {}", e);
            rouille::Response::empty_400()
        }
    }
}
