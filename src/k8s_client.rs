use anyhow::{Result, Context};
use kube::{Client, Api};
use k8s_openapi::api::core::v1::Pod;
use std::collections::HashMap;
use tracing::debug;

pub async fn get_client() -> Result<Client, kube::Error> {
    Client::try_default().await
}

/// Fetch pod information from Kubernetes API
pub async fn get_pod_info(
    client: &Client,
    namespace: &str,
    pod_name: &str,
) -> Result<(HashMap<String, String>, HashMap<String, String>)> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    
    let pod = pods.get(pod_name)
        .await
        .context(format!("Failed to get pod {}/{}", namespace, pod_name))?;
    
    debug!("Retrieved pod information for {}/{}", namespace, pod_name);
    
    // Extract metadata
    let mut metadata_map = HashMap::new();
    let metadata = &pod.metadata;
    
    if let Some(name) = &metadata.name {
        metadata_map.insert("name".to_string(), name.clone());
    }
    if let Some(ns) = &metadata.namespace {
        metadata_map.insert("namespace".to_string(), ns.clone());
    }
    if let Some(uid) = &metadata.uid {
        metadata_map.insert("uid".to_string(), uid.clone());
    }
    
    // Add labels as metadata.labels.key
    if let Some(labels) = &metadata.labels {
        for (key, value) in labels {
            metadata_map.insert(format!("labels.{}", key), value.clone());
        }
    }
    
    // Add annotations as metadata.annotations.key
    if let Some(annotations) = &metadata.annotations {
        for (key, value) in annotations {
            metadata_map.insert(format!("annotations.{}", key), value.clone());
        }
    }
    
    // Extract spec
    let mut spec_map = HashMap::new();
    if let Some(spec) = &pod.spec {
        if let Some(service_account_name) = &spec.service_account_name {
            spec_map.insert("serviceAccountName".to_string(), service_account_name.clone());
        }
        if let Some(node_name) = &spec.node_name {
            spec_map.insert("nodeName".to_string(), node_name.clone());
        }
        if let Some(hostname) = &spec.hostname {
            spec_map.insert("hostname".to_string(), hostname.clone());
        }
        if let Some(subdomain) = &spec.subdomain {
            spec_map.insert("subdomain".to_string(), subdomain.clone());
        }
        
        // Add priority class if set
        if let Some(priority_class_name) = &spec.priority_class_name {
            spec_map.insert("priorityClassName".to_string(), priority_class_name.clone());
        }
    }
    
    debug!("Extracted {} metadata fields and {} spec fields", metadata_map.len(), spec_map.len());
    
    Ok((metadata_map, spec_map))
}
