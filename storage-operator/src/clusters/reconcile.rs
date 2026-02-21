use eosin_types::*;
use futures::stream::StreamExt;
use k8s_openapi::jiff::Timestamp;
use kube::{
    Api, ResourceExt,
    client::Client,
    runtime::{Controller, controller::Action},
};
use kube_leader_election::{LeaseLock, LeaseLockParams, LeaseLockResult};
use owo_colors::OwoColorize;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::{sync::Mutex, time::Duration};
use tokio_util::sync::CancellationToken;

use super::actions;
use crate::util::{
    Error, PROBE_INTERVAL,
    colors::{FG1, FG2},
};

#[cfg(feature = "metrics")]
use crate::util::metrics::ControllerMetrics;

/// Entrypoint for the `Cluster` controller.
pub async fn run(client: Client) -> Result<(), Error> {
    println!("{}", "⚙️ Starting Cluster controller...".green());

    // Preparation of resources used by the `kube_runtime::Controller`
    let context: Arc<ContextData> = Arc::new(ContextData::new(client.clone()));

    // The controller comes from the `kube_runtime` crate and manages the reconciliation process.
    // It requires the following information:
    // - `kube::Api<T>` this controller "owns". In this case, `T = Cluster`, as this controller owns the `Cluster` resource,
    // - `kube::api::ListParams` to select the `Cluster` resources with. Can be used for Cluster filtering `Cluster` resources before reconciliation,
    // - `reconcile` function with reconciliation logic to be called each time a resource of `Cluster` kind is created/updated/deleted,
    // - `on_error` function to call whenever reconciliation fails.
    //Controller::new(crd_api, Default::default())
    //    .owns(Api::<Pod>::all(client), Default::default())
    //    .run(reconcile, on_error, context)
    //    .for_each(|_reconciliation_result| async move {})
    //    .await;
    //Ok(())

    // Namespace where we run both leader election and the controller.
    // This lets us keep RBAC namespaced rather than cluster-scoped.
    let lease_namespace = std::env::var("NAMESPACE").unwrap_or_else(|_| "default".to_string());
    // Unique identity per replica (Downward API POD_NAME is ideal).
    // Fallback to hostname if not present.
    let holder_id = std::env::var("POD_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| format!("eosin-cluster-controller-{}", uuid::Uuid::new_v4()));
    // The shared lock name across all replicas
    let lease_name = "eosin-cluster-controller-lock".to_string();
    // TTL: how long leadership is considered valid without renewal.
    // Renew should happen well before TTL expires.
    let lease_ttl = Duration::from_secs(15);
    let renew_every = Duration::from_secs(5);
    let leadership = LeaseLock::new(
        client.clone(),
        &lease_namespace,
        LeaseLockParams {
            holder_id,
            lease_name,
            lease_ttl,
        },
    );

    let shutdown = CancellationToken::new();
    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        eosin_common::shutdown::shutdown_signal().await;
        shutdown_signal.cancel();
    });
    eosin_common::signal_ready();
    println!("{}", "🌱 Starting Cluster controller...".green());
    // We run indefinitely; only the leader runs the controller.
    // On leadership loss, we abort the controller and go back to standby.
    let mut controller_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut tick = tokio::time::interval(renew_every);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                if let Some(task) = controller_task.take() {
                    task.abort();
                    task.await.ok();
                }
                break Ok(())
            },
            _ = tick.tick() => {}
        }
        let lease = match leadership.try_acquire_or_renew().await {
            Ok(l) => l,
            Err(e) => {
                // If we can't talk to the apiserver / update Lease, assume we are not safe to lead.
                eprintln!("leader election renew/acquire failed: {e}");
                if let Some(task) = controller_task.take() {
                    task.abort();
                    eprintln!("aborted controller due to leader election error");
                }
                continue;
            }
        };
        if matches!(lease, LeaseLockResult::Acquired(_)) {
            // We are leader; ensure controller is running
            if controller_task.is_none() {
                println!("{}", "👑 Acquired leadership; starting controller".green());
                let client_for_controller = client.clone();
                let context_for_controller = context.clone();
                let controller_namespace = lease_namespace.clone();
                let crd_api_for_controller: Api<Cluster> =
                    Api::namespaced(client_for_controller.clone(), &controller_namespace);
                controller_task = Some(tokio::spawn(async move {
                    println!("{}", "🚀 Cluster controller started.".green());
                    Controller::new(crd_api_for_controller, Default::default())
                        .owns(
                            Api::<Shard>::namespaced(client_for_controller, &controller_namespace),
                            Default::default(),
                        )
                        .run(reconcile, on_error, context_for_controller)
                        .for_each(|_res| async move {})
                        .await;
                }));
            }
        } else if let Some(task) = controller_task.take() {
            // We are NOT leader; ensure controller is stopped
            eprintln!("lost leadership; stopping controller");
            task.abort();
        }
    }
}

