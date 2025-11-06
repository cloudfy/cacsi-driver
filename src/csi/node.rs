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

pub struct NodeService {
    node_id: String,
    cert_manager: CertificateManager,
    ca_manager: CaManager,
    cluster_domain: String,
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

        // Create target directory
        tokio::fs::create_dir_all(&req.target_path)
            .await
            .map_err(|e| Status::internal(format!("Failed to create target path: {}", e)))?;

        // Request certificate from certificate service
        match self.cert_manager.issue_certificate(
            &cert_id,
            &format!("{}.{}.svc.{}", pod_name, pod_namespace, self.cluster_domain),
            vec![pod_name.clone()],
            vec![],
            7, // 7 days validity
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
