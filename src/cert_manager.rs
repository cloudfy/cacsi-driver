use anyhow::{Result, Context};
use chrono::Utc;
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error};

use crate::proto::certservice::{
    certificate_service_client::CertificateServiceClient,
    IssueCertificateRequest, RenewCertificateRequest,
};

#[derive(Clone)]
pub struct CertificateInfo {
    pub cert_id: String,
    pub mount_path: String,
    pub not_before: i64,
    pub not_after: i64,
}

#[derive(Clone)]
pub struct CertificateManager {
    base_path: PathBuf,
    cert_service_addr: String,
    certificates: Arc<DashMap<String, CertificateInfo>>,
}

impl CertificateManager {
    pub fn new(base_path: PathBuf, cert_service_addr: String) -> Self {
        Self {
            base_path,
            cert_service_addr,
            certificates: Arc::new(DashMap::new()),
        }
    }

    /// Issue a new certificate via the certificate service
    pub async fn issue_certificate(
        &self,
        cert_id: &str,
        common_name: &str,
        dns_names: Vec<String>,
        ip_addresses: Vec<String>,
        validity_days: i64,
    ) -> Result<(String, String, i64, i64)> {
        info!("Issuing certificate for: {}", cert_id);
        
        // Ensure the address has a proper scheme
        let addr = if !self.cert_service_addr.starts_with("http://") && !self.cert_service_addr.starts_with("https://") {
            format!("http://{}", self.cert_service_addr)
        } else {
            self.cert_service_addr.clone()
        };
        
        info!("Connecting to certificate service at: {}", addr);

        let mut client = CertificateServiceClient::connect(addr.clone())
            .await
            .context(format!("Failed to connect to certificate service at {}", addr))?;

        // Build request for certificate issuance
        let request = IssueCertificateRequest {
            certificate_id: cert_id.to_string(),
            common_name: common_name.to_string(),
            dns_names,
            ip_addresses,
            validity_days,
            metadata: std::collections::HashMap::new(),
            organizational_units: vec![],
        };

        let response = client
            .issue_certificate(request)
            .await
            .context("Failed to issue certificate")?
            .into_inner();

        info!("Certificate issued: {}", response.certificate_id);

        Ok((
            response.certificate_pem,
            response.private_key_pem,
            response.not_before,
            response.not_after,
        ))
    }

    /// Renew an existing certificate
    pub async fn renew_certificate(
        &self,
        cert_id: &str,
        validity_days: i64,
    ) -> Result<(String, String, i64, i64)> {
        info!("Renewing certificate: {}", cert_id);
        
        // Ensure the address has a proper scheme
        let addr = if !self.cert_service_addr.starts_with("http://") && !self.cert_service_addr.starts_with("https://") {
            format!("http://{}", self.cert_service_addr)
        } else {
            self.cert_service_addr.clone()
        };
        
        info!("Connecting to certificate service at: {}", addr);

        let endpoint = tonic::transport::Endpoint::from_shared(addr.clone())
            .context("Invalid endpoint URL")?
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5));

        let mut client = match CertificateServiceClient::connect(endpoint).await {
            Ok(client) => client,
            Err(e) => {
                error!("Connection error details: {:?}", e);
                return Err(anyhow::anyhow!("Failed to connect to certificate service at {}: {}", addr, e));
            }
        };

        let request = RenewCertificateRequest {
            certificate_id: cert_id.to_string(),
            validity_days,
        };

        let response = client
            .renew_certificate(request)
            .await
            .context("Failed to renew certificate")?
            .into_inner();

        info!("Certificate renewed: {}", cert_id);

        Ok((
            response.certificate_pem,
            response.private_key_pem,
            response.not_before,
            response.not_after,
        ))
    }

    /// Register a certificate for monitoring
    pub async fn register_certificate(
        &self,
        cert_id: String,
        mount_path: String,
        not_before: i64,
        not_after: i64,
    ) {
        let info = CertificateInfo {
            cert_id: cert_id.clone(),
            mount_path,
            not_before,
            not_after,
        };

        self.certificates.insert(cert_id.clone(), info);
        info!("Registered certificate for monitoring: {}", cert_id);
    }

    /// Unregister a certificate from monitoring
    pub async fn unregister_certificate(&self, cert_id: &str) {
        self.certificates.remove(cert_id);
        info!("Unregistered certificate: {}", cert_id);
    }

    /// Get all registered certificates
    pub fn get_all_certificates(&self) -> Vec<CertificateInfo> {
        self.certificates
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Update certificate files on disk
    pub async fn update_certificate_files(
        &self,
        mount_path: &str,
        cert_pem: &str,
        key_pem: &str,
    ) -> Result<()> {
        let cert_path = std::path::Path::new(mount_path).join("tls.crt");
        let key_path = std::path::Path::new(mount_path).join("tls.key");

        // Write new certificate
        tokio::fs::write(&cert_path, cert_pem)
            .await
            .context("Failed to write certificate")?;

        // Write new key
        tokio::fs::write(&key_path, key_pem)
            .await
            .context("Failed to write key")?;

        info!("Updated certificate files at: {}", mount_path);

        Ok(())
    }

    /// Check if a certificate needs renewal (renew if < 20% of lifetime remaining)
    pub fn needs_renewal(&self, not_before: i64, not_after: i64) -> bool {
        let now = Utc::now().timestamp();
        let lifetime = not_after - not_before;
        let remaining = not_after - now;
        
        // Renew if less than 20% of lifetime remains
        let threshold = (lifetime as f64 * 0.2) as i64;
        
        remaining < threshold
    }
}
