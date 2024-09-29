use std::collections::HashMap;

use crate::ir::{CallId, IRFunction};

#[derive(Debug)]
pub struct SystemInfo {
    pub num_channels: u32,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct DataIndex(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct WatchIndex(pub u32);

#[derive(Debug, Default)]
pub struct WatchedDataValues {
    pub values: HashMap<WatchIndex, Vec<f32>>,
}

#[derive(Debug)]
pub enum WispCommand {
    // System commands
    GetSystemInfo, // -> SystemInfo
    DspStart,
    DspStop,
    Exit,

    // Context commands
    ContextReset,
    ContextAddOrUpdateFunctions(Vec<IRFunction>),
    ContextRemoveFunction(String),
    ContextSetMainFunction(String),
    ContextSetDataValue(String, CallId, DataIndex, f32),
    ContextSetDataArray(String, CallId, DataIndex, String),
    ContextLearnMidiCC(String, CallId, DataIndex), // -> Option<WatchIndex>
    ContextWatchDataValue(String, CallId, DataIndex), // -> Option<WatchIndex>
    ContextUnwatchDataValue(WatchIndex),
    ContextQueryWatchedDataValues, // -> WatchedDataValues
    ContextLoadWaveFile(String, String, String),
    ContextUnloadWaveFile(String, String),
    ContextUpdate,
}

#[derive(Debug)]
pub enum WispCommandResponse {
    Ok(CommandResponse),
    NonFatalFailure,
    FatalFailure,
}

#[derive(Debug)]
pub enum CommandResponse {
    Ack,
    SystemInfo(SystemInfo),
    WatchIndex(Option<WatchIndex>),
    WatchedDataValues(WatchedDataValues),
}

impl WispCommandResponse {
    pub fn unwrap(self) -> CommandResponse {
        match self {
            WispCommandResponse::Ok(v) => v,
            WispCommandResponse::NonFatalFailure => {
                // TODO: Return Result instead
                panic!("Non-fatal failure happened")
            }
            WispCommandResponse::FatalFailure => panic!("Fatal failure happened"),
        }
    }
}
