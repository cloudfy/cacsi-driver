use anyhow::Result;
use std::env;
use std::net::SocketAddr;
use tokio::signal;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod service;

// Include generated protobuf code
pub mod proto {
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

    info!("Starting Certificate Service");

    // Get configuration from environment variables
    let listen_addr = env::var("LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string());
    let ca_secret_name = env::var("CA_SECRET_NAME")
        .unwrap_or_else(|_| "csi-ca-secret".to_string());
    let ca_secret_namespace = env::var("CA_SECRET_NAMESPACE")
        .unwrap_or_else(|_| "kube-system".to_string());

    info!("Configuration:");
    info!("  Listen Address: {}", listen_addr);
    info!("  CA Secret: {}/{}", ca_secret_namespace, ca_secret_name);

    // Parse listen address
    let addr: SocketAddr = listen_addr
        .parse()
        .expect("Invalid listen address");

    // Create certificate service
    let cert_service = service::CertificateServiceImpl::new(
        ca_secret_name,
        ca_secret_namespace,
    ).await?;

    info!("Certificate service listening on {}", addr);

    // Start gRPC server
    Server::builder()
        .add_service(proto::certservice::certificate_service_server::CertificateServiceServer::new(cert_service))
        .serve_with_shutdown(addr, async {
            signal::ctrl_c().await.ok();
            info!("Received shutdown signal");
        })
        .await?;

    info!("Certificate service shutdown complete");
    Ok(())
}
