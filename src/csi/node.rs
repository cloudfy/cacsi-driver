use std::collections::HashMap;
use tonic::{Request, Response, Status};
use tracing::{info, error, debug};

use crate::proto::csi::{
    node_server::Node,
    NodeStageVolumeRequest, NodeStageVolumeResponse,
    NodeUnstageVolumeRequest, NodeUnstageVolumeResponse,
    NodePublishVolumeRequest, NodePublishVolumeResponse,
    NodeUnpublishVolumeRequest, NodeUnpublishVolumeResponse,
    NodeGetVolumeStatsRequest, NodeGetVolumeStatsResponse,
    NodeExpandVolumeRequest, NodeExpandVolumeResponse,
    NodeGetCapabilitiesRequest, NodeGetCapabilitiesResponse,
    NodeGetInfoRequest, NodeGetInfoResponse,
};

use crate::cert_manager::CertificateManager;
use crate::ca_manager::CaManager;
use crate::template_parser::TemplateParser;

pub struct NodeService {
    node_id: String,
    cert_manager: CertificateManager,
    ca_manager: CaManager,
    cluster_domain: String,
    template_parser: TemplateParser,
}

impl NodeService {
    pub fn new(
        node_id: String,
        cert_manager: CertificateManager,
        ca_manager: CaManager,
        cluster_domain: String,
    ) -> Self {
        Self {
            node_id,
            cert_manager,
            ca_manager,
            cluster_domain,
            template_parser: TemplateParser::default(),
        }
    }

    fn extract_pod_info(&self, volume_context: &HashMap<String, String>) -> Result<(String, String), Status> {
        let pod_namespace = volume_context
            .get("csi.storage.k8s.io/pod.namespace")
            .ok_or_else(|| Status::invalid_argument("Missing pod namespace"))?;
        
        let pod_name = volume_context
            .get("csi.storage.k8s.io/pod.name")
            .ok_or_else(|| Status::invalid_argument("Missing pod name"))?;

        Ok((pod_namespace.clone(), pod_name.clone()))
    }
}

#[tonic::async_trait]
impl Node for NodeService {
    async fn node_stage_volume(
        &self,
        _request: Request<NodeStageVolumeRequest>,
    ) -> Result<Response<NodeStageVolumeResponse>, Status> {
        // Not needed for ephemeral volumes
        Ok(Response::new(NodeStageVolumeResponse {}))
    }

    async fn node_unstage_volume(
        &self,
        _request: Request<NodeUnstageVolumeRequest>,
    ) -> Result<Response<NodeUnstageVolumeResponse>, Status> {
        // Not needed for ephemeral volumes
        Ok(Response::new(NodeUnstageVolumeResponse {}))
    }

