#[allow(unused_imports)]
use super::*;

/// unix-style load average over 1, 5, and 15 minutes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}
