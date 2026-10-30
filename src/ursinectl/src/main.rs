//! `ursinectl` entrypoint: root gate → in-process daemon → one call → exit.

#![allow(clippy::print_stdout, clippy::print_stderr)] // CLI output is the interface

use std::io::Write as _;
use std::process::ExitCode;

use ursine_api::ModuleId;
use ursinectl::cli::{self, Cmd};
use ursinectl::{root_peer, EXIT_USAGE};

fn main() -> ExitCode {
    // nosemgrep: rust.lang.security.args.args — strict parsing, no shell/eval
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match cli::parse(&args) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("error: {e}\n");
            eprint!("{}", cli::USAGE);
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match cmd {
        Cmd::Help(module) => {
            print!("{}", cli::USAGE);
            if let Some(module) = module {
                eprintln!(
                    "\n(run `ursinectl --help` for the full list; module {module:?} shown above)"
                );
            }
            ExitCode::SUCCESS
        }
        Cmd::Version => {
            println!("ursinectl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Cmd::Run(opts) => run(opts),
    }
}

fn run(opts: cli::RunOpts) -> ExitCode {
    // Hard root gate before ANY side effect (config, audit, job dir).
    if let Err(e) = ursinectl::root::ensure_root() {
        eprintln!("error: {e}");
        return ExitCode::from(EXIT_USAGE);
    }
    let action = match cli::build_action(&opts.action, &opts.pairs) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    if cli::is_destructive(&action) && !opts.yes {
        eprintln!(
            "refusing destructive action {}.{} without --yes",
            opts.module, opts.action
        );
        return ExitCode::from(EXIT_USAGE);
    }
    let Ok(module) =
        serde_json::from_value::<ModuleId>(serde_json::Value::String(opts.module.clone()))
    else {
        eprintln!("error: unknown module {:?}", opts.module);
        return ExitCode::from(EXIT_USAGE);
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run_call(module, action, opts))
}

async fn run_call(module: ModuleId, action: ursine_api::Action, opts: cli::RunOpts) -> ExitCode {
    let daemon = match ursinectl::daemon::build(&opts.config) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let token = ursinectl::issue_token(&daemon);
    let peer = root_peer();
    let result =
        ursinectl::call(&daemon, &token, &peer, module, action, opts.as_user.clone()).await;

    match result {
        Ok(value) => {
            // Package mutations answer {"job_id": ...}: stream the job log
            // live (terminal text, never a percentage) and exit with its code.
            if let Some(job_id) = value.get("job_id").and_then(serde_json::Value::as_str) {
                // `Stdout` (the handle, not the lock) is `Send`: the poll
                // loop holds it across awaits.
                let mut out = std::io::stdout();
                return match ursinectl::stream_job(&daemon, &token, &peer, job_id, &mut out).await {
                    Ok(0) => ExitCode::SUCCESS,
                    Ok(_) => ExitCode::FAILURE,
                    Err(e) => {
                        eprintln!("error: {}", e.message);
                        ExitCode::FAILURE
                    }
                };
            }
            if opts.json {
                match serde_json::to_string(&value) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("error: encode result: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                match serde_json::to_string_pretty(&value) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("error: encode result: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let _ = std::io::stdout().flush();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {}", error.message);
            if let Some(fix) = error.fix {
                eprintln!("fix: {fix}");
            }
            ExitCode::FAILURE
        }
    }
}
