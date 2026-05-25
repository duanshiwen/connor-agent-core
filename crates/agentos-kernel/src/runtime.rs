use crate::KernelServices;

#[derive(Clone)]
pub struct KernelRuntime {
    services: KernelServices,
}

impl KernelRuntime {
    pub(crate) fn new(services: KernelServices) -> Self {
        Self { services }
    }

    pub fn services(&self) -> &KernelServices {
        &self.services
    }
}
