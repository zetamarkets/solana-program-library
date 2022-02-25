#![allow(missing_docs)]
use crate::math::Decimal;
use solana_program::pubkey::Pubkey;
use std::fmt;

extern crate serde;
extern crate serde_json;

#[derive(Debug, Serialize)]
pub enum LogEventType {
    PythOraclePriceUpdateType,
    PythErrorType,
    SwitchboardV1OraclePriceUpdateType,
    SwitchboardErrorType,
}

impl fmt::Display for LogEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[macro_export]
macro_rules! emit_log_event {
    ($e:expr) => {
        msg!("solend-event-log");
        msg!(&serde_json::to_string($e).unwrap());
    };
}

#[derive(Serialize)]
pub struct PythOraclePriceUpdate {
    pub event_type: LogEventType,
    pub oracle_pubkey: Pubkey,
    pub price: Decimal,
    pub confidence: u64,
    pub published_slot: u64,
}

#[derive(Serialize)]
pub struct PythError {
    pub event_type: LogEventType,
    pub oracle_pubkey: Pubkey,
    pub error_message: String,
}

#[derive(Serialize)]
pub struct SwitchboardV1OraclePriceUpdate {
    pub event_type: LogEventType,
    pub oracle_pubkey: Pubkey,
    pub price: Decimal,
    pub published_slot: u64,
}

#[derive(Serialize)]
pub struct SwitchboardError {
    pub event_type: LogEventType,
    pub oracle_pubkey: Pubkey,
    pub error_message: String,
}
