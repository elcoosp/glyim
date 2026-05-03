use bazel_remote_apis::build::bazel::remote::execution::v2::{
    content_addressable_storage_server::ContentAddressableStorage,
    Digest, FindMissingBlobsRequest, FindMissingBlobsResponse,
    BatchUpdateBlobsRequest, BatchUpdateBlobsResponse,
    BatchReadBlobsRequest, BatchReadBlobsResponse,
    GetTreeRequest, GetTreeResponse,
    SplitBlobRequest, SplitBlobResponse,
    SpliceBlobRequest, SpliceBlobResponse,

    batch_update_blobs_response,
    batch_update_blobs_request,
    batch_read_blobs_response,
};
use glyim_macro_vfs::{ContentHash, ContentStore, LocalContentStore};
use std::sync::Arc;

pub struct CasService {
    pub store: Arc<tokio::sync::Mutex<LocalContentStore>>,
}

impl CasService {
    fn grpc_status(code: tonic::Code, message: &str) -> bazel_remote_apis::google::rpc::Status {
        bazel_remote_apis::google::rpc::Status {
            code: code as i32,
            message: message.to_string(),
            details: vec![],
        }
    }
}

#[tonic::async_trait]
impl ContentAddressableStorage for CasService {
    async fn find_missing_blobs(
        &self,
        request: tonic::Request<FindMissingBlobsRequest>,
    ) -> Result<tonic::Response<FindMissingBlobsResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut missing = Vec::new();

        for blob_digest in &req.blob_digests {
            let hash: Option<ContentHash> = blob_digest.hash.parse().ok();
            if let Some(h) = hash {
                if self.store.lock().await.retrieve(h).is_none() {
                    missing.push(blob_digest.clone());
                }
            } else {
                missing.push(blob_digest.clone());
            }
        }

        Ok(tonic::Response::new(FindMissingBlobsResponse {
            missing_blob_digests: missing,
        }))
    }

    async fn batch_update_blobs(
        &self,
        request: tonic::Request<BatchUpdateBlobsRequest>,
    ) -> Result<tonic::Response<BatchUpdateBlobsResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut responses = Vec::new();

        for upload in &req.requests {
            let digest = upload.digest.clone();
            let stored_hash = self.store.lock().await.store(&upload.data);
            let expected_hash: Option<ContentHash> = digest.as_ref().and_then(|d| d.hash.parse().ok());

            let status = if let Some(expected) = expected_hash {
                if stored_hash == expected {
                    None
                } else {
                    Some(Self::grpc_status(tonic::Code::InvalidArgument, "hash mismatch"))
                }
            } else {
                Some(Self::grpc_status(tonic::Code::InvalidArgument, "invalid digest"))
            };

            responses.push(batch_update_blobs_response::Response {
                digest,
                status,
            });
        }

        Ok(tonic::Response::new(BatchUpdateBlobsResponse { responses }))
    }

    async fn batch_read_blobs(
        &self,
        request: tonic::Request<BatchReadBlobsRequest>,
    ) -> Result<tonic::Response<BatchReadBlobsResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut responses = Vec::new();

        for digest in &req.digests {
            let data = {
                let store_guard = self.store.lock().await;
                digest.hash.parse::<ContentHash>().ok()
                    .and_then(|h| store_guard.retrieve(h))
                    .unwrap_or_default()
            };

            let status = if data.is_empty() {
                Some(Self::grpc_status(tonic::Code::NotFound, "blob not found"))
            } else {
                None
            };

            responses.push(batch_read_blobs_response::Response {
                digest: Some(digest.clone()),
                data,
                compressor: 0,
                status,
            });
        }

        Ok(tonic::Response::new(BatchReadBlobsResponse { responses }))
    }

    type GetTreeStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<GetTreeResponse, tonic::Status>> + Send>,
    >;

    async fn get_tree(
        &self,
        _request: tonic::Request<GetTreeRequest>,
    ) -> Result<tonic::Response<Self::GetTreeStream>, tonic::Status> {
        let empty = GetTreeResponse {
            directories: vec![],
            next_page_token: String::new(),
        };
        let stream = tokio_stream::iter(vec![Ok(empty)]);
        Ok(tonic::Response::new(Box::pin(stream)))
    }

    async fn split_blob(
        &self,
        _request: tonic::Request<SplitBlobRequest>,
    ) -> Result<tonic::Response<SplitBlobResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("SplitBlob not implemented"))
    }

    async fn splice_blob(
        &self,
        _request: tonic::Request<SpliceBlobRequest>,
    ) -> Result<tonic::Response<SpliceBlobResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("SpliceBlob not implemented"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Request;

    fn test_store() -> Arc<tokio::sync::Mutex<LocalContentStore>> {
        let dir = tempfile::tempdir().unwrap();
        Arc::new(tokio::sync::Mutex::new(LocalContentStore::new(dir.path()).unwrap()))
    }

    #[tokio::test]
    async fn find_missing_returns_empty_after_store() {
        let store = test_store();
        let svc = CasService { store: store.clone() };

        let data = b"hello";
        let hash = store.lock().await.store(data);

        let req = FindMissingBlobsRequest {
            instance_name: String::new(),
            blob_digests: vec![Digest {
                hash: hash.to_hex(),
                size_bytes: data.len() as i64,
            }],
            digest_function: 1,
        };

        let resp = svc.find_missing_blobs(Request::new(req)).await.unwrap();
        assert!(resp.into_inner().missing_blob_digests.is_empty());
    }

    #[tokio::test]
    async fn batch_update_and_read_roundtrip() {
        let store = test_store();
        let svc = CasService { store: store.clone() };

        let data = b"roundtrip test";
        let hash = glyim_macro_vfs::ContentHash::of(data);

        let update_req = BatchUpdateBlobsRequest {
            instance_name: String::new(),
            requests: vec![batch_update_blobs_request::Request {
                digest: Some(Digest {
                    hash: hash.to_hex(),
                    size_bytes: data.len() as i64,
                }),
                data: data.to_vec(),
                compressor: 0,
            }],
            digest_function: 1,
        };
        let update_resp = svc.batch_update_blobs(Request::new(update_req)).await.unwrap();
        assert!(update_resp.into_inner().responses[0].status.is_none());

        let read_req = BatchReadBlobsRequest {
            instance_name: String::new(),
            digests: vec![Digest {
                hash: hash.to_hex(),
                size_bytes: data.len() as i64,
            }],
            acceptable_compressors: vec![],
            digest_function: 1,
        };
        let read_resp = svc.batch_read_blobs(Request::new(read_req)).await.unwrap();
        assert_eq!(read_resp.into_inner().responses[0].data, data);
    }
}
