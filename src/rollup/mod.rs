#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct GIORequest {
    #[validate(range(min = 0x10))] // avoid overlapping with our HTIF_YIELD_MANUAL_REASON_*
    pub domain: u16,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GIOResponse {
    pub response_code: u16,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exception {
    pub payload: String,
}