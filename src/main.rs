use std::io::ErrorKind;
use std::sync::Arc;

use getopts::{Options, ParsingStyle};
use salsa::{config::Config, dapp_process, http_service};
use tokio::sync::Notify;

fn print_usage(program: &str, opts: Options) {
    let brief = format!(
        "Usage: {} [options] <command> [args]\n\
        \n\
        Where command and args start the DApp.",
        program
    );
    print!("{}", opts.usage(&brief));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let program = args[0].clone();
    // Process command line arguments
    let mut opts = Options::new();
    opts.parsing_style(ParsingStyle::StopAtFirstFree);
    opts.optflag("h", "help", "show this help message and exit");
    opts.optopt(
        "",
        "address",
        "Address to listen (default: 127.0.0.1:5005)",
        "",
    );
    opts.optopt("", "dapp", "Dapp address (default: 127.0.0.1:5005)", "");
    opts.optflag("", "verbose", "print more info about application execution");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error parsing arguments: {}", &e);
            return Err(std::io::Error::new(ErrorKind::InvalidInput, e.to_string()));
        }
    };
    if matches.opt_present("h") {
        print_usage(&program, opts);
        return Ok(());
    }

    // Set log level of application
    let mut log_level = "info";
    if matches.opt_present("verbose") {
        log_level = "debug";
    }
    // Set the global log level, disable timestamp
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
        .format_timestamp(None)
        .init();

    log::info!("starting http dispatcher service...");

    // Create config
    let mut http_config = Config::new();
    {
        // Parse addresses and ports
        let address_matches = matches
            .opt_get_default("address", "127.0.0.1:5005".to_string())
            .unwrap_or_default();
        let mut address = address_matches.split(':');
        http_config.http_address = address.next().expect("address is not valid").to_string();
        http_config.http_port = address
            .next()
            .expect("port is not valid")
            .to_string()
            .parse::<u16>()
            .unwrap();
    }

    let server_ready = Arc::new(Notify::new());

    //In another thread, wait until the server is ready and then start the dapp
    if !matches.free.is_empty() {
        let server_ready = server_ready.clone();
        tokio::spawn(async move {
            server_ready.notified().await;
            dapp_process::run(matches.free).await;
        });
    } else {
        log::warn!("No command provided for dapp_process. Skipping dapp_process execution.");
    }

    // Open http service
    tokio::select! {
        result = http_service::run(&http_config, server_ready) => {
            match result {
                Ok(_) => log::info!("http service terminated successfully"),
                Err(e) => log::warn!("http service terminated with error: {}", e),
            }
        }
    }
    log::info!("ending http dispatcher service!");
    Ok(())
}