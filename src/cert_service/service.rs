use anyhow::{Result, Context};
use chrono::{Duration, Utc};
use dashmap::DashMap;
use kube::{Api, Client};
use k8s_openapi::api::core::v1::Secret;
use rcgen::{
    CertificateParams, KeyPair, DistinguishedName,
    SanType, ExtendedKeyUsagePurpose,
    KeyUsagePurpose, DnType, CustomExtension,
};
use rustls_pki_types::CertificateDer;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{info, error, debug};
use x509_parser::prelude::{X509Certificate, FromDer};

use super::proto::certservice::{
    certificate_service_server::CertificateService,
    IssueCertificateRequest, IssueCertificateResponse,
    RenewCertificateRequest, RenewCertificateResponse,
    RevokeCertificateRequest, RevokeCertificateResponse,
    GetCertificateInfoRequest, GetCertificateInfoResponse,
};

#[derive(Clone)]
struct CertificateRecord {
    certificate_id: String,
    common_name: String,
    dns_names: Vec<String>,
    organizational_units: Vec<String>,
    not_before: i64,
    not_after: i64,
    metadata: std::collections::HashMap<String, String>,
}

pub struct CertificateServiceImpl {
    ca_secret_name: String,
    ca_secret_namespace: String,
    ca_key: Arc<tokio::sync::RwLock<Option<KeyPair>>>,
    ca_cert_pem: Arc<tokio::sync::RwLock<Option<String>>>,
    certificates: Arc<DashMap<String, CertificateRecord>>,
}

impl CertificateServiceImpl {
    pub async fn new(ca_secret_name: String, ca_secret_namespace: String) -> Result<Self> {
        let service = Self {
            ca_secret_name,
            ca_secret_namespace,
            ca_key: Arc::new(tokio::sync::RwLock::new(None)),
            ca_cert_pem: Arc::new(tokio::sync::RwLock::new(None)),
            certificates: Arc::new(DashMap::new()),
        };
        
        service.load_ca().await?;
        
        Ok(service)
    }

