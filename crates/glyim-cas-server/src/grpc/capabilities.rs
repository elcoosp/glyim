use bazel_remote_apis::build::bazel::remote::execution::v2::{
    capabilities_server::Capabilities,
    GetCapabilitiesRequest, ServerCapabilities,
    CacheCapabilities, ExecutionCapabilities,
    digest_function::Value as DigestFunction,
};
use bazel_remote_apis::build::bazel::semver::SemVer;

#[derive(Default)]
pub struct CapabilitiesService;

#[tonic::async_trait]
impl Capabilities for CapabilitiesService {
    async fn get_capabilities(
        &self,
        _request: tonic::Request<GetCapabilitiesRequest>,
    ) -> Result<tonic::Response<ServerCapabilities>, tonic::Status> {
        Ok(tonic::Response::new(ServerCapabilities {
            deprecated_api_version: Some(SemVer {
                prerelease: String::new(),
                major: 2,
                minor: 0,
                patch: 0,
            }),
            execution_capabilities: Some(ExecutionCapabilities {
                digest_function: DigestFunction::Sha256.into(),
                exec_enabled: false,
                execution_priority_capabilities: None,
                supported_node_properties: vec![],
                digest_functions: vec![DigestFunction::Sha256.into()],
            }),
            cache_capabilities: Some(CacheCapabilities {
                digest_functions: vec![DigestFunction::Sha256.into()],
                action_cache_update_capabilities: None,
                cache_priority_capabilities: None,
                max_batch_total_size_bytes: 0,
                symlink_absolute_path_strategy: 0,
                supported_compressors: vec![],
                supported_batch_update_compressors: vec![],
                fast_cdc_2020_params: None,
                max_cas_blob_size_bytes: 0,
                rep_max_cdc_params: None,
                split_blob_support: false,
                splice_blob_support: false,
            }),
            low_api_version: None,
            high_api_version: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    #[tokio::test]
    async fn capabilities_reports_sha256() {
        let svc = CapabilitiesService::default();
        let resp = svc.get_capabilities(Request::new(GetCapabilitiesRequest {
            instance_name: String::new(),
        })).await.unwrap();
        let caps = resp.into_inner();
        let cache_caps = caps.cache_capabilities.unwrap();
        assert!(cache_caps.digest_functions.iter().any(|f| *f == DigestFunction::Sha256 as i32));
    }

    #[tokio::test]
    async fn capabilities_reports_api_version_2() {
        let svc = CapabilitiesService::default();
        let resp = svc.get_capabilities(Request::new(GetCapabilitiesRequest {
            instance_name: String::new(),
        })).await.unwrap();
        let caps = resp.into_inner();
        let api = caps.deprecated_api_version.unwrap();
        assert_eq!(api.major, 2);
    }
}
