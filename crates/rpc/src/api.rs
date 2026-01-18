//! DexVM REST API

use alloy_primitives::{Address, B256};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use dex_dexvm::{DexVmExecutor, DexVmOperation, DexVmTransaction};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

/// DexVM REST API service
#[derive(Clone)]
pub struct DexVmApi {
    executor: Arc<RwLock<DexVmExecutor>>,
}

impl DexVmApi {
    /// Create new API service
    pub fn new(executor: Arc<RwLock<DexVmExecutor>>) -> Self {
        Self { executor }
    }

    /// Create routes
    pub fn routes(self) -> Router {
        Router::new()
            .route("/", get(health_check))
            .route("/health", get(health_check))
            .route("/api/v1/counter/:address", get(get_counter))
            .route("/api/v1/counter/:address/increment", post(increment_counter))
            .route("/api/v1/counter/:address/decrement", post(decrement_counter))
            .route("/api/v1/state-root", get(get_state_root))
            .with_state(self)
    }
}

/// Counter query response
#[derive(Debug, Serialize, Deserialize)]
pub struct CounterResponse {
    pub address: Address,
    pub counter: u64,
}

/// Increment counter request
#[derive(Debug, Serialize, Deserialize)]
pub struct IncrementRequest {
    pub amount: u64,
}

/// Decrement counter request
#[derive(Debug, Serialize, Deserialize)]
pub struct DecrementRequest {
    pub amount: u64,
}

/// Operation response
#[derive(Debug, Serialize, Deserialize)]
pub struct OperationResponse {
    pub success: bool,
    pub tx_hash: B256,
    pub old_counter: u64,
    pub new_counter: u64,
    pub gas_used: u64,
    pub error: Option<String>,
}

/// State root response
#[derive(Debug, Serialize, Deserialize)]
pub struct StateRootResponse {
    pub state_root: B256,
}

/// API error type
#[derive(Debug)]
pub struct ApiError {
    message: String,
    status: StatusCode,
}

impl ApiError {
    fn new(message: impl Into<String>, status: StatusCode) -> Self {
        Self { message: message.into(), status }
    }

    fn internal_error(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::INTERNAL_SERVER_ERROR)
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::BAD_REQUEST)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
        }));
        (self.status, body).into_response()
    }
}

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: "dexvm-api".to_string(),
        version: "0.1.0".to_string(),
    })
}

async fn get_counter(
    Path(address): Path<Address>,
    State(api): State<DexVmApi>,
) -> Result<Json<CounterResponse>, ApiError> {
    let executor = api.executor.read().map_err(|e| ApiError::internal_error(e.to_string()))?;

    let counter = executor.state().get_counter(&address);

    debug!(address = %address, counter = counter, "DexVM counter queried");

    Ok(Json(CounterResponse { address, counter }))
}

async fn increment_counter(
    Path(address): Path<Address>,
    State(api): State<DexVmApi>,
    Json(req): Json<IncrementRequest>,
) -> Result<Json<OperationResponse>, ApiError> {
    if req.amount == 0 {
        warn!(address = %address, "DexVM increment rejected: amount is 0");
        return Err(ApiError::bad_request("Amount must be greater than 0"));
    }

    let mut executor = api.executor.write().map_err(|e| ApiError::internal_error(e.to_string()))?;

    let tx = DexVmTransaction {
        from: address,
        operation: DexVmOperation::Increment(req.amount),
        signature: vec![],
    };

    let tx_hash = tx.hash();

    let result =
        executor.execute_transaction(&tx).map_err(|e| ApiError::internal_error(e.to_string()))?;

    executor.commit();

    info!(
        address = %address,
        operation = "increment",
        amount = req.amount,
        old_counter = result.old_counter,
        new_counter = result.new_counter,
        tx_hash = %tx_hash,
        gas_used = result.gas_used,
        "DexVM counter incremented"
    );

    Ok(Json(OperationResponse {
        success: result.success,
        tx_hash,
        old_counter: result.old_counter,
        new_counter: result.new_counter,
        gas_used: result.gas_used,
        error: result.error,
    }))
}

async fn decrement_counter(
    Path(address): Path<Address>,
    State(api): State<DexVmApi>,
    Json(req): Json<DecrementRequest>,
) -> Result<Json<OperationResponse>, ApiError> {
    if req.amount == 0 {
        warn!(address = %address, "DexVM decrement rejected: amount is 0");
        return Err(ApiError::bad_request("Amount must be greater than 0"));
    }

    let mut executor = api.executor.write().map_err(|e| ApiError::internal_error(e.to_string()))?;

    let tx = DexVmTransaction {
        from: address,
        operation: DexVmOperation::Decrement(req.amount),
        signature: vec![],
    };

    let tx_hash = tx.hash();

    let result =
        executor.execute_transaction(&tx).map_err(|e| ApiError::internal_error(e.to_string()))?;

    executor.commit();

    if result.success {
        info!(
            address = %address,
            operation = "decrement",
            amount = req.amount,
            old_counter = result.old_counter,
            new_counter = result.new_counter,
            tx_hash = %tx_hash,
            gas_used = result.gas_used,
            "DexVM counter decremented"
        );
    } else {
        warn!(
            address = %address,
            operation = "decrement",
            amount = req.amount,
            old_counter = result.old_counter,
            tx_hash = %tx_hash,
            error = ?result.error,
            "DexVM decrement failed"
        );
    }

    Ok(Json(OperationResponse {
        success: result.success,
        tx_hash,
        old_counter: result.old_counter,
        new_counter: result.new_counter,
        gas_used: result.gas_used,
        error: result.error,
    }))
}

async fn get_state_root(State(api): State<DexVmApi>) -> Result<Json<StateRootResponse>, ApiError> {
    let executor = api.executor.read().map_err(|e| ApiError::internal_error(e.to_string()))?;

    let state_root = executor.state_root();

    Ok(Json(StateRootResponse { state_root }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;
    use axum::{body::Body, http::Request};
    use dex_dexvm::DexVmState;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_get_counter() {
        let executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let api = DexVmApi::new(executor);
        let app = api.routes();

        let addr = address!("1111111111111111111111111111111111111111");
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/counter/{}", addr))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_increment_counter() {
        let executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let api = DexVmApi::new(executor.clone());
        let app = api.routes();

        let addr = address!("2222222222222222222222222222222222222222");
        let req_body = serde_json::to_string(&IncrementRequest { amount: 10 }).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/counter/{}/increment", addr))
                    .header("content-type", "application/json")
                    .body(Body::from(req_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let exec = executor.read().unwrap();
        assert_eq!(exec.state().get_counter(&addr), 10);
    }
}
