use anyhow::{Result, Context};
use kube::{Api, Client};
use k8s_openapi::api::core::v1::Secret;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Manages the CA certificate and key retrieved from Kubernetes secret
/// The CA never leaves the node and is stored in memory
#[derive(Clone)]
pub struct CaManager {
    secret_name: String,
    secret_namespace: String,
    ca_cert: Arc<RwLock<Option<String>>>,
    ca_key: Arc<RwLock<Option<String>>>,
}

impl CaManager {
    pub async fn new(secret_name: String, secret_namespace: String) -> Result<Self> {
        let manager = Self {
            secret_name,
            secret_namespace,
            ca_cert: Arc::new(RwLock::new(None)),
            ca_key: Arc::new(RwLock::new(None)),
        };

        // Load CA from Kubernetes secret
        manager.load_ca().await?;

        Ok(manager)
    }

    /// Load CA certificate and key from Kubernetes secret
    async fn load_ca(&self) -> Result<()> {
        info!("Loading CA from secret: {}/{}", self.secret_namespace, self.secret_name);

        let client = Client::try_default()
            .await
            .context("Failed to create Kubernetes client")?;

        let secrets: Api<Secret> = Api::namespaced(client, &self.secret_namespace);

        let secret = secrets
            .get(&self.secret_name)
            .await
            .context("Failed to get CA secret")?;

        let data = secret
            .data
            .ok_or_else(|| anyhow::anyhow!("Secret has no data"))?;

        // Extract CA certificate
        let ca_cert_bytes = data
            .get("tls.crt")
            .ok_or_else(|| anyhow::anyhow!("Secret missing tls.crt"))?;
        
        let ca_cert = String::from_utf8(ca_cert_bytes.0.clone())
            .context("Invalid UTF-8 in CA certificate")?;

        // Extract CA key
        let ca_key_bytes = data
            .get("tls.key")
            .ok_or_else(|| anyhow::anyhow!("Secret missing tls.key"))?;
        
        let ca_key = String::from_utf8(ca_key_bytes.0.clone())
            .context("Invalid UTF-8 in CA key")?;

        // Store in memory (never written to disk)
        *self.ca_cert.write().await = Some(ca_cert);
        *self.ca_key.write().await = Some(ca_key);

        info!("CA loaded successfully from secret");

        Ok(())
    }

    /// Get CA certificate (PEM format)
    pub async fn get_ca_cert(&self) -> Result<String> {
        self.ca_cert
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("CA certificate not loaded"))
    }

    /// Get CA private key (PEM format)
    pub async fn get_ca_key(&self) -> Result<String> {
        self.ca_key
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("CA key not loaded"))
    }

    /// Reload CA from Kubernetes secret (for rotation scenarios)
    pub async fn reload_ca(&self) -> Result<()> {
        info!("Reloading CA from secret");
        self.load_ca().await
    }

    /// Check if CA is loaded
    pub async fn is_loaded(&self) -> bool {
        self.ca_cert.read().await.is_some() && self.ca_key.read().await.is_some()
    }
}
