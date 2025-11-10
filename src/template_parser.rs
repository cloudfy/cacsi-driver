use anyhow::{Result, anyhow};
use regex::Regex;
use std::collections::HashMap;
use tracing::debug;

/// Parse and resolve template strings with pod metadata/spec placeholders
/// Supports syntax like: {metadata.namespace}, {spec.serviceAccountName}, {metadata.name}
pub struct TemplateParser {
    template_regex: Regex,
}

impl TemplateParser {
    pub fn new() -> Result<Self> {
        // Regex to match template placeholders like {metadata.namespace} or {spec.serviceAccountName}
        let template_regex = Regex::new(r"\{([^}]+)\}")
            .map_err(|e| anyhow!("Failed to compile template regex: {}", e))?;
        
        Ok(Self { template_regex })
    }

    /// Resolve a template string using pod information
    /// 
    /// # Arguments
    /// * `template` - Template string containing placeholders like {metadata.namespace}
    /// * `pod_metadata` - Map of metadata fields (namespace, name, labels, annotations)
    /// * `pod_spec` - Map of spec fields (serviceAccountName, nodeName, etc.)
    /// 
    /// # Returns
    /// Resolved string with all placeholders replaced
    pub fn resolve(
        &self,
        template: &str,
        pod_metadata: &HashMap<String, String>,
        pod_spec: &HashMap<String, String>,
    ) -> Result<String> {
        let mut result = template.to_string();
        
        debug!("Resolving template: {}", template);
        
        // Find all template placeholders
        for captures in self.template_regex.captures_iter(template) {
            if let Some(placeholder) = captures.get(1) {
                let placeholder_str = placeholder.as_str();
                let replacement = self.resolve_placeholder(placeholder_str, pod_metadata, pod_spec)?;
                
                // Replace {placeholder} with the resolved value
                result = result.replace(&format!("{{{}}}", placeholder_str), &replacement);
                
                debug!("Resolved {} -> {}", placeholder_str, replacement);
            }
        }
        
        Ok(result)
    }

    /// Resolve a single placeholder like "metadata.namespace" or "spec.serviceAccountName"
    fn resolve_placeholder(
        &self,
        placeholder: &str,
        pod_metadata: &HashMap<String, String>,
        pod_spec: &HashMap<String, String>,
    ) -> Result<String> {
        let parts: Vec<&str> = placeholder.split('.').collect();
        
        if parts.len() < 2 {
            return Err(anyhow!("Invalid placeholder format: {}. Expected format: metadata.field or spec.field", placeholder));
        }
        
        let section = parts[0];
        let field = parts[1..].join(".");
        
        match section {
            "metadata" => {
                pod_metadata.get(&field)
                    .cloned()
                    .ok_or_else(|| anyhow!("Metadata field not found: {}", field))
            }
            "spec" => {
                pod_spec.get(&field)
                    .cloned()
                    .ok_or_else(|| anyhow!("Spec field not found: {}", field))
            }
            _ => Err(anyhow!("Unknown section: {}. Supported sections: metadata, spec", section)),
        }
    }

    /// Check if a string contains template placeholders
    pub fn has_templates(&self, text: &str) -> bool {
        self.template_regex.is_match(text)
    }
}

impl Default for TemplateParser {
    fn default() -> Self {
        Self::new().expect("Failed to create default TemplateParser")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_metadata_namespace() {
        let parser = TemplateParser::new().unwrap();
        let mut metadata = HashMap::new();
        metadata.insert("namespace".to_string(), "default".to_string());
        metadata.insert("name".to_string(), "my-pod".to_string());
        
        let spec = HashMap::new();
        
        let result = parser.resolve("{metadata.namespace}", &metadata, &spec).unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_resolve_spec_service_account() {
        let parser = TemplateParser::new().unwrap();
        let metadata = HashMap::new();
        let mut spec = HashMap::new();
        spec.insert("serviceAccountName".to_string(), "my-sa".to_string());
        
        let result = parser.resolve("{spec.serviceAccountName}", &metadata, &spec).unwrap();
        assert_eq!(result, "my-sa");
    }

    #[test]
    fn test_resolve_multiple_placeholders() {
        let parser = TemplateParser::new().unwrap();
        let mut metadata = HashMap::new();
        metadata.insert("namespace".to_string(), "prod".to_string());
        metadata.insert("name".to_string(), "web-app".to_string());
        
        let mut spec = HashMap::new();
        spec.insert("serviceAccountName".to_string(), "web-sa".to_string());
        
        let result = parser.resolve(
            "{spec.serviceAccountName}.{metadata.name}.{metadata.namespace}",
            &metadata,
            &spec,
        ).unwrap();
        assert_eq!(result, "web-sa.web-app.prod");
    }

    #[test]
    fn test_has_templates() {
        let parser = TemplateParser::new().unwrap();
        
        assert!(parser.has_templates("{metadata.namespace}"));
        assert!(parser.has_templates("prefix-{spec.serviceAccountName}"));
        assert!(!parser.has_templates("no-templates-here"));
    }

    #[test]
    fn test_invalid_placeholder() {
        let parser = TemplateParser::new().unwrap();
        let metadata = HashMap::new();
        let spec = HashMap::new();
        
        let result = parser.resolve("{invalid.field}", &metadata, &spec);
        assert!(result.is_err());
    }
}