    async fn load_ca(&self) -> Result<()> {
        let client = Client::try_default()
            .await
            .context("Failed to create Kubernetes client")?;

        let secrets: Api<Secret> = Api::namespaced(client, &self.ca_secret_namespace);

        let secret = secrets
            .get(&self.ca_secret_name)
            .await
            .context("Failed to get CA secret")?;

        let data = secret
            .data
            .ok_or_else(|| anyhow::anyhow!("Secret has no data"))?;

        let ca_cert_pem = data
            .get("tls.crt")
            .ok_or_else(|| anyhow::anyhow!("Secret missing tls.crt"))?;
        
        let ca_cert_str = String::from_utf8(ca_cert_pem.0.clone())
            .context("Invalid UTF-8 in CA certificate")?;

        let ca_key_pem = data
            .get("tls.key")
            .ok_or_else(|| anyhow::anyhow!("Secret missing tls.key"))?;
        
        let ca_key_str = String::from_utf8(ca_key_pem.0.clone())
            .context("Invalid UTF-8 in CA key")?;
        
        let ca_keypair = KeyPair::from_pem(&ca_key_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse CA key: {}", e))?;

        *self.ca_key.write().await = Some(ca_keypair);
        *self.ca_cert_pem.write().await = Some(ca_cert_str);

        info!("CA loaded successfully from secret");

        Ok(())
    }

    async fn generate_certificate(
        &self,
        common_name: &str,
        dns_names: Vec<String>,
        ip_addresses: Vec<String>,
        organizational_units: Vec<String>,
        validity_days: i64,
    ) -> Result<(String, String, i64, i64)> {
        let ca_key_lock = self.ca_key.read().await;
        let ca_key = ca_key_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CA key not loaded"))?;
        
        let ca_pem_lock = self.ca_cert_pem.read().await;
        let ca_cert_pem_str = ca_pem_lock
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("CA certificate PEM not loaded"))?;

        // Parse CA certificate to extract DN fields
        let ca_pems = pem::parse_many(ca_cert_pem_str.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to parse CA cert PEM: {}", e))?;
        let ca_cert_pem = ca_pems.into_iter().next()
            .ok_or_else(|| anyhow::anyhow!("No certificate in PEM"))?;
        let ca_cert_der = CertificateDer::from(ca_cert_pem.contents().to_vec());
        
        let (_, ca_cert) = X509Certificate::from_der(&ca_cert_der)
            .map_err(|e| anyhow::anyhow!("Failed to parse CA certificate: {}", e))?;
        
        let ca_org = ca_cert.subject().iter_organization().next().and_then(|o| o.as_str().ok()).map(|s| s);
        let ca_country = ca_cert.subject().iter_country().next().and_then(|c| c.as_str().ok()).map(|s| s);

        let server_kp = KeyPair::generate()
            .map_err(|e| anyhow::anyhow!("Failed to generate server key pair: {}", e))?;

        let mut server_params = CertificateParams::default();

        // Build DN in standard X.509 order
        server_params.distinguished_name.push(DnType::CountryName, ca_country.unwrap_or("DK"));
        server_params.distinguished_name.push(DnType::OrganizationName, ca_org.unwrap_or("Akuzo"));
        
        // Handle organizational units
        // NOTE: rcgen 0.14 has a CRITICAL LIMITATION where DistinguishedName uses a BTreeMap<DnType, DnValue>,
        // which fundamentally cannot store multiple values for the same key (DnType::OrganizationalUnitName).
        //
        // WORKAROUND: Join all OUs into a single OU field separated by " + "
        // This is a valid X.509 DN representation where multiple values can be combined.
        // Example: OU=t:tenantid + e:environment + n:sandbox
        debug!("Processing {} organizational units", organizational_units.len());
        
        if !organizational_units.is_empty() {
            let combined_ou = organizational_units.join(" + ");
            debug!("Combined OUs into single field: '{}'", combined_ou);
            server_params.distinguished_name.push(DnType::OrganizationalUnitName, combined_ou.as_str());
            info!("Added combined OU with {} components: {}", organizational_units.len(), combined_ou);
        }
        
        server_params.distinguished_name.push(DnType::CommonName, common_name);

        server_params.subject_alt_names = dns_names
            .iter()
            .map(|name| SanType::DnsName(rcgen::string::Ia5String::try_from(name.as_str()).unwrap()))
            .collect();

        for ip in ip_addresses {
            if let Ok(addr) = ip.parse() {
                server_params.subject_alt_names.push(SanType::IpAddress(addr));
            }
        }

        server_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
            KeyUsagePurpose::KeyAgreement,
        ];

        server_params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];

        server_params.is_ca = rcgen::IsCa::NoCa;

        let not_before = Utc::now();
        let not_after = not_before + Duration::days(validity_days);
        
        use std::time::SystemTime;
        let not_before_system: SystemTime = not_before.into();
        let not_after_system: SystemTime = not_after.into();
        
        server_params.not_before = time::OffsetDateTime::from(not_before_system);
        server_params.not_after = time::OffsetDateTime::from(not_after_system);

        // Sign the server certificate with the CA
        let ca_issuer = rcgen::Issuer::from_ca_cert_der(&ca_cert_der, ca_key)
            .map_err(|e| anyhow::anyhow!("Failed to create issuer from CA cert: {}", e))?;
        let server_cert_signed = server_params.signed_by(&server_kp, &ca_issuer)
            .map_err(|e| anyhow::anyhow!("Failed to sign certificate with CA: {}", e))?;
        let server_cert_der: Vec<u8> = server_cert_signed.der().to_vec();
        
        let server_cert_pem = pem::encode(&pem::Pem::new("CERTIFICATE", server_cert_der));
        let server_key_pem = server_kp.serialize_pem();

        // 02 - bug, do not include CA cert in chain for now
        //let cert_chain = format!("{}\n{}", server_cert_pem.trim(), ca_cert_pem_str.trim());

        Ok((
            server_cert_pem, //cert_chain,
            server_key_pem,
            not_before.timestamp(),
            not_after.timestamp(),
        ))
    }
}

