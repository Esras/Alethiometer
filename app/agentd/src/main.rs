//! `alethiometer-agentd --dir <data dir> [--idle-secs N]`. Started by the app
//! when it needs it; see `app/docs/agents.md` ("Agent host").

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir = None;
    let mut idle = 600u64;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => dir = args.next(),
            "--idle-secs" => idle = args.next().and_then(|v| v.parse().ok()).unwrap_or(idle),
            "--version" => {
                println!("alethiometer-agentd {} protocol {}", agentd::proto::VERSION, agentd::proto::PROTOCOL);
                return;
            }
            other => {
                eprintln!("agentd: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let Some(dir) = dir else {
        eprintln!("usage: alethiometer-agentd --dir <data dir> [--idle-secs N]");
        std::process::exit(2);
    };
    #[cfg(unix)]
    {
        let opts = agentd::server::Options { dir: dir.into(), idle: std::time::Duration::from_secs(idle.max(10)) };
        if let Err(e) = agentd::server::run(opts) {
            eprintln!("agentd: {e}");
            std::process::exit(if e.kind() == std::io::ErrorKind::AddrInUse { 0 } else { 1 });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, idle);
        eprintln!("agentd: not available on this platform yet");
        std::process::exit(1);
    }
}
