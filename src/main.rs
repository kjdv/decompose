extern crate clap;
extern crate log;
extern crate simple_logger;

use std::error::Error;

mod config;
mod executor;
mod graph;
mod output;
mod process;
mod readysignals;
mod tokio_utils;

fn main() -> Result<(), Box<dyn Error>> {
    do_main().map_err(|e| {
        log::error!("{:?}", e);
        e
    })
}

fn do_main() -> Result<(), Box<dyn Error>> {
    let default_od = default_outdir();
    let args = clap::App::new("decompose")
        .author("Klaas de Vries")
        .about("service orchestration for devs")
        .arg(
            clap::Arg::with_name("output")
                .long_help(
                    "specify what to do with child processes output:
null => the output will be ignored
inline => output streams from the child processes will be inlined with decompose's output
files => log files for each process will be places in --outdir",
                )
                .short("o")
                .long("output")
                .takes_value(true)
                .possible_values(&["null", "inline", "files"])
                .default_value("inline"),
        )
        .arg(
            clap::Arg::with_name("outdir")
                .help("output directory, used if --output=files")
                .default_value(default_od.as_str())
                .short("d")
                .long("outdir"),
        )
        .arg(
            clap::Arg::with_name("loglevel")
                .help("set the logging level")
                .short("l")
                .long("log")
                .takes_value(true)
                .possible_values(&["off", "error", "warning", "info", "debug", "trace"])
                .default_value("warning"),
        )
        .arg(
            clap::Arg::with_name("config")
                .help("configuration file, in toml format")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::with_name("dot")
                .help("write the system dependency graph to stdout, in dot format")
                .long("dot"),
        )
        .get_matches();

    init_logging(args.value_of("loglevel").expect("log level"))?;
    log::debug!("arguments are config file is {:?}", args);

    let sys = config::System::from_file(args.value_of("config").unwrap())?;

    if args.is_present("dot") {
        let g = graph::Graph::from_config(&sys)?;
        g.dot(&mut std::io::stdout());
        return Ok(());
    }

    log::debug!("system is {:?}", sys);

    let of = output_factory(
        args.value_of("output").expect("output"),
        args.value_of("outdir").expect("outdir"),
    )?;

    tokio_utils::run(async move {
        let mut exec = executor::Executor::from_config(&sys)?;
        exec.start(of).await?;
        exec.run().await
    })?;

    Ok(())
}

fn default_outdir() -> String {
    use std::str::FromStr;
    String::from_str(".decompose").unwrap()
}

fn init_logging(arg: &str) -> Result<(), Box<dyn Error>> {
    let level = match arg {
        "off" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warning" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => panic!("invalid log level {}", arg),
    };

    simple_logger::SimpleLogger::new()
        .with_level(level)
        .init()?;
    Ok(())
}

fn output_factory(
    arg: &str,
    od_arg: &str,
) -> Result<Box<dyn output::OutputFactory>, Box<dyn Error>> {
    let of: Box<dyn output::OutputFactory> = match arg {
        "null" => Box::new(output::NullOutputFactory {}),
        "inline" => Box::new(output::InlineOutputFactory::new()),
        "files" => {
            let od_arg = std::path::Path::new(od_arg);
            let of = output::OutputFileFactory::new(od_arg)?;
            Box::new(of)
        }
        _ => panic!("invalid output type {}", arg),
    };
    Ok(of)
}
