//! Neolaas Operator
//!
//! Kubernetes operator for managing hardware inventory CRDs.
//! Syncs Machine, Switch, and Vlan resources to etcd.

use clap::{Parser, Subcommand};
use kube::CustomResourceExt;
use neolaas_operator::{
    controllers::{Context, MachineController, SwitchController, VlanController},
    crds::{Machine, Switch, Vlan},
    etcd::EtcdSync,
};
use std::sync::Arc;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(name = "neolaas-operator")]
#[command(about = "Kubernetes operator for neolaas hardware inventory")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print CRD manifests to stdout
    Crds,
    /// Run the operator
    Run {
        /// etcd endpoints (comma-separated)
        #[arg(long, env = "ETCD_ENDPOINTS", default_value = "http://127.0.0.1:2379")]
        etcd_endpoints: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    match cli.command {
        Commands::Crds => {
            print_crds();
            Ok(())
        }
        Commands::Run { etcd_endpoints } => run_operator(&etcd_endpoints).await,
    }
}

fn print_crds() {
    println!("---");
    println!(
        "{}",
        serde_yaml::to_string(&Machine::crd()).expect("Failed to serialize Machine CRD")
    );
    println!("---");
    println!(
        "{}",
        serde_yaml::to_string(&Switch::crd()).expect("Failed to serialize Switch CRD")
    );
    println!("---");
    println!(
        "{}",
        serde_yaml::to_string(&Vlan::crd()).expect("Failed to serialize Vlan CRD")
    );
}

async fn run_operator(etcd_endpoints: &str) -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();

    info!("Starting neolaas-operator");

    let endpoints: Vec<String> = etcd_endpoints.split(',').map(|s| s.to_string()).collect();
    let etcd = EtcdSync::new(&endpoints).await?;
    let ctx = Arc::new(Context::new(etcd));

    let client = kube::Client::try_default().await?;
    info!("Connected to Kubernetes");

    // Run all controllers concurrently
    tokio::select! {
        _ = MachineController::run(client.clone(), ctx.clone()) => {}
        _ = SwitchController::run(client.clone(), ctx.clone()) => {}
        _ = VlanController::run(client.clone(), ctx.clone()) => {}
    }

    Ok(())
}
