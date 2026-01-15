//! Machine controller

use super::Context;
use crate::crds::Machine;
use futures::StreamExt;
use kube::{
    api::Api,
    runtime::{
        controller::{Action, Controller},
        finalizer::{finalizer, Error as FinalizerError, Event},
        watcher::Config,
    },
    Client, ResourceExt,
};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::{error, info, instrument};

const FINALIZER: &str = "neolaas.io/machine-finalizer";

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("Kubernetes error: {0}")]
    Kube(#[source] kube::Error),
    #[error("etcd sync error: {0}")]
    Etcd(#[from] anyhow::Error),
}

pub struct MachineController;

impl MachineController {
    pub async fn run(client: Client, ctx: Arc<Context>) {
        let api: Api<Machine> = Api::all(client);

        Controller::new(api, Config::default())
            .run(
                |machine, ctx| async move { reconcile(machine, ctx).await },
                error_policy,
                ctx,
            )
            .for_each(|res| async move {
                match res {
                    Ok((obj, _)) => info!(name = %obj.name, "Reconciled Machine"),
                    Err(e) => error!(error = %e, "Reconcile error"),
                }
            })
            .await;
    }
}

#[instrument(skip(ctx), fields(name = %machine.name_any()))]
async fn reconcile(
    machine: Arc<Machine>,
    ctx: Arc<Context>,
) -> Result<Action, FinalizerError<ReconcileError>> {
    let ns = machine.namespace().unwrap_or_default();
    let name = machine.name_any();

    let etcd_client = ctx.etcd.clone();

    let client = kube::Client::try_default()
        .await
        .map_err(|e| FinalizerError::ApplyFailed(ReconcileError::Kube(e)))?;

    finalizer(
        &Api::<Machine>::namespaced(client, &ns),
        FINALIZER,
        machine,
        |event| async {
            match event {
                Event::Apply(machine) => {
                    info!(name = %name, "Syncing machine to etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.put_machine(&name, &machine.spec).await?;
                    Ok(Action::requeue(Duration::from_secs(300)))
                }
                Event::Cleanup(_machine) => {
                    info!(name = %name, "Removing machine from etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.delete_machine(&name).await?;
                    Ok(Action::await_change())
                }
            }
        },
    )
    .await
}

fn error_policy(
    _machine: Arc<Machine>,
    error: &FinalizerError<ReconcileError>,
    _ctx: Arc<Context>,
) -> Action {
    error!(error = %error, "Reconcile failed");
    Action::requeue(Duration::from_secs(60))
}