#[tonic::async_trait]
impl CertificateService for CertificateServiceImpl {
    async fn issue_certificate(
        &self,
        request: Request<IssueCertificateRequest>,
    ) -> Result<Response<IssueCertificateResponse>, Status> {
        let req = request.into_inner();
        
        info!("Issuing certificate: {}", req.certificate_id);
        debug!("Common name: {}", req.common_name);
        debug!("DNS names: {:?}", req.dns_names);
        debug!("Organizational units: {:?}", req.organizational_units);

        match self
            .generate_certificate(
                &req.common_name,
                req.dns_names.clone(),
                req.ip_addresses.clone(),
                req.organizational_units.clone(),
                req.validity_days,
            )
            .await
        {
            Ok((cert_pem, key_pem, not_before, not_after)) => {
                let record = CertificateRecord {
                    certificate_id: req.certificate_id.clone(),
                    common_name: req.common_name.clone(),
                    dns_names: req.dns_names.clone(),
                    organizational_units: req.organizational_units.clone(),
                    not_before,
                    not_after,
                    metadata: req.metadata.clone(),
                };

                self.certificates.insert(req.certificate_id.clone(), record);

                info!("Certificate issued successfully: {}", req.certificate_id);

                let response = IssueCertificateResponse {
                    certificate_pem: cert_pem,
                    private_key_pem: key_pem,
                    certificate_id: req.certificate_id,
                    not_before,
                    not_after,
                };

                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Failed to issue certificate: {}", e);
                Err(Status::internal(format!("Failed to issue certificate: {}", e)))
            }
        }
    }

    async fn renew_certificate(
        &self,
        request: Request<RenewCertificateRequest>,
    ) -> Result<Response<RenewCertificateResponse>, Status> {
        let req = request.into_inner();
        
        info!("Renewing certificate: {}", req.certificate_id);

        let existing = self
            .certificates
            .get(&req.certificate_id)
            .ok_or_else(|| Status::not_found("Certificate not found"))?;

        let common_name = existing.common_name.clone();
        let dns_names = existing.dns_names.clone();
        let organizational_units = existing.organizational_units.clone();
        
        drop(existing);

        match self
            .generate_certificate(
                &common_name,
                dns_names.clone(),
                vec![],
                organizational_units.clone(),
                req.validity_days,
            )
            .await
        {
            Ok((cert_pem, key_pem, not_before, not_after)) => {
                if let Some(mut record) = self.certificates.get_mut(&req.certificate_id) {
                    record.not_before = not_before;
                    record.not_after = not_after;
                }

                info!("Certificate renewed successfully: {}", req.certificate_id);

                let response = RenewCertificateResponse {
                    certificate_pem: cert_pem,
                    private_key_pem: key_pem,
                    not_before,
                    not_after,
                };

                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Failed to renew certificate: {}", e);
                Err(Status::internal(format!("Failed to renew certificate: {}", e)))
            }
        }
    }

    async fn revoke_certificate(
        &self,
        request: Request<RevokeCertificateRequest>,
    ) -> Result<Response<RevokeCertificateResponse>, Status> {
        let req = request.into_inner();
        
        info!("Revoking certificate: {}", req.certificate_id);

        self.certificates.remove(&req.certificate_id);

        let response = RevokeCertificateResponse {
            success: true,
        };

        Ok(Response::new(response))
    }

    async fn get_certificate_info(
        &self,
        request: Request<GetCertificateInfoRequest>,
    ) -> Result<Response<GetCertificateInfoResponse>, Status> {
        let req = request.into_inner();
        
        debug!("Getting certificate info: {}", req.certificate_id);

        let record = self
            .certificates
            .get(&req.certificate_id)
            .ok_or_else(|| Status::not_found("Certificate not found"))?;

        let now = Utc::now().timestamp();
        let is_valid = now >= record.not_before && now <= record.not_after;

        let response = GetCertificateInfoResponse {
            certificate_id: record.certificate_id.clone(),
            common_name: record.common_name.clone(),
            dns_names: record.dns_names.clone(),
            not_before: record.not_before,
            not_after: record.not_after,
            is_valid,
            metadata: record.metadata.clone(),
        };

        Ok(Response::new(response))
    }
}
