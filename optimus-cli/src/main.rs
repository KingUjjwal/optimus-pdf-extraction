use clap::{Parser, Subcommand, ArgAction};
use anyhow::Result;
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(name = "optimus", about = "High-Throughput Spatial Layout Analyzer")]
struct Cli {
    /// Verbose output (repeat for more detail: -v = info, -vv = debug, -vvv = trace)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Extract data from a single PDF
    Extract {
        /// Path to PDF file
        path: PathBuf,
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
        /// Output format: json | arrow
        #[arg(short, long, default_value = "json")]
        format: String,
    },
    /// Batch process a directory of PDFs
    Batch {
        /// Input directory
        #[arg(short, long)]
        input: PathBuf,
        /// Output file for Arrow IPC
        #[arg(short, long, default_value = "output.arrow")]
        output: PathBuf,
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
    /// Manage layout cache
    Cache {
        #[command(subcommand)]
        subcommand: CacheCommand,
    },
    /// Generate layout grid
    GenerateGrid {
        /// Path to PDF file
        path: PathBuf,
        /// Format: ascii | markdown
        #[arg(short, long, default_value = "ascii")]
        format: String,
    },
    /// Ingest a directory of PDFs to pre-warm the cache
    Ingest {
        /// Input directory
        #[arg(short, long)]
        input: PathBuf,
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
    /// Show status of the cache
    Status {
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
    /// Benchmark extraction throughput
    Benchmark {
        /// Number of iterations
        #[arg(short, long, default_value = "1000")]
        count: usize,
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
    /// Watch a directory for new PDFs and extract them automatically
    Watch {
        /// Input directory to watch
        #[arg(short, long)]
        dir: PathBuf,
        /// Cache directory
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
}

#[derive(Subcommand)]
enum CacheCommand {
    /// List cached layout IDs
    List {
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
    /// Clear all cached layouts
    Clear {
        #[arg(short, long, default_value = "./optimus_cache")]
        cache: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let default_filter = match cli.verbose {
        0 => "warn,optimus_agent=info,optimus_runtime=info,optimus_router=info",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_string());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_target(false)
        .init();

    match cli.command {
        Commands::Extract { path, cache, format } => {
            commands::extract_single(&path, &cache, &format)?;
        }
        Commands::Batch { input, output, cache } => {
            commands::batch_extract(&input, &output, &cache)?;
        }
        Commands::Cache { subcommand } => match subcommand {
            CacheCommand::List { cache } => commands::cache_list(&cache)?,
            CacheCommand::Clear { cache } => commands::cache_clear(&cache)?,
        },
        Commands::GenerateGrid { path, format } => {
            commands::generate_grid(&path, &format)?;
        }
        Commands::Ingest { input, cache } => {
            commands::ingest(&input, &cache)?;
        }
        Commands::Status { cache } => {
            commands::status(&cache)?;
        }
        Commands::Benchmark { count, cache } => {
            commands::benchmark(count, &cache)?;
        }
        Commands::Watch { dir, cache } => {
            commands::watch(&dir, &cache)?;
        }
    }

    Ok(())
}
