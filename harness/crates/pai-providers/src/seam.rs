//! This crate's seam: one key whose `Api` is a concrete type, not a trait object, because the tree needs
//! exactly [`ProviderRuntime`]'s single path -- a trait would open a second way to forget an update.

use pai_core::ServiceKey;

use crate::runtime::ProviderRuntime;

pub enum Providers {}

impl ServiceKey for Providers {
    type Api = ProviderRuntime;
    const NAME: &'static str = "providers";
}