    async fn node_publish_volume(
        &self,
        request: Request<NodePublishVolumeRequest>,
    ) -> Result<Response<NodePublishVolumeResponse>, Status> {
        let req = request.into_inner();
        
        info!("NodePublishVolume called for volume: {}", req.volume_id);
        debug!("Target path: {}", req.target_path);
        debug!("Volume context: {:?}", req.volume_context);

        // Extract pod information from volume context
        let (pod_namespace, pod_name) = self.extract_pod_info(&req.volume_context)?;
        
        info!("Publishing volume for pod: {}/{}", pod_namespace, pod_name);

        // Generate certificate ID from pod info and volume ID
        let cert_id = format!("{}-{}-{}", pod_namespace, pod_name, req.volume_id);

        // Fetch pod details from Kubernetes API once for all template resolution
        let needs_pod_info = req.volume_context.get("cn_template").map(|t| self.template_parser.has_templates(t)).unwrap_or(false)
            || req.volume_context.get("organizational_units").map(|ou| self.template_parser.has_templates(ou)).unwrap_or(false);
        
        let (pod_metadata, pod_spec) = if needs_pod_info {
            let client = crate::k8s_client::get_client()
                .await
                .map_err(|e| Status::internal(format!("Failed to get Kubernetes client: {}", e)))?;
            
            crate::k8s_client::get_pod_info(&client, &pod_namespace, &pod_name)
                .await
                .map_err(|e| Status::internal(format!("Failed to get pod info: {}", e)))?
        } else {
            (HashMap::new(), HashMap::new())
        };

        // Determine the common name (CN) to use
        let common_name = if let Some(cn_template) = req.volume_context.get("cn_template") {
            // CN template is provided - resolve it using pod information
            info!("Using CN template: {}", cn_template);
            
            // Resolve template
            self.template_parser.resolve(cn_template, &pod_metadata, &pod_spec)
                .map_err(|e| Status::invalid_argument(format!("Failed to resolve CN template: {}", e)))?
        } else {
            // Default CN format: pod-name.namespace.svc.cluster-domain
            format!("{}.{}.svc.{}", pod_name, pod_namespace, self.cluster_domain)
        };

        info!("Certificate CN: {}", common_name);

        // Create target directory
        tokio::fs::create_dir_all(&req.target_path)
            .await
            .map_err(|e| Status::internal(format!("Failed to create target path: {}", e)))?;

        // Extract validity_days from volume attributes (default: 7 days)
        let validity_days = match req.volume_context.get("validity_days") {
            Some(v_str) => {
                match v_str.parse::<i64>() {
                    Ok(days) if days > 0 => days,
                    Ok(days) => {
                        error!("Invalid validity_days value (must be positive): {}", days);
                        return Err(Status::invalid_argument(format!("validity_days must be a positive integer, got {}", days)));
                    }
                    Err(e) => {
                        error!("Failed to parse validity_days '{}': {}", v_str, e);
                        return Err(Status::invalid_argument(format!("validity_days must be a positive integer, got '{}'", v_str)));
                    }
                }
            }
            None => 7,
        };

        // Extract organizational_units from volume attributes (optional, comma-separated)
        // Format can be either:
        // - Simple values: "IT, Engineering, Security"
        // - Key-value pairs: "t:tenantid, e:environment, n:{metadata.namespace}"
        // Template placeholders will be resolved
        let organizational_units = match req.volume_context.get("organizational_units") {
            Some(ou_str) => {
                // Parse each OU entry
                let mut parsed_ous = Vec::new();
                for ou_entry in ou_str.split(',') {
                    let trimmed = ou_entry.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    
                    // Check if this is a key-value pair (e.g., "t:tenantid" or "n:{metadata.namespace}")
                    let ou_value = if let Some(colon_pos) = trimmed.find(':') {
                        // Extract the value part after the colon
                        let value_part = trimmed[colon_pos + 1..].trim();
                        
                        // Check if value contains templates and resolve them
                        if self.template_parser.has_templates(value_part) {
                            match self.template_parser.resolve(value_part, &pod_metadata, &pod_spec) {
                                Ok(resolved) => resolved,
                                Err(e) => {
                                    error!("Failed to resolve OU template '{}': {}", value_part, e);
                                    return Err(Status::invalid_argument(format!("Failed to resolve OU template '{}': {}", value_part, e)));
                                }
                            }
                        } else {
                            value_part.to_string()
                        }
                    } else {
                        // No colon, treat as simple value
                        // Check if it contains templates and resolve them
                        if self.template_parser.has_templates(trimmed) {
                            match self.template_parser.resolve(trimmed, &pod_metadata, &pod_spec) {
                                Ok(resolved) => resolved,
                                Err(e) => {
                                    error!("Failed to resolve OU template '{}': {}", trimmed, e);
                                    return Err(Status::invalid_argument(format!("Failed to resolve OU template '{}': {}", trimmed, e)));
                                }
                            }
                        } else {
                            trimmed.to_string()
                        }
                    };
                    
                    parsed_ous.push(ou_value);
                }
                
                parsed_ous
            }
            None => vec![],
        };

        if !organizational_units.is_empty() {
            info!("Organizational units: {:?}", organizational_units);
        }

        // Request certificate from certificate service
        match self.cert_manager.issue_certificate(
            &cert_id,
            &common_name,
            vec![pod_name.clone()],
            vec![],
            organizational_units,
            validity_days,
        ).await {
            Ok((cert_pem, key_pem, not_before, not_after)) => {
                info!("Certificate issued for {}", cert_id);
                
                // Write certificate and key to target path
                let cert_path = std::path::Path::new(&req.target_path).join("tls.crt");
                let key_path = std::path::Path::new(&req.target_path).join("tls.key");

                tokio::fs::write(&cert_path, cert_pem)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to write certificate: {}", e)))?;

                tokio::fs::write(&key_path, key_pem)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to write key: {}", e)))?;

                // Store certificate metadata for monitoring
                self.cert_manager.register_certificate(
                    cert_id.clone(),
                    req.target_path.clone(),
                    not_before,
                    not_after,
                ).await;

                info!("Certificate written to {}", req.target_path);
                
                Ok(Response::new(NodePublishVolumeResponse {}))
            }
            Err(e) => {
                error!("Failed to issue certificate: {}", e);
                Err(Status::internal(format!("Failed to issue certificate: {}", e)))
            }
        }
    }

    async fn node_unpublish_volume(
        &self,
        request: Request<NodeUnpublishVolumeRequest>,
    ) -> Result<Response<NodeUnpublishVolumeResponse>, Status> {
        let req = request.into_inner();
        
        info!("NodeUnpublishVolume called for volume: {}", req.volume_id);

        // Unregister certificate from monitoring
        self.cert_manager.unregister_certificate(&req.volume_id).await;

        // Remove target directory
        if let Err(e) = tokio::fs::remove_dir_all(&req.target_path).await {
            error!("Failed to remove target path: {}", e);
            // Don't fail the operation if cleanup fails
        }

        info!("Volume unpublished: {}", req.volume_id);

        Ok(Response::new(NodeUnpublishVolumeResponse {}))
    }

    async fn node_get_volume_stats(
        &self,
        _request: Request<NodeGetVolumeStatsRequest>,
    ) -> Result<Response<NodeGetVolumeStatsResponse>, Status> {
        // Not implemented for ephemeral volumes
        Err(Status::unimplemented("Volume stats not supported"))
    }

    async fn node_expand_volume(
        &self,
        _request: Request<NodeExpandVolumeRequest>,
    ) -> Result<Response<NodeExpandVolumeResponse>, Status> {
        // Not supported for ephemeral volumes
        Err(Status::unimplemented("Volume expansion not supported"))
    }

    async fn node_get_capabilities(
        &self,
        _request: Request<NodeGetCapabilitiesRequest>,
    ) -> Result<Response<NodeGetCapabilitiesResponse>, Status> {
        debug!("NodeGetCapabilities called");

        let capabilities = vec![
            // No special capabilities needed for ephemeral volumes
        ];

        Ok(Response::new(NodeGetCapabilitiesResponse { capabilities }))
    }

    async fn node_get_info(
        &self,
        _request: Request<NodeGetInfoRequest>,
    ) -> Result<Response<NodeGetInfoResponse>, Status> {
        debug!("NodeGetInfo called");

        let response = NodeGetInfoResponse {
            node_id: self.node_id.clone(),
            max_volumes_per_node: 0, // No limit
            accessible_topology: None,
        };

        Ok(Response::new(response))
    }
}
