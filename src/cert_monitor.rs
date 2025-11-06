use anyhow::Result;
use chrono::Utc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, warn};

use crate::cert_manager::CertificateManager;
use crate::ca_manager::CaManager;

pub struct CertificateMonitor {
    cert_manager: CertificateManager,
    ca_manager: CaManager,
    check_interval: Duration,
}

impl CertificateMonitor {
    pub fn new(cert_manager: CertificateManager, ca_manager: CaManager) -> Self {
        Self {
            cert_manager,
            ca_manager,
            check_interval: Duration::from_secs(300), // Check every 5 minutes
        }
    }

    /// Start the certificate monitoring service
    pub async fn start(&self) -> Result<()> {
        info!("Starting certificate monitor");

        loop {
            if let Err(e) = self.check_and_renew_certificates().await {
                error!("Error checking certificates: {}", e);
            }

            sleep(self.check_interval).await;
        }
    }

    /// Check all registered certificates and renew if needed
    async fn check_and_renew_certificates(&self) -> Result<()> {
        let certificates = self.cert_manager.get_all_certificates();
        
        if certificates.is_empty() {
            return Ok(());
        }

        info!("Checking {} certificates for renewal", certificates.len());

        let now = Utc::now().timestamp();

        for cert_info in certificates {
            // Check if certificate needs renewal
            if self.cert_manager.needs_renewal(cert_info.not_before, cert_info.not_after) {
                warn!(
                    "Certificate {} needs renewal (expires at: {})",
                    cert_info.cert_id,
                    chrono::DateTime::from_timestamp(cert_info.not_after, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_else(|| "unknown".to_string())
                );

                // Attempt renewal
                match self.renew_certificate(&cert_info).await {
                    Ok(_) => {
                        info!("Successfully renewed certificate: {}", cert_info.cert_id);
                    }
                    Err(e) => {
                        error!("Failed to renew certificate {}: {}", cert_info.cert_id, e);
                    }
                }
            } else {
                let remaining_secs = cert_info.not_after - now;
                let remaining_days = remaining_secs / 86400;
                
                if remaining_days <= 2 {
                    warn!(
                        "Certificate {} expires in {} days",
                        cert_info.cert_id,
                        remaining_days
                    );
                }
            }
        }

        Ok(())
    }

    /// Renew a specific certificate
    async fn renew_certificate(&self, cert_info: &crate::cert_manager::CertificateInfo) -> Result<()> {
        info!("Renewing certificate: {}", cert_info.cert_id);

        // Request renewal from certificate service
        let (cert_pem, key_pem, not_before, not_after) = self
            .cert_manager
            .renew_certificate(&cert_info.cert_id, 7) // 7 days validity
            .await?;

        // Update certificate files on disk
        self.cert_manager
            .update_certificate_files(&cert_info.mount_path, &cert_pem, &key_pem)
            .await?;

        // Update certificate metadata
        self.cert_manager
            .register_certificate(
                cert_info.cert_id.clone(),
                cert_info.mount_path.clone(),
                not_before,
                not_after,
            )
            .await;

        info!("Certificate renewed successfully: {}", cert_info.cert_id);

        Ok(())
    }
}
