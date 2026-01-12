mod log;
mod model;
mod render;
mod spec;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "flowlog-profiler")]
#[command(about = "FlowLog profiler report generator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a profiling report (validates inputs while running).
    Report {
        #[arg(long)]
        log: String,

        #[arg(long)]
        ops: String,

        #[arg(short = 'o', long)]
        out: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Commands::Report { log, ops, out } => {
            // 1) Parse + validate ops.json (contains both topology + operator mapping).
            let ops_spec: spec::OpsSpec = serde_json::from_str(&std::fs::read_to_string(&ops)?)?;
            let validated = ops_spec.validate_and_build()?;

            // Prepare node map keyed by stringified id for downstream rendering.
            let mut nodes_by_name = std::collections::BTreeMap::new();
            for (id, node) in validated.nodes {
                nodes_by_name.insert(id.to_string(), node);
            }
            let roots: Vec<String> = validated.roots.iter().map(|id| id.to_string()).collect();

            // 2) Parse log.
            let log_index = log::parse_log_file(&log)?;

            // 3) Aggregate.
            let data = model::build_report_data(&nodes_by_name, &roots, &log_index)?;

            // 4) Render HTML.
            let html = render::render_html_report(&data)?;
            std::fs::write(&out, html)?;
            eprintln!("Wrote {}", out);
        }
    }

    Ok(())
}
