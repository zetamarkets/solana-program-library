#![allow(missing_docs)]
use solana_program::{
    msg,
    pubkey::Pubkey,
};
use std::{fmt};
use crate::{
    pyth,
    math::{Decimal},
};


#[derive(Debug)]
enum LogEventType {
    PythOraclePriceUpdateType,
    SwitchboardV1OraclePriceUpdateType,
}

impl fmt::Display for LogEventType{
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
    oracle_pubkey: Pubkey,
    price: i64,
    conf: u64,
    status: pyth::PriceStatus,
    published_slot: u64,
}

impl LogEvent for PythOraclePriceUpdate {
    fn to_string(&self) -> String {
        return format!(
            "{},{},{},{},{},{}", 
            LogEventType::PythOraclePriceUpdateType.to_string(),
            self.oracle_pubkey.to_string(),
            self.price.to_string(),
            self.conf.to_string(),
            self.status.to_string(),
            self.published_slot,
        );
    }
}

pub struct SwitchboardV1OraclePriceUpdate {
    oracle_pubkey: Pubkey,
    price: Decimal,
    published_slot: u64,
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
