//! VLAN controller

use super::Context;
use crate::crds::Vlan;
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

const FINALIZER: &str = "neolaas.io/vlan-finalizer";

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("Kubernetes error: {0}")]
    Kube(#[source] kube::Error),
    #[error("etcd sync error: {0}")]
    Etcd(#[from] anyhow::Error),
}

pub struct VlanController;

impl VlanController {
    pub async fn run(client: Client, ctx: Arc<Context>) {
        let api: Api<Vlan> = Api::all(client);

        Controller::new(api, Config::default())
            .run(
                |vlan, ctx| async move { reconcile(vlan, ctx).await },
                error_policy,
                ctx,
            )
            .for_each(|res| async move {
                match res {
                    Ok((obj, _)) => info!(name = %obj.name, "Reconciled Vlan"),
                    Err(e) => error!(error = %e, "Reconcile error"),
                }
            })
            .await;
    }
}

#[instrument(skip(ctx), fields(name = %vlan.name_any()))]
async fn reconcile(
    vlan: Arc<Vlan>,
    ctx: Arc<Context>,
) -> Result<Action, FinalizerError<ReconcileError>> {
    let ns = vlan.namespace().unwrap_or_default();
    let name = vlan.name_any();

    let etcd_client = ctx.etcd.clone();

    let client = kube::Client::try_default()
        .await
        .map_err(|e| FinalizerError::ApplyFailed(ReconcileError::Kube(e)))?;

    finalizer(
        &Api::<Vlan>::namespaced(client, &ns),
        FINALIZER,
        vlan,
        |event| async {
            match event {
                Event::Apply(vlan) => {
                    info!(name = %name, "Syncing VLAN to etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.put_vlan(&name, &vlan.spec).await?;
                    Ok(Action::requeue(Duration::from_secs(300)))
                }
                Event::Cleanup(_vlan) => {
                    info!(name = %name, "Removing VLAN from etcd");
                    let mut etcd = etcd_client.write().await;
                    etcd.delete_vlan(&name).await?;
                    Ok(Action::await_change())
                }
            }
        },
    )
    .await
}

fn error_policy(
    _vlan: Arc<Vlan>,
    error: &FinalizerError<ReconcileError>,
    _ctx: Arc<Context>,
) -> Action {
    error!(error = %error, "Reconcile failed");
    Action::requeue(Duration::from_secs(60))
}
