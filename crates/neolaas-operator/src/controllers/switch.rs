//! Switch controller

use super::Context;
use crate::crds::Switch;
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

const FINALIZER: &str = "neolaas.io/switch-finalizer";

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("Kubernetes error: {0}")]
    Kube(#[source] kube::Error),
    #[error("etcd sync error: {0}")]
    Etcd(#[from] anyhow::Error),
}

pub struct SwitchController;

impl SwitchController {
    pub async fn run(client: Client, ctx: Arc<Context>) {
        let api: Api<Switch> = Api::all(client);

        Controller::new(api, Config::default())
            .run(
                |switch, ctx| async move { reconcile(switch, ctx).await },
                error_policy,
                ctx,
            )
            .for_each(|res| async move {
                match res {
                    Ok((obj, _)) => info!(name = %obj.name, "Reconciled Switch"),
                    Err(e) => error!(error = %e, "Reconcile error"),
                }
            })
            .await;
    }
}

#[instrument(skip(ctx), fields(name = %switch.name_any()))]
async fn reconcile(
    switch: Arc<Switch>,
    ctx: Arc<Context>,
) -> Result<Action, FinalizerError<ReconcileError>> {
    let ns = switch.namespace().unwrap_or_default();
    let name = switch.name_any();

    let etcd_client = ctx.etcd.clone();

    let client = kube::Client::try_default()
        .await
        .map_err(|e| FinalizerError::ApplyFailed(ReconcileError::Kube(e)))?;

    finalizer(
        &Api::<Switch>::namespaced(client, &ns),
        FINALIZER,
        switch,
        |event| async {
            match event {
                Event::Apply(switch) => {
                    info!(name = %name, "Syncing switch to etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.put_switch(&name, &switch.spec).await?;
                    Ok(Action::requeue(Duration::from_secs(300)))
                }
                Event::Cleanup(_switch) => {
                    info!(name = %name, "Removing switch from etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.delete_switch(&name).await?;
                    Ok(Action::await_change())
                }
            }
        },
    )
    .await
}

fn error_policy(
    _switch: Arc<Switch>,
    error: &FinalizerError<ReconcileError>,
    _ctx: Arc<Context>,
) -> Action {
    error!(error = %error, "Reconcile failed");
    Action::requeue(Duration::from_secs(60))
}
