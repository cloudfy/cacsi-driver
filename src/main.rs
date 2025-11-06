use anyhow::Result;
use std::env;
use std::path::PathBuf;
use tokio::signal;
use tonic::transport::Server;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod csi;
mod cert_manager;
mod ca_manager;
mod cert_monitor;
mod k8s_client;

use csi::{identity::IdentityService, node::NodeService};
use cert_monitor::CertificateMonitor;

// Include generated protobuf code
pub mod proto {
    pub mod csi {
        tonic::include_proto!("csi.v1");
    }
    pub mod certservice {
        tonic::include_proto!("certservice.v1");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting CSI Certificate Driver");

    // Get configuration from environment variables
    let socket_path = env::var("CSI_ENDPOINT")
        .unwrap_or_else(|_| "unix:///csi/csi.sock".to_string());
    let node_id = env::var("NODE_ID")
        .unwrap_or_else(|_| hostname::get()
            .unwrap()
            .to_string_lossy()
            .to_string());
    let cert_service_addr = env::var("CERT_SERVICE_ADDR")
        .unwrap_or_else(|_| "http://cacsi-service:50051".to_string());
    let ca_secret_name = env::var("CA_SECRET_NAME")
        .unwrap_or_else(|_| "csi-ca-secret".to_string());
    let ca_secret_namespace = env::var("CA_SECRET_NAMESPACE")
        .unwrap_or_else(|_| "kube-system".to_string());
    let cert_base_path = env::var("CERT_BASE_PATH")
        .unwrap_or_else(|_| "/var/lib/csi-certs".to_string());
    let cluster_domain = env::var("CLUSTER_DOMAIN")
        .unwrap_or_else(|_| "cluster.local".to_string());

    info!("Configuration:");
    info!("  Socket: {}", socket_path);
    info!("  Node ID: {}", node_id);
    info!("  Cert Service: {}", cert_service_addr);
    info!("  CA Secret: {}/{}", ca_secret_namespace, ca_secret_name);
    info!("  Cert Base Path: {}", cert_base_path);
    info!("  Cluster Domain: {}", cluster_domain);

    // Initialize CA manager
    let ca_manager = ca_manager::CaManager::new(
        ca_secret_name,
        ca_secret_namespace,
    ).await?;

    // Initialize certificate manager
    let cert_manager = cert_manager::CertificateManager::new(
        PathBuf::from(cert_base_path),
        cert_service_addr.clone(),
    );

    // Initialize certificate monitor
    let cert_monitor = CertificateMonitor::new(
        cert_manager.clone(),
        ca_manager.clone(),
    );

    // Start certificate monitoring in background
    let monitor_handle = tokio::spawn(async move {
        if let Err(e) = cert_monitor.start().await {
            error!("Certificate monitor error: {}", e);
        }
    });

    // Create CSI services
    let identity_service = IdentityService::new();
    let node_service = NodeService::new(
        node_id,
        cert_manager,
        ca_manager,
        cluster_domain,
    );

    // Parse socket path
    let socket_path = socket_path
        .strip_prefix("unix://")
        .unwrap_or(&socket_path);

    // Remove existing socket file if it exists
    let _ = std::fs::remove_file(socket_path);

    // Create socket directory if it doesn't exist
    if let Some(parent) = PathBuf::from(socket_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create UDS listener
    let uds = tokio::net::UnixListener::bind(socket_path)?;
    let uds_stream = tokio_stream::wrappers::UnixListenerStream::new(uds);

    info!("CSI driver listening on {}", socket_path);

    // Start gRPC server
    Server::builder()
        .add_service(proto::csi::identity_server::IdentityServer::new(identity_service))
        .add_service(proto::csi::node_server::NodeServer::new(node_service))
        .serve_with_incoming_shutdown(uds_stream, async {
            signal::ctrl_c().await.ok();
            info!("Received shutdown signal");
        })
        .await?;

    // Wait for monitor to finish
    monitor_handle.abort();

    info!("CSI driver shutdown complete");
    Ok(())
}
