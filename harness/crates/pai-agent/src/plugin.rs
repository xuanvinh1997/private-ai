//! Cắm phần ráp prompt vào cây.
//!
//! Vòng lặp không phải một service: nó là thứ *dùng* các service, và mỗi phiên chạy một
//! bản riêng. Cái cần dùng chung là sổ các khối prompt, vì plugin khác đóng góp vào đó.

use async_trait::async_trait;
use pai_core::{Context, Plugin};

use crate::prompt::{Prompt, SystemPrompt, order};

pub struct AgentPlugin {
    identity: String,
}

impl AgentPlugin {
    pub fn new(identity: impl Into<String>) -> AgentPlugin {
        AgentPlugin {
            identity: identity.into(),
        }
    }
}

#[async_trait]
impl Plugin for AgentPlugin {
    fn name(&self) -> &'static str {
        "agent"
    }

    async fn apply(&self, ctx: &Context) -> anyhow::Result<()> {
        let prompt = SystemPrompt::new();
        ctx.keep(ctx.provide::<Prompt>(prompt.clone())?);
        let identity = self.identity.clone();
        ctx.keep(prompt.contribute(order::IDENTITY, move || Some(identity.clone())));
        Ok(())
    }
}
