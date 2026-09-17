#[allow(unused_imports)]
use super::*;

pub trait IngressControlStore: RunOperationsStore + DeliveryStore + DefinitionStore {}

impl<T> IngressControlStore for T where T: RunOperationsStore + DeliveryStore + DefinitionStore {}
