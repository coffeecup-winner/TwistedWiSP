use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct FlowNodeIndex(pub u32);
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct FlowNodeOutletIndex(pub u32);
#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct FlowNodeInletIndex(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetadata {
    pub num_inlets: u32,
    pub num_outlets: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WispCommand {
    // System commands
    DspStart,
    DspStop,
    Exit,

    // Function commands
    FunctionCreate, // -> String
    FunctionRemove(String),
    FunctionList,                // -> Vec<String>
    FunctionGetMetadata(String), // -> FunctionMetadata

    // Flow commands
    FlowAddNode(String, String), // -> FlowNodeIndex
    FlowConnect(
        String,
        FlowNodeIndex,
        FlowNodeOutletIndex,
        FlowNodeIndex,
        FlowNodeInletIndex,
    ),
    FlowDisconnect(
        String,
        FlowNodeIndex,
        FlowNodeOutletIndex,
        FlowNodeIndex,
        FlowNodeInletIndex,
    ),
}

impl WispCommand {
    pub fn from_json(json: &str) -> WispCommand {
        serde_json::from_str(json).expect("Failed to deserialize a WiSP command")
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize a WiSP command")
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum WispCommandResponse<T> {
    Ok(T),
    NonFatalFailure,
    FatalFailure,
}

pub trait CommandResponse: Serialize + DeserializeOwned {}
impl CommandResponse for () {}
impl CommandResponse for String {}
impl CommandResponse for Vec<String> {}
impl CommandResponse for FunctionMetadata {}
impl CommandResponse for FlowNodeIndex {}

impl<T> WispCommandResponse<T>
where
    T: CommandResponse,
{
    pub fn from_json(json: &str) -> WispCommandResponse<T> {
        serde_json::from_str(json).expect("Failed to deserialize a WiSP command response")
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Failed to serialize a WiSP command response")
    }

    pub fn unwrap(self) -> T {
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
