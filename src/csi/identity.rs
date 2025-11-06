use tonic::{Request, Response, Status};
use crate::proto::csi::{
    identity_server::Identity,
    GetPluginInfoRequest, GetPluginInfoResponse,
    GetPluginCapabilitiesRequest, GetPluginCapabilitiesResponse,
    ProbeRequest, ProbeResponse,
};

const PLUGIN_NAME: &str = "csi.k8s.cacsi-driver";
const PLUGIN_VERSION: &str = "0.1.0";

pub struct IdentityService {}

impl IdentityService {
    pub fn new() -> Self {
        Self {}
    }
}

#[tonic::async_trait]
impl Identity for IdentityService {
    async fn get_plugin_info(
        &self,
        _request: Request<GetPluginInfoRequest>,
    ) -> Result<Response<GetPluginInfoResponse>, Status> {
        tracing::debug!("GetPluginInfo called");

        let response = GetPluginInfoResponse {
            name: PLUGIN_NAME.to_string(),
            vendor_version: PLUGIN_VERSION.to_string(),
            manifest: std::collections::HashMap::new(),
        };

        Ok(Response::new(response))
    }

    async fn get_plugin_capabilities(
        &self,
        _request: Request<GetPluginCapabilitiesRequest>,
    ) -> Result<Response<GetPluginCapabilitiesResponse>, Status> {
        tracing::debug!("GetPluginCapabilities called");

        // This driver only supports node service (ephemeral volumes)
        let capabilities = vec![];

        let response = GetPluginCapabilitiesResponse {
            capabilities,
        };

        Ok(Response::new(response))
    }

    async fn probe(
        &self,
        _request: Request<ProbeRequest>,
    ) -> Result<Response<ProbeResponse>, Status> {
        tracing::debug!("Probe called");

        let response = ProbeResponse {
            ready: true,
        };

        Ok(Response::new(response))
    }
}
