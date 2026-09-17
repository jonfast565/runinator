use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

mod wait_state;
pub use wait_state::WaitState;

mod wait_elapsed_output;
pub use wait_elapsed_output::WaitElapsedOutput;

mod output_payload;
pub use output_payload::OutputPayload;

mod input_state;
pub use input_state::InputState;

mod subflow_state;
pub use subflow_state::SubflowState;

mod approval_state;
pub use approval_state::ApprovalState;

mod gate_state;
pub use gate_state::GateState;

mod signal_state;
pub use signal_state::SignalState;
