//! `desigma-server`: keeps a `.desigma` file shared permanently. See the crate README.

use desigma_net::Network;
use desigma_server::{log, Config, Server};
use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

const USAGE: &str = "\
desigma-server FILE [options]

Keeps FILE (.desigma, or a .fig / .pen / REST .json / .svg to import) shared permanently.

  --password P            require this password to join (visible in `ps`; prefer --password-env)
  --password-env VAR      read the password from environment variable VAR
  --view-only             guests can look but not edit
  --approve-none          let in everyone who knows the link and password (the default; there is
                          nobody to approve requests on a headless host)
  --name NAME             label in the log (default \"Server\")
  --key-file PATH         secret key that keeps the link stable (default .<name>.desigma-key next
                          to FILE; created on first start)
  --link-file PATH        also write the join link to PATH
  --direct                no relay, no discovery: LAN / VPN / fixed address only (combine with --port)
  --port PORT             fixed UDP port to listen on (open it in the firewall)
  --autosave SECS         save changes at most every SECS seconds (default 10)
  --backups N             rotating backups to keep, FILE.bak.1 newest (default 3, 0 for none)
  -h, --help              this text
";

fn parse(args: impl Iterator<Item = String>) -> Result<Option<Config>, String> {
    let mut args = args.peekable();
    let mut file = None;
    let mut cfg = Config::new("");
    let mut password_env = None;
    while let Some(a) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match a.as_str() {
            "-h" | "--help" => return Ok(None),
            "--password" => cfg.password = Some(value("--password")?),
            "--password-env" => password_env = Some(value("--password-env")?),
            "--view-only" => cfg.view_only = true,
            "--approve-none" => {}
            "--name" => cfg.name = value("--name")?,
            "--key-file" => cfg.key_file = Some(value("--key-file")?.into()),
            "--link-file" => cfg.link_file = Some(value("--link-file")?.into()),
            "--direct" => cfg.network = Network::Direct,
            "--port" => cfg.port = Some(value("--port")?.parse().map_err(|_| "--port needs a number from 1 to 65535")?),
            "--autosave" => {
                let secs: f64 = value("--autosave")?.parse().map_err(|_| "--autosave needs a number of seconds")?;
                if !(secs >= 0.0 && secs.is_finite()) {
                    return Err("--autosave needs a number of seconds".into());
                }
                cfg.autosave = Duration::from_secs_f64(secs);
            }
            "--backups" => cfg.backups = value("--backups")?.parse().map_err(|_| "--backups needs a number")?,
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}")),
            _ => {
                if file.replace(PathBuf::from(&a)).is_some() {
                    return Err("only one FILE is served per process".into());
                }
            }
        }
    }
    if let Some(var) = password_env {
        if cfg.password.is_some() {
            return Err("use either --password or --password-env, not both".into());
        }
        cfg.password = Some(std::env::var(&var).map_err(|_| format!("environment variable {var} is not set"))?);
    }
    cfg.file = file.ok_or("missing FILE")?;
    Ok(Some(cfg))
}

/// Sets `stop` on Ctrl-C (and SIGTERM on Unix, which is what `systemctl stop` sends).
fn watch_signals(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        rt.block_on(async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                if let Ok(mut term) = signal(SignalKind::terminate()) {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = term.recv() => {}
                    }
                    return;
                }
            }
            let _ = tokio::signal::ctrl_c().await;
        });
        stop.store(true, Ordering::Relaxed);
    });
}

fn main() -> ExitCode {
    let cfg = match parse(std::env::args().skip(1)) {
        Ok(Some(c)) => c,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("desigma-server: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let name = cfg.name.clone();
    let mut server = match Server::start(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("desigma-server: {e}");
            return ExitCode::FAILURE;
        }
    };
    log(&name, &format!("join link: {}", server.link()));
    let stop = Arc::new(AtomicBool::new(false));
    watch_signals(stop.clone());
    server.run(&stop);
    log(&name, "stopping");
    match server.shutdown() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("desigma-server: final save failed: {e}");
            ExitCode::FAILURE
        }
    }
}