/// Context injected with each `reconcile` and `on_error` method invocation.
struct ContextData {
    /// Kubernetes client to make Kubernetes API requests with. Required for K8S resource management.
    client: Client,

    #[cfg(feature = "metrics")]
    metrics: ControllerMetrics,

    last_action: Mutex<HashMap<(String, String), (ClusterAction, Instant)>>,
}

impl ContextData {
    /// Constructs a new instance of ContextData.
    ///
    /// # Arguments:
    /// - `client`: A Kubernetes client to make Kubernetes REST API requests with. Resources
    ///   will be created and deleted with this client.
    pub fn new(client: Client) -> Self {
        #[cfg(feature = "metrics")]
        {
            ContextData {
                client,
                metrics: ControllerMetrics::new("consumers"),
                last_action: Mutex::new(HashMap::new()),
            }
        }
        #[cfg(not(feature = "metrics"))]
        {
            ContextData {
                client,
                last_action: Mutex::new(HashMap::new()),
            }
        }
    }
}

/// Action to be taken upon an `Cluster` resource during reconciliation
#[derive(Debug, PartialEq, Clone)]
enum ClusterAction {
    Pending {
        reason: String,
    },

    Terminating {
        reason: String,
    },

    Starting {
        reason: String,
    },

    /// Signals that the [`Cluster`] is fully reconciled.
    Active {
        pod_name: String,
    },

    /// An error occurred during reconciliation.
    Error(String),

    /// The [`Cluster`] resource is in desired state and requires no actions to be taken.
    NoOp,

    Requeue(Duration),
}

impl ClusterAction {
    fn to_str(&self) -> &str {
        match self {
            ClusterAction::Terminating { .. } => "Terminating",
            ClusterAction::Starting { .. } => "Starting",
            ClusterAction::Active { .. } => "Active",
            ClusterAction::NoOp => "NoOp",
            ClusterAction::Error(_) => "Error",
            ClusterAction::Requeue(_) => "Requeue",
            ClusterAction::Pending { .. } => "Pending",
        }
    }
}

