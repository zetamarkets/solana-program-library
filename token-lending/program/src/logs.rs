#![allow(missing_docs)]
use crate::math::Decimal;
use solana_program::{msg, pubkey::Pubkey};
use std::fmt;

#[derive(Debug)]
enum LogEventType {
    PythOraclePriceUpdateType,
    SwitchboardV1OraclePriceUpdateType,
}

impl fmt::Display for LogEventType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn emit_log_event(e: &dyn LogEvent) {
    msg!("Solend Log Event");
    msg!(&e.to_string());
}

pub trait LogEvent {
    fn to_string(&self) -> String;
}

pub struct PythOraclePriceUpdate {
    pub oracle_pubkey: Pubkey,
    pub price: Decimal,
    pub conf: u64,
    pub published_slot: u64,
}

impl LogEvent for PythOraclePriceUpdate {
    fn to_string(&self) -> String {
        return format!(
            "{},{},{},{},{}",
            LogEventType::PythOraclePriceUpdateType.to_string(),
            self.oracle_pubkey.to_string(),
            self.price.to_string(),
            self.conf.to_string(),
            self.published_slot,
        );
    }
}

pub struct SwitchboardV1OraclePriceUpdate {
    pub oracle_pubkey: Pubkey,
    pub price: Decimal,
    pub published_slot: u64,
}

impl LogEvent for SwitchboardV1OraclePriceUpdate {
    fn to_string(&self) -> String {
        return format!(
            "{},{},{},{}",
            LogEventType::SwitchboardV1OraclePriceUpdateType.to_string(),
            self.oracle_pubkey.to_string(),
            self.price.to_string(),
            self.published_slot,
        );
    }
}
