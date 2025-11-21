// gRPC client for Bazel Remote Execution API v2

use tonic::transport::Channel;
use std::time::Duration;

// Include the generated protobuf code
pub mod proto {
    tonic::include_proto!("build.bazel.remote.execution.v2");
}

use proto::{
    content_addressable_storage_client::ContentAddressableStorageClient,
    action_cache_client::ActionCacheClient,
    capabilities_client::CapabilitiesClient,
    Digest, BatchReadBlobsRequest, BatchUpdateBlobsRequest,
    FindMissingBlobsRequest, GetActionResultRequest,
    UpdateActionResultRequest, GetCapabilitiesRequest,
};

/// gRPC Remote Cache Client
pub struct GrpcCacheClient {
    cas_client: ContentAddressableStorageClient<Channel>,
    action_cache_client: ActionCacheClient<Channel>,
    capabilities_client: CapabilitiesClient<Channel>,
    instance_name: String,
}

impl GrpcCacheClient {
    /// Create a new gRPC cache client
    pub async fn connect(endpoint: impl Into<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let endpoint = endpoint.into();
        let channel = Channel::from_shared(endpoint.clone())?
            .timeout(Duration::from_secs(30))
            .connect()
            .await?;

        Ok(Self {
            cas_client: ContentAddressableStorageClient::new(channel.clone()),
            action_cache_client: ActionCacheClient::new(channel.clone()),
            capabilities_client: CapabilitiesClient::new(channel),
            instance_name: String::new(),  // Default instance
        })
    }

    /// Set the instance name for remote cache operations
    pub fn with_instance_name(mut self, instance_name: impl Into<String>) -> Self {
        self.instance_name = instance_name.into();
        self
    }

    /// Get server capabilities
    pub async fn get_capabilities(&mut self) -> Result<proto::ServerCapabilities, Box<dyn std::error::Error>> {
        let request = GetCapabilitiesRequest {
            instance_name: self.instance_name.clone(),
        };

        let response = self.capabilities_client
            .get_capabilities(request)
            .await?
            .into_inner();

        Ok(response)
    }

    /// Find missing blobs in the CAS
    pub async fn find_missing_blobs(&mut self, digests: Vec<Digest>) -> Result<Vec<Digest>, Box<dyn std::error::Error>> {
        let request = FindMissingBlobsRequest {
            instance_name: self.instance_name.clone(),
            blob_digests: digests,
        };

        let response = self.cas_client
            .find_missing_blobs(request)
            .await?
            .into_inner();

        Ok(response.missing_blob_digests)
    }

    /// Upload blobs to the CAS
    pub async fn upload_blobs(&mut self, blobs: Vec<(Digest, Vec<u8>)>) -> Result<(), Box<dyn std::error::Error>> {
        let requests = blobs
            .into_iter()
            .map(|(digest, data)| {
                proto::batch_update_blobs_request::Request {
                    digest: Some(digest),
                    data,
                }
            })
            .collect();

        let request = BatchUpdateBlobsRequest {
            instance_name: self.instance_name.clone(),
            requests,
        };

        let response = self.cas_client
            .batch_update_blobs(request)
            .await?
            .into_inner();

        // Check for errors in responses
        for resp in response.responses {
            if resp.status_code != 0 {
                return Err(format!(
                    "Upload failed for blob: {} (code: {})",
                    resp.status_message, resp.status_code
                ).into());
            }
        }

        Ok(())
    }

    /// Download blobs from the CAS
    pub async fn download_blobs(&mut self, digests: Vec<Digest>) -> Result<Vec<(Digest, Vec<u8>)>, Box<dyn std::error::Error>> {
        let request = BatchReadBlobsRequest {
            instance_name: self.instance_name.clone(),
            digests,
        };

        let response = self.cas_client
            .batch_read_blobs(request)
            .await?
            .into_inner();

        let mut results = Vec::new();
        for resp in response.responses {
            if resp.status_code != 0 {
                return Err(format!(
                    "Download failed for blob: {} (code: {})",
                    resp.status_message, resp.status_code
                ).into());
            }

            if let Some(digest) = resp.digest {
                results.push((digest, resp.data));
            }
        }

        Ok(results)
    }

    /// Get action result from cache
    pub async fn get_action_result(&mut self, action_digest: Digest) -> Result<Option<proto::ActionResult>, Box<dyn std::error::Error>> {
        let request = GetActionResultRequest {
            instance_name: self.instance_name.clone(),
            action_digest: Some(action_digest),
        };

        match self.action_cache_client.get_action_result(request).await {
            Ok(response) => Ok(Some(response.into_inner())),
            Err(status) => {
                if status.code() == tonic::Code::NotFound {
                    Ok(None)  // Cache miss
                } else {
                    Err(status.into())
                }
            }
        }
    }

    /// Update action result in cache
    pub async fn update_action_result(
        &mut self,
        action_digest: Digest,
        action_result: proto::ActionResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = UpdateActionResultRequest {
            instance_name: self.instance_name.clone(),
            action_digest: Some(action_digest),
            action_result: Some(action_result),
        };

        self.action_cache_client
            .update_action_result(request)
            .await?;

        Ok(())
    }
}

/// Convert SHA-256 hash to Digest
#[must_use] 
pub fn sha256_digest(hash: &str, size: i64) -> Digest {
    Digest {
        hash: hash.to_string(),
        size_bytes: size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_digest() {
        let digest = sha256_digest("abcd1234", 1024);
        assert_eq!(digest.hash, "abcd1234");
        assert_eq!(digest.size_bytes, 1024);
    }

    // Note: Integration tests require a running gRPC server
    // Add them to tests/ directory with #[ignore] attribute
}