/// Reconciliation function for the `Cluster` resource.
async fn reconcile(instance: Arc<Cluster>, context: Arc<ContextData>) -> Result<Action, Error> {
    // The `Client` is shared -> a clone from the reference is obtained
    let client: Client = context.client.clone();

    // The resource of `Cluster` kind is required to have a namespace set. However, it is not guaranteed
    // the resource will have a `namespace` set. Therefore, the `namespace` field on object's metadata
    // is optional and Rust forces the programmer to check for it's existence first.
    let namespace: String = match instance.namespace() {
        None => {
            // If there is no namespace to deploy to defined, reconciliation ends with an error immediately.
            return Err(Error::UserInput(
                "Expected Cluster resource to be namespaced. Can't deploy to an unknown namespace."
                    .to_owned(),
            ));
        }
        // If namespace is known, proceed. In a more advanced version of the operator, perhaps
        // the namespace could be checked for existence first.
        Some(namespace) => namespace,
    };

    // Name of the Cluster resource is used to name the subresources as well.
    let name = instance.name_any();

    // Increment total number of reconciles for the Cluster resource.
    #[cfg(feature = "metrics")]
    context
        .metrics
        .reconcile_counter
        .with_label_values(&[&name, &namespace])
        .inc();

    // Benchmark the read phase of reconciliation.
    #[cfg(feature = "metrics")]
    let start = std::time::Instant::now();

    // Read phase of reconciliation determines goal during the write phase.
    let action = determine_action(client.clone(), &name, &namespace, &instance).await?;

    if action != ClusterAction::NoOp {
        let value = {
            let mut la = context.last_action.lock().await;
            la.insert(
                (namespace.clone(), name.clone()),
                (action.clone(), Instant::now()),
            )
        };
        if let Some((last_action, last_instant)) = value
            && (Some(&action) != Some(&last_action)
                || last_instant.elapsed() > Duration::from_secs(300))
        {
            println!(
                "🔧 {}{}{}{}{}",
                namespace.color(FG2),
                "/".color(FG1),
                name.color(FG2),
                " ACTION: ".color(FG1),
                format!("{:?}", action).color(FG2),
            );
        }
    }

    // Report the read phase performance.
    #[cfg(feature = "metrics")]
    context
        .metrics
        .read_histogram
        .with_label_values(&[&name, &namespace, action.to_str()])
        .observe(start.elapsed().as_secs_f64());

    // Increment the counter for the action.
    #[cfg(feature = "metrics")]
    context
        .metrics
        .action_counter
        .with_label_values(&[&name, &namespace, action.to_str()])
        .inc();

    // Benchmark the write phase of reconciliation.
    #[cfg(feature = "metrics")]
    let timer = match action {
        // Don't measure performance for NoOp actions.
        ClusterAction::NoOp => None,
        // Start a performance timer for the write phase.
        _ => Some(
            context
                .metrics
                .write_histogram
                .with_label_values(&[&name, &namespace, action.to_str()])
                .start_timer(),
        ),
    };

    // Performs action as decided by the `determine_action` function.
    // This is the write phase of reconciliation.
    let result = match action {
        ClusterAction::Requeue(duration) => Action::requeue(duration),
        ClusterAction::Terminating { reason } => {
            //actions::terminating(client, &instance, reason).await?;
            Action::await_change()
        }
        ClusterAction::Pending { reason } => {
            actions::pending(client, &instance, reason).await?;
            Action::await_change()
        }
        ClusterAction::Starting { reason } => {
            //actions::starting(client, &instance, reason).await?;
            Action::await_change()
        }
        ClusterAction::Error(message) => {
            actions::error(client.clone(), &instance, message).await?;
            Action::await_change()
        }
        ClusterAction::Active { pod_name } => {
            actions::active(client, &instance, &pod_name).await?;
            Action::requeue(PROBE_INTERVAL)
        }
        ClusterAction::NoOp => Action::requeue(PROBE_INTERVAL),
    };

    #[cfg(feature = "metrics")]
    if let Some(timer) = timer {
        timer.observe_duration();
    }

    Ok(result)
}

/// Resources arrives into reconciliation queue in a certain state. This function looks at
/// the state of given `Cluster` resource and decides which actions needs to be performed.
/// The finite set of possible actions is represented by the `ClusterAction` enum.
///
/// # Arguments
/// - `instance`: A reference to `Cluster` being reconciled to decide next action upon.
async fn determine_action(
    client: Client,
    _name: &str,
    namespace: &str,
    instance: &Cluster,
) -> Result<ClusterAction, Error> {
    // Don't do anything while being deleted.
    if instance.metadata.deletion_timestamp.is_some() {
        return Ok(ClusterAction::Requeue(Duration::from_secs(2)));
    }

    unimplemented!()
}

/// Returns the phase of the Cluster.
pub fn get_phase(instance: &Cluster) -> Option<ClusterPhase> {
    instance.status.as_ref().map(|status| status.phase)
}

pub fn get_last_updated(instance: &Cluster) -> Option<Duration> {
    let Some(status) = instance.status.as_ref() else {
        return None;
    };
    let Some(last_updated) = status.last_updated.as_ref() else {
        return None;
    };
    let age = Timestamp::now().duration_since(last_updated.0);
    let Ok(age) = age.try_into() else {
        return None;
    };
    Some(age)
}

/// Actions to be taken when a reconciliation fails - for whatever reason.
/// Prints out the error to `stderr` and requeues the resource for another reconciliation after
/// five seconds.
///
/// # Arguments
/// - `instance`: The erroneous resource.
/// - `error`: A reference to the `kube::Error` that occurred during reconciliation.
/// - `_context`: Unused argument. Context Data "injected" automatically by kube-rs.
fn on_error(instance: Arc<Cluster>, error: &Error, _context: Arc<ContextData>) -> Action {
    eprintln!(
        "{}",
        format!("Reconciliation error: {:?} {:?}", error, instance).red()
    );
    Action::requeue(Duration::from_secs(5))
}
