#[allow(unused_imports)]
use super::*;

pub trait AdapterHostClient:
    AdapterPoller + AdapterVerifier + AdapterValidator + AdapterHostAdmin
{
}
