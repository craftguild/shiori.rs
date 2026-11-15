#![forbid(unsafe_code)]

use clap::{CommandFactory, Parser, Subcommand};
use shiori::{config::Config, generate, server};
use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

include!(concat!(env!("OUT_DIR"), "/craftguild.rs"));

#[derive(Parser)]
#[command(
    version,
    about = "Public filesystem Markdown CMS",
    after_help = "Use --machine-version to print the build identity as JSON."
)]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Print the embedded CycloneDX SBOM.
    Sbom,
    /// Create articles/<epoch>.md and its matching assets/<epoch>/ directory.
    Generate {
        #[arg(long, default_value = "Untitled")]
        title: String,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:4000")]
        bind: SocketAddr,
        #[arg(long, default_value = "http://127.0.0.1:4000")]
        public_base_url: String,
        /// Serve the bundled GraphiQL query editor and enable schema introspection.
        #[arg(long)]
        graphiql: bool,
        /// Notify development clients when Markdown or assets change.
        #[arg(long)]
        watch: bool,
    },
}

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let branding = craftguild_cli::Brand {
        banner: include_str!("../banner.txt"),
        project: "shiori.rs",
    };
    let mut output = std::io::stdout().lock();
    let presentation = match arguments.as_slice() {
        [] => Some(craftguild_cli::help(
            &mut output,
            branding,
            &Cli::command().render_help().to_string(),
        )),
        [flag] if flag == "--help" || flag == "-h" => Some(craftguild_cli::help(
            &mut output,
            branding,
            &Cli::command().render_help().to_string(),
        )),
        [flag] if flag == "--version" || flag == "-V" => {
            Some(craftguild_cli::version(&mut output, branding, BUILD_INFO))
        }
        [flag] if flag == "--machine-version" => {
            use std::io::Write;
            Some(output.write_all(BUILD_INFO_JSON.as_bytes()))
        }
        [command] if command == "sbom" => Some(craftguild_cli::sbom(&mut output, SBOM)),
        _ => None,
    };
    if let Some(result) = presentation {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("shiori: {error}");
                ExitCode::FAILURE
            }
        };
    }
    drop(output);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "shiori=info".into()),
        )
        .json()
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Sbom => {
            craftguild_cli::sbom(&mut std::io::stdout().lock(), SBOM).map_err(anyhow::Error::from)
        }
        Command::Generate { title } => {
            generate::generate(&cli.root, &title).map(|path| println!("{}", path.display()))
        }
        Command::Serve {
            bind,
            public_base_url,
            graphiql,
            watch,
        } => Config::new(&cli.root, bind, &public_base_url).and_then(|mut config| {
            config.graphiql = graphiql;
            config.watch = watch;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            let result = runtime.block_on(server::serve(config));
            runtime.shutdown_timeout(std::time::Duration::from_secs(30));
            result
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(event="fatal", error=%format!("{error:#}"));
            ExitCode::FAILURE
        }
    }
}
